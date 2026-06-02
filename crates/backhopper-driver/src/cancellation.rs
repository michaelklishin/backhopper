// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Cooperative cancellation.
//!
//! A [`CancellationToken`] is `Clone` and freely shareable across
//! threads; an [`Aborter`] (returned alongside the token at
//! construction) flips the flag. The backend polls the token between
//! output reads and, when set, signals the child process.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Cooperative cancellation token. Cheap to clone; share freely.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

/// Handle that flips a [`CancellationToken`]. Held by the caller
/// (often a signal handler).
#[derive(Debug, Clone)]
#[must_use = "an Aborter signals cancellation only when used; \
              dropping it without calling abort() leaves the token live"]
pub struct Aborter {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Construct a paired token and aborter.
    ///
    /// ```
    /// use backhopper_driver::CancellationToken;
    /// let (token, aborter) = CancellationToken::new();
    /// assert!(!token.is_cancelled());
    /// aborter.abort();
    /// assert!(token.is_cancelled());
    /// ```
    pub fn new() -> (Self, Aborter) {
        let flag = Arc::new(AtomicBool::new(false));
        (Self { flag: flag.clone() }, Aborter { flag })
    }

    /// True once any matching [`Aborter`] has been triggered.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// Spin-wait until cancellation is signalled or `timeout`
    /// elapses. Returns `true` if cancellation tripped, `false` on
    /// timeout. With `timeout = None` waits forever.
    ///
    /// The polling interval is small and bounded; not appropriate
    /// for long sleeps but adequate for the millisecond-scale
    /// shutdown coordination the backend uses internally.
    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        let deadline = timeout.map(|d| Instant::now() + d);
        let interval = Duration::from_millis(5);
        loop {
            if self.is_cancelled() {
                return true;
            }
            match deadline {
                Some(d) if Instant::now() >= d => return false,
                Some(d) => {
                    let remaining = d.saturating_duration_since(Instant::now());
                    thread::sleep(interval.min(remaining));
                }
                None => thread::sleep(interval),
            }
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new().0
    }
}

impl Aborter {
    /// Signal cancellation. Idempotent.
    pub fn abort(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// True if the matching [`CancellationToken`] is still alive
    /// in any clone.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        Arc::strong_count(&self.flag) > 1
    }
}
