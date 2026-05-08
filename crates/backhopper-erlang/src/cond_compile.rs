//! Conditional compilation tracker for `-ifdef`/`-ifndef`/`-if`/`-else`/`-endif`.
//!
//! Semantics: every conditional branch is recorded (we want symbols visible to
//! queries with optional include-hidden / include-test). Only `-ifdef(TEST)`
//! flips a `test_only` tag on entries inside it. `-else` flips that tag.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CondState {
    Active,
}

#[derive(Debug, Clone)]
pub struct CondStack {
    frames: Vec<Frame>,
}

#[derive(Debug, Clone)]
struct Frame {
    is_test_only: bool,
}

impl CondStack {
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    pub fn current(&self) -> CondState {
        CondState::Active
    }

    pub fn is_test_only(&self) -> bool {
        self.frames.iter().any(|f| f.is_test_only)
    }

    pub fn push_ifdef(&mut self, ident: &str) {
        self.frames.push(Frame {
            is_test_only: ident == "TEST",
        });
    }

    pub fn push_ifndef(&mut self, _ident: &str) {
        self.frames.push(Frame {
            is_test_only: false,
        });
    }

    pub fn push_if(&mut self, _expr: &str) {
        self.frames.push(Frame {
            is_test_only: false,
        });
    }

    pub fn flip_else(&mut self) {
        if let Some(f) = self.frames.last_mut() {
            f.is_test_only = !f.is_test_only;
        }
    }

    pub fn pop_endif(&mut self) {
        self.frames.pop();
    }
}

impl Default for CondStack {
    fn default() -> Self {
        Self::new()
    }
}
