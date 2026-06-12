// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use backhopper_cli::commands::rabbitmq_components::{
    DepPin, DepSource, dep_to_tag, parse_components_mk, parse_pin_line, series_name_for_branch,
    version_to_tag,
};

const SAMPLE: &str = r#"
# RabbitMQ-style dep pins
dep_cowboy = hex 2.14.1
dep_khepri = hex 0.18.0
dep_osiris = git https://github.com/rabbitmq/osiris v1.13.1
dep_ra = hex 3.1.6
dep_jose = hex 1.11.12
dep_some_alias = hex 1.2.3 actual_pkg_name
dep_aux = git_rmq cowboy 2.13.0

# community plugins use a function call we should ignore
dep_rabbitmq_lvc_exchange = $(call community_dep,rabbitmq-lvc-exchange)

# unrelated lines
PROJECT = rabbit
some_other_var = no
"#;

#[test]
fn parses_canonical_hex_and_git_lines() {
    let pins = parse_components_mk(SAMPLE);
    let by_name: BTreeMap<_, _> = pins.iter().map(|p| (p.name.clone(), p)).collect();
    assert_eq!(by_name["cowboy"].source, DepSource::Hex);
    assert_eq!(by_name["cowboy"].version, "2.14.1");
    assert_eq!(by_name["khepri"].version, "0.18.0");
    assert_eq!(by_name["osiris"].source, DepSource::Git);
    assert_eq!(by_name["osiris"].version, "v1.13.1");
    assert_eq!(by_name["aux"].source, DepSource::GitRmq);
    assert_eq!(by_name["aux"].version, "2.13.0");
}

#[test]
fn ignores_function_call_form_and_unrelated_lines() {
    let pins = parse_components_mk(SAMPLE);
    assert!(!pins.iter().any(|p| p.name == "rabbitmq_lvc_exchange"));
    assert!(!pins.iter().any(|p| p.name == "PROJECT"));
}

#[test]
fn keeps_only_first_word_when_hex_has_alternate_pkg() {
    let pins = parse_components_mk("dep_x = hex 1.2.3 alt_name\n");
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].version, "1.2.3");
}

#[test]
fn ignores_comment_lines() {
    let pins = parse_components_mk("# dep_foo = hex 1.0.0\n   # dep_bar = hex 2.0.0\n");
    assert!(pins.is_empty());
}

#[test]
fn series_name_for_branch_strips_version_suffix() {
    assert_eq!(series_name_for_branch("v4.2.x"), "rabbitmq-4.2");
    assert_eq!(series_name_for_branch("v4.3.x"), "rabbitmq-4.3");
    assert_eq!(series_name_for_branch("main"), "rabbitmq-main");
    assert_eq!(series_name_for_branch("master"), "rabbitmq-master");
    assert_eq!(series_name_for_branch("refs/heads/v4.2.x"), "rabbitmq-4.2");
}

#[test]
fn version_to_tag_applies_prefix_only_when_missing() {
    assert_eq!(version_to_tag("3.1.6", "v"), "v3.1.6");
    assert_eq!(version_to_tag("v3.1.6", "v"), "v3.1.6");
    assert_eq!(version_to_tag("2.14.1", ""), "2.14.1");
}

#[test]
fn skips_dep_x_commit_overrides() {
    let pins = parse_components_mk("dep_ra_commit = abcdef1234\n");
    assert!(pins.is_empty());
}

#[test]
fn dep_to_tag_uses_prefix_for_hex_and_verbatim_for_git() {
    let hex = DepPin {
        name: "ra".into(),
        source: DepSource::Hex,
        version: "2.16.7".into(),
    };
    assert_eq!(dep_to_tag(&hex, "v"), "v2.16.7");
    assert_eq!(dep_to_tag(&hex, ""), "2.16.7");

    let git = DepPin {
        name: "osiris".into(),
        source: DepSource::Git,
        version: "v1.10.3".into(),
    };
    assert_eq!(
        dep_to_tag(&git, "v"),
        "v1.10.3",
        "git deps use the literal version regardless of prefix"
    );

    let git_rmq = DepPin {
        name: "cowboy".into(),
        source: DepSource::GitRmq,
        version: "2.13.0".into(),
    };
    assert_eq!(dep_to_tag(&git_rmq, "v"), "2.13.0");
}

#[test]
fn parse_pin_line_accepts_each_source_form() {
    assert_eq!(
        parse_pin_line("dep_ra = hex 3.1.6").unwrap().source,
        DepSource::Hex
    );
    assert_eq!(
        parse_pin_line("dep_osiris = git https://github.com/rabbitmq/osiris v1.13.1")
            .unwrap()
            .version,
        "v1.13.1"
    );
    assert_eq!(
        parse_pin_line("dep_cowboy = git_rmq cowboy 2.13.0")
            .unwrap()
            .version,
        "2.13.0"
    );
}

#[test]
fn parse_pin_line_rejects_comments_and_noise() {
    assert_eq!(parse_pin_line("# dep_ra = hex 3.1.6"), None);
    assert_eq!(parse_pin_line("PROJECT = rabbit"), None);
    assert_eq!(parse_pin_line("dep_ra_commit = abcdef"), None);
    assert_eq!(
        parse_pin_line("dep_lvc = $(call community_dep,rabbitmq-lvc-exchange)"),
        None
    );
}

#[test]
fn display_and_parse_display_round_trip() {
    for pin in parse_components_mk(SAMPLE) {
        let displayed = pin.display();
        let back = DepPin::parse_display(&pin.name, &displayed).unwrap();
        assert_eq!(back, pin);
    }
}

#[test]
fn parse_display_rejects_unknown_source_labels() {
    assert_eq!(DepPin::parse_display("ra", "cargo 1.0.0"), None);
    assert_eq!(DepPin::parse_display("ra", "3.1.6"), None);
}
