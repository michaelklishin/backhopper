// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::sync::atomic::{AtomicUsize, Ordering};

use backhopper_cli::commands::parallel::{Jobs, run_in_parallel};

fn jobs(n: usize) -> Jobs {
    n.to_string().parse().unwrap()
}

#[test]
fn a_worker_count_is_a_whole_number_of_one_or_more() {
    assert_eq!("1".parse::<Jobs>().unwrap().get(), 1);
    assert_eq!("28".parse::<Jobs>().unwrap().get(), 28);
    for text in ["0", "-1", "many", "", "1.5"] {
        assert!(text.parse::<Jobs>().is_err(), "{text:?}");
    }
}

#[test]
fn the_default_worker_count_is_at_least_one() {
    assert!(Jobs::available().get() >= 1);
    assert_eq!(Jobs::or_available(Some(jobs(3))).get(), 3);
    assert_eq!(Jobs::or_available(None), Jobs::available());
}

#[test]
fn results_come_back_in_the_order_of_the_items() {
    let items: Vec<u32> = (0..50).collect();
    let squares = run_in_parallel(jobs(8), &items, || (), |(), n| n * n, |_, _| {});
    assert_eq!(squares, items.iter().map(|n| n * n).collect::<Vec<_>>());
}

#[test]
fn no_items_start_no_workers() {
    let items: Vec<u32> = Vec::new();
    let built = AtomicUsize::new(0);
    let out = run_in_parallel(
        jobs(8),
        &items,
        || built.fetch_add(1, Ordering::SeqCst),
        |_, n| *n,
        |_, _| {},
    );
    assert!(out.is_empty());
    assert_eq!(built.load(Ordering::SeqCst), 0);
}

#[test]
fn every_item_is_worked_on_exactly_once() {
    let items: Vec<usize> = (0..200).collect();
    let calls = AtomicUsize::new(0);
    let out = run_in_parallel(
        jobs(6),
        &items,
        || (),
        |(), n| {
            calls.fetch_add(1, Ordering::SeqCst);
            *n
        },
        |_, _| {},
    );
    assert_eq!(calls.load(Ordering::SeqCst), items.len());
    assert_eq!(out, items);
}

#[test]
fn each_worker_builds_its_own_state_and_never_more_workers_than_items() {
    let built = AtomicUsize::new(0);
    let items = [1u8, 2, 3];
    run_in_parallel(
        jobs(16),
        &items,
        || built.fetch_add(1, Ordering::SeqCst),
        |_, n| *n,
        |_, _| {},
    );
    let states = built.load(Ordering::SeqCst);
    assert!((1..=items.len()).contains(&states), "{states}");
}

#[test]
fn one_worker_builds_one_state() {
    let built = AtomicUsize::new(0);
    let items: Vec<u32> = (0..20).collect();
    run_in_parallel(
        jobs(1),
        &items,
        || built.fetch_add(1, Ordering::SeqCst),
        |_, n| *n,
        |_, _| {},
    );
    assert_eq!(built.load(Ordering::SeqCst), 1);
}

#[test]
fn progress_is_reported_on_the_calling_thread_once_per_item() {
    let caller = std::thread::current().id();
    let items: Vec<u32> = (0..30).collect();
    let mut reported = Vec::new();
    run_in_parallel(
        jobs(4),
        &items,
        || (),
        |(), n| n + 1,
        |item, output| {
            assert_eq!(std::thread::current().id(), caller);
            reported.push((*item, *output));
        },
    );
    reported.sort_unstable();
    assert_eq!(
        reported,
        items.iter().map(|n| (*n, n + 1)).collect::<Vec<_>>()
    );
}

#[test]
fn a_failed_item_does_not_stop_the_others() {
    let items: Vec<u32> = (0..40).collect();
    let out = run_in_parallel(
        jobs(4),
        &items,
        || (),
        |(), n| {
            if n % 7 == 3 {
                Err(format!("{n} failed"))
            } else {
                Ok(*n)
            }
        },
        |_, _| {},
    );
    assert_eq!(out.iter().filter(|r| r.is_err()).count(), 6);
    assert_eq!(out.iter().filter(|r| r.is_ok()).count(), 34);
    assert_eq!(out[3], Err("3 failed".to_owned()));
}
