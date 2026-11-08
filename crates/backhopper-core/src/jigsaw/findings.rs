// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

use crate::model::app_manifest::ManifestUnreadable;
use crate::model::macro_decl::MacroKey;
use crate::model::names::{
    ApplicationName, Arity, BehaviourName, ConfKey, FieldName, Mfa, ModuleName, RecordName,
    SpecText, TypeName, ValidatorName, vocabulary,
};
use crate::model::non_empty::NonEmpty;
use crate::model::normalised::{NormalisedDefault, NormalisedMacroBody};
use crate::model::schema_files::PartialRef;
use crate::model::snapshot::{FunArity, HeaderRef};

vocabulary!(
    /// The wire label of a `Finding` variant.
    #[derive(PartialOrd, Ord, Hash)]
    pub enum JigsawFindingKind: "finding kind" {
        ModuleMissing => "module_missing",
        FunctionMissing => "function_missing",
        ReturnShapeChanged => "return_shape_changed",
        HeaderMissing => "header_missing",
        RecordMissing => "record_missing",
        RecordFieldsChanged => "record_fields_changed",
        MacroMissing => "macro_missing",
        MacroChanged => "macro_changed",
        CallbackRequired => "callback_required",
        BehaviourMissing => "behaviour_missing",
        TypeMissing => "type_missing",
        PartialMissing => "partial_missing",
        PartialKeyMissing => "partial_key_missing",
        ValidatorMissing => "validator_missing",
        AppMissing => "app_missing",
    }
);

vocabulary!(
    /// The wire label of a `Warning` variant.
    #[derive(PartialOrd, Ord, Hash)]
    pub enum JigsawWarningKind: "warning kind" {
        BundledAppShippedByHost => "bundled_app_shipped_by_host",
        RecordDefaultChanged => "record_default_changed",
        MacroTextChanged => "macro_text_changed",
        ManifestUnreadable => "manifest_unreadable",
        AppNotTracked => "app_not_tracked",
    }
);

vocabulary!(
    /// Why a comparison was not made. Each is counted in the report.
    #[derive(PartialOrd, Ord, Hash)]
    pub enum WithheldReason: "withheld reason" {
        ConditionalMacro => "conditional_macro",
        UnknownSpec => "unknown_spec",
        UnreadableManifest => "unreadable_manifest",
        UntrackedApp => "untracked_app",
        ExcludedOtpApp => "excluded_otp_app",
    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stop {
    UnreadableManifest,
    UntrackedApp,
}

impl Stop {
    #[must_use]
    pub fn reason(self) -> WithheldReason {
        match self {
            Self::UnreadableManifest => WithheldReason::UnreadableManifest,
            Self::UntrackedApp => WithheldReason::UntrackedApp,
        }
    }
}

/// The variant decides the reason, so a symbol cannot be listed under another.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WithheldSymbol {
    ConditionalMacro { header: HeaderRef, key: MacroKey },
    UnknownSpec(Mfa),
    ExcludedOtpApp(ApplicationName),
}

impl WithheldSymbol {
    #[must_use]
    pub fn reason(&self) -> WithheldReason {
        match self {
            Self::ConditionalMacro { .. } => WithheldReason::ConditionalMacro,
            Self::UnknownSpec(_) => WithheldReason::UnknownSpec,
            Self::ExcludedOtpApp(_) => WithheldReason::ExcludedOtpApp,
        }
    }
}

impl fmt::Display for WithheldSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConditionalMacro { header, key } => write!(f, "?{key} in {header}"),
            Self::UnknownSpec(mfa) => write!(f, "{mfa}"),
            Self::ExcludedOtpApp(app) => write!(f, "{app}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Withheld {
    Symbol(WithheldSymbol),
    Stopped(Stop),
}

/// A reference the build patch satisfies and a target does not. Every
/// variant is a blocker, so there is no severity to read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Finding {
    ModuleMissing {
        module: ModuleName,
    },
    FunctionMissing {
        mfa: Mfa,
        other_arities: BTreeSet<Arity>,
    },
    ReturnShapeChanged {
        mfa: Mfa,
        build_spec: SpecText,
        target_spec: SpecText,
    },
    HeaderMissing {
        header: HeaderRef,
    },
    RecordMissing {
        header: HeaderRef,
        name: RecordName,
    },
    RecordFieldsChanged {
        header: HeaderRef,
        name: RecordName,
        build: Vec<FieldName>,
        target: Vec<FieldName>,
    },
    MacroMissing {
        header: HeaderRef,
        key: MacroKey,
    },
    MacroChanged {
        header: HeaderRef,
        key: MacroKey,
        build: NormalisedMacroBody,
        target: NormalisedMacroBody,
    },
    CallbackRequired {
        behaviour: BehaviourName,
        implementer: ModuleName,
        callback: FunArity,
    },
    BehaviourMissing {
        behaviour: BehaviourName,
    },
    TypeMissing {
        module: ModuleName,
        name: TypeName,
        arity: Arity,
    },
    PartialMissing {
        partial: PartialRef,
    },
    PartialKeyMissing {
        partial: PartialRef,
        key: ConfKey,
    },
    ValidatorMissing {
        name: ValidatorName,
    },
    AppMissing {
        app: ApplicationName,
        required_by: NonEmpty<ApplicationName>,
    },
}

