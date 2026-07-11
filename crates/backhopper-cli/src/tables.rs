// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Renderers for text-mode output: every table takes a `bel7_cli::TableStyle`
//! so the global `--table-style` flag governs all tables uniformly.

use bel7_cli::TableStyle;
use tabled::{Table, Tabled};

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolKind;
use backhopper_core::model::verdict::{
    ArtifactKind, InapplicableReason, PinVerdict, Reason, SeriesEvaluation, Verdict,
};

#[derive(Tabled)]
struct ReasonRow {
    pin: String,
    verdict: &'static str,
    reason: &'static str,
    detail: String,
}

/// Build a styled `tabled::Table`: one place owns table construction so
/// every renderer applies the global `--table-style` identically.
pub fn styled_table<I, R>(rows: I, style: TableStyle) -> Table
where
    I: IntoIterator<Item = R>,
    R: Tabled,
{
    let mut table = Table::new(rows);
    style.apply(&mut table);
    table
}

pub fn render_evaluation_table(evaluation: &SeriesEvaluation, style: TableStyle) -> String {
    styled_table(collect_rows(&evaluation.verdict.results), style).to_string()
}

fn collect_rows(results: &[PinVerdict]) -> Vec<ReasonRow> {
    let mut rows: Vec<ReasonRow> = Vec::new();
    for r in results {
        let pin = format_pin(&r.pin);
        let verdict_label = verdict_label(&r.verdict);
        if let Verdict::Inapplicable { reason } = &r.verdict {
            rows.push(ReasonRow {
                pin,
                verdict: verdict_label,
                reason: "-",
                detail: format!("inapplicable: {}", inapplicable_detail(reason)),
            });
            continue;
        }
        if r.verdict.reasons().is_empty() {
            let symbols = r.tracked_refs;
            rows.push(ReasonRow {
                pin,
                verdict: verdict_label,
                reason: "-",
                detail: format!(
                    "{symbols} tracked symbol{} referenced",
                    if symbols == 1 { "" } else { "s" }
                ),
            });
            continue;
        }
        for reason in r.verdict.reasons() {
            rows.push(ReasonRow {
                pin: pin.clone(),
                verdict: verdict_label,
                reason: reason_kind(reason),
                detail: reason_detail(reason),
            });
        }
    }
    rows
}

fn inapplicable_detail(reason: &InapplicableReason) -> String {
    match reason {
        InapplicableReason::OutOfScopeFor { project } => {
            format!("out_of_scope_for (owned by project '{project}')")
        }
        InapplicableReason::Untracked => {
            "untracked (no configured project owns the touched paths)".to_owned()
        }
        InapplicableReason::PathsMissingOnTarget { paths } => {
            let mut sample: Vec<String> = paths.iter().take(3).map(|p| p.to_string()).collect();
            if paths.len() > 3 {
                sample.push(format!("… +{} more", paths.len() - 3));
            }
            format!(
                "paths_missing_on_target ({} path(s): {})",
                paths.len(),
                sample.join(", ")
            )
        }
        other => other.as_str().to_owned(),
    }
}

fn verdict_label(v: &Verdict) -> &'static str {
    match v {
        Verdict::Compatible => "Compatible",
        Verdict::RequiresAdaptation { .. } => "RequiresAdaptation",
        Verdict::Incompatible { .. } => "Incompatible",
        Verdict::Inapplicable { .. } => "Inapplicable",
    }
}

fn format_pin(pin: &Pin) -> String {
    format!("{}@{}", pin.project, pin.tag)
}

