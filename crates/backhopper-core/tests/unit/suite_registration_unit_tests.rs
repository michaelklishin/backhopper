// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use backhopper_core::compat::patch::Patch;
use backhopper_core::compat::suite_registration::analyse_suite_registration;
use backhopper_core::model::names::RelativePath;
use backhopper_core::model::verdict::Reason;

const MARKER: &str = "PARALLEL_CT";

fn reasons(diff: &[u8], targets: &[(&str, &str)]) -> Vec<Reason> {
    let map: BTreeMap<RelativePath, String> = targets
        .iter()
        .map(|(p, c)| (RelativePath::new(*p).unwrap(), (*c).to_owned()))
        .collect();
    let read_target = |path: &RelativePath| map.get(path).cloned();
    let parsed = Patch::parse(diff).unwrap();
    analyse_suite_registration(parsed.files(), MARKER, &read_target)
}

const ADD_SUITE: &[u8] = b"\
diff --git a/deps/rabbit/test/new_thing_SUITE.erl b/deps/rabbit/test/new_thing_SUITE.erl
new file mode 100644
--- /dev/null
+++ b/deps/rabbit/test/new_thing_SUITE.erl
@@ -0,0 +1,2 @@
+-module(new_thing_SUITE).
+-export([all/0]).
";

#[test]
fn fires_for_added_suite_in_parallel_ct_app() {
    let found = reasons(
        ADD_SUITE,
        &[(
            "deps/rabbit/Makefile",
            "PARALLEL_CT_SET_1_A = existing_suite\n",
        )],
    );
    assert!(
        found
            .iter()
            .any(|r| matches!(r, Reason::SuiteNotRegisteredForCt { .. })),
        "expected a finding, got {found:?}"
    );
}

#[test]
fn silent_when_app_has_no_marker() {
    let found = reasons(ADD_SUITE, &[("deps/rabbit/Makefile", "all:\n\techo hi\n")]);
    assert!(found.is_empty(), "expected silence, got {found:?}");
}

#[test]
fn silent_when_makefile_absent() {
    let found = reasons(ADD_SUITE, &[]);
    assert!(found.is_empty(), "expected silence, got {found:?}");
}

#[test]
fn marker_in_comment_does_not_conscript() {
    let found = reasons(
        ADD_SUITE,
        &[(
            "deps/rabbit/Makefile",
            "# uses PARALLEL_CT elsewhere\nall:\n\techo hi\n",
        )],
    );
    assert!(found.is_empty(), "expected silence, got {found:?}");
}

#[test]
fn word_boundary_no_substring_credit() {
    let found = reasons(
        b"\
diff --git a/deps/rabbit/test/cluster_SUITE.erl b/deps/rabbit/test/cluster_SUITE.erl
new file mode 100644
--- /dev/null
+++ b/deps/rabbit/test/cluster_SUITE.erl
@@ -0,0 +1,1 @@
+-module(cluster_SUITE).
",
        &[(
            "deps/rabbit/Makefile",
            "PARALLEL_CT_SET_1_A = cluster_minority\n",
        )],
    );
    assert!(
        found
            .iter()
            .any(|r| matches!(r, Reason::SuiteNotRegisteredForCt { .. })),
        "cluster_minority must not satisfy cluster's registration, got {found:?}"
    );
}

#[test]
fn nested_test_dir_not_a_suite() {
    let found = reasons(
        b"\
diff --git a/deps/rabbit/test/helpers/x_SUITE.erl b/deps/rabbit/test/helpers/x_SUITE.erl
new file mode 100644
--- /dev/null
+++ b/deps/rabbit/test/helpers/x_SUITE.erl
@@ -0,0 +1,1 @@
+-module(x_SUITE).
",
        &[("deps/rabbit/Makefile", "PARALLEL_CT_SET_1_A = existing\n")],
    );
    assert!(found.is_empty(), "expected silence, got {found:?}");
}

#[test]
fn silent_when_patch_registers_it() {
    let diff: &[u8] = b"\
diff --git a/deps/rabbit/test/new_thing_SUITE.erl b/deps/rabbit/test/new_thing_SUITE.erl
new file mode 100644
--- /dev/null
+++ b/deps/rabbit/test/new_thing_SUITE.erl
@@ -0,0 +1,1 @@
+-module(new_thing_SUITE).
diff --git a/deps/rabbit/Makefile b/deps/rabbit/Makefile
--- a/deps/rabbit/Makefile
+++ b/deps/rabbit/Makefile
@@ -1,1 +1,2 @@
 PARALLEL_CT_SET_1_A = existing_suite
+PARALLEL_CT_SET_1_B = new_thing
";
    let found = reasons(
        diff,
        &[(
            "deps/rabbit/Makefile",
            "PARALLEL_CT_SET_1_A = existing_suite\n",
        )],
    );
    assert!(found.is_empty(), "expected silence, got {found:?}");
}

#[test]
fn multi_app_attribution() {
    let diff: &[u8] = b"\
diff --git a/deps/rabbit/test/a_SUITE.erl b/deps/rabbit/test/a_SUITE.erl
new file mode 100644
--- /dev/null
+++ b/deps/rabbit/test/a_SUITE.erl
@@ -0,0 +1,1 @@
+-module(a_SUITE).
diff --git a/deps/rabbitmq_mqtt/test/b_SUITE.erl b/deps/rabbitmq_mqtt/test/b_SUITE.erl
new file mode 100644
--- /dev/null
+++ b/deps/rabbitmq_mqtt/test/b_SUITE.erl
@@ -0,0 +1,1 @@
+-module(b_SUITE).
";
    let found = reasons(
        diff,
        &[
            ("deps/rabbit/Makefile", "PARALLEL_CT_SET_1_A = existing\n"),
            (
                "deps/rabbitmq_mqtt/Makefile",
                "PARALLEL_CT_SET_1_A = existing\n",
            ),
        ],
    );
    assert_eq!(
        found.len(),
        2,
        "expected one finding per app, got {found:?}"
    );
}
