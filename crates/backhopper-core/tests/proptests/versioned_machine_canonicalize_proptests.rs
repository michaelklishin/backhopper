// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use proptest::prelude::*;
use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, DependencyName, DependencyVersion, FunctionName, MacroName, ModuleName,
    ProjectName, TagName,
};
use backhopper_core::model::snapshot::{
    Module, Provenance, Snapshot, SnapshotHeader, VendoredDep, VendoredDepSource,
    VersionedMachineVersion, WireConstantBinding, WireValue, state,
};
use backhopper_core::snapshot::{format, parser};

fn arb_macro_name() -> impl Strategy<Value = MacroName> {
    "[A-Z][A-Z0-9_]{0,15}".prop_map(|s| MacroName::from_str(&s).unwrap())
}

fn arb_module_name() -> impl Strategy<Value = ModuleName> {
    "[a-z][a-z0-9_]{0,15}".prop_map(|s| ModuleName::from_str(&s).unwrap())
}

fn arb_dep_name() -> impl Strategy<Value = DependencyName> {
    "[a-z][a-z0-9_]{0,15}".prop_map(|s| DependencyName::from_str(&s).unwrap())
}

fn arb_dep_version() -> impl Strategy<Value = DependencyVersion> {
    "[0-9]\\.[0-9]\\.[0-9]".prop_map(|s| DependencyVersion::from_str(&s).unwrap())
}

fn arb_wire_value() -> impl Strategy<Value = WireValue> {
    prop_oneof![
        any::<u64>().prop_map(WireValue::U64),
        "[A-Z]{1,4}".prop_map(|s| WireValue::Bytes(s.into_bytes())),
        "[a-z][a-z0-9_]{0,8}".prop_map(WireValue::Opaque),
    ]
}

fn arb_provenance() -> impl Strategy<Value = Provenance> {
    prop_oneof![
        Just(Provenance::Literal),
        (
            arb_macro_name(),
            prop::option::of("[a-z]{1,4}/[a-z]{1,4}\\.hrl")
        )
            .prop_map(|(macro_name, defined_in)| Provenance::MacroBody {
                macro_name,
                defined_in,
            },),
    ]
}

fn arb_wire_constant() -> impl Strategy<Value = WireConstantBinding> {
    (
        arb_macro_name(),
        arb_wire_value(),
        prop::option::of("[a-z]{1,4}/[a-z]{1,4}\\.erl"),
    )
        .prop_map(|(macro_name, value, defined_in)| WireConstantBinding {
            macro_name,
            value,
            defined_in,
        })
}

fn arb_versioned_machine() -> impl Strategy<Value = VersionedMachineVersion> {
    (any::<Option<u64>>(), arb_provenance()).prop_map(|(value, provenance)| {
        VersionedMachineVersion {
            function: FunctionName::from_str("version").unwrap(),
            arity: Arity::new(0),
            value,
            provenance,
        }
    })
}

fn arb_dep_pin() -> impl Strategy<Value = VendoredDep> {
    (
        arb_dep_name(),
        arb_dep_version(),
        prop_oneof![
            Just(VendoredDepSource::Hex),
            Just(VendoredDepSource::Git),
            Just(VendoredDepSource::GitRmq),
        ],
    )
        .prop_map(|(name, version, source)| VendoredDep {
            name,
            version,
            source,
        })
}

fn arb_module() -> impl Strategy<Value = Module> {
    (
        arb_module_name(),
        prop::option::of(arb_versioned_machine()),
        prop::collection::vec(arb_wire_constant(), 0..4),
    )
        .prop_map(|(name, vmv, wcs)| {
            let mut m = Module::new(name);
            m.versioned_machine_version = vmv;
            m.wire_constants = wcs;
            m
        })
}

fn arb_header() -> impl Strategy<Value = SnapshotHeader> {
    prop::collection::vec(arb_dep_pin(), 0..4).prop_map(|pins| SnapshotHeader {
        project: ProjectName::new("p").unwrap(),
        tag: TagName::new("v0.1.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "backhopper test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
        dep_pins: pins,
    })
}

fn arb_snapshot() -> impl Strategy<Value = Snapshot<state::Canonical>> {
    (arb_header(), prop::collection::vec(arb_module(), 0..4)).prop_map(|(header, modules)| {
        Snapshot::from_extracted(header, modules, vec![]).into_canonical()
    })
}

proptest! {
    #[test]
    fn snapshot_round_trips_through_writer_and_parser_with_new_fields(
        snap in arb_snapshot()
    ) {
        let text = format::to_string(&snap).unwrap();
        let back = parser::parse(&text).unwrap();
        prop_assert_eq!(snap, back);
    }

    #[test]
    fn snapshot_text_is_byte_stable_after_one_round_trip(
        snap in arb_snapshot()
    ) {
        let text = format::to_string(&snap).unwrap();
        let back = parser::parse(&text).unwrap();
        let text2 = format::to_string(&back).unwrap();
        prop_assert_eq!(text, text2);
    }

    #[test]
    fn into_canonical_is_idempotent_for_wire_constants(
        mut wcs in prop::collection::vec(arb_wire_constant(), 0..8)
    ) {
        let mut m = Module::new(ModuleName::new("ra").unwrap());
        m.wire_constants = wcs.clone();
        let snap1 = Snapshot::from_extracted(
            SnapshotHeader {
                project: ProjectName::new("p").unwrap(),
                tag: TagName::new("v0.1.0").unwrap(),
                branch: None,
                commit: CommitSha::new("0".repeat(40)).unwrap(),
                scanned_paths: vec!["src".into()],
                apps_scanned: Vec::new(),
                generated_by: "test".into(),
                generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
                extractor_version: String::new(),
                dep_pins: Vec::new(),
            },
            vec![m.clone()],
            vec![],
        )
        .into_canonical();
        wcs.sort_by(|a, b| a.macro_name.cmp(&b.macro_name));
        wcs.dedup_by(|a, b| a.macro_name == b.macro_name);
        let stored = &snap1.modules()[0].wire_constants;
        prop_assert_eq!(stored.clone(), wcs);
    }

    #[test]
    fn into_canonical_sorts_dep_pins_by_name_and_dedups(
        pins in prop::collection::vec(arb_dep_pin(), 0..6)
    ) {
        let header = SnapshotHeader {
            project: ProjectName::new("p").unwrap(),
            tag: TagName::new("v0.1.0").unwrap(),
            branch: None,
            commit: CommitSha::new("0".repeat(40)).unwrap(),
            scanned_paths: vec!["src".into()],
            apps_scanned: Vec::new(),
            generated_by: "test".into(),
            generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
            extractor_version: String::new(),
            dep_pins: pins.clone(),
        };
        let snap = Snapshot::from_extracted(header, vec![], vec![]).into_canonical();
        let kept = &snap.header().dep_pins;
        for w in kept.windows(2) {
            prop_assert!(w[0].name < w[1].name);
        }
    }
}
