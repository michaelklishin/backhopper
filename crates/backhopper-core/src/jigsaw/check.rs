// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;

use crate::jigsaw::anchor::{
    AppAnchor, BehaviourAnchor, CallAnchor, HostAnchor, MacroAnchor, OtpAnchor, PartialAnchor,
    RecordAnchor, TypeAnchor, find_spec, required_callbacks,
};
use crate::jigsaw::findings::{Checked, Finding, Warning, Withheld, WithheldSymbol};
use crate::jigsaw::surface::{AppPresence, Lookup, OtpAppPresence, OtpSurface, ReleaseSurface};
use crate::model::macro_decl::MacroBody;
use crate::model::names::{Arity, Mfa};
use crate::model::normalised::NormalisedDefault;
use crate::model::snapshot::Module;
use crate::model::spec_ast::SpecType;
use crate::model::spec_parser::parse_signature_return;

fn other_arities(module: &Module, mfa: &Mfa) -> BTreeSet<Arity> {
    module
        .exports
        .iter()
        .filter(|export| export.name == mfa.function)
        .map(|export| export.arity)
        .collect()
}

fn missing_function(module: &Module, mfa: &Mfa) -> Finding {
    Finding::FunctionMissing {
        mfa: mfa.clone(),
        other_arities: other_arities(module, mfa),
    }
}

fn app_missing(anchor: &AppAnchor) -> Finding {
    Finding::AppMissing {
        app: anchor.app.clone(),
        required_by: anchor.required_by.clone(),
    }
}

fn app_presence(presence: AppPresence, anchor: &AppAnchor) -> Checked {
    match presence {
        AppPresence::Provided => Checked::satisfied(),
        AppPresence::Absent => Checked::broken(app_missing(anchor)),
        AppPresence::Stopped(stop) => Checked::withheld(Withheld::Stopped(stop)),
    }
}

fn otp_app_presence(presence: OtpAppPresence, anchor: &AppAnchor) -> Checked {
    match presence {
        OtpAppPresence::Provided => Checked::satisfied(),
        OtpAppPresence::Absent => Checked::broken(app_missing(anchor)),
        OtpAppPresence::Excluded => Checked::withheld(Withheld::Symbol(
            WithheldSymbol::ExcludedOtpApp(anchor.app.clone()),
        )),
    }
}

/// Checks one host anchor against one host release.
#[must_use]
pub fn check_host(anchor: &HostAnchor, target: &ReleaseSurface<'_>) -> Checked {
    match anchor {
        HostAnchor::Call(call) => check_call(call, target),
        HostAnchor::Header(header) => {
            if target.header(header).is_some() {
                Checked::satisfied()
            } else {
                Checked::broken(Finding::HeaderMissing {
                    header: header.clone(),
                })
            }
        }
        HostAnchor::Record(record) => check_record(record, target),
        HostAnchor::Macro(macro_use) => check_macro(macro_use, target),
        HostAnchor::Behaviour(behaviour) => check_behaviour(behaviour, target),
        HostAnchor::Type(type_ref) => check_type(type_ref, target),
        HostAnchor::Partial(partial) => check_partial(partial, target),
        HostAnchor::Validator(name) => match target.validator(name) {
            Lookup::Found(_) => Checked::satisfied(),
            Lookup::Missing => Checked::broken(Finding::ValidatorMissing { name: name.clone() }),
            Lookup::Stopped(stop) => Checked::withheld(Withheld::Stopped(stop)),
        },
        HostAnchor::App(app) => app_presence(target.shipped().lookup(&app.app), app),
    }
}

/// Checks one OTP anchor against one OTP tag.
#[must_use]
pub fn check_otp(anchor: &OtpAnchor, target: &OtpSurface<'_>) -> Checked {
    match anchor {
        OtpAnchor::Call(mfa) => match target.module(&mfa.module) {
            None => Checked::broken(Finding::ModuleMissing {
                module: mfa.module.clone(),
            }),
            Some(module) if module.exports_function(&mfa.function, mfa.arity) => {
                Checked::satisfied()
            }
            Some(module) => Checked::broken(missing_function(module, mfa)),
        },
        OtpAnchor::Type(type_ref) => {
            let present = target
                .module(&type_ref.module)
                .is_some_and(|module| module.exports_type(&type_ref.name, type_ref.arity));
            if present {
                Checked::satisfied()
            } else {
                Checked::broken(type_missing(type_ref))
            }
        }
        OtpAnchor::Behaviour { name, module } => {
            if target.module(module).is_some() {
                Checked::satisfied()
            } else {
                Checked::broken(Finding::BehaviourMissing {
                    behaviour: name.clone(),
                })
            }
        }
        OtpAnchor::App(app) => otp_app_presence(target.app(&app.app), app),
    }
}

fn type_missing(type_ref: &TypeAnchor) -> Finding {
    Finding::TypeMissing {
        module: type_ref.module.clone(),
        name: type_ref.name.clone(),
        arity: type_ref.arity,
    }
}