impl Finding {
    #[must_use]
    pub fn kind(&self) -> JigsawFindingKind {
        match self {
            Self::ModuleMissing { .. } => JigsawFindingKind::ModuleMissing,
            Self::FunctionMissing { .. } => JigsawFindingKind::FunctionMissing,
            Self::ReturnShapeChanged { .. } => JigsawFindingKind::ReturnShapeChanged,
            Self::HeaderMissing { .. } => JigsawFindingKind::HeaderMissing,
            Self::RecordMissing { .. } => JigsawFindingKind::RecordMissing,
            Self::RecordFieldsChanged { .. } => JigsawFindingKind::RecordFieldsChanged,
            Self::MacroMissing { .. } => JigsawFindingKind::MacroMissing,
            Self::MacroChanged { .. } => JigsawFindingKind::MacroChanged,
            Self::CallbackRequired { .. } => JigsawFindingKind::CallbackRequired,
            Self::BehaviourMissing { .. } => JigsawFindingKind::BehaviourMissing,
            Self::TypeMissing { .. } => JigsawFindingKind::TypeMissing,
            Self::PartialMissing { .. } => JigsawFindingKind::PartialMissing,
            Self::PartialKeyMissing { .. } => JigsawFindingKind::PartialKeyMissing,
            Self::ValidatorMissing { .. } => JigsawFindingKind::ValidatorMissing,
            Self::AppMissing { .. } => JigsawFindingKind::AppMissing,
        }
    }
}

impl Finding {
    /// The subject, printed the way a reader would search for it.
    #[must_use]
    pub fn symbol(&self) -> String {
        match self {
            Self::ModuleMissing { module } => module.to_string(),
            Self::FunctionMissing { mfa, .. } | Self::ReturnShapeChanged { mfa, .. } => {
                mfa.to_string()
            }
            Self::HeaderMissing { header } => header.to_string(),
            Self::RecordMissing { header, name }
            | Self::RecordFieldsChanged { header, name, .. } => {
                format!("#{name}{{}} in {header}")
            }
            Self::MacroMissing { header, key } | Self::MacroChanged { header, key, .. } => {
                format!("?{key} in {header}")
            }
            Self::CallbackRequired {
                behaviour,
                callback,
                ..
            } => format!("{behaviour}:{callback}"),
            Self::BehaviourMissing { behaviour } => behaviour.to_string(),
            Self::TypeMissing {
                module,
                name,
                arity,
            } => format!("{module}:{name}/{arity}"),
            Self::PartialMissing { partial } => partial.to_string(),
            Self::PartialKeyMissing { partial, key } => format!("{partial} key {key}"),
            Self::ValidatorMissing { name } => name.to_string(),
            Self::AppMissing { app, .. } => app.to_string(),
        }
    }