fn reason_kind(r: &Reason) -> &'static str {
    match r {
        Reason::MissingSymbol { .. } => "MissingSymbol",
        Reason::ArityChanged { .. } => "ArityChanged",
        Reason::SignatureChanged { .. } => "SignatureChanged",
        Reason::FileAbsent { .. } => "FileAbsent",
        Reason::ContextDrift { .. } => "ContextDrift",
        Reason::DeprecatedUsage { .. } => "Deprecated",
        Reason::NowHidden { .. } => "NowHidden",
        Reason::RecordFieldsChanged { .. } => "RecordFields",
        Reason::UnsupportedFileType { .. } => "UnsupportedFileType",
        Reason::UntrackedModuleMissing { .. } => "UntrackedModuleMissing",
        Reason::ClauseMismatch { .. } => "ClauseMismatch",
        Reason::MissingPrereq { .. } => "MissingPrereq",
        Reason::SyntacticArtifact { .. } => "SyntacticArtifact",
        Reason::BehaviourCallbackSignatureChanged { .. } => "BehaviourCallbackSignatureChanged",
        Reason::BehaviourCallbackRemoved { .. } => "BehaviourCallbackRemoved",
        Reason::BehaviourCallbackAdded { .. } => "BehaviourCallbackAdded",
        Reason::ModuleRelocated { .. } => "ModuleRelocated",
        Reason::WireConstantChanged { .. } => "WireConstantChanged",
        Reason::HistoricalImplementationMissing { .. } => "HistoricalImplementationMissing",
        Reason::WireContractBodyDrift { .. } => "WireContractBodyDrift",
        Reason::WireContractRegression { .. } => "WireContractRegression",
        Reason::ReturnShapeMismatch { .. } => "ReturnShapeMismatch",
        Reason::MissingType { .. } => "MissingType",
        Reason::PreimageDrifted { .. } => "PreimageDrifted",
        Reason::PreimageMissing { .. } => "PreimageMissing",
        Reason::PathRename { .. } => "PathRename",
        Reason::TargetPathAbsent { .. } => "TargetPathAbsent",
        Reason::MacroUndefinedOnTarget { .. } => "MacroUndefinedOnTarget",
        Reason::RecordUndefinedOnTarget { .. } => "RecordUndefinedOnTarget",
        Reason::LocalCallUndefinedOnTarget { .. } => "LocalCallUndefinedOnTarget",
        Reason::QualifiedCallUndefinedOnTarget { .. } => "QualifiedCallUndefinedOnTarget",
        Reason::IndirectCallUndefinedOnTarget { .. } => "IndirectCallUndefinedOnTarget",
        Reason::QualifiedCallReturnShapeDrift { .. } => "QualifiedCallReturnShapeDrift",
        Reason::LocalCallReturnShapeDrift { .. } => "LocalCallReturnShapeDrift",
        Reason::MacroValueDrift { .. } => "MacroValueDrift",
        Reason::BehaviourCallbackAddedOnTarget { .. } => "BehaviourCallbackAddedOnTarget",
        Reason::BehaviourCallbackDriftOnTarget { .. } => "BehaviourCallbackDriftOnTarget",
        Reason::VersionedMachineSnapshotMissing { .. } => "VersionedMachineSnapshotMissing",
        Reason::WireConstantBindingsMissing { .. } => "WireConstantBindingsMissing",
        _ => "UnknownReason",
    }
}

