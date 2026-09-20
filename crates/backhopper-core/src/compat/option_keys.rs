// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! C2: option-map key drift (`OptionKeyUnknownOnPin`). A map key found
//! in added lines fires only when it is a recognized option-type key
//! on the source side and absent from the same type on the target
//! pin. Both-side membership is the entire noise filter.

use std::collections::BTreeSet;

use backhopper_erlang_scan::{
    matching_bracket, quoted_atom_span, skip_char_literal_span, split_top_level_commas, string_span,
};

use crate::config::FamilyDefaults;
use crate::model::names::{MapKey, ModuleName, ProjectName, TagName, TypeName};
use crate::model::snapshot::{Snapshot, state};
use crate::model::symbol::LineClass;

/// The top-level key universe of a family-declared option map type,
/// parsed from its stored `TypeDecl.rhs`. The firing rule below
/// consumes only the `Closed` form: an `Open` or `Unresolved` set on
/// the side being asked "is this key legal here" is unrepresentable
/// as a drifted key, by construction rather than by a runtime flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionKeySet {
    /// A literal map with only literal atom keys.
    Closed(BTreeSet<MapKey>),
    /// A literal map with at least one wildcard key entry (`atom()`,
    /// `term()`, `_`). The literal keys found alongside the wildcard
    /// are still recorded, for display.
    Open(BTreeSet<MapKey>),
    /// The right-hand side is not a literal map at the top level (a
    /// union, or a reference to another type).
    Unresolved,
}

impl OptionKeySet {
    /// The literal keys this set carries, for either closed or open
    /// forms; empty for `Unresolved`.
    #[must_use]
    pub fn literal_keys(&self) -> &BTreeSet<MapKey> {
        static EMPTY: BTreeSet<MapKey> = BTreeSet::new();
        match self {
            Self::Closed(keys) | Self::Open(keys) => keys,
            Self::Unresolved => &EMPTY,
        }
    }
}

/// Parse a `TypeDecl.rhs` string into its top-level key universe. The
/// whole trimmed text must be one `#{ ... }` map literal; a union or
/// an indirection to another type name leaves the set `Unresolved`.
///
/// The closing brace and the top-level comma split both reuse
/// `backhopper-erlang-scan`'s bracket and comma scanners rather than a
/// bespoke byte walk here, so a line comment or a same-kind-only
/// bracket match inside the map literal is handled the same way every
/// other parser in this codebase handles it.
#[must_use]
pub fn parse_option_key_set(rhs: &str) -> OptionKeySet {
    let trimmed = rhs.trim();
    if !trimmed.starts_with("#{") {
        return OptionKeySet::Unresolved;
    }
    let bytes = trimmed.as_bytes();
    let Some(close) = matching_bracket(bytes, 1) else {
        return OptionKeySet::Unresolved;
    };
    // Anything after the matching `}` (besides trailing whitespace or
    // a bare `.`) means the map is not the entire top-level type: a
    // union member, or trailing syntax this parser does not model.
    let after = trimmed[close + 1..].trim().trim_end_matches('.').trim();
    if !after.is_empty() {
        return OptionKeySet::Unresolved;
    }
    let inner = &trimmed[2..close];
    let mut keys = BTreeSet::new();
    let mut open = false;
    for entry in split_top_level_commas(inner) {
        let Some(key_text) = key_part(entry) else {
            open = true;
            continue;
        };
        match literal_atom_key(key_text.trim()) {
            Some(atom) => {
                if let Ok(k) = MapKey::new(atom) {
                    keys.insert(k);
                }
            }
            None => open = true,
        }
    }
    if open {
        OptionKeySet::Open(keys)
    } else {
        OptionKeySet::Closed(keys)
    }
}

/// The key half of one `key := value` or `key => value` map-type
/// entry, split at the first top-level `:=` or `=>`. `None` when
/// neither operator appears at depth zero: a malformed or unsupported
/// entry shape, treated as a wildcard (open) rather than guessed.
fn key_part(entry: &str) -> Option<&str> {
    let bytes = entry.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'"' | b'~' => {
                i += string_span(bytes, i).unwrap_or(1);
                continue;
            }
            b'\'' => {
                i += quoted_atom_span(bytes, i).unwrap_or(1);
                continue;
            }
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b':' if depth == 0 && bytes[i + 1] == b'=' => return Some(&entry[..i]),
            b'=' if depth == 0 && bytes[i + 1] == b'>' => return Some(&entry[..i]),
            _ => {}
        }
        i += 1;
    }
    None
}

/// A literal atom key: a bare identifier atom, or a quoted atom.
/// Anything else (`atom()`, `term()`, `_`, a variable) is not a
/// literal key.
fn literal_atom_key(key_text: &str) -> Option<String> {
    if let Some(rest) = key_text.strip_prefix('\'') {
        let end = rest.find('\'')?;
        return Some(rest[..end].to_owned());
    }
    let mut chars = key_text.chars();
    let first = chars.next()?;
    if !first.is_ascii_lowercase() {
        return None;
    }
    if !key_text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
    {
        return None;
    }
    Some(key_text.to_owned())
}

