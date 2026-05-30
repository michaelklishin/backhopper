// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;
use std::str::FromStr;

use backhopper_core::config::{ProjectFamily, ProjectRaw};
use backhopper_core::model::verdict::{ConflictMarker, FileKind, InapplicableReason, TouchedKinds};

// FileKind classification additions

#[test]
fn classify_recognizes_app_src() {
    assert_eq!(
        TouchedKinds::classify(Path::new("apps/rabbit/src/rabbit.app.src")),
        FileKind::AppSrc,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("src/foo.app.src.script")),
        FileKind::AppSrc,
    );
}

#[test]
fn classify_recognizes_rebar_config() {
    assert_eq!(
        TouchedKinds::classify(Path::new("rebar.config")),
        FileKind::RebarConfig,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("deps/foo/rebar.config")),
        FileKind::RebarConfig,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("rebar3.config")),
        FileKind::RebarConfig,
    );
}

#[test]
fn classify_recognizes_mix_exs_and_mix_lock() {
    assert_eq!(
        TouchedKinds::classify(Path::new("mix.exs")),
        FileKind::MixExs,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("deps/rabbitmq_cli/mix.exs")),
        FileKind::MixExs,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("mix.lock")),
        FileKind::MixExs,
    );
}

#[test]
fn classify_recognizes_ci_workflow() {
    assert_eq!(
        TouchedKinds::classify(Path::new(".github/workflows/ci.yml")),
        FileKind::CiWorkflow,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new(".github/workflows/release.yaml")),
        FileKind::CiWorkflow,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new(".github/actions/checkout/action.yml")),
        FileKind::CiWorkflow,
    );
}

#[test]
fn classify_recognizes_makefile_family() {
    assert_eq!(
        TouchedKinds::classify(Path::new("Makefile")),
        FileKind::Makefile,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("deps/rabbit/Makefile")),
        FileKind::Makefile,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("rabbitmq-components.mk")),
        FileKind::Makefile,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("erlang.mk")),
        FileKind::Makefile,
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("plugins/custom.mk")),
        FileKind::Makefile,
    );
}

#[test]
fn classify_does_not_misidentify_docs_yaml() {
    // a .yml outside .github/workflows/ should not be CiWorkflow
    assert_eq!(
        TouchedKinds::classify(Path::new("config/app.yml")),
        FileKind::Other,
    );
}

// InapplicableReason derivations from new FileKind classes

#[test]
fn only_makefile_touched_promotes_inapplicable() {
    let kinds =
        TouchedKinds::from_paths(vec![Path::new("Makefile"), Path::new("deps/foo/Makefile")]);
    assert_eq!(
        kinds.inapplicable_reason(),
        Some(InapplicableReason::OnlyMakefileTouched)
    );
}

#[test]
fn only_app_src_touched_promotes_inapplicable() {
    let kinds = TouchedKinds::from_paths(vec![Path::new("apps/rabbit/src/rabbit.app.src")]);
    assert_eq!(
        kinds.inapplicable_reason(),
        Some(InapplicableReason::OnlyAppSrcTouched)
    );
}

#[test]
fn only_rebar_config_touched_promotes_inapplicable() {
    let kinds = TouchedKinds::from_paths(vec![Path::new("rebar.config")]);
    assert_eq!(
        kinds.inapplicable_reason(),
        Some(InapplicableReason::OnlyRebarConfigTouched)
    );
}

#[test]
fn only_mix_exs_touched_promotes_inapplicable() {
    let kinds = TouchedKinds::from_paths(vec![Path::new("mix.exs"), Path::new("mix.lock")]);
    assert_eq!(
        kinds.inapplicable_reason(),
        Some(InapplicableReason::OnlyMixExsTouched)
    );
}

#[test]
fn only_ci_workflow_touched_promotes_inapplicable() {
    let kinds = TouchedKinds::from_paths(vec![Path::new(".github/workflows/ci.yml")]);
    assert_eq!(
        kinds.inapplicable_reason(),
        Some(InapplicableReason::OnlyCiWorkflowTouched)
    );
}

#[test]
fn erl_touch_still_blocks_inapplicable_promotion() {
    let kinds = TouchedKinds::from_paths(vec![Path::new("Makefile"), Path::new("src/rabbit.erl")]);
    // .erl in the mix means there IS an analyzable surface
    assert_eq!(kinds.inapplicable_reason(), None);
}