pub(crate) fn reason_detail(r: &Reason) -> String {
    match r {
        Reason::MissingSymbol {
            symbol,
            first_seen_at_tag,
            ..
        } => match first_seen_at_tag {
            Some(tag) => format!(
                "{}; first appears at {tag}: land the dep pin bump first",
                format_symbol(&symbol.kind)
            ),
            None => format_symbol(&symbol.kind),
        },
        Reason::ArityChanged {
            module,
            function,
            expected,
            found,
            expected_available_at,
            ..
        } => {
            let found_str = found
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let base =
                format!("{module}:{function} expected /{expected}, snapshot has /{found_str}");
            match expected_available_at {
                Some(tag) => {
                    format!("{base}; /{expected} appears at {tag}: land the dep pin bump first")
                }
                None => base,
            }
        }
        Reason::SignatureChanged {
            module,
            function,
            arity,
            expected_spec,
            found_spec,
        } => format!(
            "{module}:{function}/{arity}: expected {expected_spec}; snapshot has {found_spec}"
        ),
        Reason::FileAbsent { path } | Reason::UnsupportedFileType { path } => {
            path.display().to_string()
        }
        Reason::ContextDrift { path, hunk_index } => {
            format!("{} (hunk #{hunk_index})", path.display())
        }
        Reason::DeprecatedUsage {
            symbol,
            since,
            replacement,
        } => {
            let s = format_symbol(&symbol.kind);
            match (since, replacement) {
                (Some(tag), Some(rep)) => {
                    format!("{s} (since {tag}; use {})", format_symbol(&rep.kind))
                }
                (Some(tag), None) => format!("{s} (since {tag})"),
                (None, Some(rep)) => format!("{s} (use {})", format_symbol(&rep.kind)),
                (None, None) => s,
            }
        }
        Reason::NowHidden { module } => format!("{module} is hidden at this pin"),
        Reason::RecordFieldsChanged {
            record,
            expected,
            found,
        } => {
            let e = expected
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let g = found
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("#{record}: expected fields [{e}]; snapshot has [{g}]")
        }
        Reason::UntrackedModuleMissing { module } => {
            format!("{module}.erl is absent in the target checkout")
        }
        Reason::ClauseMismatch {
            module,
            function,
            arity,
            call_args,
            pin_clauses,
        } => {
            let call = format_arg_shapes(call_args);
            let pins = pin_clauses
                .iter()
                .map(|c| format_arg_shapes(c))
                .collect::<Vec<_>>()
                .join(" | ");
            format!("{module}:{function}/{arity}: called with ({call}); pin clause heads: ({pins})")
        }
        Reason::MissingPrereq {
            symbol,
            self_branch,
            suggested_source_for_prereq,
        } => {
            let s = format_symbol(&symbol.kind);
            match suggested_source_for_prereq {
                Some(sha) => format!("{s}: absent on {self_branch}; candidate prereq: {sha}"),
                None => format!("{s}: absent on {self_branch}"),
            }
        }
        Reason::SyntacticArtifact {
            path,
            line,
            artifact,
        } => match artifact {
            ArtifactKind::ConflictMarker { marker } => {
                format!("{}:{line}: conflict marker ({:?})", path.display(), marker)
            }
            ArtifactKind::ExportWithoutBody {
                module,
                function,
                arity,
            } => format!(
                "{}:{line}: {module}:{function}/{arity} exported with no body",
                path.display()
            ),
        },
        Reason::BehaviourCallbackSignatureChanged {
            behaviour,
            callback,
            arity,
            expected_after_patch,
            implementer,
            implementer_signature,
        } => format!(
            "{behaviour}:{callback}/{arity} on {implementer}: patch expects {expected_after_patch:?}, pin has {implementer_signature:?}"
        ),
        Reason::BehaviourCallbackRemoved {
            behaviour,
            callback,
            arity,
            implementer,
        } => format!(
            "{behaviour}:{callback}/{arity} removed; {implementer} still exports the removed callback"
        ),
        Reason::BehaviourCallbackAdded {
            behaviour,
            callback,
            arity,
            implementer,
        } => format!(
            "{behaviour}:{callback}/{arity} added; {implementer} is missing the new required callback"
        ),
        Reason::ModuleRelocated { module, patch_path } => format!(
            "{module} exists on pin but not at {}: cherry-pick path needs adjustment",
            patch_path.display()
        ),
        Reason::WireConstantChanged {
            module,
            macro_name,
            before,
            after,
        } => format!("{module}.?{macro_name}: {before:?} -> {after:?}"),
        Reason::HistoricalImplementationMissing {
            module,
            advertised_version_before,
            advertised_version_after,
            expected_historical_module,
        } => format!(
            "{module}:version/0 advanced {advertised_version_before} -> {advertised_version_after} without {expected_historical_module}"
        ),
        Reason::WireContractBodyDrift {
            module,
            functions,
            advertised_version,
        } => {
            let fns: Vec<&str> = functions.iter().map(|f| f.as_str()).collect();
            format!(
                "{module} bodies drifted at version {advertised_version}: {}",
                fns.join(", ")
            )
        }
        Reason::WireContractRegression {
            module,
            pin_version,
            patch_version,
        } => format!("{module}:version/0 regressed {pin_version} -> {patch_version}"),
        Reason::ReturnShapeMismatch {
            module,
            function,
            arity,
            source_signature,
            pin_signature,
        } => format!(
            "{module}:{function}/{arity}: source spec {source_signature:?} vs pin spec {pin_signature:?}"
        ),
        Reason::MissingType {
            module,
            name,
            arity,
        } => format!("{module}:{name}/{arity} is not exported as a type at this pin"),
        Reason::PreimageDrifted {
            path,
            hunk_index,
            line_delta,
        } => format!(
            "{} (hunk #{hunk_index}) preimage found at line delta {line_delta:+}",
            path.display()
        ),
        Reason::PreimageMissing {
            path,
            hunk_index,
            preimage_excerpt,
        } => format!(
            "{} (hunk #{hunk_index}): preimage block absent; first lines: {preimage_excerpt:?}",
            path.display()
        ),
        Reason::PathRename {
            source_path,
            target_path,
            ..
        } => format!("{} → {}", source_path.display(), target_path.display()),
        Reason::TargetPathAbsent { path } => format!("{path}: absent on target, drop these hunks"),
        Reason::MacroUndefinedOnTarget {
            source_path,
            macro_name,
            line,
        } => format!("?{macro_name} undefined on target ({source_path}:{line})"),
        Reason::RecordUndefinedOnTarget {
            source_path,
            record_name,
            line,
        } => format!("#{record_name} undefined on target ({source_path}:{line})"),
        Reason::LocalCallUndefinedOnTarget {
            source_path,
            function,
            arity,
            line,
        } => format!("{function}/{arity} undefined on target ({source_path}:{line})"),
        Reason::QualifiedCallUndefinedOnTarget {
            source_path,
            module,
            function,
            arity,
            line,
        } => format!("{module}:{function}/{arity} undefined on target ({source_path}:{line})"),
        Reason::IndirectCallUndefinedOnTarget {
            source_path,
            module,
            function,
            arity,
            via,
            line,
        } => format!(
            "{module}:{function}/{arity} undefined on target (via {}, {source_path}:{line})",
            via.display_form()
        ),
        Reason::QualifiedCallReturnShapeDrift {
            source_path,
            module,
            function,
            arity,
            source_signature,
            target_signature,
            line,
        } => format!(
            "{module}:{function}/{arity}: source spec {source_signature:?} vs target spec {target_signature:?} ({source_path}:{line})"
        ),
        Reason::LocalCallReturnShapeDrift {
            source_path,
            function,
            arity,
            source_signature,
            target_signature,
            line,
        } => format!(
            "{function}/{arity}: source spec {source_signature:?} vs target spec {target_signature:?} ({source_path}:{line})"
        ),
        Reason::MacroValueDrift {
            source_path,
            macro_name,
            source_value,
            target_value,
            line,
        } => format!(
            "?{macro_name}: source value {source_value:?} vs target value {target_value:?} ({source_path}:{line})"
        ),
        Reason::BehaviourCallbackAddedOnTarget {
            source_path,
            behaviour,
            callback,
            arity,
            line,
        } => format!(
            "{behaviour} on target requires {callback}/{arity} the source does not declare ({source_path}:{line})"
        ),
        Reason::BehaviourCallbackDriftOnTarget {
            source_path,
            behaviour,
            callback,
            arity,
            source_signature,
            target_signature,
            line,
        } => format!(
            "{behaviour}:{callback}/{arity}: source {source_signature:?} vs target {target_signature:?} ({source_path}:{line})"
        ),
        Reason::VersionedMachineSnapshotMissing { module, side } => {
            format!("{module}: versioned-machine snapshot missing on {side:?}")
        }
        Reason::WireConstantBindingsMissing {
            module,
            macros,
            side,
        } => {
            let names: Vec<String> = macros.iter().map(|m| format!("?{m}")).collect();
            format!(
                "{module}: wire-constant bindings [{}] missing on {side:?}",
                names.join(", ")
            )
        }
        _ => format!("{r:?}"),
    }
}