/// Every map key found at a key position (`atom =>` or `atom :=`) in
/// `added_text`'s non-attribute lines. Quoted atoms count as literal
/// keys. Lines classified as an attribute region (`-spec`, `-type`,
/// `-opaque`, `-callback`, and every other attribute) are skipped: a
/// mirrored option type in a carried spec must not false-positive.
#[must_use]
pub fn scan_added_map_keys(added_text: &str, line_ctx: &[LineClass]) -> BTreeSet<MapKey> {
    let mut keys = BTreeSet::new();
    for (line, ctx) in added_text.lines().zip(line_ctx.iter()) {
        if ctx.context.is_attribute() {
            continue;
        }
        scan_line_for_map_keys(line, &mut keys);
    }
    keys
}

fn scan_line_for_map_keys(line: &str, keys: &mut BTreeSet<MapKey>) {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => break,
            b'"' | b'~' => i += string_span(bytes, i).unwrap_or(1),
            b'\'' => {
                let span = quoted_atom_span(bytes, i).unwrap_or(1);
                if span >= 2 {
                    let atom_text = &line[i + 1..i + span - 1];
                    if let Some(consumed) = arrow_after(bytes, i + span)
                        && let Ok(k) = MapKey::new(atom_text.to_owned())
                    {
                        keys.insert(k);
                        i += span + consumed;
                        continue;
                    }
                }
                i += span;
            }
            b'$' => i += skip_char_literal_span(bytes, i),
            b'a'..=b'z' => {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'@')
                {
                    i += 1;
                }
                let atom_text = &line[start..i];
                if let Some(consumed) = arrow_after(bytes, i)
                    && let Ok(k) = MapKey::new(atom_text.to_owned())
                {
                    keys.insert(k);
                    i += consumed;
                }
            }
            _ => i += 1,
        }
    }
}

/// If `bytes[i..]` is optional whitespace followed by `=>` or `:=`,
/// the number of bytes to skip to land just past the operator.
fn arrow_after(bytes: &[u8], mut i: usize) -> Option<usize> {
    let start = i;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i + 1 < bytes.len()
        && ((bytes[i] == b':' && bytes[i + 1] == b'=')
            || (bytes[i] == b'=' && bytes[i + 1] == b'>'))
    {
        Some(i + 2 - start)
    } else {
        None
    }
}

/// Per-`(project, tag, module, type)` memoized key set: a 60-pair
/// batch parses each declared type's `rhs` once. Callers own the map
/// across a run; this module only defines the key shape.
pub type OptionKeySetCache =
    std::collections::BTreeMap<(ProjectName, TagName, ModuleName, TypeName), OptionKeySet>;

/// The declared type's key set at `snapshot`'s tag, memoized in
/// `cache` under `(project, tag, module, type)`.
pub fn cached_option_key_set<'a>(
    cache: &'a mut OptionKeySetCache,
    snapshot: &Snapshot<state::Canonical>,
    module: &ModuleName,
    type_name: &TypeName,
) -> Option<&'a OptionKeySet> {
    let key = (
        snapshot.header().project.clone(),
        snapshot.header().tag.clone(),
        module.clone(),
        type_name.clone(),
    );
    if !cache.contains_key(&key) {
        let rhs = snapshot
            .module_named(module)?
            .types
            .iter()
            .find(|t| &t.name == type_name)
            .map(|t| t.rhs.as_str())?;
        cache.insert(key.clone(), parse_option_key_set(rhs));
    }
    cache.get(&key)
}

/// `drifted_keys(T) = keys(source_pin, T) \ keys(target_pin, T)`,
/// defined only for `(Closed | Open, Closed)` source-target pairs: an
/// open target set suppresses the whole type, by match arm rather
/// than by flag.
#[must_use]
pub fn drifted_keys(source: &OptionKeySet, target: &OptionKeySet) -> BTreeSet<MapKey> {
    match target {
        OptionKeySet::Closed(target_keys) => source
            .literal_keys()
            .iter()
            .filter(|k| !target_keys.contains(*k))
            .cloned()
            .collect(),
        OptionKeySet::Open(_) | OptionKeySet::Unresolved => BTreeSet::new(),
    }
}

/// The family-declared option types with both-side `Closed`-eligible
/// drift sets, i.e. every `(module, type_name, drifted_keys)` worth
/// scanning the patch for.
pub fn drifted_key_sets(
    defaults: &FamilyDefaults,
    target: &Snapshot<state::Canonical>,
    source: &Snapshot<state::Canonical>,
    cache: &mut OptionKeySetCache,
) -> Vec<(ModuleName, TypeName, BTreeSet<MapKey>)> {
    let mut out = Vec::new();
    for decl in &defaults.option_types {
        let Some(source_set) =
            cached_option_key_set(cache, source, &decl.module, &decl.type_name).cloned()
        else {
            continue;
        };
        let Some(target_set) =
            cached_option_key_set(cache, target, &decl.module, &decl.type_name).cloned()
        else {
            continue;
        };
        let drifted = drifted_keys(&source_set, &target_set);
        if !drifted.is_empty() {
            out.push((decl.module.clone(), decl.type_name.clone(), drifted));
        }
    }
    out
}
