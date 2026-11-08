// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Builders for hand-made snapshots that carry apps, headers, macros and
//! Cuttlefish elements, for the `jigsaw` tests.

use std::collections::BTreeSet;

use backhopper_core::model::app_manifest::{
    AppManifest, ManifestDeps, ManifestSource, ManifestUnreadable,
};
use backhopper_core::model::macro_decl::{MacroBody, MacroDecl, MacroKey};
use backhopper_core::model::names::{
    ApplicationName, Arity, ConfKey, FieldName, FunctionName, MacroName, ModuleName, PartialName,
    RecordName, RelativePath, TypeName, ValidatorName,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::normalised::{NormalisedDefault, NormalisedMacroBody};
use backhopper_core::model::schema_files::{PartialFile, SchemaFile};
use backhopper_core::model::snapshot::{
    CallbackSig, FunArity, HeaderRef, HostReleaseFacts, HrlFile, Module, RecordDecl, RecordField,
    Snapshot, SpecSig, TypeArity, state,
};

use crate::snapshot_header;

fn app_name(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

/// A snapshot at format 5 with apps, modules, headers and schema files.
#[derive(Debug, Clone)]
pub struct SnapshotBuilder {
    project: String,
    tag: String,
    roots: Vec<ApplicationName>,
    apps: Vec<AppManifest>,
    modules: Vec<Module>,
    headers: Vec<HrlFile>,
    schemas: Vec<SchemaFile>,
    partials: Vec<PartialFile>,
}

impl SnapshotBuilder {
    #[must_use]
    pub fn new(project: &str, tag: &str) -> Self {
        Self {
            project: project.to_owned(),
            tag: tag.to_owned(),
            roots: Vec::new(),
            apps: Vec::new(),
            modules: Vec::new(),
            headers: Vec::new(),
            schemas: Vec::new(),
            partials: Vec::new(),
        }
    }

    #[must_use]
    pub fn roots(mut self, roots: &[&str]) -> Self {
        self.roots = roots.iter().map(|name| app_name(name)).collect();
        self
    }

    #[must_use]
    pub fn app(mut self, name: &str, requires: &[&str]) -> Self {
        self.apps.push(AppManifest {
            name: app_name(name),
            source: ManifestSource::ErlangMk,
            requires: ManifestDeps::Listed(requires.iter().map(|app| app_name(app)).collect()),
        });
        self
    }

    #[must_use]
    pub fn unreadable_app(mut self, name: &str) -> Self {
        self.apps.push(AppManifest {
            name: app_name(name),
            source: ManifestSource::ErlangMk,
            requires: ManifestDeps::Unreadable(ManifestUnreadable::ParseFailed),
        });
        self
    }

    #[must_use]
    pub fn module(mut self, app: &str, mut module: Module) -> Self {
        module.app = Some(app_name(app));
        module.path = RelativePath::new(format!("deps/{app}/src/{}.erl", module.name)).ok();
        self.modules.push(module);
        self
    }

    #[must_use]
    pub fn header(mut self, header: HrlFile) -> Self {
        self.headers.push(header);
        self
    }

    #[must_use]
    pub fn partial(mut self, app: &str, name: &str, keys: &[&str], validators: &[&str]) -> Self {
        self.partials.push(PartialFile {
            path: RelativePath::new(format!("deps/{app}/priv/schema/{name}.partial")).unwrap(),
            app: app_name(app),
            name: PartialName::new(name).unwrap(),
            mappings: keys.iter().map(|k| ConfKey::new(*k).unwrap()).collect(),
            validators: validators
                .iter()
                .map(|v| ValidatorName::new(*v).unwrap())
                .collect(),
        });
        self
    }

    #[must_use]
    pub fn schema(mut self, app: &str, name: &str, keys: &[&str], validators: &[&str]) -> Self {
        self.schemas.push(SchemaFile {
            path: RelativePath::new(format!("deps/{app}/priv/schema/{name}.schema")).unwrap(),
            app: app_name(app),
            mappings: keys.iter().map(|k| ConfKey::new(*k).unwrap()).collect(),
            validators: validators
                .iter()
                .map(|v| ValidatorName::new(*v).unwrap())
                .collect(),
        });
        self
    }

    #[must_use]
    pub fn build(self) -> Snapshot<state::Canonical> {
        let mut header = snapshot_header(&self.project, &self.tag);
        header.format_version = 5;
        "9".clone_into(&mut header.extractor_version);
        if let Ok(roots) = NonEmpty::try_from(self.roots) {
            header.host_facts = Some(HostReleaseFacts {
                release_roots: roots,
                digest: backhopper_core::model::names::HostFactsDigest::new("a".repeat(64))
                    .unwrap(),
            });
        }
        Snapshot::from_extracted(header, self.modules, self.headers)
            .with_apps(self.apps)
            .with_cuttlefish(self.schemas, self.partials)
            .into_canonical()
    }
}

#[must_use]
pub fn exports(mut module: Module, exports: &[(&str, u8)]) -> Module {
    for (name, arity) in exports {
        module.exports.push(FunArity {
            name: FunctionName::new(*name).unwrap(),
            arity: Arity::new(*arity),
        });
    }
    module.exports.sort();
    module
}

#[must_use]
pub fn with_spec(mut module: Module, name: &str, arity: u8, signature: &str) -> Module {
    module.specs.push(SpecSig {
        name: FunctionName::new(name).unwrap(),
        arity: Arity::new(arity),
        signature: signature.to_owned(),
    });
    module
}

#[must_use]
pub fn with_callbacks(
    mut module: Module,
    callbacks: &[(&str, u8)],
    optional: &[(&str, u8)],
) -> Module {
    for (name, arity) in callbacks {
        module.callbacks.push(CallbackSig {
            name: FunctionName::new(*name).unwrap(),
            arity: Arity::new(*arity),
            signature: format!("{name}() -> ok"),
        });
    }
    for (name, arity) in optional {
        module.optional_callbacks.push(FunArity {
            name: FunctionName::new(*name).unwrap(),
            arity: Arity::new(*arity),
        });
    }
    module
}

#[must_use]
pub fn with_exported_types(mut module: Module, types: &[(&str, u8)]) -> Module {
    for (name, arity) in types {
        module.export_types.push(TypeArity {
            name: TypeName::new(*name).unwrap(),
            arity: Arity::new(*arity),
        });
    }
    module.export_types.sort();
    module
}

/// A header of `app` at `path` in the tree, such as
/// `deps/rabbit_common/include/rabbit.hrl`.
#[must_use]
pub fn hrl(app: &str, path: &str) -> HrlFile {
    let mut file = HrlFile::new(RelativePath::new(path).unwrap());
    file.app = Some(app_name(app));
    file
}

#[must_use]
pub fn including(mut file: HrlFile, app: &str, app_path: &str) -> HrlFile {
    file.includes.push(HeaderRef {
        app: app_name(app),
        app_path: RelativePath::new(app_path).unwrap(),
    });
    file
}

/// An object-like macro with a text body.
#[must_use]
pub fn defining(mut file: HrlFile, name: &str, arity: Option<u8>, body: &str) -> HrlFile {
    file.defines.push(MacroDecl {
        key: MacroKey {
            name: MacroName::new(name).unwrap(),
            arity: arity.map(Arity::new),
        },
        body: MacroBody::Text(NormalisedMacroBody::from_source(body)),
    });
    file
}

/// A macro defined under a build conditional.
#[must_use]
pub fn defining_conditional(mut file: HrlFile, name: &str, arity: Option<u8>) -> HrlFile {
    file.defines.push(MacroDecl {
        key: MacroKey {
            name: MacroName::new(name).unwrap(),
            arity: arity.map(Arity::new),
        },
        body: MacroBody::Conditional,
    });
    file
}

#[must_use]
pub fn with_record(mut file: HrlFile, name: &str, fields: &[(&str, Option<&str>)]) -> HrlFile {
    file.records.push(RecordDecl {
        name: RecordName::new(name).unwrap(),
        fields: fields
            .iter()
            .map(|(field, default)| RecordField {
                name: FieldName::new(*field).unwrap(),
                type_repr: None,
                default: default.and_then(NormalisedDefault::from_source),
            })
            .collect(),
    });
    file
}

#[must_use]
pub fn app_names(names: &[&str]) -> BTreeSet<ApplicationName> {
    names.iter().map(|name| app_name(name)).collect()
}

#[must_use]
pub fn module_name(name: &str) -> ModuleName {
    ModuleName::new(name).unwrap()
}
