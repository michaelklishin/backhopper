// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Conditional compilation tracker for `-ifdef`/`-ifndef`/`-if`/`-else`/`-endif`.
//!
//! Semantics: every conditional branch is recorded (we want symbols visible to
//! queries with optional include-hidden / include-test). Only `-ifdef(TEST)`
//! flips a `test_only` tag on entries inside it. `-else` flips that tag.

#[derive(Debug, Clone, Default)]
pub struct CondStack {
    frames: Vec<Frame>,
}

#[derive(Debug, Clone)]
struct Frame {
    is_test_only: bool,
    // Only `-ifdef(TEST)` and `-ifndef(TEST)` make `-else` toggle the test-only tag.
    // For other idents, the else branch is neither more nor less test-only than the if branch.
    else_flips_test_only: bool,
}

impl CondStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_test_only(&self) -> bool {
        self.frames.iter().any(|f| f.is_test_only)
    }

    pub fn push_ifdef(&mut self, ident: &str) {
        let is_test = ident == "TEST";
        self.frames.push(Frame {
            is_test_only: is_test,
            else_flips_test_only: is_test,
        });
    }

    pub fn push_ifndef(&mut self, ident: &str) {
        let is_test = ident == "TEST";
        self.frames.push(Frame {
            is_test_only: false,
            else_flips_test_only: is_test,
        });
    }

    pub fn push_if(&mut self, _expr: &str) {
        self.frames.push(Frame {
            is_test_only: false,
            else_flips_test_only: false,
        });
    }

    pub fn flip_else(&mut self) {
        if let Some(f) = self.frames.last_mut()
            && f.else_flips_test_only
        {
            f.is_test_only = !f.is_test_only;
        }
    }

    pub fn pop_endif(&mut self) {
        self.frames.pop();
    }
}
