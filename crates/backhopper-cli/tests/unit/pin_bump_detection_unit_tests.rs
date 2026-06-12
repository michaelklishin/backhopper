// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Hunk-based pin-bump detection. Detection reads only the added and
//! removed lines of the root `rabbitmq-components.mk`; context lines
//! and the vendored `deps/*/` copies must never produce a bump.

use backhopper_cli::commands::rabbitmq_components::detect_pin_bumps;
use backhopper_core::compat::patch::Patch;
use backhopper_core::model::verdict::PinBump;

/// One-file unified diff with a correct hunk header for `body`.
fn one_file_patch(path: &str, body: &str) -> String {
    let old = body.lines().filter(|l| !l.starts_with('+')).count();
    let new = body.lines().filter(|l| !l.starts_with('-')).count();
    format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,{old} +1,{new} @@\n{body}"
    )
}

fn bumps_in(path: &str, body: &str) -> Vec<PinBump> {
    let text = one_file_patch(path, body);
    let patch = Patch::parse(text.as_bytes()).expect("patch parses");
    detect_pin_bumps(&patch.files)
}

#[test]
fn version_bump_yields_from_and_to() {
    let bumps = bumps_in(
        "rabbitmq-components.mk",
        " PROJECT = rabbit\n-dep_cowlib = hex 2.16.0\n+dep_cowlib = hex 2.17.1\n",
    );
    assert_eq!(bumps.len(), 1);
    assert_eq!(bumps[0].dep.as_str(), "cowlib");
    assert_eq!(bumps[0].from.as_deref(), Some("hex 2.16.0"));
    assert_eq!(bumps[0].to, "hex 2.17.1");
    assert_eq!(bumps[0].status, None);
}

#[test]
fn introduced_pin_has_no_from() {
    let bumps = bumps_in(
        "rabbitmq-components.mk",
        "+dep_osiris = git https://github.com/rabbitmq/osiris v1.13.1\n",
    );
    assert_eq!(bumps.len(), 1);
    assert_eq!(bumps[0].from, None);
    assert_eq!(bumps[0].to, "git v1.13.1");
}

#[test]
fn removed_pin_yields_nothing() {
    let bumps = bumps_in("rabbitmq-components.mk", "-dep_cowlib = hex 2.16.0\n");
    assert!(bumps.is_empty());
}

#[test]
fn source_kind_flip_at_equal_version_is_a_bump() {
    let bumps = bumps_in(
        "rabbitmq-components.mk",
        "-dep_cowboy = hex 2.13.0\n+dep_cowboy = git_rmq cowboy 2.13.0\n",
    );
    assert_eq!(bumps.len(), 1);
    assert_eq!(bumps[0].from.as_deref(), Some("hex 2.13.0"));
    assert_eq!(bumps[0].to, "git_rmq 2.13.0");
}

#[test]
fn moved_but_unchanged_pin_yields_nothing() {
    let bumps = bumps_in(
        "rabbitmq-components.mk",
        "-dep_ra = hex 3.1.6\n+dep_ra = hex 3.1.6\n",
    );
    assert!(bumps.is_empty());
}

#[test]
fn multiple_bumps_come_out_sorted_by_dep_name() {
    let bumps = bumps_in(
        "rabbitmq-components.mk",
        "-dep_ra = hex 3.1.5\n+dep_ra = hex 3.1.6\n-dep_cowlib = hex 2.16.0\n+dep_cowlib = hex 2.17.1\n",
    );
    let names: Vec<&str> = bumps.iter().map(|b| b.dep.as_str()).collect();
    assert_eq!(names, ["cowlib", "ra"]);
}

#[test]
fn vendored_deps_copy_is_ignored() {
    let bumps = bumps_in(
        "deps/rabbit_common/rabbitmq-components.mk",
        "-dep_cowlib = hex 2.16.0\n+dep_cowlib = hex 2.17.1\n",
    );
    assert!(bumps.is_empty());
}

#[test]
fn context_pin_lines_yield_nothing() {
    let bumps = bumps_in(
        "rabbitmq-components.mk",
        " dep_cowlib = hex 2.16.0\n-PROJECT = rabbit\n+PROJECT = rabbitmq\n",
    );
    assert!(bumps.is_empty());
}

#[test]
fn other_files_in_the_same_patch_do_not_contribute() {
    let mk = one_file_patch(
        "rabbitmq-components.mk",
        "-dep_cowlib = hex 2.16.0\n+dep_cowlib = hex 2.17.1\n",
    );
    let erl = one_file_patch("src/rabbit.erl", "+%% dep_ra = hex 9.9.9\n");
    let text = format!("{mk}{erl}");
    let patch = Patch::parse(text.as_bytes()).expect("patch parses");
    let bumps = detect_pin_bumps(&patch.files);
    assert_eq!(bumps.len(), 1);
    assert_eq!(bumps[0].dep.as_str(), "cowlib");
}

#[test]
fn newly_created_manifest_detects_introduced_pins() {
    let text = "diff --git a/rabbitmq-components.mk b/rabbitmq-components.mk\n\
                --- /dev/null\n\
                +++ b/rabbitmq-components.mk\n\
                @@ -0,0 +1,1 @@\n\
                +dep_ra = hex 3.1.6\n";
    let patch = Patch::parse(text.as_bytes()).expect("patch parses");
    let bumps = detect_pin_bumps(&patch.files);
    assert_eq!(bumps.len(), 1);
    assert_eq!(bumps[0].from, None);
}

#[test]
fn deleted_manifest_yields_nothing() {
    let text = "diff --git a/rabbitmq-components.mk b/rabbitmq-components.mk\n\
                --- a/rabbitmq-components.mk\n\
                +++ /dev/null\n\
                @@ -1,1 +0,0 @@\n\
                -dep_ra = hex 3.1.6\n";
    let patch = Patch::parse(text.as_bytes()).expect("patch parses");
    assert!(detect_pin_bumps(&patch.files).is_empty());
}
