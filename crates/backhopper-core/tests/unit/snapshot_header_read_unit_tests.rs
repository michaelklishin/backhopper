// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use tempfile::TempDir;

use backhopper_core::errors::StoreError;
use backhopper_core::snapshot::parser;
use backhopper_core::store::SnapshotStore;

use crate::jigsaw_support::{StoredSnapshot, commit, config, hex_pin, project, tag};

#[test]
fn a_header_is_read_without_the_body() {
    let dir = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(dir.path()).unwrap();
    StoredSnapshot::host(
        &config(),
        "v4.2.0",
        commit('1'),
        vec![hex_pin("ra", "2.17.1")],
    )
    .write(&store);
    let header = store
        .read_header(&project("rabbitmq-server"), &tag("v4.2.0"))
        .unwrap();
    assert_eq!(header.commit, commit('1'));
    assert_eq!(header.dep_pins.len(), 1);
    assert!(header.host_facts.is_some());
    assert_eq!(
        header,
        store
            .read(&project("rabbitmq-server"), &tag("v4.2.0"))
            .unwrap()
            .header()
            .clone()
    );
}

#[test]
fn a_missing_snapshot_has_no_header() {
    let dir = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(dir.path()).unwrap();
    assert!(matches!(
        store.read_header(&project("ra"), &tag("v2.17.1")),
        Err(StoreError::SnapshotNotFound { .. })
    ));
}

#[test]
fn a_file_without_a_header_is_refused() {
    assert!(parser::parse_header("module ra\n").is_err());
}

#[test]
fn text_after_the_header_block_is_not_read() {
    let dir = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(dir.path()).unwrap();
    StoredSnapshot::otp("OTP-27.3").write(&store);
    let text = store.read_text(&project("otp"), &tag("OTP-27.3")).unwrap();
    let broken = format!("{text}\nthis is not a section\n");
    assert!(parser::parse(&broken).is_err());
    assert!(parser::parse_header(&broken).is_ok());
}
