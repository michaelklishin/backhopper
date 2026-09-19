// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared `skip_serializing_if` predicates and `serde(with = ...)` helpers
//! for the model types.

// serde's skip_serializing_if predicate shape requires &bool
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Wire a `Display` and `FromStr` type through its string form instead of
/// its field-by-field shape, for `#[serde(with = "backhopper_core::model::serde_util::display_from_str")]`.
/// `pub`, not `pub(crate)`: the CLI's `IntroducedRow.mfa` uses it too, to
/// keep `Mfa`'s CLI-side wire spelling the string it always was.
pub mod display_from_str {
    use std::fmt::Display;
    use std::str::FromStr;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Display,
        S: Serializer,
    {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: FromStr,
        T::Err: Display,
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// A `BTreeMap` keyed by `FunArity` serialises as a JSON object, so the
/// key travels as its `name/arity` string rather than a nested object:
/// `serde_json` refuses a non-string map key outright.
pub(crate) mod fun_arity_keyed_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::model::snapshot::FunArity;

    pub(crate) fn serialize<V, S>(
        map: &BTreeMap<FunArity, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        V: Serialize,
        S: Serializer,
    {
        let stringified: BTreeMap<String, &V> =
            map.iter().map(|(k, v)| (k.to_string(), v)).collect();
        stringified.serialize(serializer)
    }

    pub(crate) fn deserialize<'de, V, D>(deserializer: D) -> Result<BTreeMap<FunArity, V>, D::Error>
    where
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let stringified: BTreeMap<String, V> = BTreeMap::deserialize(deserializer)?;
        stringified
            .into_iter()
            .map(|(k, v)| {
                k.parse::<FunArity>()
                    .map(|fa| (fa, v))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}
