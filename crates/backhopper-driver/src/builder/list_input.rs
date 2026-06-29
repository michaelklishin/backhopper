// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A newline-framed list input shared by the verbs that take one:
//! `check batch` (commits) and `suites plan` (modified paths). Both
//! send the list on stdin (the CLI reads `-`) or name a file as the
//! flag value.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::stdin::StdinPayload;

#[derive(Debug, Clone)]
pub(crate) enum ListInput {
    Bytes(Vec<u8>),
    File(PathBuf),
}

impl ListInput {
    /// Push `flag` and either `-` (returning the bytes as stdin) or the
    /// file path (returning no stdin).
    pub(crate) fn apply<'a>(&'a self, flag: &str, args: &mut Vec<OsString>) -> StdinPayload<'a> {
        args.push(OsString::from(flag));
        match self {
            ListInput::Bytes(b) => {
                args.push(OsString::from("-"));
                StdinPayload::Bytes(b.as_slice())
            }
            ListInput::File(p) => {
                args.push(p.as_os_str().to_owned());
                StdinPayload::None
            }
        }
    }
}
