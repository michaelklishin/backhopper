// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Resolves qualified `m:f/a` calls a patch adds against the called
//! module's exports on the target tree: the snapshot-free counterpart
//! of `local_call_resolve` for the qualified case. A call is flagged
//! only when its module is first-party (present on the target tree and
//! covered by no pin snapshot), the export set is fully readable, and
//! neither the target nor the patch exports `f/a`. A call that does
//! resolve gets its `-spec` return shape compared between the source
//! and target checkouts, with the same comparator and guards the
//! tracked-dependency axis uses; every withheld comparison is counted
//! in a `ShapeCheckTally` so a shape-blind round is distinguishable
//! from a compared-clean one. Everything else is withheld and left to
//! the build.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use crate::compat::added_lines::{AddedLinesSubject, file_line};
use crate::compat::call_sites::extract_qualified_calls_with_context;
use crate::compat::indirect_calls::{
    IndirectExtraction, extract_indirect_calls_elixir, extract_indirect_calls_with_context,
};
use crate::compat::source_attributes::{
    ExportSet, SpecTable, extract_exports, extract_function_signatures, extract_specs,
};
use crate::model::names::{Arity, FunctionName, ModuleName, RelativePath};
use crate::model::spec_ast::SpecType;
use crate::model::spec_parser::parse_signature_return;
use crate::model::symbol::RefContext;
use crate::model::verdict::{IndirectCallForm, IndirectCallTally, Reason, ShapeCheckTally};

/// `(function, arity)` pairs per module.
pub type PerModuleFunctionSet = BTreeMap<ModuleName, BTreeSet<(FunctionName, Arity)>>;

/// An `AddedLinesSubject` paired with its per-blob-line attribute-region
/// classification, computed against the full hunk (`Context` lines
/// included) rather than the added-only blob: a continuation line whose
/// multi-line attribute opener is unchanged, and so absent from
/// `added_text`, still classifies against the region its unseen opener
/// started. Only the qualified-call and Erlang indirect-call extractors
/// need this; every other axis over `AddedLinesSubject` carries no
/// attribute-context concept.
#[derive(Debug, Clone, Copy)]
pub struct ContextAwareSubject<'a> {
    pub subject: AddedLinesSubject<'a>,
    pub line_context: &'a [RefContext],
}

/// What the patch's added lines themselves provide per module. A
/// qualified call to an added function is not flagged, since the patch
/// introduces it (often in a different file than the call). A resolved
/// call whose `-spec` the patch rewrites is not shape-compared:
/// post-apply, both trees carry the patch's version, so there is no
/// pre-existing drift to find.
#[derive(Debug, Default)]
pub struct PatchProvided {
    pub functions: PerModuleFunctionSet,
    pub specs: PerModuleFunctionSet,
}

/// Reads one module file's text by tree-relative path: a git blob for
/// the target side, the on-disk checkout for the source side.
pub type TreeReader<'a> = &'a dyn Fn(&RelativePath) -> Option<String>;

/// The immutable inputs a single `m:f/arity` resolution needs. Shared
/// by the qualified-call axis and the import-resolved local-call path
/// so both resolve a reference against the target tree the same way.
pub struct ReferenceContext<'a> {
    pub covered_modules: &'a BTreeSet<ModuleName>,
    pub patch_added: &'a PatchProvided,
    pub resolve_module_path: &'a dyn Fn(&ModuleName) -> Option<RelativePath>,
    pub read_target: TreeReader<'a>,
    pub read_source: Option<TreeReader<'a>>,
}

impl fmt::Debug for ReferenceContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReferenceContext")
            .field("covered_modules", self.covered_modules)
            .field("patch_added", self.patch_added)
            .finish_non_exhaustive()
    }
}

/// The caches a run of reference resolutions accumulates. The module
/// surface and spec caches persist across subjects; the dedup sets are
/// per-subject and cleared by `begin_subject`.
#[derive(Debug, Default)]
pub struct ReferenceCaches {
    surfaces: BTreeMap<ModuleName, Option<TargetModuleSurface>>,
    specs: BTreeMap<ModuleName, (SpecTable, Option<SpecTable>)>,
    flagged: BTreeSet<(ModuleName, FunctionName, Arity)>,
    shape_seen: BTreeSet<(ModuleName, FunctionName, Arity)>,
}

