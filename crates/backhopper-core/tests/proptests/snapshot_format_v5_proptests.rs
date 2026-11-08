// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;

use proptest::prelude::*;
use time::OffsetDateTime;

use backhopper_core::model::app_manifest::{
    AppManifest, ManifestDeps, ManifestSource, ManifestUnreadable,
};
use backhopper_core::model::macro_decl::{MacroBody, MacroDecl, MacroKey};
use backhopper_core::model::names::{
    ApplicationName, Arity, CommitSha, ConfKey, ExtractionSettingsDigest, FieldName,
    HostFactsDigest, MacroName, MakeVariableName, ModuleName, PartialName, ProjectName, RecordName,
    RelativePath, TagName, ValidatorName,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::normalised::{NormalisedDefault, NormalisedMacroBody};
use backhopper_core::model::schema_files::{PartialFile, SchemaFile};
use backhopper_core::model::snapshot::{
    HeaderRef, HostReleaseFacts, HrlFile, Module, RecordDecl, RecordField, Snapshot,
    SnapshotHeader, state,
};
use backhopper_core::snapshot::{format, parser};

fn arb_atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,6}"
}

fn arb_app() -> impl Strategy<Value = ApplicationName> {
    arb_atom().prop_map(|s| ApplicationName::new(s).unwrap())
}

fn arb_path(ext: &'static str) -> impl Strategy<Value = RelativePath> {
    ("[a-z]{1,4}", "[a-z]{1,4}")
        .prop_map(move |(dir, file)| RelativePath::new(format!("{dir}/{file}.{ext}")).unwrap())
}

fn arb_manifest() -> impl Strategy<Value = AppManifest> {
    let source = prop_oneof![Just(ManifestSource::AppSrc), Just(ManifestSource::ErlangMk)];
    let listed = prop::collection::btree_set(arb_app(), 0..4).prop_map(ManifestDeps::Listed);
    let unreadable = prop_oneof![
        Just(ManifestUnreadable::Missing),
        Just(ManifestUnreadable::ParseFailed),
        "[A-Z][A-Z_]{0,6}".prop_map(|v| ManifestUnreadable::UnexpandedVariable(
            MakeVariableName::new(v).unwrap()
        )),
    ]
    .prop_map(ManifestDeps::Unreadable);
    (arb_app(), source, prop_oneof![listed, unreadable]).prop_map(|(name, source, requires)| {
        AppManifest {
            name,
            source,
            requires,
        }
    })
}

fn arb_default() -> impl Strategy<Value = Option<NormalisedDefault>> {
    let texts = prop_oneof![
        Just("false"),
        Just("[]"),
        Just("#{max_length => 100,\n   overflow => drop_head}"),
        Just("<<\"\">>"),
        Just("{ok, 1}"),
        Just("\"a = b\""),
    ];
    prop::option::of(texts.prop_map(|t| NormalisedDefault::from_source(t).unwrap()))
}

fn arb_field() -> impl Strategy<Value = RecordField> {
    let type_repr = prop::option::of(
        prop::collection::vec("[a-z][a-z0-9_]*\\(\\)", 1..4)
            .prop_map(|parts| parts.join(" |\n      ")),
    );
    (
        arb_atom().prop_map(|s| FieldName::new(s).unwrap()),
        type_repr,
        arb_default(),
    )
        .prop_map(|(name, type_repr, default)| RecordField {
            name,
            type_repr,
            default,
        })
}

fn arb_record() -> impl Strategy<Value = RecordDecl> {
    (
        arb_atom().prop_map(|s| RecordName::new(s).unwrap()),
        prop::collection::vec(arb_field(), 0..4),
    )
        .prop_map(|(name, fields)| RecordDecl { name, fields })
}

fn arb_define() -> impl Strategy<Value = MacroDecl> {
    let key = (
        "[A-Z][A-Z_]{0,8}".prop_map(|s| MacroName::new(s).unwrap()),
        prop::option::of((0u8..4).prop_map(Arity::new)),
    )
        .prop_map(|(name, arity)| MacroKey { name, arity });
    let body = prop_oneof![
        Just(MacroBody::Conditional),
        prop_oneof![
            Just("1800000"),
            Just("is_record(Q, amqqueue)"),
            Just(""),
            Just("{a, b}"),
            Just("\"100% done\""),
        ]
        .prop_map(|t| MacroBody::Text(NormalisedMacroBody::from_source(t))),
    ];
    (key, body).prop_map(|(key, body)| MacroDecl { key, body })
}

