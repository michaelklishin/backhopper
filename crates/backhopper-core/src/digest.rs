// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

/// Length-prefixed fields, so two field lists never feed the hasher the same bytes.
pub(crate) struct DigestWriter(blake3::Hasher);

impl DigestWriter {
    pub(crate) fn new(domain: &str) -> Self {
        let mut writer = Self(blake3::Hasher::new());
        writer.text(domain);
        writer
    }

    pub(crate) fn text(&mut self, value: &str) -> &mut Self {
        self.count(value.len());
        self.0.update(value.as_bytes());
        self
    }

    pub(crate) fn count(&mut self, count: usize) -> &mut Self {
        self.0.update(&(count as u64).to_le_bytes());
        self
    }

    pub(crate) fn finish(&self) -> [u8; 32] {
        *self.0.finalize().as_bytes()
    }
}