impl ReferenceCaches {
    /// Clear the per-subject dedup sets while keeping the module surface
    /// and spec caches, so one `m:f/a` flags once per subject but a
    /// module is read once per run.
    pub fn begin_subject(&mut self) {
        self.flagged.clear();
        self.shape_seen.clear();
    }
}

/// Resolve one `m:f/arity` reference against the target tree: classify
/// the module, read its surface once, and push a reason when the callee
/// is undefined or its `-spec` return shape drifts. The single
/// definition of undefined-on-target for both the qualified-call axis
/// and a call resolved through a patch-added `-import`.
#[allow(clippy::too_many_arguments)]
pub fn resolve_qualified_reference(
    subject: &AddedLinesSubject<'_>,
    module: &ModuleName,
    key: &(FunctionName, Arity),
    call_line: u32,
    ctx: &ReferenceContext<'_>,
    caches: &mut ReferenceCaches,
    reasons: &mut Vec<Reason>,
    shape_checks: &mut ShapeCheckTally,
) {
    let surface = caches.surfaces.entry(module.clone()).or_insert_with(|| {
        target_module_surface(
            module,
            ctx.covered_modules,
            ctx.resolve_module_path,
            ctx.read_target,
        )
    });
    let Some(surface) = surface.as_ref() else {
        return;
    };
    let is_exported = surface.exports.exports.contains(key);
    let path = surface.path.clone();
    // A callee the patch itself introduces has no meaningful
    // pre-existing shape on either tree.
    if ctx
        .patch_added
        .functions
        .get(module)
        .is_some_and(|s| s.contains(key))
    {
        return;
    }
    if is_exported {
        // A spec the patch rewrites lands on the target with the pick:
        // no pre-existing drift to compare.
        if ctx
            .patch_added
            .specs
            .get(module)
            .is_some_and(|s| s.contains(key))
        {
            return;
        }
        if !caches
            .shape_seen
            .insert((module.clone(), key.0.clone(), key.1))
        {
            return;
        }
        check_return_shape(
            subject,
            module,
            &path,
            key,
            call_line,
            ctx.read_target,
            ctx.read_source,
            &mut caches.specs,
            reasons,
            shape_checks,
        );
        return;
    }
    if !caches
        .flagged
        .insert((module.clone(), key.0.clone(), key.1))
    {
        return;
    }
    reasons.push(Reason::QualifiedCallUndefinedOnTarget {
        source_path: subject.source_path.clone(),
        module: module.clone(),
        function: key.0.clone(),
        arity: key.1,
        line: file_line(subject.line_map, call_line),
    });
}

/// Where a called module sits relative to this axis. Only `FirstParty`
/// is resolved; the other two are withheld, so a dependency call or an
/// OTP call cannot be flagged by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleProvenance {
    /// A pin snapshot (tracked dep or self-pin) already resolves this
    /// module's calls: defer to the snapshot axis.
    CoveredBySnapshot,
    /// A first-party `.erl` module present on the target tree, covered
    /// by no snapshot: this axis resolves it live against its exports.
    FirstParty { path: RelativePath },
    /// Covered by no snapshot and absent from the tree (OTP, stdlib, or
    /// a genuinely-missing module): withhold, the build is the backstop.
    Unknown,
}

/// Classify a called module. Snapshot coverage wins over an in-tree
/// file, so a tracked dep that also has a vendored copy in the tree
/// still defers to its snapshot.
pub fn classify_module(
    module: &ModuleName,
    covered_modules: &BTreeSet<ModuleName>,
    resolve_module_path: &dyn Fn(&ModuleName) -> Option<RelativePath>,
) -> ModuleProvenance {
    if covered_modules.contains(module) {
        return ModuleProvenance::CoveredBySnapshot;
    }
    match resolve_module_path(module) {
        Some(path) => ModuleProvenance::FirstParty { path },
        None => ModuleProvenance::Unknown,
    }
}

