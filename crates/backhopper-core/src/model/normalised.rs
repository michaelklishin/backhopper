// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Erlang source text reduced to a reformatting-proof, single-line form.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::errors::NameError;

const EMPTY_BODY: &str = "%empty%";
const BARE_QUESTION_MARK: &str = "%bare_question_mark%";

/// Comments stripped, whitespace runs collapsed to one space, ends
/// trimmed, raw line breaks inside literals written as escapes: enough
/// to equate reformatting without equating texts that differ.
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    let mut in_string = false;
    let mut in_atom = false;
    while let Some(c) = chars.next() {
        if in_string || in_atom {
            push_literal_char(&mut out, c);
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        push_literal_char(&mut out, next);
                    }
                }
                '"' if in_string => in_string = false,
                '\'' if in_atom => in_atom = false,
                _ => {}
            }
            continue;
        }
        match c {
            '%' => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            '"' => {
                in_string = true;
                out.push(c);
            }
            '\'' => {
                in_atom = true;
                out.push(c);
            }
            '$' => {
                out.push(c);
                if let Some(next) = chars.next() {
                    push_literal_char(&mut out, next);
                }
            }
            c if c.is_whitespace() => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            c => out.push(c),
        }
    }
    out.trim().to_owned()
}

fn push_literal_char(out: &mut String, c: char) {
    match c {
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        c => out.push(c),
    }
}

fn not_normalised(kind: &'static str, text: &str) -> NameError {
    NameError::Invalid {
        kind,
        value: text.to_owned(),
        reason: "not in normalised form",
    }
}

/// The body of a `-define`, normalised. An empty body is the token
/// `%empty%`, which cannot be an Erlang token sequence, so no text a
/// snapshot line holds reads as an absent body.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NormalisedMacroBody(Arc<str>);

impl NormalisedMacroBody {
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        let text = normalise(source);
        let text = match text.as_str() {
            "" => EMPTY_BODY,
            "?" => BARE_QUESTION_MARK,
            other => other,
        };
        Self(Arc::from(text))
    }

    pub fn from_stored(text: &str) -> Result<Self, NameError> {
        let body = Self::from_source(text);
        if body.as_str() == text {
            Ok(body)
        } else {
            Err(not_normalised("macro body", text))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when the body is only string literals, so it cannot change
    /// what a use of the macro compiles against.
    #[must_use]
    pub fn is_string_literal(&self) -> bool {
        let mut chars = self.0.chars();
        let mut in_string = false;
        let mut seen = false;
        while let Some(c) = chars.next() {
            match (in_string, c) {
                (true, '\\') => {
                    chars.next();
                }
                (true, '"') => in_string = false,
                (true, _) => {}
                (false, '"') => {
                    in_string = true;
                    seen = true;
                }
                (false, c) if c.is_whitespace() => {}
                (false, _) => return false,
            }
        }
        seen && !in_string
    }
}

impl fmt::Display for NormalisedMacroBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for NormalisedMacroBody {
    type Error = NameError;

    fn try_from(text: String) -> Result<Self, NameError> {
        Self::from_stored(&text)
    }
}

impl From<NormalisedMacroBody> for String {
    fn from(body: NormalisedMacroBody) -> Self {
        body.as_str().to_owned()
    }
}

/// A record field's default expression, normalised. A field without a
/// default is `None` on the field, never an empty default.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NormalisedDefault(Arc<str>);

impl NormalisedDefault {
    #[must_use]
    pub fn from_source(source: &str) -> Option<Self> {
        let text = normalise(source);
        (!text.is_empty()).then(|| Self(Arc::from(text)))
    }

    pub fn from_stored(text: &str) -> Result<Self, NameError> {
        Self::from_source(text)
            .filter(|default| default.as_str() == text)
            .ok_or_else(|| not_normalised("record field default", text))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NormalisedDefault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for NormalisedDefault {
    type Error = NameError;

    fn try_from(text: String) -> Result<Self, NameError> {
        Self::from_stored(&text)
    }
}

impl From<NormalisedDefault> for String {
    fn from(default: NormalisedDefault) -> Self {
        default.as_str().to_owned()
    }
}
