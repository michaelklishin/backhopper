// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Standard input source for a verb invocation.

use std::fmt::{self, Debug, Formatter};
use std::io::Read;

/// Standard-input payload for a single invocation.
///
/// `Bytes` is the common case (patches, commit lists). `Reader` is
/// for streaming inputs that do not fit comfortably in memory.
/// `None` skips the stdin pipe entirely.
#[non_exhaustive]
#[derive(Default)]
pub enum StdinPayload<'a> {
    /// No standard input.
    #[default]
    None,
    /// Pre-buffered bytes piped verbatim.
    Bytes(&'a [u8]),
    /// Bytes pulled from a reader. The backend reads to end-of-file
    /// before waiting for the child.
    Reader(&'a mut dyn Read),
}

impl Debug for StdinPayload<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("StdinPayload::None"),
            Self::Bytes(b) => write!(f, "StdinPayload::Bytes({} bytes)", b.len()),
            Self::Reader(_) => f.write_str("StdinPayload::Reader(_)"),
        }
    }
}

impl StdinPayload<'_> {
    /// True when no stdin will be piped.
    #[must_use]
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}
