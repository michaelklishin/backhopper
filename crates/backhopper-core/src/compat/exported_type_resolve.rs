// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Resolves `-export_type` entries a patch adds against the type
//! declarations the target module reaches. A module cannot export a
//! type it does not define, so an entry naming a type the target
//! declares under another name is an `erlc` error the pick introduces:
//! the case where a rename landed on the source branch and never
//! cascaded.
//!
//! Same-module by construction, so no cross-module gate applies. The
//! resolved set is every `-type`, `-opaque`, and `-nominal` the target
//! version of the file or a header it includes declares, unioned with
//! the patch's own. Following includes is not optional here: unlike
//! functions, types are routinely declared in headers, so reading only
//! the `.erl` would flag every module that exports one.
//!
//! Conservative in the same way as the sibling axes: it withholds when
//! the target file is unreadable, when it declares a `parse_transform`,
//! when a first-party header it includes could not be read, when any
//! form in that closure is a macro-expanded attribute (`rabbit_framing`
//! declares its whole type surface as `-?PROTOCOL_TYPE(t()).`), or when
//! the added `-export_type` list names a macro.
//!
//! A skipped stdlib header does not withhold, unlike the macro and
//! record axes: a module can only export a type declared in its own
//! text, so an OTP header would have to declare a type that a
//! first-party module then re-exports as its own.

use std::collections::BTreeSet;

use crate::compat::added_lines::{AddedLinesSubject, file_line};
use crate::compat::define_resolve::collect_target_defines;
use crate::compat::qualified_call_resolve::TreeReader;
use crate::compat::source_attributes::{
    declares_parse_transform, extract_defined_types, extract_exported_types,
};
use crate::compat::target_tree_index::TargetTreeIndex;
use crate::model::verdict::Reason;

/// Flag each `-export_type` entry a patch adds that resolves to no type
/// declaration the target module reaches and that the patch does not
/// itself declare. One reason per `(file, type, arity)`.
pub fn analyse_exported_types(
    subjects: &[AddedLinesSubject<'_>],
    target: &TargetTreeIndex,
    read_target: TreeReader<'_>,
) -> Vec<Reason> {
    let mut reasons = Vec::new();
    for subject in subjects {
        let added = extract_exported_types(subject.added_text);
        if added.types.is_empty() || !added.complete {
            continue;
        }
        let Some(text) = read_target(subject.source_path) else {
            continue;
        };
        // A parse_transform can inject type declarations.
        if declares_parse_transform(&text) {
            continue;
        }
        let defines = collect_target_defines(subject.source_path, target, read_target);
        // An unread first-party header, or a macro-expanded attribute
        // form, may hold the declaration.
        if !defines.complete || defines.macro_attributes {
            continue;
        }
        let mut defined = defines.types;
        defined.extend(extract_defined_types(subject.added_text));
        let mut flagged = BTreeSet::new();
        for exported in added.types {
            let key = (exported.name, exported.arity);
            if defined.contains(&key) || !flagged.insert(key.clone()) {
                continue;
            }
            reasons.push(Reason::ExportedTypeUndefinedOnTarget {
                source_path: subject.source_path.clone(),
                type_name: key.0,
                arity: key.1,
                line: file_line(subject.line_map, exported.line),
            });
        }
    }
    reasons
}
