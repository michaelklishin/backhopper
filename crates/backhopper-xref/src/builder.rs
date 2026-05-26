// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `XrefBuilder`: collects modules and produces an [`Xref`].

use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::path::PathBuf;

use backhopper_core::{ApplicationName, Mfa, ModuleName};
use backhopper_xref_graph::{
    Building, CallGraph, CallTarget, FunctionRef, Functions, LocalFunctionRef, ModuleSummary,
    VertexSet,
};
use backhopper_xref_reader::{ModuleData, ProjectLayout, ReadOutput, SourceReader};

use crate::errors::XrefError;
use crate::xref::Xref;

#[derive(Debug, Clone, Default)]
pub struct XrefBuilder {
    layout: ProjectLayout,
    modules: BTreeMap<ModuleName, ModuleData>,
    applications: BTreeSet<ApplicationName>,
    builtins: VertexSet,
    warnings: Vec<backhopper_xref_reader::ReadWarning>,
}

impl XrefBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_layout(mut self, layout: ProjectLayout) -> Self {
        self.layout = layout;
        self
    }

    #[must_use]
    pub fn with_builtins(mut self, b: VertexSet) -> Self {
        self.builtins = b;
        self
    }

    /// Add an application from in-memory `(path, bytes)` pairs.
    pub fn add_application<I>(
        &mut self,
        app: ApplicationName,
        files: I,
    ) -> Result<&mut Self, XrefError>
    where
        I: IntoIterator<Item = (PathBuf, Vec<u8>)>,
    {
        if !self.applications.insert(app.clone()) {
            return Err(XrefError::DuplicateApplication(app));
        }
        let reader = SourceReader::with_layout(self.layout.clone());
        let ReadOutput {
            modules,
            warnings,
            paths: _,
        } = reader.read_tree(files)?;
        self.warnings.extend(warnings);
        for m in modules {
            if let Some(prev) = self.modules.insert(m.module.clone(), m) {
                let name = prev.module.clone();
                let new_app = self
                    .modules
                    .get(&name)
                    .and_then(|d| d.application.assigned())
                    .map(|a| a.as_str().to_owned());
                let prev_app = prev.application.assigned().map(|a| a.as_str().to_owned());
                self.warnings.push(
                    backhopper_xref_reader::ReadWarning::DuplicateModuleAcrossApplications {
                        name,
                        first_application: prev_app,
                        second_application: new_app,
                    },
                );
            }
        }
        Ok(self)
    }

    pub fn add_modules<I>(&mut self, modules: I) -> &mut Self
    where
        I: IntoIterator<Item = ModuleData>,
    {
        for m in modules {
            self.modules.insert(m.module.clone(), m);
        }
        self
    }

    pub fn warnings(&self) -> &[backhopper_xref_reader::ReadWarning] {
        &self.warnings
    }

    pub fn build(self) -> Result<Xref<Functions>, XrefError> {
        let mut graph: CallGraph<Functions, Building> = CallGraph::new();
        graph.set_builtins(self.builtins);
        let mut modules = self.modules;
        for (name, data) in &modules {
            let summary = ModuleSummary {
                application: data.application.assigned().cloned(),
                exports: data.exports.clone(),
                locals: data.locals.clone(),
                on_load: data.on_load.clone(),
                behaviours: data.behaviours.clone(),
                callbacks_required: data.callbacks.clone(),
                callbacks_optional: data.optional_callbacks.clone(),
            };
            graph.insert_module(name.clone(), summary);
        }
        for (_, data) in mem::take(&mut modules) {
            let module = data.module;
            for (sig, loc) in data.definitions {
                let mfa = Mfa::new(module.clone(), sig.name, sig.arity);
                graph.record_definition(mfa, loc);
            }
            if let Some(sig) = data.on_load {
                graph.record_on_load(Mfa::new(module.clone(), sig.name, sig.arity));
            }
            for (sig, dep) in data.deprecated {
                graph.record_deprecation(Mfa::new(module.clone(), sig.name, sig.arity), dep);
            }
            for cs in data.local_calls {
                if let CallTarget::Local(LocalFunctionRef::Concrete { function, arity }) = cs.callee
                {
                    let caller = Mfa::new(module.clone(), cs.caller.name, cs.caller.arity);
                    let callee = Mfa::new(module.clone(), function, arity);
                    graph.insert_local_call(caller, callee);
                }
            }
            for cs in data.external_calls {
                if let CallTarget::External(FunctionRef::Concrete(callee)) = cs.callee {
                    let caller = Mfa::new(module.clone(), cs.caller.name, cs.caller.arity);
                    graph.insert_external_call(caller, callee);
                }
            }
            for u in data.unresolved {
                graph.insert_unresolved(u);
            }
        }
        Ok(Xref::from_graph(graph.finish(), self.warnings))
    }
}
