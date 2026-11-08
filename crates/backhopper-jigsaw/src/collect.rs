// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::str::{self, FromStr};

use backhopper_core::compat::indirect_calls::extract_indirect_calls;
use backhopper_core::jigsaw::anchor::is_predefined_macro;
use backhopper_core::jigsaw::plugin_set::{PluginFile, PluginSet, declared_module};
use backhopper_core::jigsaw::references::{
    CallForm, DynamicCall, NotChecked, RecordConstruction, Reference, ReferenceSet, ReferenceSite,
    SiteDetail, UncheckedSite, UncheckedSymbol,
};
use backhopper_core::jigsaw::surface::ReleaseSurface;
use backhopper_core::model::macro_decl::MacroKey;
use backhopper_core::model::names::{
    ApplicationName, FieldName, MacroName, ModuleName, RecordName, RelativePath,
};
use backhopper_core::model::snapshot::{FunArity, HeaderRef};
use backhopper_xref_graph::{CallKind, CallTarget, FunctionRef, Loc, PathInterner};
use backhopper_xref_reader::{ReadError, SourceReader};
use thiserror::Error;

use crate::includes::{HeaderIndex, Include, Target};
use crate::schema::SchemaFile;
use crate::source::{Code, Region, region_at, regions};
use crate::synthetic::render;
use crate::uses::{
    Construction, MacroUse, RecordUse, behaviours, declared_records, defined_macros, elixir_calls,
    macro_uses, record_uses, type_refs,
};

#[derive(Debug, Error)]
pub enum CollectError {
    #[error("{app}/{path} is not valid UTF-8")]
    NotUtf8 {
        app: ApplicationName,
        path: RelativePath,
    },
    #[error(transparent)]
    Read(#[from] ReadError),
    #[error("{app}/{path}: {message}")]
    Schema {
        app: ApplicationName,
        path: RelativePath,
        message: String,
    },
}

struct ErlangFile<'a> {
    id: PluginFile,
    text: &'a str,
    code: Code,
    regions: Vec<Region>,
    includes: Vec<Include>,
    macro_uses: Vec<MacroUse>,
    record_uses: Vec<RecordUse>,
    defined_macros: BTreeSet<MacroName>,
    declared_records: BTreeSet<RecordName>,
}

impl ErlangFile<'_> {
    fn is_header(&self) -> bool {
        self.id.path.as_str().ends_with(".hrl")
    }

    fn region(&self, line: u32) -> Region {
        region_at(&self.regions, line)
    }

    fn site(&self, line: u32, detail: SiteDetail) -> ReferenceSite {
        ReferenceSite {
            file: self.id.clone(),
            line,
            detail,
        }
    }
}

/// References and the regions their sites are in, before the conditional
/// ones are set aside.
#[derive(Default)]
struct Pending {
    entries: BTreeMap<Reference, Vec<(ReferenceSite, Region)>>,
}

impl Pending {
    fn add(&mut self, reference: Reference, site: ReferenceSite, region: Region) {
        if region != Region::TestOnly {
            self.entries
                .entry(reference)
                .or_default()
                .push((site, region));
        }
    }

    /// A reference every site of which is under a build conditional is
    /// counted, not checked.
    fn settle(self, out: &mut ReferenceSet) {
        for (reference, sites) in self.entries {
            if sites
                .iter()
                .all(|(_, region)| *region == Region::Conditional)
            {
                out.count(NotChecked::ConditionalSite, 1);
                continue;
            }
            for (site, _) in sites {
                out.add(reference.clone(), site);
            }
        }
    }
}