fn format_arg_shapes(args: &[ArgShape]) -> String {
    args.iter()
        .map(format_arg_shape)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_arg_shape(a: &ArgShape) -> String {
    match a {
        ArgShape::Variable => "_".to_string(),
        ArgShape::Atom { name } => format!("'{name}'"),
        ArgShape::Integer => "int".to_string(),
        ArgShape::Float => "float".to_string(),
        ArgShape::Binary => "<<>>".to_string(),
        ArgShape::List => "[..]".to_string(),
        ArgShape::Tuple { size } => format!("{{{size}-tuple}}"),
        ArgShape::Record { name } => format!("#{name}{{}}"),
        ArgShape::String => "\"\"".to_string(),
        ArgShape::Fun => "fun".to_string(),
        ArgShape::Unknown => "?".to_string(),
    }
}

pub(crate) fn format_symbol(kind: &SymbolKind) -> String {
    match kind {
        SymbolKind::Function { mfa } => mfa.to_string(),
        SymbolKind::FunctionAnyArity { module, function } => format!("{module}:{function}/?"),
        SymbolKind::Type {
            module,
            name,
            arity,
        } => format!("{module}:{name}/{arity}"),
        SymbolKind::Record { name } => format!("#{name}"),
        SymbolKind::Macro { name } => format!("?{name}"),
        SymbolKind::Behaviour { module } => format!("behaviour {module}"),
        SymbolKind::Callback {
            module,
            function,
            arity,
        } => format!("callback {module}:{function}/{arity}"),
    }
}
