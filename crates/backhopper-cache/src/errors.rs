// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("cache i/o error: {0}")]
    Io(#[from] io::Error),
}