/// Reads every app of the plugin set once and returns the references it
/// makes outside itself. `build` is the surface of the build patch: host
/// headers the plugins include are rendered from it so macros that expand
/// to calls are seen, and a record construction is measured against the
/// record's fields there.
pub fn collect(
    plugins: &PluginSet,
    build: &ReleaseSurface<'_>,
) -> Result<ReferenceSet, CollectError> {
    let mut sources: Vec<(PluginFile, &[u8])> = Vec::new();
    for (id, file) in plugins.files() {
        sources.push((id, file.bytes.as_slice()));
    }
    let mut erlang: Vec<ErlangFile<'_>> = Vec::new();
    let mut schemas: Vec<SchemaFile> = Vec::new();
    for (id, bytes) in &sources {
        let path = id.path.as_str();
        let is_erlang = path.ends_with(".erl") || path.ends_with(".hrl");
        let is_schema = path.ends_with(".schema") || path.ends_with(".partial");
        if !is_erlang && !is_schema {
            continue;
        }
        let text = str::from_utf8(bytes).map_err(|_| CollectError::NotUtf8 {
            app: id.app.clone(),
            path: id.path.clone(),
        })?;
        if is_schema {
            schemas.push(SchemaFile::parse(id.clone(), text).map_err(|e| {
                CollectError::Schema {
                    app: id.app.clone(),
                    path: id.path.clone(),
                    message: e.to_string(),
                }
            })?);
            continue;
        }
        let code = Code::new(text);
        erlang.push(ErlangFile {
            id: id.clone(),
            text,
            regions: regions(&code),
            includes: Vec::new(),
            macro_uses: macro_uses(&code),
            record_uses: record_uses(&code, text),
            defined_macros: defined_macros(&code),
            declared_records: declared_records(&code),
            code,
        });
    }

    let header_ids: Vec<&PluginFile> = erlang
        .iter()
        .filter(|file| file.is_header())
        .map(|file| &file.id)
        .collect();
    let index = HeaderIndex::new(plugins.apps().keys(), header_ids);
    for file in &mut erlang {
        file.includes = index.includes_of(&file.id, &file.code);
    }

    let mut out = ReferenceSet::new();
    let mut pending = Pending::default();
    let context = Context::new(&erlang, build);

    // The reader sees the plugin set's files first, so a header of the
    // plugin set wins over a rendered host header of the same name.
    let plugin_apps: BTreeSet<&ApplicationName> = plugins.apps().keys().collect();
    let synthetic = render(build, &context.all_host_roots(), &plugin_apps);
    let mut tree: Vec<(PathBuf, Vec<u8>)> = erlang
        .iter()
        .map(|file| (reader_path(&file.id), file.text.as_bytes().to_vec()))
        .collect();
    tree.extend(
        synthetic
            .into_iter()
            .map(|header| (header.path, header.text.into_bytes())),
    );
    let read = SourceReader::new().read_tree(tree)?;

    let p_modules: BTreeSet<ModuleName> = read.modules.iter().map(|m| m.module.clone()).collect();
    let by_id: BTreeMap<&PluginFile, &ErlangFile<'_>> =
        erlang.iter().map(|file| (&file.id, file)).collect();

    for module in &read.modules {
        out.set_exports(
            module.module.clone(),
            module
                .exports
                .iter()
                .filter(|sig| {
                    !module.definitions.get(sig).is_some_and(|loc| {
                        site_file(&read.paths, loc, &by_id)
                            .is_some_and(|file| file.region(loc.start.line) == Region::TestOnly)
                    })
                })
                .map(|sig| FunArity {
                    name: sig.name.clone(),
                    arity: sig.arity,
                })
                .collect(),
        );
        for call in &module.external_calls {
            let CallTarget::External(FunctionRef::Concrete(mfa)) = &call.callee else {
                continue;
            };
            let Some(file) = site_file(&read.paths, &call.loc, &by_id) else {
                continue;
            };
            let line = call.loc.start.line;
            let region = file.region(line);
            if region == Region::TestOnly {
                continue;
            }
            if p_modules.contains(&mfa.module) {
                out.count(NotChecked::Internal, 1);
                continue;
            }
            let form = match call.kind {
                CallKind::FunRef => CallForm::FunRef,
                CallKind::Apply | CallKind::Spawn => CallForm::Apply,
                _ => CallForm::Direct,
            };
            pending.add(
                Reference::Call(mfa.clone()),
                file.site(line, SiteDetail::Call(form)),
                region,
            );
        }
        for call in &module.unresolved {
            let Some(file) = site_file(&read.paths, &call.loc, &by_id) else {
                continue;
            };
            let line = call.loc.start.line;
            if file.region(line) != Region::TestOnly {
                out.add_unchecked(UncheckedSite {
                    symbol: UncheckedSymbol::Dynamic(dynamic_call(&call.partial)),
                    file: file.id.clone(),
                    line,
                });
            }
        }
    }

    for file in erlang.iter().filter(|file| !file.is_header()) {
        let line_map: Vec<u32> =
            (1..=u32::try_from(file.text.lines().count()).unwrap_or(0)).collect();
        let indirect = extract_indirect_calls(file.text, &line_map);
        for withheld in indirect.withheld_dynamic {
            if file.region(withheld.line) != Region::TestOnly {
                out.add_unchecked(UncheckedSite {
                    symbol: UncheckedSymbol::Dynamic(DynamicCall {
                        module: Some(withheld.module),
                        function: Some(withheld.function),
                        arity: None,
                    }),
                    file: file.id.clone(),
                    line: withheld.line,
                });
            }
        }
        for site in indirect.sites {
            if p_modules.contains(&site.mfa.module) {
                continue;
            }
            pending.add(
                Reference::Call(site.mfa),
                file.site(site.line, SiteDetail::Call(CallForm::Indirect(site.via))),
                file.region(site.line),
            );
        }
    }

    for file in &erlang {
        for (mfa, line) in elixir_calls(&file.code) {
            pending.add(
                Reference::Call(mfa),
                file.site(line, SiteDetail::Call(CallForm::Direct)),
                file.region(line),
            );
        }
        for (module, name, arity, line) in type_refs(file.text) {
            if !p_modules.contains(&module) {
                pending.add(
                    Reference::Type {
                        module,
                        name,
                        arity,
                    },
                    file.site(line, SiteDetail::Plain),
                    file.region(line),
                );
            }
        }
        for include in &file.includes {
            if let Target::Host(header) = &include.target {
                pending.add(
                    Reference::Header(header.clone()),
                    file.site(include.line, SiteDetail::Plain),
                    file.region(include.line),
                );
            }
        }
        if let Some(implementer) = declared_module_of(plugins, &file.id) {
            for (behaviour, line) in behaviours(&file.code) {
                let defined_here = ModuleName::from_str(behaviour.as_str())
                    .is_ok_and(|module| p_modules.contains(&module));
                if !defined_here {
                    pending.add(
                        Reference::Behaviour {
                            implementer: implementer.clone(),
                            behaviour,
                        },
                        file.site(line, SiteDetail::Plain),
                        file.region(line),
                    );
                }
            }
        }
    }

    context.uses(&mut pending, &mut out);
    context.unresolved_includes(&mut out);

    collect_schemas(plugins, &schemas, &p_modules, &mut pending);
    for (name, app) in plugins.apps() {
        for required in app.manifest().requires() {
            let site = ReferenceSite {
                file: PluginFile {
                    app: name.clone(),
                    path: app.manifest_path().clone(),
                },
                line: 1,
                detail: SiteDetail::Plain,
            };
            pending.add(Reference::App(required.clone()), site, Region::Live);
        }
    }
    pending.settle(&mut out);
    Ok(out)
}

