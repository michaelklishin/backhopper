//! Application-membership resolution.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use backhopper_core::ApplicationName;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ApplicationAssignment {
    Assigned(ApplicationName),
    NoLayoutMatch { path: PathBuf },
    Ambiguous { candidates: Vec<ApplicationName> },
}

impl ApplicationAssignment {
    pub fn assigned(&self) -> Option<&ApplicationName> {
        if let ApplicationAssignment::Assigned(a) = self {
            Some(a)
        } else {
            None
        }
    }
}

/// Maps file paths under a repository root to application names.
///
/// Common RabbitMQ patterns:
/// * `deps/<app>/src/<module>.erl`
/// * `apps/<app>/src/<module>.erl`
///
/// `ProjectLayout::rabbitmq_main()` configures both prefixes by default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectLayout {
    prefixes: Vec<PathBuf>,
}

impl ProjectLayout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rabbitmq_main() -> Self {
        let mut l = Self::new();
        l.add_prefix("deps");
        l.add_prefix("apps");
        l
    }

    pub fn add_prefix(&mut self, prefix: impl Into<PathBuf>) -> &mut Self {
        self.prefixes.push(prefix.into());
        self
    }

    /// Resolve a source-file path to its application. Returns `NoLayoutMatch`
    /// when the path is not under any known prefix.
    pub fn resolve(&self, path: &Path) -> ApplicationAssignment {
        let mut candidates: Vec<ApplicationName> = Vec::new();
        for prefix in &self.prefixes {
            if let Some(app) = match_under_prefix(prefix, path) {
                candidates.push(app);
            }
        }
        candidates.sort();
        candidates.dedup();
        match candidates.len() {
            0 => ApplicationAssignment::NoLayoutMatch {
                path: path.to_path_buf(),
            },
            1 => ApplicationAssignment::Assigned(candidates.remove(0)),
            _ => ApplicationAssignment::Ambiguous { candidates },
        }
    }
}

fn match_under_prefix(prefix: &Path, path: &Path) -> Option<ApplicationName> {
    // Find `<prefix...>/<app>/...` via OsStr equality so non-UTF-8 paths are safe.
    let components: Vec<&OsStr> = path.components().map(|c| c.as_os_str()).collect();
    let prefix_os: Vec<&OsStr> = prefix.components().map(|c| c.as_os_str()).collect();
    if prefix_os.is_empty() {
        return None;
    }
    let plen = prefix_os.len();
    let mut i = 0;
    while i + plen < components.len() {
        if components[i..i + plen] == prefix_os[..] {
            let app = components[i + plen].to_string_lossy();
            return ApplicationName::new(app.into_owned()).ok();
        }
        i += 1;
    }
    None
}
