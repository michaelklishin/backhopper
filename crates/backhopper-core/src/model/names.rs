//! Newtypes for every domain primitive: `String` should never represent a
//! project name, tag, module, function, or commit SHA.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::errors::NameError;

const MAX_PROJECT_NAME_LEN: usize = 64;
const MAX_SERIES_NAME_LEN: usize = 64;
const MAX_TAG_NAME_LEN: usize = 128;
const MAX_MODULE_NAME_LEN: usize = 256;
const MAX_FUNCTION_NAME_LEN: usize = 256;
const MAX_TYPE_NAME_LEN: usize = 256;
const MAX_RECORD_NAME_LEN: usize = 256;
const MAX_FIELD_NAME_LEN: usize = 256;
const MAX_APPLICATION_NAME_LEN: usize = 128;
const MAX_BEHAVIOUR_NAME_LEN: usize = 256;
const MAX_ATTRIBUTE_NAME_LEN: usize = 64;
const MAX_CALLBACK_NAME_LEN: usize = 256;

const APPLICATION_PATTERN: &str = "[a-z][a-z0-9_]{0,127}";

const PROJECT_PATTERN: &str = "[a-z][a-z0-9_-]{0,63}";
const SERIES_PATTERN: &str = "[a-z][a-z0-9_.-]{0,63}";

fn is_project_name_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'
}

fn is_series_name_char(c: char) -> bool {
    is_project_name_char(c) || c == '.'
}

fn is_application_name_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
}

fn validate_simple_name(
    kind: &'static str,
    value: &str,
    max_len: usize,
    first_char_ok: fn(char) -> bool,
    rest_char_ok: fn(char) -> bool,
    pattern: &'static str,
) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::Empty { kind });
    }
    if value.len() > max_len {
        return Err(NameError::TooLong {
            kind,
            len: value.len(),
            max: max_len,
        });
    }
    let mut chars = value.chars();
    let first = chars.next().expect("non-empty");
    if !first_char_ok(first) {
        return Err(NameError::PatternMismatch {
            kind,
            value: value.to_owned(),
            pattern,
        });
    }
    for ch in chars {
        if !rest_char_ok(ch) {
            return Err(NameError::InvalidCharacter {
                kind,
                ch,
                value: value.to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_tag_name(value: &str) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::Empty { kind: "tag" });
    }
    if value.len() > MAX_TAG_NAME_LEN {
        return Err(NameError::TooLong {
            kind: "tag",
            len: value.len(),
            max: MAX_TAG_NAME_LEN,
        });
    }
    for ch in value.chars() {
        if ch.is_ascii_control()
            || ch.is_whitespace()
            || ch == '/'
            || ch == '\\'
            || ch == '\0'
            || ch == ':'
            || ch == '~'
            || ch == '^'
            || ch == '?'
            || ch == '*'
            || ch == '['
        {
            return Err(NameError::InvalidCharacter {
                kind: "tag",
                ch,
                value: value.to_owned(),
            });
        }
    }
    if value == "." || value == ".." || value.starts_with(".") || value.ends_with(".") {
        return Err(NameError::PatternMismatch {
            kind: "tag",
            value: value.to_owned(),
            pattern: "non-empty, no path separators or git ref-magic",
        });
    }
    Ok(())
}

fn validate_erlang_name(kind: &'static str, value: &str, max_len: usize) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::Empty { kind });
    }
    if value.len() > max_len {
        return Err(NameError::TooLong {
            kind,
            len: value.len(),
            max: max_len,
        });
    }
    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return Ok(());
    }
    let first = value.chars().next().expect("non-empty");
    if !(first.is_ascii_lowercase() || first == '_') {
        return Err(NameError::PatternMismatch {
            kind,
            value: value.to_owned(),
            pattern: "lowercase-led atom or single-quoted",
        });
    }
    let last = value.chars().last().expect("non-empty");
    let drop_trailing_punct = if matches!(last, '?' | '!') {
        value.len() - last.len_utf8()
    } else {
        value.len()
    };
    for ch in value[..drop_trailing_punct].chars() {
        if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '@') {
            return Err(NameError::InvalidCharacter {
                kind,
                ch,
                value: value.to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_module_name(kind: &'static str, value: &str, max_len: usize) -> Result<(), NameError> {
    if value.is_empty() {
        return Err(NameError::Empty { kind });
    }
    if value.len() > max_len {
        return Err(NameError::TooLong {
            kind,
            len: value.len(),
            max: max_len,
        });
    }
    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return Ok(());
    }
    let first = value.chars().next().expect("non-empty");
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(NameError::PatternMismatch {
            kind,
            value: value.to_owned(),
            pattern: "Erlang atom or Elixir module name",
        });
    }
    for ch in value.chars() {
        if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '@' || ch == '.') {
            return Err(NameError::InvalidCharacter {
                kind,
                ch,
                value: value.to_owned(),
            });
        }
    }
    Ok(())
}