fn dynamic_call(partial: &FunctionRef) -> DynamicCall {
    match partial {
        FunctionRef::Concrete(mfa) => DynamicCall {
            module: Some(mfa.module.clone()),
            function: Some(mfa.function.clone()),
            arity: Some(mfa.arity),
        },
        FunctionRef::UnresolvedModule { function, arity } => DynamicCall {
            module: None,
            function: Some(function.clone()),
            arity: *arity,
        },
        FunctionRef::UnresolvedFunction { module, arity } => DynamicCall {
            module: Some(module.clone()),
            function: None,
            arity: *arity,
        },
        FunctionRef::UnresolvedBoth { arity } => DynamicCall {
            module: None,
            function: None,
            arity: *arity,
        },
    }
}

fn reader_path(id: &PluginFile) -> PathBuf {
    PathBuf::from(format!("{}/{}", id.app, id.path))
}

fn site_file<'a, 'b>(
    paths: &PathInterner,
    loc: &Loc,
    by_id: &'a BTreeMap<&PluginFile, &'a ErlangFile<'b>>,
) -> Option<&'a ErlangFile<'b>> {
    let path = paths.get(loc.path)?.to_str()?;
    let (app, rest) = path.split_once('/')?;
    let id = PluginFile {
        app: ApplicationName::from_str(app).ok()?,
        path: RelativePath::new(rest).ok()?,
    };
    by_id.get(&id).copied()
}

