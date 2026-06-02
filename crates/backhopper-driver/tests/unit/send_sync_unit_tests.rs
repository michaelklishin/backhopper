// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Compile-time assertions that public types satisfy the
//! thread-safety bounds documented in the design.

use backhopper_driver::{
    Backhopper, CancellationToken, DriverError, Envelope, EnvelopeWarning, GlobalOptions,
    RawOutcome, SchemaVersion, SubprocessBackend, Verb, VerbId,
};

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn fundamental_types_are_send_and_sync() {
    assert_send::<Verb>();
    assert_sync::<Verb>();
    assert_send::<VerbId<'static>>();
    assert_sync::<VerbId<'static>>();
    assert_send::<SchemaVersion>();
    assert_sync::<SchemaVersion>();
    assert_send::<EnvelopeWarning>();
    assert_sync::<EnvelopeWarning>();
    assert_send::<Envelope<()>>();
    assert_sync::<Envelope<()>>();
    assert_send::<RawOutcome>();
    assert_sync::<RawOutcome>();
    assert_send::<DriverError>();
    assert_sync::<DriverError>();
}

#[test]
fn options_and_token_are_freely_shareable() {
    assert_send::<GlobalOptions>();
    assert_sync::<GlobalOptions>();
    assert_send::<CancellationToken>();
    assert_sync::<CancellationToken>();
}

#[test]
fn backhopper_over_subprocess_is_send_and_sync() {
    assert_send::<Backhopper<SubprocessBackend>>();
    assert_sync::<Backhopper<SubprocessBackend>>();
}