fn check_call(anchor: &CallAnchor, target: &ReleaseSurface<'_>) -> Checked {
    let mfa = &anchor.mfa;
    let Some(entry) = target.module(&mfa.module) else {
        return Checked::broken(Finding::ModuleMissing {
            module: mfa.module.clone(),
        });
    };
    if !entry.module.exports_function(&mfa.function, mfa.arity) {
        return Checked::broken(missing_function(entry.module, mfa));
    }
    let Some(build_spec) = &anchor.spec else {
        return Checked::satisfied();
    };
    let Some(target_spec) = find_spec(entry.module, &mfa.function, mfa.arity) else {
        return Checked::satisfied();
    };
    if &target_spec == build_spec {
        return Checked::satisfied();
    }
    let build_shape = parse_signature_return(build_spec.as_str());
    let target_shape = parse_signature_return(target_spec.as_str());
    if matches!(build_shape, SpecType::Unknown) || matches!(target_shape, SpecType::Unknown) {
        return Checked::withheld(Withheld::Symbol(WithheldSymbol::UnknownSpec(mfa.clone())));
    }
    if target_shape.matches(&build_shape) {
        Checked::satisfied()
    } else {
        Checked::broken(Finding::ReturnShapeChanged {
            mfa: mfa.clone(),
            build_spec: build_spec.clone(),
            target_spec,
        })
    }
}

/// A missing default and a written `undefined` compile the same.
fn default_value(default: Option<&NormalisedDefault>) -> Option<&NormalisedDefault> {
    default.filter(|value| value.as_str() != "undefined")
}

fn check_record(anchor: &RecordAnchor, target: &ReleaseSurface<'_>) -> Checked {
    let Some((_, decl)) = target.find_record(&anchor.roots, &anchor.name) else {
        return if target.has_any_header(&anchor.roots) {
            Checked::broken(Finding::RecordMissing {
                header: anchor.header.clone(),
                name: anchor.name.clone(),
            })
        } else {
            Checked::satisfied()
        };
    };
    let target_fields: Vec<_> = decl.fields.iter().map(|f| f.name.clone()).collect();
    let mut checked = Checked::satisfied();
    if target_fields != anchor.fields {
        checked.findings.push(Finding::RecordFieldsChanged {
            header: anchor.header.clone(),
            name: anchor.name.clone(),
            build: anchor.fields.clone(),
            target: target_fields,
        });
    }
    for field in &anchor.unset {
        let Some(build_default) = anchor.defaults.get(field) else {
            continue;
        };
        let Some(target_field) = decl.fields.iter().find(|f| &f.name == field) else {
            continue;
        };
        if default_value(build_default.as_ref()) != default_value(target_field.default.as_ref()) {
            checked.warnings.push(Warning::RecordDefaultChanged {
                header: anchor.header.clone(),
                record: anchor.name.clone(),
                field: field.clone(),
                build: build_default.clone(),
                target: target_field.default.clone(),
            });
        }
    }
    checked
}

fn check_macro(anchor: &MacroAnchor, target: &ReleaseSurface<'_>) -> Checked {
    let mut checked = Checked::satisfied();
    for entry in &anchor.entries {
        let Some((_, decl)) = target.find_macro(&anchor.roots, &entry.key) else {
            if target.has_any_header(&anchor.roots) {
                checked.findings.push(Finding::MacroMissing {
                    header: entry.header.clone(),
                    key: entry.key.clone(),
                });
            }
            continue;
        };
        match (&entry.body, &decl.body) {
            (MacroBody::Text(build), MacroBody::Text(found))
                if build != found && build.is_string_literal() && found.is_string_literal() =>
            {
                checked.warnings.push(Warning::MacroTextChanged {
                    header: entry.header.clone(),
                    key: entry.key.clone(),
                    build: build.clone(),
                    target: found.clone(),
                });
            }
            (MacroBody::Text(build), MacroBody::Text(found)) if build != found => {
                checked.findings.push(Finding::MacroChanged {
                    header: entry.header.clone(),
                    key: entry.key.clone(),
                    build: build.clone(),
                    target: found.clone(),
                });
            }
            (MacroBody::Text(_), MacroBody::Text(_)) => {}
            (MacroBody::Conditional, _) | (_, MacroBody::Conditional) => {
                checked
                    .withheld
                    .insert(Withheld::Symbol(WithheldSymbol::ConditionalMacro {
                        header: entry.header.clone(),
                        key: entry.key.clone(),
                    }));
            }
        }
    }
    checked
}

fn check_behaviour(anchor: &BehaviourAnchor, target: &ReleaseSurface<'_>) -> Checked {
    let Some(entry) = target.module(&anchor.module) else {
        return Checked::broken(Finding::BehaviourMissing {
            behaviour: anchor.behaviour.clone(),
        });
    };
    let mut checked = Checked::satisfied();
    for callback in required_callbacks(entry.module) {
        if !anchor.build_required.contains(&callback)
            && !anchor.implementer_exports.contains(&callback)
        {
            checked.findings.push(Finding::CallbackRequired {
                behaviour: anchor.behaviour.clone(),
                implementer: anchor.implementer.clone(),
                callback,
            });
        }
    }
    checked
}

fn check_type(anchor: &TypeAnchor, target: &ReleaseSurface<'_>) -> Checked {
    let present = target
        .module(&anchor.module)
        .is_some_and(|entry| entry.module.exports_type(&anchor.name, anchor.arity));
    if present {
        Checked::satisfied()
    } else {
        Checked::broken(type_missing(anchor))
    }
}

fn check_partial(anchor: &PartialAnchor, target: &ReleaseSurface<'_>) -> Checked {
    match target.partial(&anchor.partial) {
        Lookup::Found(file) => {
            let findings = anchor
                .keys
                .difference(&file.mappings)
                .map(|key| Finding::PartialKeyMissing {
                    partial: anchor.partial.clone(),
                    key: key.clone(),
                })
                .collect();
            Checked {
                findings,
                ..Checked::default()
            }
        }
        Lookup::Missing => Checked::broken(Finding::PartialMissing {
            partial: anchor.partial.clone(),
        }),
        Lookup::Stopped(stop) => Checked::withheld(Withheld::Stopped(stop)),
    }
}
