// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use thiserror::Error;

#[derive(Debug, Error)]
#[error("{0:?} is not a worker count: expected a whole number of 1 or more")]
pub struct JobsError(String);

const DEFAULT_CEILING: NonZeroUsize = NonZeroUsize::new(32).unwrap();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jobs(NonZeroUsize);

impl Jobs {
    #[must_use]
    pub fn available() -> Self {
        let cores = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        Self(cores.min(DEFAULT_CEILING))
    }

    #[must_use]
    pub fn or_available(requested: Option<Self>) -> Self {
        requested.unwrap_or_else(Self::available)
    }

    #[must_use]
    pub fn get(self) -> usize {
        self.0.get()
    }

    fn workers_for(self, items: usize) -> usize {
        self.0.get().min(items)
    }
}

impl FromStr for Jobs {
    type Err = JobsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<NonZeroUsize>()
            .map(Self)
            .map_err(|_| JobsError(s.to_owned()))
    }
}

/// Results come back in the order of the items. `init` builds each thread's
/// state, and `on_done` runs on the calling thread.
pub fn run_in_parallel<I, S, O>(
    jobs: Jobs,
    items: &[I],
    init: impl Fn() -> S + Sync,
    work: impl Fn(&mut S, &I) -> O + Sync,
    mut on_done: impl FnMut(&I, &O),
) -> Vec<O>
where
    I: Sync,
    O: Send,
{
    if items.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel::<(usize, O)>();
    let mut results: Vec<(usize, O)> = Vec::with_capacity(items.len());
    thread::scope(|scope| {
        for _ in 0..jobs.workers_for(items.len()) {
            let sender = sender.clone();
            let (next, init, work) = (&next, &init, &work);
            scope.spawn(move || {
                let mut state = init();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    if sender.send((index, work(&mut state, item))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        for (index, output) in receiver {
            on_done(&items[index], &output);
            results.push((index, output));
        }
    });
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, output)| output).collect()
}