fn declared_module_of(plugins: &PluginSet, id: &PluginFile) -> Option<ModuleName> {
    plugins
        .app(&id.app)?
        .files()
        .iter()
        .find(|file| file.path == id.path)
        .and_then(declared_module)
}

fn collect_schemas(
    plugins: &PluginSet,
    schemas: &[SchemaFile],
    p_modules: &BTreeSet<ModuleName>,
    pending: &mut Pending,
) {
    let defined_partials: BTreeSet<_> = schemas
        .iter()
        .filter_map(SchemaFile::defined_partial)
        .collect();
    let defined_validators: BTreeSet<_> = schemas
        .iter()
        .flat_map(SchemaFile::defined_validators)
        .collect();
    let site = |schema: &SchemaFile, line: u32| ReferenceSite {
        file: schema.id.clone(),
        line,
        detail: SiteDetail::Plain,
    };
    for schema in schemas {
        for (partial, keys, line) in schema.partial_includes() {
            if defined_partials.contains(&partial) || plugins.contains_app(&partial.app) {
                continue;
            }
            pending.add(
                Reference::Partial { partial, keys },
                site(schema, line),
                Region::Live,
            );
        }
        for (name, line) in schema.referenced_validators() {
            if !defined_validators.contains(&name) {
                pending.add(Reference::Validator(name), site(schema, line), Region::Live);
            }
        }
        for (mfa, line) in schema.calls() {
            if !p_modules.contains(&mfa.module) {
                pending.add(Reference::Call(mfa), site(schema, line), Region::Live);
            }
        }
    }
}

/// The include graph of the plugin set, resolved once per `.erl` file.
struct Context<'a, 'b> {
    files: &'a [ErlangFile<'b>],
    build: &'a ReleaseSurface<'a>,
    closures: BTreeMap<PluginFile, Vec<usize>>,
}

