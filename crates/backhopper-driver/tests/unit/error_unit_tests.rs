// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use backhopper_driver::{DriverError, ErrorKind, OutputChannel, SchemaVersion, Verb, VerbId};

fn known_verb_id(v: Verb) -> VerbId<'static> {
    VerbId::Known(v)
}

#[test]
fn kind_round_trip_for_every_variant() {
    let cases: Vec<(DriverError, ErrorKind)> = vec![
        (
            DriverError::BinaryNotFound {
                searched: vec![PathBuf::from("/x")],
                source: io::Error::new(io::ErrorKind::NotFound, "x"),
            },
            ErrorKind::BinaryNotFound,
        ),
        (
            DriverError::Spawn {
                binary_path: PathBuf::from("/x"),
                source: io::Error::other("y"),
            },
            ErrorKind::Spawn,
        ),
        (
            DriverError::Timeout {
                verb: known_verb_id(Verb::CheckPatch),
                after: Duration::from_secs(1),
                argv: Vec::new(),
            },
            ErrorKind::Timeout,
        ),
        (
            DriverError::Cancelled {
                verb: known_verb_id(Verb::CheckPatch),
                argv: Vec::new(),
            },
            ErrorKind::Cancelled,
        ),
        (
            DriverError::SchemaVersionMismatch {
                got: SchemaVersion::new(99),
                supported_min: SchemaVersion::new(1),
                supported_max: SchemaVersion::new(1),
            },
            ErrorKind::SchemaVersionMismatch,
        ),
        (
            DriverError::ReaderPanicked {
                verb: known_verb_id(Verb::CheckPatch),
                channel: OutputChannel::Stdout,
            },
            ErrorKind::ReaderPanicked,
        ),
        (
            DriverError::OutputTooLarge {
                verb: known_verb_id(Verb::CheckBatch),
                channel: OutputChannel::Stdout,
                limit: 1024,
            },
            ErrorKind::OutputTooLarge,
        ),
        (
            DriverError::Transport {
                source: "boom".into(),
            },
            ErrorKind::Transport,
        ),
    ];
    for (err, expected) in cases {
        assert_eq!(err.kind(), expected, "{err}");
    }
}

#[test]
fn error_kind_display_uses_snake_case() {
    assert_eq!(ErrorKind::BinaryNotFound.to_string(), "binary_not_found");
    assert_eq!(ErrorKind::ToolError.to_string(), "tool_error");
    assert_eq!(ErrorKind::MissingRepoDir.to_string(), "missing_repo_dir");
    assert_eq!(ErrorKind::ReaderPanicked.to_string(), "reader_panicked");
    assert_eq!(
        ErrorKind::SchemaVersionMismatch.to_string(),
        "schema_version_mismatch"
    );
}

#[test]
fn schema_version_mismatch_display_quotes_range() {
    let err = DriverError::SchemaVersionMismatch {
        got: SchemaVersion::new(7),
        supported_min: SchemaVersion::new(1),
        supported_max: SchemaVersion::new(1),
    };
    let s = format!("{err}");
    assert!(s.contains("7"));
    assert!(s.contains("1"));
}
