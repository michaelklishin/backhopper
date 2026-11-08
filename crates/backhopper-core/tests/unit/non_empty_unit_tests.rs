// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::non_empty::NonEmpty;

#[test]
fn empty_vec_is_refused() {
    assert!(NonEmpty::<u8>::try_from(Vec::new()).is_err());
}

#[test]
fn first_and_last_never_fail() {
    let list = NonEmpty::new(1, [2, 3]);
    assert_eq!(
        (*list.first(), *list.last(), list.as_slice().len()),
        (1, 3, 3)
    );
    let single = NonEmpty::new(7, []);
    assert_eq!((*single.first(), *single.last()), (7, 7));
}

#[test]
fn serde_refuses_an_empty_list_where_it_is_read() {
    assert!(serde_json::from_str::<NonEmpty<u8>>("[]").is_err());
    let list: NonEmpty<u8> = serde_json::from_str("[4,5]").unwrap();
    assert_eq!(serde_json::to_string(&list).unwrap(), "[4,5]");
}

#[test]
fn map_keeps_order() {
    let mapped = NonEmpty::new(1, [2, 3]).map(|n| n * 10);
    assert_eq!(mapped.into_vec(), vec![10, 20, 30]);
}