fn arb_header_file() -> impl Strategy<Value = HrlFile> {
    let include =
        (arb_app(), arb_path("hrl")).prop_map(|(app, app_path)| HeaderRef { app, app_path });
    (
        arb_path("hrl"),
        arb_app(),
        prop::collection::vec(include, 0..3),
        prop::collection::vec(arb_record(), 0..3),
        prop::collection::vec(arb_define(), 0..4),
    )
        .prop_map(|(path, app, includes, records, defines)| {
            let mut hrl = HrlFile::new(path);
            hrl.app = Some(app);
            hrl.includes = includes;
            hrl.records = records;
            hrl.defines = defines;
            hrl
        })
}

fn arb_module() -> impl Strategy<Value = Module> {
    (arb_atom(), arb_app(), arb_path("erl")).prop_map(|(name, app, path)| {
        let mut m = Module::new(ModuleName::new(name).unwrap());
        m.app = Some(app);
        m.path = Some(path);
        m
    })
}

fn arb_conf_keys() -> impl Strategy<Value = BTreeSet<ConfKey>> {
    prop::collection::btree_set(
        "[a-z]{1,4}(\\.[a-z$]{1,4}){0,2}".prop_map(|k| ConfKey::new(k).unwrap()),
        0..4,
    )
}

fn arb_validators() -> impl Strategy<Value = BTreeSet<ValidatorName>> {
    prop::collection::btree_set(
        "[a-z][a-z_]{0,8}".prop_map(|v| ValidatorName::new(v).unwrap()),
        0..3,
    )
}

fn arb_schema() -> impl Strategy<Value = SchemaFile> {
    (
        arb_path("schema"),
        arb_app(),
        arb_conf_keys(),
        arb_validators(),
    )
        .prop_map(|(path, app, mappings, validators)| SchemaFile {
            path,
            app,
            mappings,
            validators,
        })
}

fn arb_partial() -> impl Strategy<Value = PartialFile> {
    (
        arb_path("partial"),
        arb_app(),
        "[a-z][a-z_]{0,8}".prop_map(|n| PartialName::new(n).unwrap()),
        arb_conf_keys(),
        arb_validators(),
    )
        .prop_map(|(path, app, name, mappings, validators)| PartialFile {
            path,
            app,
            name,
            mappings,
            validators,
        })
}

fn arb_host_facts() -> impl Strategy<Value = Option<HostReleaseFacts>> {
    prop::option::of(
        (prop::collection::vec(arb_app(), 1..4), "[0-9a-f]{64}").prop_map(|(roots, digest)| {
            let mut roots = roots.into_iter();
            let first = roots.next().expect("one root at least");
            HostReleaseFacts {
                release_roots: NonEmpty::new(first, roots),
                digest: HostFactsDigest::new(digest).unwrap(),
            }
        }),
    )
}

fn arb_header() -> impl Strategy<Value = SnapshotHeader> {
    (
        arb_host_facts(),
        prop::option::of("[0-9a-f]{64}"),
        "[a-z][a-z0-9_]{0,5}",
        "v[0-9]\\.[0-9]\\.[0-9]",
    )
        .prop_map(|(host_facts, settings, project, tag)| SnapshotHeader {
            extraction_settings: settings
                .map(|digest| ExtractionSettingsDigest::new(digest).unwrap()),
            project: ProjectName::new(project).unwrap(),
            tag: TagName::new(tag).unwrap(),
            branch: None,
            commit: CommitSha::new("0".repeat(40)).unwrap(),
            scanned_paths: vec!["deps/*".into()],
            apps_scanned: Vec::new(),
            generated_by: "backhopper test".into(),
            generated_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
            extractor_version: "9".into(),
            format_version: 5,
            dep_pins: Vec::new(),
            host_facts,
        })
}

fn arb_v5_snapshot() -> impl Strategy<Value = Snapshot<state::Canonical>> {
    (
        arb_header(),
        prop::collection::vec(arb_manifest(), 0..4),
        prop::collection::vec(arb_module(), 0..4),
        prop::collection::vec(arb_header_file(), 0..4),
        prop::collection::vec(arb_schema(), 0..3),
        prop::collection::vec(arb_partial(), 0..3),
    )
        .prop_map(|(header, apps, modules, headers, schemas, partials)| {
            Snapshot::from_extracted(header, modules, headers)
                .with_apps(apps)
                .with_cuttlefish(schemas, partials)
                .into_canonical()
        })
}

const ORDERED_ENTRIES: [&str; 6] = [
    "  requires ",
    "  mapping ",
    "  validator ",
    "  include ",
    "  define ",
    "# release-root: ",
];