impl<'a, 'b> Context<'a, 'b> {
    fn new(files: &'a [ErlangFile<'b>], build: &'a ReleaseSurface<'a>) -> Self {
        let position: BTreeMap<&PluginFile, usize> = files
            .iter()
            .enumerate()
            .map(|(index, file)| (&file.id, index))
            .collect();
        let mut closures = BTreeMap::new();
        for (start, file) in files.iter().enumerate() {
            if file.is_header() {
                continue;
            }
            let mut seen = BTreeSet::from([start]);
            let mut queue = vec![start];
            while let Some(next) = queue.pop() {
                for include in &files[next].includes {
                    let Target::Local(local) = &include.target else {
                        continue;
                    };
                    if files[next].region(include.line) == Region::TestOnly {
                        continue;
                    }
                    if let Some(&index) = position.get(local)
                        && seen.insert(index)
                    {
                        queue.push(index);
                    }
                }
            }
            closures.insert(file.id.clone(), seen.into_iter().collect());
        }
        Self {
            files,
            build,
            closures,
        }
    }

    fn includes_in_closure(&self, includer: &ErlangFile<'_>) -> Vec<&Include> {
        self.closures[&includer.id]
            .iter()
            .flat_map(|&index| {
                let file = &self.files[index];
                file.includes
                    .iter()
                    .filter(|include| file.region(include.line) != Region::TestOnly)
            })
            .collect()
    }

    fn host_roots(&self, includer: &ErlangFile<'_>) -> BTreeSet<HeaderRef> {
        self.includes_in_closure(includer)
            .into_iter()
            .filter_map(|include| match &include.target {
                Target::Host(header) => Some(header.clone()),
                Target::Local(_) | Target::Unresolved => None,
            })
            .collect()
    }

    fn all_host_roots(&self) -> BTreeSet<HeaderRef> {
        self.files
            .iter()
            .filter(|file| !file.is_header())
            .flat_map(|file| self.host_roots(file))
            .collect()
    }

    fn includers(&self) -> impl Iterator<Item = &ErlangFile<'b>> {
        self.files.iter().filter(|file| !file.is_header())
    }

    /// Macro and record uses, resolved in the context of each `.erl` file
    /// that sees them: its own and its plugin set headers' definitions
    /// first, the host's headers at the build patch after. A use in a
    /// header is sited at the header's line for every file that includes it.
    fn uses(&self, pending: &mut Pending, out: &mut ReferenceSet) {
        for includer in self.includers() {
            let closure = &self.closures[&includer.id];
            let roots = self.host_roots(includer);
            out.set_host_includes(includer.id.clone(), roots.clone());
            let mut macros: BTreeSet<MacroName> = BTreeSet::new();
            let mut records: BTreeSet<RecordName> = BTreeSet::new();
            for &index in closure {
                macros.extend(self.files[index].defined_macros.iter().cloned());
                records.extend(self.files[index].declared_records.iter().cloned());
            }
            for &index in closure {
                let file = &self.files[index];
                for used in &file.macro_uses {
                    if is_predefined_macro(used.name.as_str()) || macros.contains(&used.name) {
                        continue;
                    }
                    pending.add(
                        Reference::Macro {
                            includer: includer.id.clone(),
                            key: MacroKey {
                                name: used.name.clone(),
                                arity: used.arity,
                            },
                        },
                        file.site(used.line, SiteDetail::Plain),
                        file.region(used.line),
                    );
                }
                for used in &file.record_uses {
                    if records.contains(&used.name) {
                        continue;
                    }
                    let detail =
                        used.construction
                            .as_ref()
                            .map_or(SiteDetail::Plain, |construction| {
                                let unset = self.unset_fields(&roots, &used.name, construction);
                                SiteDetail::Construction(RecordConstruction { unset })
                            });
                    pending.add(
                        Reference::Record {
                            includer: includer.id.clone(),
                            name: used.name.clone(),
                        },
                        file.site(used.line, detail),
                        file.region(used.line),
                    );
                }
            }
        }
    }

    fn unset_fields(
        &self,
        roots: &BTreeSet<HeaderRef>,
        name: &RecordName,
        construction: &Construction,
    ) -> BTreeSet<FieldName> {
        if construction.sets_the_rest {
            return BTreeSet::new();
        }
        self.build
            .find_record(roots, name)
            .map_or_else(BTreeSet::new, |(_, decl)| {
                decl.fields
                    .iter()
                    .map(|field| field.name.clone())
                    .filter(|field| !construction.set.contains(field))
                    .collect()
            })
    }

    /// Counts the files with an include nothing satisfies, and marks the
    /// `.erl` files that see one: a use they cannot resolve is not the
    /// plugin's to explain.
    fn unresolved_includes(&self, out: &mut ReferenceSet) {
        let mut counted: HashSet<PluginFile> = HashSet::new();
        for file in self.files {
            let unresolved = file.includes.iter().any(|include| {
                file.region(include.line) != Region::TestOnly && self.is_unresolved(&include.target)
            });
            if unresolved {
                counted.insert(file.id.clone());
            }
        }
        out.count(NotChecked::UnresolvedInclude, counted.len());
        for includer in self.includers() {
            if self.closures[&includer.id]
                .iter()
                .any(|&index| counted.contains(&self.files[index].id))
            {
                out.mark_unresolved_include(includer.id.clone());
            }
        }
    }

    fn is_unresolved(&self, target: &Target) -> bool {
        match target {
            Target::Local(_) => false,
            Target::Host(header) => self.build.header(header).is_none(),
            Target::Unresolved => true,
        }
    }
}
