//! Visibility heuristics.
//!
//! A module is `public` unless:
//!  * its source contains `%% @hidden` or `-doc(hidden).`
//!  * the project's config explicitly lists it as internal
//!  * its `-export` is wrapped in `-ifdef(TEST).` (then `test_only`)

use backhopper_core::Snapshot;
use backhopper_core::model::names::ModuleName;
use backhopper_core::model::snapshot::Visibility;

pub fn detect_visibility_hints(source: &str) -> VisibilityHints {
    let mut hints = VisibilityHints::default();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("%% @hidden") || trimmed.starts_with("%%@hidden") {
            hints.hidden = true;
        } else if trimmed.starts_with("-doc(hidden)") {
            hints.hidden = true;
        }
    }
    hints
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VisibilityHints {
    pub hidden: bool,
}

pub fn classify(
    module: &ModuleName,
    hints: VisibilityHints,
    test_only: bool,
    public_modules: &[String],
    internal_modules: &[String],
) -> Visibility {
    if internal_modules.iter().any(|n| n == module.as_str()) {
        return Visibility::Hidden;
    }
    if hints.hidden && !public_modules.iter().any(|n| n == module.as_str()) {
        return Visibility::Hidden;
    }
    if test_only {
        return Visibility::TestOnly;
    }
    Visibility::Public
}

pub fn _unused_marker(_s: &Snapshot) {}
