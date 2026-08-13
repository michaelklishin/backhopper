// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Soundness of the exported-type axis: it may only flag a type that
//! is in neither the target's declarations nor the patch's.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::exported_type_resolve::analyse_exported_types;
use backhopper_core::compat::source_attributes::{extract_defined_types, extract_exported_types};
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::model::names::{Arity, CommitSha, GitRef, RelativePath, TypeName};
use backhopper_core::model::verdict::Reason;

fn type_name() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "socket",
        "proxy_socket",
        "rabbit_proxy_socket",
        "hostname",
        "ip_port",
        "result",
    ])
    .prop_map(str::to_owned)
}

fn declarations(names: &[(String, u8)]) -> String {
    names.iter().fold(String::new(), |mut out, (n, a)| {
        let args = (0..*a)
            .map(|i| format!("A{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "-type {n}({args}) :: term().");
        out
    })
}

fn export_list(names: &[(String, u8)]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let entries = names
        .iter()
        .map(|(n, a)| format!("{n}/{a}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("-export_type([{entries}]).\n")
}

const ERL: &str = "deps/rabbit_common/src/rabbit_net.erl";

fn run(added: &str, target: &str) -> Vec<Reason> {
    let path = RelativePath::new(ERL.to_owned()).unwrap();
    let line_map: Vec<u32> = (1..=added.lines().count().max(1) as u32).collect();
    let subjects = vec![AddedLinesSubject {
        source_path: &path,
        added_text: added,
        line_map: &line_map,
    }];
    let index = TargetTreeIndex::from_parts(
        PathBuf::from("/repo"),
        GitRef::new("HEAD").unwrap(),
        CommitSha::new("a".repeat(40)).unwrap(),
        [PathBuf::from(ERL)].into_iter().collect(),
    );
    let read_target = |p: &RelativePath| (p.as_str() == ERL).then(|| target.to_owned());
    analyse_exported_types(&subjects, &BTreeMap::new(), &index, &read_target)
}

fn flagged(reasons: &[Reason]) -> BTreeSet<(TypeName, Arity)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::ExportedTypeUndefinedOnTarget {
                type_name, arity, ..
            } => Some((type_name.clone(), *arity)),
            _ => None,
        })
        .collect()
}

prop_compose! {
    fn name_arity_set()(
        names in prop::collection::vec((type_name(), 0u8..3), 0..5)
    ) -> Vec<(String, u8)> {
        let mut seen = BTreeSet::new();
        names.into_iter().filter(|k| seen.insert(k.clone())).collect()
    }
}

proptest! {
    /// The axis's soundness rule: a flagged type is in neither the
    /// target's declarations nor the patch's own.
    #[test]
    fn a_flagged_type_is_never_in_the_union_of_target_and_patch_definitions(
        exported in name_arity_set(),
        target_defined in name_arity_set(),
        patch_defined in name_arity_set(),
    ) {
        let added = format!("{}{}", export_list(&exported), declarations(&patch_defined));
        let target = format!("-module(rabbit_net).\n{}", declarations(&target_defined));
        let mut union = extract_defined_types(&target);
        union.extend(extract_defined_types(&added));
        for key in flagged(&run(&added, &target)) {
            prop_assert!(!union.contains(&key), "flagged a defined type: {key:?}");
        }
    }

    /// Every flagged type was actually exported by the added text.
    #[test]
    fn a_flagged_type_was_exported_by_the_patch(
        exported in name_arity_set(),
        target_defined in name_arity_set(),
    ) {
        let added = export_list(&exported);
        let target = format!("-module(rabbit_net).\n{}", declarations(&target_defined));
        let added_exports: BTreeSet<_> = extract_exported_types(&added)
            .types
            .into_iter()
            .map(|t| (t.name, t.arity))
            .collect();
        for key in flagged(&run(&added, &target)) {
            prop_assert!(added_exports.contains(&key), "flagged an unexported type: {key:?}");
        }
    }

    /// A parse_transform on the target can inject declarations, so the
    /// axis must produce nothing at all.
    #[test]
    fn a_target_parse_transform_never_produces_a_reason(exported in name_arity_set()) {
        let target = "-module(rabbit_net).\n-compile({parse_transform, lager_transform}).\n";
        prop_assert!(run(&export_list(&exported), target).is_empty());
    }

    /// Exporting exactly what the target declares is always clean.
    #[test]
    fn exporting_only_target_declared_types_is_clean(declared in name_arity_set()) {
        let target = format!("-module(rabbit_net).\n{}", declarations(&declared));
        prop_assert!(run(&export_list(&declared), &target).is_empty());
    }

    #[test]
    fn extraction_never_panics_on_arbitrary_attribute_text(text in ".{0,200}") {
        let _ = extract_exported_types(&text);
        let _ = extract_defined_types(&text);
    }
}