/// What the patch adds per module, gathered across every touched
/// `.erl` file so a cross-file add (call in one file, callee and its
/// `-export` in the module's own file) does not false-positive.
pub fn patch_provided(per_file: &[(ModuleName, &str)]) -> PatchProvided {
    let mut out = PatchProvided::default();
    for (module, added_text) in per_file {
        let functions = out.functions.entry(module.clone()).or_default();
        for sig in extract_function_signatures(added_text) {
            if !sig.is_definition {
                continue;
            }
            if let (Ok(function), Ok(arity)) = (
                FunctionName::from_str(&sig.name),
                Arity::try_from(sig.arity),
            ) {
                functions.insert((function, arity));
            }
        }
        functions.extend(extract_exports(added_text).exports);
        let specs = extract_specs(added_text);
        if !specs.is_empty() {
            out.specs
                .entry(module.clone())
                .or_default()
                .extend(specs.into_keys());
        }
    }
    out
}

/// What the qualified-call gate produced: the export-resolution
/// reasons plus the return-shape findings, and the tally of what the
/// shape check did with each resolved call.
#[derive(Debug, Default)]
pub struct QualifiedCallAnalysis {
    pub reasons: Vec<Reason>,
    pub shape_checks: ShapeCheckTally,
    pub indirect_checks: IndirectCallTally,
}

/// The target module's readable surface for this axis: what it exports
/// and what it defines. The defined set exists for the indirect gate,
/// where a meck expectation on an unexported function is legitimate.
#[derive(Debug)]
struct TargetModuleSurface {
    exports: ExportSet,
    defined: BTreeSet<(FunctionName, Arity)>,
    path: RelativePath,
}