    /// The build and target sides, where the variant has them.
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::FunctionMissing { other_arities, .. } if !other_arities.is_empty() => {
                let arities: Vec<String> = other_arities.iter().map(|a| format!("/{a}")).collect();
                Some(format!("target has {}", arities.join(", ")))
            }
            Self::ReturnShapeChanged {
                build_spec,
                target_spec,
                ..
            } => Some(format!("build: {build_spec}; target: {target_spec}")),
            Self::RecordFieldsChanged { build, target, .. } => Some(format!(
                "build: {}; target: {}",
                join(build.iter()),
                join(target.iter())
            )),
            Self::MacroChanged { build, target, .. } => {
                Some(format!("build: {build}; target: {target}"))
            }
            Self::CallbackRequired { implementer, .. } => {
                Some(format!("{implementer} does not export it"))
            }
            Self::AppMissing { required_by, .. } => {
                Some(format!("required by {}", join(required_by.iter())))
            }
            Self::ModuleMissing { .. }
            | Self::FunctionMissing { .. }
            | Self::HeaderMissing { .. }
            | Self::RecordMissing { .. }
            | Self::MacroMissing { .. }
            | Self::BehaviourMissing { .. }
            | Self::TypeMissing { .. }
            | Self::PartialMissing { .. }
            | Self::PartialKeyMissing { .. }
            | Self::ValidatorMissing { .. } => None,
        }
    }
}

fn join<T: Display>(items: impl Iterator<Item = T>) -> String {
    items
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Something worth reading that never fails the run. A warning and a
/// finding never share a list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Warning {
    BundledAppShippedByHost {
        app: ApplicationName,
    },
    RecordDefaultChanged {
        header: HeaderRef,
        record: RecordName,
        field: FieldName,
        build: Option<NormalisedDefault>,
        target: Option<NormalisedDefault>,
    },
    MacroTextChanged {
        header: HeaderRef,
        key: MacroKey,
        build: NormalisedMacroBody,
        target: NormalisedMacroBody,
    },
    ManifestUnreadable {
        app: ApplicationName,
        reason: ManifestUnreadable,
    },
    AppNotTracked {
        app: ApplicationName,
    },
}

impl Warning {
    #[must_use]
    pub fn kind(&self) -> JigsawWarningKind {
        match self {
            Self::BundledAppShippedByHost { .. } => JigsawWarningKind::BundledAppShippedByHost,
            Self::RecordDefaultChanged { .. } => JigsawWarningKind::RecordDefaultChanged,
            Self::MacroTextChanged { .. } => JigsawWarningKind::MacroTextChanged,
            Self::ManifestUnreadable { .. } => JigsawWarningKind::ManifestUnreadable,
            Self::AppNotTracked { .. } => JigsawWarningKind::AppNotTracked,
        }
    }
}

impl Warning {
    #[must_use]
    pub fn symbol(&self) -> String {
        match self {
            Self::BundledAppShippedByHost { app } => app.to_string(),
            Self::RecordDefaultChanged { record, field, .. } => format!("#{record}.{field}"),
            Self::MacroTextChanged { header, key, .. } => format!("?{key} in {header}"),
            Self::ManifestUnreadable { app, .. } | Self::AppNotTracked { app } => app.to_string(),
        }
    }

    #[must_use]
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::BundledAppShippedByHost { .. } => None,
            Self::RecordDefaultChanged { build, target, .. } => {
                let show = |default: &Option<NormalisedDefault>| {
                    default
                        .as_ref()
                        .map_or_else(|| "undefined".to_owned(), ToString::to_string)
                };
                Some(format!("build: {}; target: {}", show(build), show(target)))
            }
            Self::MacroTextChanged { build, target, .. } => {
                Some(format!("build: {build}; target: {target}"))
            }
            Self::ManifestUnreadable { reason, .. } => Some(match reason {
                ManifestUnreadable::Missing => {
                    "no manifest file found: check the project's `manifest` setting".to_owned()
                }
                ManifestUnreadable::UnexpandedVariable(variable) => {
                    format!("the manifest uses the unexpanded variable {variable}")
                }
                ManifestUnreadable::ParseFailed => "the manifest could not be parsed".to_owned(),
            }),
            Self::AppNotTracked { .. } => {
                Some("no `[[project]]` for the dependency that provides it".to_owned())
            }
        }
    }
}

/// What checking one anchor at one target produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    pub findings: Vec<Finding>,
    pub warnings: Vec<Warning>,
    pub withheld: BTreeSet<Withheld>,
}

impl Checked {
    #[must_use]
    pub fn satisfied() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn broken(finding: Finding) -> Self {
        Self {
            findings: vec![finding],
            ..Self::default()
        }
    }

    #[must_use]
    pub fn withheld(withheld: Withheld) -> Self {
        Self {
            withheld: BTreeSet::from([withheld]),
            ..Self::default()
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.findings.extend(other.findings);
        self.warnings.extend(other.warnings);
        self.withheld.extend(other.withheld);
    }
}
