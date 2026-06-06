// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::model::names::{
    Arity, DependencyName, DependencyVersion, FunctionName, MacroName,
};
use backhopper_core::model::snapshot::{
    Provenance, VendoredDep, VendoredDepSource, VersionedMachineVersion, WireConstantBinding,
    WireValue,
};

#[test]
fn macro_name_accepts_shouty_identifier() {
    let m = MacroName::new("MACHINE_VERSION").unwrap();
    assert_eq!(m.as_str(), "MACHINE_VERSION");
}

#[test]
fn macro_name_accepts_leading_underscore() {
    assert!(MacroName::new("_INTERNAL").is_ok());
}

#[test]
fn macro_name_accepts_lowercase() {
    assert!(MacroName::new("ra_proto_version").is_ok());
}

#[test]
fn macro_name_rejects_empty() {
    assert!(MacroName::new("").is_err());
}

#[test]
fn macro_name_rejects_leading_digit() {
    assert!(MacroName::new("9LIVES").is_err());
}

#[test]
fn macro_name_rejects_dash() {
    assert!(MacroName::new("RA-PROTO").is_err());
}

#[test]
fn macro_name_rejects_whitespace() {
    assert!(MacroName::new("RA PROTO").is_err());
}

#[test]
fn macro_name_rejects_macro_reference_marker() {
    assert!(MacroName::new("?MV").is_err());
}

#[test]
fn macro_name_rejects_punctuation() {
    for bad in [".", "MV.", "MV(N)", "MV/0", "MV!", "MV@"] {
        assert!(MacroName::new(bad).is_err(), "should reject {bad:?}");
    }
}

#[test]
fn dependency_version_rejects_null_byte() {
    assert!(DependencyVersion::new("3.1.6\0").is_err());
}

#[test]
fn dependency_version_rejects_newline() {
    assert!(DependencyVersion::new("3.1\n6").is_err());
}

#[test]
fn dependency_name_rejects_empty() {
    assert!(DependencyName::new("").is_err());
}

#[test]
fn dependency_name_rejects_leading_digit() {
    assert!(DependencyName::new("9foo").is_err());
}

#[test]
fn dependency_name_accepts_lower_snake() {
    assert_eq!(DependencyName::new("ra").unwrap().as_str(), "ra");
    assert!(DependencyName::new("rabbitmq_stream").is_ok());
}

#[test]
fn dependency_name_rejects_uppercase_or_dash() {
    assert!(DependencyName::new("Ra").is_err());
    assert!(DependencyName::new("rabbit-mq").is_err());
}

#[test]
fn dependency_version_accepts_real_release_strings() {
    for v in &[
        "3.1.6",
        "v3.1.6",
        "3.0.0-rc.1",
        "2.13.0+build.1",
        "0.10.0-alpha",
    ] {
        assert!(DependencyVersion::new(*v).is_ok(), "expected ok: {v}");
    }
}

#[test]
fn dependency_version_rejects_whitespace_or_control() {
    assert!(DependencyVersion::new("3.1 .6").is_err());
    assert!(DependencyVersion::new("").is_err());
}

#[test]
fn versioned_machine_version_with_literal_value_round_trips_through_serde() {
    let v = VersionedMachineVersion {
        function: FunctionName::from_str("version").unwrap(),
        arity: Arity::new(0),
        value: Some(7),
        provenance: Provenance::Literal,
    };
    let encoded = serde_json::to_string(&v).unwrap();
    let back: VersionedMachineVersion = serde_json::from_str(&encoded).unwrap();
    assert_eq!(v, back);
}

#[test]
fn versioned_machine_version_with_macro_body_round_trips_through_serde() {
    let v = VersionedMachineVersion {
        function: FunctionName::from_str("version").unwrap(),
        arity: Arity::new(0),
        value: Some(8),
        provenance: Provenance::MacroBody {
            macro_name: MacroName::from_str("MACHINE_VERSION").unwrap(),
            defined_in: Some("deps/rabbit/include/rabbit_fifo.hrl".into()),
        },
    };
    let encoded = serde_json::to_string(&v).unwrap();
    let back: VersionedMachineVersion = serde_json::from_str(&encoded).unwrap();
    assert_eq!(v, back);
}

#[test]
fn versioned_machine_version_unresolved_value_round_trips() {
    let v = VersionedMachineVersion {
        function: FunctionName::from_str("version").unwrap(),
        arity: Arity::new(0),
        value: None,
        provenance: Provenance::Literal,
    };
    let encoded = serde_json::to_string(&v).unwrap();
    let back: VersionedMachineVersion = serde_json::from_str(&encoded).unwrap();
    assert_eq!(v, back);
}

#[test]
fn wire_constant_u64_round_trips_through_serde() {
    let wc = WireConstantBinding {
        macro_name: MacroName::from_str("RA_PROTO_VERSION").unwrap(),
        value: WireValue::U64(1),
        defined_in: None,
    };
    let encoded = serde_json::to_string(&wc).unwrap();
    let back: WireConstantBinding = serde_json::from_str(&encoded).unwrap();
    assert_eq!(wc, back);
}

#[test]
fn wire_constant_bytes_round_trips_through_serde() {
    let wc = WireConstantBinding {
        macro_name: MacroName::from_str("MAGIC").unwrap(),
        value: WireValue::Bytes(b"RASG".to_vec()),
        defined_in: Some("src/ra_log_segment.erl".into()),
    };
    let encoded = serde_json::to_string(&wc).unwrap();
    let back: WireConstantBinding = serde_json::from_str(&encoded).unwrap();
    assert_eq!(wc, back);
}

#[test]
fn wire_constant_opaque_preserves_text() {
    let wc = WireConstantBinding {
        macro_name: MacroName::from_str("WHATEVER").unwrap(),
        value: WireValue::Opaque("{tuple, 1, 2}".into()),
        defined_in: None,
    };
    let encoded = serde_json::to_string(&wc).unwrap();
    let back: WireConstantBinding = serde_json::from_str(&encoded).unwrap();
    assert_eq!(wc, back);
}

#[test]
fn vendored_dep_round_trips_through_serde_for_each_source_variant() {
    for source in [
        VendoredDepSource::Hex,
        VendoredDepSource::Git,
        VendoredDepSource::GitRmq,
    ] {
        let pin = VendoredDep {
            name: DependencyName::from_str("ra").unwrap(),
            version: DependencyVersion::from_str("3.1.6").unwrap(),
            source,
        };
        let encoded = serde_json::to_string(&pin).unwrap();
        let back: VendoredDep = serde_json::from_str(&encoded).unwrap();
        assert_eq!(pin, back);
    }
}
