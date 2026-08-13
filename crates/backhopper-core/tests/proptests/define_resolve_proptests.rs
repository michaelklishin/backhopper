// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Soundness of the added-text include walk: new includes can only
//! remove findings or withhold, a subject without includes behaves
//! like the target-only walk, and a name any reachable header defines
//! is never flagged.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::define_resolve::{analyse_define_symbols, collect_target_defines};
use backhopper_core::compat::source_attributes::extract_defined_macros;
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::model::names::{CommitSha, GitRef, RelativePath};
use backhopper_core::model::verdict::Reason;

const ERL: &str = "deps/app/src/mod.erl";

const MACROS: [&str; 4] = ["M_A", "M_B", "M_C", "M_D"];

/// Where one generated header lives relative to the walk.
#[derive(Debug, Clone, Copy, PartialEq)]
enum HeaderSide {
    OnTarget,
    PatchAdded,
    Absent,
}

#[derive(Debug, Clone)]
struct Header {
    defines: Vec<&'static str>,
    side: HeaderSide,
}

#[derive(Debug, Clone)]
struct Case {
    headers: Vec<Header>,
    target_file: Option<Vec<&'static str>>,
    includes: Vec<usize>,
    uses: Vec<&'static str>,
}

fn header_path(i: usize) -> String {
    format!("deps/app/src/h{i}.hrl")
}

fn defines_text(defines: &[&str]) -> String {
    defines.iter().fold(String::new(), |mut out, m| {
        let _ = writeln!(out, "-define({m}, 1).");
        out
    })
}

fn added_text(case: &Case) -> String {
    let mut out = String::new();
    for i in &case.includes {
        let _ = writeln!(out, "-include(\"h{i}.hrl\").");
    }
    for m in &case.uses {
        let _ = writeln!(out, "f() -> ?{m}.");
    }
    out
}

fn tree(case: &Case) -> (TargetTreeIndex, BTreeMap<String, String>) {
    let mut files = BTreeMap::new();
    if let Some(defines) = &case.target_file {
        files.insert(ERL.to_owned(), defines_text(defines));
    }
    for (i, h) in case.headers.iter().enumerate() {
        if h.side == HeaderSide::OnTarget {
            files.insert(header_path(i), defines_text(&h.defines));
        }
    }
    let present: BTreeSet<PathBuf> = files.keys().map(PathBuf::from).collect();
    let index = TargetTreeIndex::from_parts(
        PathBuf::from("/repo"),
        GitRef::new("HEAD").unwrap(),
        CommitSha::new("a".repeat(40)).unwrap(),
        present,
    );
    (index, files)
}

fn patch_added_map(case: &Case) -> BTreeMap<RelativePath, String> {
    case.headers
        .iter()
        .enumerate()
        .filter(|(_, h)| h.side == HeaderSide::PatchAdded)
        .map(|(i, h)| {
            (
                RelativePath::new(header_path(i)).unwrap(),
                defines_text(&h.defines),
            )
        })
        .collect()
}

fn run(case: &Case) -> Vec<Reason> {
    let path = RelativePath::new(ERL).unwrap();
    let added = added_text(case);
    let subjects = [AddedLinesSubject {
        source_path: &path,
        added_text: &added,
        line_map: &[],
    }];
    let (index, files) = tree(case);
    let read_target = |p: &RelativePath| files.get(p.as_str()).cloned();
    analyse_define_symbols(&subjects, &patch_added_map(case), &index, &read_target)
}

fn flagged(reasons: &[Reason]) -> BTreeSet<String> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::MacroUndefinedOnTarget { macro_name, .. } => Some(macro_name.clone()),
            _ => None,
        })
        .collect()
}

fn macro_name() -> impl Strategy<Value = &'static str> {
    prop::sample::select(MACROS.to_vec())
}

fn header() -> impl Strategy<Value = Header> {
    (
        prop::collection::vec(macro_name(), 0..3),
        prop::sample::select(vec![
            HeaderSide::OnTarget,
            HeaderSide::PatchAdded,
            HeaderSide::Absent,
        ]),
    )
        .prop_map(|(defines, side)| Header { defines, side })
}

fn case() -> impl Strategy<Value = Case> {
    (
        prop::collection::vec(header(), 1..4),
        prop::option::of(prop::collection::vec(macro_name(), 0..3)),
        prop::collection::vec(macro_name(), 0..4),
    )
        .prop_flat_map(|(headers, target_file, uses)| {
            let n = headers.len();
            (
                Just(headers),
                Just(target_file),
                Just(uses),
                prop::collection::vec(0..n, 0..n),
            )
                .prop_map(|(headers, target_file, uses, includes)| Case {
                    headers,
                    target_file,
                    includes,
                    uses,
                })
        })
}

proptest! {
    // Adding an include directive to the added text never invents a finding.
    #[test]
    fn an_extra_include_never_adds_a_flagged_name(case in case(), extra in 0usize..4) {
        prop_assume!(extra < case.headers.len());
        let base = flagged(&run(&case));
        let mut widened = case.clone();
        widened.includes.push(extra);
        let after = flagged(&run(&widened));
        prop_assert!(after.is_subset(&base), "base {base:?}, after {after:?}");
    }

    // With no include in the added text, the walk reduces to the target-only closure.
    #[test]
    fn a_subject_without_includes_matches_the_target_only_walk(case in case()) {
        let mut case = case;
        case.includes.clear();
        let path = RelativePath::new(ERL).unwrap();
        let added = added_text(&case);
        let subject = AddedLinesSubject {
            source_path: &path,
            added_text: &added,
            line_map: &[],
        };
        let (index, files) = tree(&case);
        let read_target = |p: &RelativePath| files.get(p.as_str()).cloned();
        let defs = collect_target_defines(&subject, &patch_added_map(&case), &index, &read_target);
        // the target-only closure: the generated target file has no includes of its own
        let expected: BTreeSet<String> = files
            .get(ERL)
            .map(|t| extract_defined_macros(t))
            .unwrap_or_default();
        prop_assert_eq!(defs.macros, expected);
        prop_assert!(defs.complete);
        prop_assert!(!defs.stdlib_unread);
    }

    // A name defined in any header the added-text includes reach, target-side or patch-added, is never flagged.
    #[test]
    fn a_reachable_name_is_never_flagged(case in case()) {
        let mut reachable: BTreeSet<&str> = BTreeSet::new();
        for i in &case.includes {
            let h = &case.headers[*i];
            if h.side != HeaderSide::Absent {
                reachable.extend(h.defines.iter().copied());
            }
        }
        let hit = flagged(&run(&case));
        for name in reachable {
            prop_assert!(!hit.contains(name), "reachable {name} flagged: {hit:?}");
        }
    }
}