/// Flag each added qualified call whose module is first-party on the
/// target and does not export the called `f/a`, and compare the
/// `-spec` return shape of each call that does resolve.
/// `resolve_module_path` maps a module to its `.erl` path on the
/// target tree (the first-party gate: `None` means absent, so
/// withhold); `read_target` reads that path at the resolved target
/// commit; `read_source` reads it from the source checkout (`None`
/// when no checkout is available, which withholds every shape check).
/// `covered_modules` is the union of every pin snapshot's modules: a
/// call into one of them is the snapshot axis's, not this one's. One
/// reason per `(file, module, function, arity)`.
pub fn analyse_qualified_calls(
    subjects: &[ContextAwareSubject<'_>],
    covered_modules: &BTreeSet<ModuleName>,
    patch_added: &PatchProvided,
    resolve_module_path: &dyn Fn(&ModuleName) -> Option<RelativePath>,
    read_target: TreeReader<'_>,
    read_source: Option<TreeReader<'_>>,
) -> QualifiedCallAnalysis {
    let mut analysis = QualifiedCallAnalysis::default();
    let ctx = ReferenceContext {
        covered_modules,
        patch_added,
        resolve_module_path,
        read_target,
        read_source,
    };
    let mut caches = ReferenceCaches::default();
    for subject in subjects {
        let calls = extract_qualified_calls_with_context(
            subject.subject.added_text,
            subject.subject.line_map,
            subject.line_context,
        );
        if calls.is_empty() {
            continue;
        }
        caches.begin_subject();
        for call in calls {
            let key = (call.mfa.function.clone(), call.mfa.arity);
            resolve_qualified_reference(
                &subject.subject,
                &call.mfa.module,
                &key,
                call.line,
                &ctx,
                &mut caches,
                &mut analysis.reasons,
                &mut analysis.shape_checks,
            );
        }
    }
    for subject in subjects {
        resolve_indirect_calls(subject, &ctx, &mut caches, &mut analysis);
    }
    analysis
}

/// Classify a module and read its export and definition surface once.
/// `None` caches every withheld case: covered, absent, or an
/// incompletely readable export surface.
fn target_module_surface(
    module: &ModuleName,
    covered_modules: &BTreeSet<ModuleName>,
    resolve_module_path: &dyn Fn(&ModuleName) -> Option<RelativePath>,
    read_target: TreeReader<'_>,
) -> Option<TargetModuleSurface> {
    match classify_module(module, covered_modules, resolve_module_path) {
        ModuleProvenance::FirstParty { path } => {
            let text = read_target(&path)?;
            let exports = extract_exports(&text);
            if !exports.complete {
                return None;
            }
            let defined = extract_function_signatures(&text)
                .into_iter()
                .filter(|sig| sig.is_definition)
                .filter_map(|sig| {
                    match (
                        FunctionName::from_str(&sig.name),
                        Arity::try_from(sig.arity),
                    ) {
                        (Ok(function), Ok(arity)) => Some((function, arity)),
                        _ => None,
                    }
                })
                .collect();
            Some(TargetModuleSurface {
                exports,
                defined,
                path,
            })
        }
        ModuleProvenance::CoveredBySnapshot | ModuleProvenance::Unknown => None,
    }
}

/// The indirect-reference axis for Elixir sources. `:module.function`
/// calls of a recognized form are extracted from the Elixir added lines
/// and resolved against the same Erlang target tree the qualified-call
/// axis reads. Only the extractor differs from the Erlang path; the
/// resolution, gate, and reason are shared.
pub fn analyse_indirect_elixir_calls(
    subjects: &[AddedLinesSubject<'_>],
    ctx: &ReferenceContext<'_>,
    caches: &mut ReferenceCaches,
) -> IndirectCallAnalysis {
    let mut out = IndirectCallAnalysis::default();
    for subject in subjects {
        let extraction = extract_indirect_calls_elixir(subject.added_text, subject.line_map);
        resolve_extraction(
            &extraction,
            subject,
            ctx,
            caches,
            &mut out.reasons,
            &mut out.tally,
        );
    }
    out
}

/// The reasons and tally the indirect-reference axis produces on its
/// own, so the Elixir path can be wired beside the qualified stream.
#[derive(Debug, Default)]
pub struct IndirectCallAnalysis {
    pub reasons: Vec<Reason>,
    pub tally: IndirectCallTally,
}

/// Resolve one subject's Erlang indirect MFA references against the
/// target modules' surfaces, folding the reasons and tally into the
/// qualified-call analysis.
fn resolve_indirect_calls(
    subject: &ContextAwareSubject<'_>,
    ctx: &ReferenceContext<'_>,
    caches: &mut ReferenceCaches,
    analysis: &mut QualifiedCallAnalysis,
) {
    let extraction = extract_indirect_calls_with_context(
        subject.subject.added_text,
        subject.subject.line_map,
        subject.line_context,
    );
    resolve_extraction(
        &extraction,
        &subject.subject,
        ctx,
        caches,
        &mut analysis.reasons,
        &mut analysis.indirect_checks,
    );
}

/// Resolve extracted indirect sites against the target modules' export
/// and definition sets, language-independent below the extractor. A
/// site is flagged only when the target module neither exports nor
/// defines it and the patch does not add it. Shares `ReferenceContext`
/// and the surface cache with the qualified-call axis, so a module read
/// once serves both.
fn resolve_extraction(
    extraction: &IndirectExtraction,
    subject: &AddedLinesSubject<'_>,
    ctx: &ReferenceContext<'_>,
    caches: &mut ReferenceCaches,
    reasons: &mut Vec<Reason>,
    tally: &mut IndirectCallTally,
) {
    tally.withheld_dynamic += extraction.withheld_dynamic;
    let mut seen: BTreeSet<(ModuleName, FunctionName, Arity, IndirectCallForm)> = BTreeSet::new();
    for site in &extraction.sites {
        let (module, function, arity, via) = (
            &site.mfa.module,
            &site.mfa.function,
            site.mfa.arity,
            site.via,
        );
        let entry = caches.surfaces.entry(module.clone()).or_insert_with(|| {
            target_module_surface(
                module,
                ctx.covered_modules,
                ctx.resolve_module_path,
                ctx.read_target,
            )
        });
        let Some(surface) = entry.as_ref() else {
            continue;
        };
        if !seen.insert((module.clone(), function.clone(), arity, via)) {
            continue;
        }
        tally.checked += 1;
        let key = (function.clone(), arity);
        if surface.exports.exports.contains(&key)
            || surface.defined.contains(&key)
            || ctx
                .patch_added
                .functions
                .get(module)
                .is_some_and(|s| s.contains(&key))
        {
            continue;
        }
        reasons.push(Reason::IndirectCallUndefinedOnTarget {
            source_path: subject.source_path.clone(),
            module: module.clone(),
            function: function.clone(),
            arity,
            via,
            line: file_line(subject.line_map, site.line),
        });
    }
}

/// Outcome of comparing one function's `-spec` return shape between
/// two spec tables. The guards are `check_return_shapes`' from the
/// snapshot axis: present on both sides, both parse to a known
/// `SpecType`, shapes disagree.
#[derive(Debug, PartialEq, Eq)]
pub enum ShapeComparison {
    /// Both sides have a spec and the shapes agree.
    Same,
    /// Both sides have a spec and the shapes disagree.
    Drift {
        source_signature: String,
        target_signature: String,
    },
    /// One or both sides declare no `-spec` for the key.
    NoSpec,
    /// Both sides have a spec but at least one parses to
    /// `SpecType::Unknown`.
    UnknownType,
}

pub fn compare_return_shapes(
    target_specs: &SpecTable,
    source_specs: &SpecTable,
    key: &(FunctionName, Arity),
) -> ShapeComparison {
    let (Some(target_sig), Some(source_sig)) = (target_specs.get(key), source_specs.get(key))
    else {
        return ShapeComparison::NoSpec;
    };
    // Identical text means identical shape: no parse needed.
    if target_sig == source_sig {
        return ShapeComparison::Same;
    }
    let target_ast = parse_signature_return(target_sig);
    let source_ast = parse_signature_return(source_sig);
    if matches!(target_ast, SpecType::Unknown) || matches!(source_ast, SpecType::Unknown) {
        return ShapeComparison::UnknownType;
    }
    if target_ast.matches(&source_ast) {
        ShapeComparison::Same
    } else {
        ShapeComparison::Drift {
            source_signature: source_sig.clone(),
            target_signature: target_sig.clone(),
        }
    }
}

/// Every guard that declines to compare is counted rather than
/// dropped.
#[allow(clippy::too_many_arguments)]
fn check_return_shape(
    subject: &AddedLinesSubject<'_>,
    module: &ModuleName,
    path: &RelativePath,
    key: &(FunctionName, Arity),
    call_line: u32,
    read_target: TreeReader<'_>,
    read_source: Option<TreeReader<'_>>,
    specs_by_module: &mut BTreeMap<ModuleName, (SpecTable, Option<SpecTable>)>,
    reasons: &mut Vec<Reason>,
    tally: &mut ShapeCheckTally,
) {
    let Some(read_source) = read_source else {
        tally.withheld_no_source += 1;
        return;
    };
    let (target_specs, source_specs) = specs_by_module.entry(module.clone()).or_insert_with(|| {
        (
            read_target(path)
                .map(|t| extract_specs(&t))
                .unwrap_or_default(),
            read_source(path).map(|t| extract_specs(&t)),
        )
    });
    let Some(source_specs) = source_specs.as_ref() else {
        tally.withheld_no_source += 1;
        return;
    };
    match compare_return_shapes(target_specs, source_specs, key) {
        ShapeComparison::Same => tally.compared += 1,
        ShapeComparison::NoSpec => tally.withheld_no_spec += 1,
        ShapeComparison::UnknownType => tally.withheld_unknown_type += 1,
        ShapeComparison::Drift {
            source_signature,
            target_signature,
        } => {
            tally.compared += 1;
            reasons.push(Reason::QualifiedCallReturnShapeDrift {
                source_path: subject.source_path.clone(),
                module: module.clone(),
                function: key.0.clone(),
                arity: key.1,
                source_signature,
                target_signature,
                line: file_line(subject.line_map, call_line),
            });
        }
    }
}
