// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `analyse_macro_values`: same-file `-define` value comparison over
//! macros the patch uses, everything else withheld and counted.

use std::collections::BTreeMap;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::define_resolve::{MacroValueAnalysis, analyse_macro_values};
use backhopper_core::model::names::RelativePath;
use backhopper_core::model::verdict::Reason;

const RA_SERVER: &str = "deps/ra/src/ra_server.erl";

fn reader(files: &[(&str, &str)]) -> impl Fn(&RelativePath) -> Option<String> {
    let map: BTreeMap<String, String> = files
        .iter()
        .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
        .collect();
    move |path: &RelativePath| map.get(path.as_str()).cloned()
}

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn analyse(added: &str, target: &str, source: Option<&str>) -> MacroValueAnalysis {
    let path = rp(RA_SERVER);
    let subjects = [AddedLinesSubject {
        source_path: &path,
        added_text: added,
        line_map: &[],
    }];
    let target_rows = [(RA_SERVER, target)];
    let read_target = reader(&target_rows);
    let source_rows = source.map(|s| [(RA_SERVER, s)]);
    let read_source_fn = source_rows.as_ref().map(|rows| reader(rows));
    let read_source = read_source_fn
        .as_ref()
        .map(|f| f as &dyn Fn(&RelativePath) -> Option<String>);
    analyse_macro_values(&subjects, &read_target, read_source)
}

fn drift(reasons: &[Reason]) -> Vec<(String, String, String)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::MacroValueDrift {
                macro_name,
                source_value,
                target_value,
                ..
            } => Some((
                macro_name.clone(),
                source_value.clone(),
                target_value.clone(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_changed_value_is_flagged_with_both_bodies() {
    let analysis = analyse(
        "handle(S) -> ?SNAPSHOT_INTERVAL + S.\n",
        "-module(ra_server).\n-define(SNAPSHOT_INTERVAL, 4096).\n",
        Some("-module(ra_server).\n-define(SNAPSHOT_INTERVAL, 8192).\n"),
    );
    assert_eq!(
        drift(&analysis.reasons),
        [(
            "SNAPSHOT_INTERVAL".to_owned(),
            "8192".to_owned(),
            "4096".to_owned()
        )]
    );
    assert_eq!(analysis.checks.compared, 1);
    assert!(analysis.reasons.iter().all(|r| !r.is_blocking()));
}

#[test]
fn identical_modulo_whitespace_and_comments_is_clean() {
    let analysis = analyse(
        "handle(S) -> ?LIMITS(S).\n",
        "-module(ra_server).\n-define(LIMITS(X), #{max => X,\n    min => 0}). % target note\n",
        Some("-module(ra_server).\n-define(LIMITS(X), #{max => X, min => 0}).\n"),
    );
    assert!(
        analysis.reasons.is_empty(),
        "unexpected: {:?}",
        analysis.reasons
    );
    assert_eq!(analysis.checks.compared, 1);
}

#[test]
fn a_parameter_count_change_is_drift() {
    let analysis = analyse(
        "handle(S) -> ?WRAP(S).\n",
        "-module(ra_server).\n-define(WRAP(X), {X}).\n",
        Some("-module(ra_server).\n-define(WRAP(X, Y), {X, Y}).\n"),
    );
    let d = drift(&analysis.reasons);
    assert_eq!(d.len(), 1);
    assert_ne!(d[0].1, d[0].2, "rendered values must differ: {d:?}");
}

#[test]
fn an_ifdef_double_definition_withholds_and_counts() {
    let analysis = analyse(
        "handle(S) -> ?LOG_LEVEL(S).\n",
        "-module(ra_server).\n-ifdef(TEST).\n-define(LOG_LEVEL, debug).\n-else.\n-define(LOG_LEVEL, info).\n-endif.\n",
        Some("-module(ra_server).\n-define(LOG_LEVEL, info).\n"),
    );
    assert!(analysis.reasons.is_empty());
    assert_eq!(analysis.checks.withheld_multiple_defines, 1);
}

#[test]
fn coexisting_object_and_function_forms_withhold() {
    // ?FOO and ?FOO(X) are distinct macros; keyed by name they look duplicated, so the check withholds.
    let both = "-module(ra_server).\n-define(FOO, 1).\n-define(FOO(X), X).\n";
    let analysis = analyse("handle(S) -> ?FOO + S.\n", both, Some(both));
    assert!(analysis.reasons.is_empty());
    assert_eq!(analysis.checks.withheld_multiple_defines, 1);
}

#[test]
fn an_include_resolved_macro_withholds_as_defined_elsewhere() {
    let analysis = analyse(
        "handle(S) -> ?RA_LOG(S).\n",
        "-module(ra_server).\n-include(\"ra.hrl\").\n",
        Some("-module(ra_server).\n-include(\"ra.hrl\").\n"),
    );
    assert!(analysis.reasons.is_empty());
    assert_eq!(analysis.checks.withheld_definition_elsewhere, 1);
}

#[test]
fn a_patch_redefined_macro_is_skipped() {
    let analysis = analyse(
        "-define(SNAPSHOT_INTERVAL, 16384).\nhandle(S) -> ?SNAPSHOT_INTERVAL + S.\n",
        "-module(ra_server).\n-define(SNAPSHOT_INTERVAL, 4096).\n",
        Some("-module(ra_server).\n-define(SNAPSHOT_INTERVAL, 16384).\n"),
    );
    assert!(analysis.reasons.is_empty());
    assert!(analysis.checks.is_empty());
}

#[test]
fn module_in_both_bodies_compares_textually_and_is_clean() {
    let body = "-module(ra_server).\n-define(TAG, {?MODULE, tag}).\n";
    let analysis = analyse("handle(S) -> {?TAG, S}.\n", body, Some(body));
    assert!(analysis.reasons.is_empty());
    assert_eq!(analysis.checks.compared, 1);
}

#[test]
fn no_source_checkout_withholds_each_name_once() {
    let analysis = analyse(
        "handle(S) -> ?SNAPSHOT_INTERVAL + ?SNAPSHOT_INTERVAL + S.\n",
        "-module(ra_server).\n-define(SNAPSHOT_INTERVAL, 4096).\n",
        None,
    );
    assert!(analysis.reasons.is_empty());
    assert_eq!(analysis.checks.withheld_no_source, 1);
}

#[test]
fn a_predefined_macro_is_never_checked() {
    let analysis = analyse(
        "handle(S) -> {?MODULE, S}.\n",
        "-module(ra_server).\n",
        Some("-module(ra_server).\n"),
    );
    assert!(analysis.reasons.is_empty());
    assert!(analysis.checks.is_empty());
}