macro_rules! string_newtype {
    ($name:ident, $kind:literal, $max:expr, $validate:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, NameError> {
                let v = value.into();
                $validate(&v)?;
                Ok(Self(v))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = NameError;

            fn from_str(s: &str) -> Result<Self, NameError> {
                Self::new(s.to_owned())
            }
        }
    };
}

string_newtype!(
    ProjectName,
    "project name",
    MAX_PROJECT_NAME_LEN,
    |v: &str| {
        validate_simple_name(
            "project name",
            v,
            MAX_PROJECT_NAME_LEN,
            |c| c.is_ascii_lowercase(),
            is_project_name_char,
            PROJECT_PATTERN,
        )
    }
);

string_newtype!(SeriesName, "series name", MAX_SERIES_NAME_LEN, |v: &str| {
    validate_simple_name(
        "series name",
        v,
        MAX_SERIES_NAME_LEN,
        |c| c.is_ascii_lowercase(),
        is_series_name_char,
        SERIES_PATTERN,
    )
});

string_newtype!(TagName, "tag", MAX_TAG_NAME_LEN, validate_tag_name);

string_newtype!(ModuleName, "module name", MAX_MODULE_NAME_LEN, |v: &str| {
    validate_module_name("module name", v, MAX_MODULE_NAME_LEN)
});

string_newtype!(
    FunctionName,
    "function name",
    MAX_FUNCTION_NAME_LEN,
    |v: &str| { validate_erlang_name("function name", v, MAX_FUNCTION_NAME_LEN) }
);

string_newtype!(TypeName, "type name", MAX_TYPE_NAME_LEN, |v: &str| {
    validate_erlang_name("type name", v, MAX_TYPE_NAME_LEN)
});

string_newtype!(RecordName, "record name", MAX_RECORD_NAME_LEN, |v: &str| {
    validate_erlang_name("record name", v, MAX_RECORD_NAME_LEN)
});

string_newtype!(FieldName, "field name", MAX_FIELD_NAME_LEN, |v: &str| {
    validate_erlang_name("field name", v, MAX_FIELD_NAME_LEN)
});

string_newtype!(
    ApplicationName,
    "application name",
    MAX_APPLICATION_NAME_LEN,
    |v: &str| {
        validate_simple_name(
            "application name",
            v,
            MAX_APPLICATION_NAME_LEN,
            |c| c.is_ascii_lowercase(),
            is_application_name_char,
            APPLICATION_PATTERN,
        )
    }
);

string_newtype!(
    BehaviourName,
    "behaviour name",
    MAX_BEHAVIOUR_NAME_LEN,
    |v: &str| { validate_module_name("behaviour name", v, MAX_BEHAVIOUR_NAME_LEN) }
);

string_newtype!(
    AttributeName,
    "attribute name",
    MAX_ATTRIBUTE_NAME_LEN,
    |v: &str| { validate_erlang_name("attribute name", v, MAX_ATTRIBUTE_NAME_LEN) }
);

string_newtype!(
    CallbackName,
    "callback name",
    MAX_CALLBACK_NAME_LEN,
    |v: &str| { validate_erlang_name("callback name", v, MAX_CALLBACK_NAME_LEN) }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Arity(u8);

impl Arity {
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl fmt::Display for Arity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Arity {
    type Err = NameError;

    fn from_str(s: &str) -> Result<Self, NameError> {
        let parsed: i64 = s
            .parse()
            .map_err(|_| NameError::InvalidArity { value: -1 })?;
        if !(0..=255).contains(&parsed) {
            return Err(NameError::InvalidArity { value: parsed });
        }
        Ok(Self(parsed as u8))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommitSha(String);

impl CommitSha {
    pub fn new(value: impl Into<String>) -> Result<Self, NameError> {
        let v = value.into();
        if v.len() != 40
            || !v
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(NameError::InvalidCommitSha { value: v });
        }
        Ok(Self(v))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommitSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for CommitSha {
    type Err = NameError;

    fn from_str(s: &str) -> Result<Self, NameError> {
        Self::new(s.to_owned())
    }
}

impl AsRef<str> for CommitSha {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Mfa {
    pub module: ModuleName,
    pub function: FunctionName,
    pub arity: Arity,
}

impl Mfa {
    pub fn new(module: ModuleName, function: FunctionName, arity: Arity) -> Self {
        Self {
            module,
            function,
            arity,
        }
    }
}

impl fmt::Display for Mfa {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}/{}", self.module, self.function, self.arity)
    }
}

impl FromStr for Mfa {
    type Err = NameError;

    fn from_str(s: &str) -> Result<Self, NameError> {
        let (head, arity_part) = s.rsplit_once('/').ok_or_else(|| NameError::InvalidMfa {
            value: s.to_owned(),
        })?;
        let (module_part, function_part) =
            head.split_once(':').ok_or_else(|| NameError::InvalidMfa {
                value: s.to_owned(),
            })?;
        let arity = arity_part
            .parse::<Arity>()
            .map_err(|_| NameError::InvalidMfa {
                value: s.to_owned(),
            })?;
        Ok(Self {
            module: module_part.parse()?,
            function: function_part.parse()?,
            arity,
        })
    }
}