fn swap_adjacent_entries(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let at = lines.windows(2).position(|pair| {
        pair[0] != pair[1]
            && ORDERED_ENTRIES
                .iter()
                .any(|kw| pair[0].starts_with(kw) && pair[1].starts_with(kw))
    })?;
    let mut swapped = lines.clone();
    swapped.swap(at, at + 1);
    Some(swapped.join("\n") + "\n")
}

fn swap_adjacent_sections(text: &str) -> Option<String> {
    let blocks: Vec<&str> = text.split("\n\n").collect();
    let keyword = |block: &str| block.split_whitespace().next().map(str::to_owned);
    let at = (1..blocks.len().saturating_sub(1)).find(|&i| {
        blocks[i] != blocks[i + 1]
            && keyword(blocks[i]).is_some_and(|k| Some(k) == keyword(blocks[i + 1]))
    })?;
    let mut swapped = blocks.clone();
    swapped.swap(at, at + 1);
    Some(swapped.join("\n\n"))
}

fn any_whitespace_or_comment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(" ".to_owned()),
        "[ \t]{1,4}".prop_map(String::from),
        "\n[ ]{0,6}".prop_map(String::from),
        "[a-z ]{0,8}".prop_map(|c| format!(" %% {c}\n")),
    ]
}

fn arb_token() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("#{max_length"),
        Just("=>"),
        Just("100,"),
        Just("\"a  b\""),
        Just("'quoted atom'"),
        Just("overflow"),
        Just("drop_head}"),
        Just("fun(X)"),
        Just("->"),
        Just("X + 1"),
        Just("$%"),
        Just("[1, 2]"),
    ]
}

proptest! {
    #[test]
    fn any_format_5_snapshot_writes_and_parses_to_an_equal_value(snap in arb_v5_snapshot()) {
        let text = format::to_string(&snap).unwrap();
        let back = parser::parse(&text).unwrap();
        prop_assert_eq!(&snap, &back);
        prop_assert_eq!(format::to_string(&back).unwrap(), text);
    }

    #[test]
    fn any_format_5_snapshot_round_trips_through_json(snap in arb_v5_snapshot()) {
        let json = serde_json::to_string(&snap).unwrap();
        let back: Snapshot<state::Canonical> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(snap, back);
    }

    #[test]
    fn reordering_two_adjacent_entries_is_refused(snap in arb_v5_snapshot()) {
        let text = format::to_string(&snap).unwrap();
        prop_assume!(swap_adjacent_entries(&text).is_some());
        let swapped = swap_adjacent_entries(&text).unwrap();
        prop_assert!(parser::parse(&swapped).is_err(), "accepted:\n{}", swapped);
    }

    #[test]
    fn reordering_two_adjacent_sections_of_one_kind_is_refused(snap in arb_v5_snapshot()) {
        let text = format::to_string(&snap).unwrap();
        prop_assume!(swap_adjacent_sections(&text).is_some());
        let swapped = swap_adjacent_sections(&text).unwrap();
        prop_assert!(parser::parse(&swapped).is_err(), "accepted:\n{}", swapped);
    }

    #[test]
    fn a_macro_body_normalises_to_the_same_text_after_whitespace_and_comments_are_inserted(
        tokens in prop::collection::vec(arb_token(), 1..8),
        separators in prop::collection::vec(any_whitespace_or_comment(), 8),
        lead in any_whitespace_or_comment(),
        trail in any_whitespace_or_comment(),
    ) {
        let plain = tokens.join(" ");
        let mut noisy = lead;
        for (token, separator) in tokens.iter().zip(&separators) {
            noisy.push_str(token);
            noisy.push_str(separator);
        }
        noisy.push_str(&trail);
        prop_assert_eq!(
            NormalisedMacroBody::from_source(&plain),
            NormalisedMacroBody::from_source(&noisy)
        );
    }

    #[test]
    fn normalising_a_macro_body_twice_changes_nothing(
        tokens in prop::collection::vec(arb_token(), 0..8),
        separators in prop::collection::vec(any_whitespace_or_comment(), 8),
    ) {
        let mut source = String::new();
        for (token, separator) in tokens.iter().zip(&separators) {
            source.push_str(token);
            source.push_str(separator);
        }
        let once = NormalisedMacroBody::from_source(&source);
        prop_assert_eq!(NormalisedMacroBody::from_source(once.as_str()), once.clone());
        prop_assert_eq!(NormalisedMacroBody::from_stored(once.as_str()).unwrap(), once);
    }
}