#[test]
fn mixed_categories_fall_back_to_no_erlang_surface() {
    let kinds = TouchedKinds::from_paths(vec![Path::new("Makefile"), Path::new("rebar.config")]);
    assert_eq!(
        kinds.inapplicable_reason(),
        Some(InapplicableReason::NoErlangSurfaceTouched)
    );
}

#[test]
fn inapplicable_reason_as_str_round_trip() {
    let cases = [
        (
            InapplicableReason::OnlyMakefileTouched,
            "only_makefile_touched",
        ),
        (
            InapplicableReason::OnlyMixExsTouched,
            "only_mix_exs_touched",
        ),
        (
            InapplicableReason::OnlyCiWorkflowTouched,
            "only_ci_workflow_touched",
        ),
        (
            InapplicableReason::OnlyAppSrcTouched,
            "only_app_src_touched",
        ),
        (
            InapplicableReason::OnlyRebarConfigTouched,
            "only_rebar_config_touched",
        ),
    ];
    for (variant, text) in cases {
        assert_eq!(variant.as_str(), text);
    }
}

// ConflictMarker detection

#[test]
fn conflict_marker_detects_each_side() {
    assert_eq!(
        ConflictMarker::detect("<<<<<<< HEAD"),
        Some(ConflictMarker::Ours)
    );
    assert_eq!(
        ConflictMarker::detect("======="),
        Some(ConflictMarker::Divider)
    );
    assert_eq!(
        ConflictMarker::detect(">>>>>>> theirs"),
        Some(ConflictMarker::Theirs)
    );
    assert_eq!(
        ConflictMarker::detect("||||||| ancestor"),
        Some(ConflictMarker::Ancestor)
    );
}

#[test]
fn conflict_marker_requires_min_seven_chars() {
    // git's tooling never emits shorter runs
    assert_eq!(ConflictMarker::detect("<<<<<<"), None);
    assert_eq!(ConflictMarker::detect("======"), None);
    assert_eq!(ConflictMarker::detect(">>>>>>"), None);
}

#[test]
fn conflict_marker_rejects_non_separator_suffix() {
    // text immediately after a run of tokens (no whitespace) is NOT a marker
    assert_eq!(ConflictMarker::detect("<<<<<<<X"), None);
    assert_eq!(ConflictMarker::detect(">>>>>>>HEAD"), None);
}

#[test]
fn conflict_marker_accepts_longer_runs() {
    // merge.conflictStyle.markerSize is configurable; runs longer than seven are valid
    assert_eq!(
        ConflictMarker::detect("<<<<<<<< HEAD"),
        Some(ConflictMarker::Ours)
    );
    assert_eq!(
        ConflictMarker::detect("==========="),
        Some(ConflictMarker::Divider)
    );
}

#[test]
fn conflict_marker_accepts_eol_only() {
    assert_eq!(
        ConflictMarker::detect("<<<<<<<"),
        Some(ConflictMarker::Ours)
    );
}

#[test]
fn conflict_marker_rejects_unrelated_lines() {
    assert_eq!(ConflictMarker::detect("    Some(p)"), None);
    assert_eq!(ConflictMarker::detect("normal source line"), None);
    assert_eq!(ConflictMarker::detect("<< some shift"), None);
}

// ProjectFamily

#[test]
fn project_family_defaults_to_generic() {
    let raw = ProjectRaw {
        name: "x".to_owned(),
        git_url: Some("https://example.com/x.git".to_owned()),
        kind: None,
        family: None,
        language: None,
        tag_prefix: None,
        public_modules: None,
        internal_modules: None,
        scan_paths: None,
        layout: None,
        app_roots: None,
        include_apps: None,
        exclude_apps: None,
        excluded_subdirs: None,
        tag_pattern: None,
        min_tag: None,
        exclude_tag_markers: None,
    };
    assert!(raw.family.is_none(), "raw config carries None by default");
    assert_eq!(ProjectFamily::default(), ProjectFamily::Generic);
}

#[test]
fn project_family_round_trips_through_str() {
    let cases = [
        ProjectFamily::Generic,
        ProjectFamily::ErlangOtp,
        ProjectFamily::Ra,
        ProjectFamily::Osiris,
        ProjectFamily::Khepri,
        ProjectFamily::Rabbitmq,
    ];
    for f in cases {
        let s = f.as_str();
        let parsed = ProjectFamily::from_str(s).expect("known family parses");
        assert_eq!(f, parsed);
    }
}

#[test]
fn project_family_rejects_unknown() {
    assert!(ProjectFamily::from_str("rocket").is_err());
    assert!(
        ProjectFamily::from_str("Generic").is_err(),
        "case sensitive"
    );
}
