//! `XrefBuilder`: collects modules and produces an [`Xref`](crate::xref::Xref).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use backhopper_core::{ApplicationName, ModuleName};
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

    pub fn with_layout(mut self, layout: ProjectLayout) -> Self {
        self.layout = layout;
        self
    }

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
        for (name, data) in &self.modules {
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
        for data in self.modules.values() {
            let module = &data.module;
            for (sig, loc) in &data.definitions {
                let mfa = data.function_mfa(sig);
                graph.record_definition(mfa, loc.clone());
            }
            if let Some(sig) = &data.on_load {
                graph.record_on_load(data.function_mfa(sig));
            }
            for (sig, dep) in &data.deprecated {
                graph.record_deprecation(data.function_mfa(sig), dep.clone());
            }
            for cs in &data.local_calls {
                if let CallTarget::Local(LocalFunctionRef::Concrete { function, arity }) =
                    &cs.callee
                {
                    let caller = data.function_mfa(&cs.caller);
                    let callee =
                        backhopper_core::Mfa::new(module.clone(), function.clone(), *arity);
                    graph.insert_local_call(caller, callee);
                }
            }
            for cs in &data.external_calls {
                if let CallTarget::External(FunctionRef::Concrete(callee)) = &cs.callee {
                    let caller = data.function_mfa(&cs.caller);
                    graph.insert_external_call(caller, callee.clone());
                }
            }
            for u in &data.unresolved {
                graph.insert_unresolved(u.clone());
            }
        }
        Ok(Xref::from_graph(graph.finish(), self.warnings))
    }
}
