// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Release versions of a host and of Erlang/OTP: one parser, separate
//! types for the two meanings.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Project;
use crate::model::names::TagName;

const MIN_SEGMENTS: usize = 2;
const MAX_SEGMENTS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReleaseVersionError {
    #[error("release version is empty")]
    Empty,

    #[error(
        "release version {value:?} has {count} segments: needs {MIN_SEGMENTS} to {MAX_SEGMENTS}"
    )]
    SegmentCount { value: String, count: usize },

    #[error("release version {value:?} has a non-numeric segment {segment:?}")]
    NonNumeric { value: String, segment: String },

    #[error("Erlang major {value} is out of range")]
    MajorOutOfRange { value: u32 },

    #[error("tag {tag:?} does not start with the project's tag prefix {prefix:?}")]
    MissingPrefix { tag: String, prefix: String },

    #[error("series {value:?} is not of the form MAJOR.MINOR")]
    Series { value: String },

    #[error(
        "Erlang version range {value:?} is not of the form [FROM,BELOW), with FROM below BELOW"
    )]
    Range { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReleaseVersion {
    segments: Vec<u32>,
}

impl ReleaseVersion {
    pub fn parse(value: &str) -> Result<Self, ReleaseVersionError> {
        if value.is_empty() {
            return Err(ReleaseVersionError::Empty);
        }
        let segments = value
            .split('.')
            .map(|segment| {
                segment
                    .parse::<u32>()
                    .map_err(|_| ReleaseVersionError::NonNumeric {
                        value: value.to_owned(),
                        segment: segment.to_owned(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !(MIN_SEGMENTS..=MAX_SEGMENTS).contains(&segments.len()) {
            return Err(ReleaseVersionError::SegmentCount {
                value: value.to_owned(),
                count: segments.len(),
            });
        }
        Ok(Self { segments })
    }

    fn major(&self) -> u32 {
        self.segments[0]
    }

    fn minor(&self) -> u32 {
        self.segments[1]
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut segments = self.segments.iter();
        if let Some(first) = segments.next() {
            write!(f, "{first}")?;
        }
        for segment in segments {
            write!(f, ".{segment}")?;
        }
        Ok(())
    }
}

impl FromStr for ReleaseVersion {
    type Err = ReleaseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

fn strip_tag_prefix<'t>(prefix: &str, tag: &'t TagName) -> Result<&'t str, ReleaseVersionError> {
    tag.as_str()
        .strip_prefix(prefix)
        .ok_or_else(|| ReleaseVersionError::MissingPrefix {
            tag: tag.to_string(),
            prefix: prefix.to_owned(),
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ErlangMajor(u16);

impl ErlangMajor {
    #[must_use]
    pub fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u32> for ErlangMajor {
    type Error = ReleaseVersionError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        u16::try_from(value)
            .map(Self)
            .map_err(|_| ReleaseVersionError::MajorOutOfRange { value })
    }
}

impl fmt::Display for ErlangMajor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ErlangMajor {
    type Err = ReleaseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = s
            .parse::<u32>()
            .map_err(|_| ReleaseVersionError::NonNumeric {
                value: s.to_owned(),
                segment: s.to_owned(),
            })?;
        Self::try_from(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HostVersion(ReleaseVersion);

impl HostVersion {
    pub fn parse(value: &str) -> Result<Self, ReleaseVersionError> {
        ReleaseVersion::parse(value).map(Self)
    }

    #[must_use]
    pub fn series(&self) -> HostSeries {
        HostSeries {
            major: self.0.major(),
            minor: self.0.minor(),
        }
    }
}

impl fmt::Display for HostVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for HostVersion {
    type Err = ReleaseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for HostVersion {
    type Error = ReleaseVersionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<HostVersion> for String {
    fn from(version: HostVersion) -> Self {
        version.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OtpVersion {
    version: ReleaseVersion,
    major: ErlangMajor,
}

impl OtpVersion {
    pub fn parse(value: &str) -> Result<Self, ReleaseVersionError> {
        let version = ReleaseVersion::parse(value)?;
        let major = ErlangMajor::try_from(version.major())?;
        Ok(Self { version, major })
    }

    #[must_use]
    pub fn major(&self) -> ErlangMajor {
        self.major
    }

    #[must_use]
    pub fn line(&self) -> OtpLineVersion {
        OtpLineVersion {
            major: self.major,
            minor: self.version.minor(),
        }
    }
}

impl fmt::Display for OtpVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.version.fmt(f)
    }
}

impl FromStr for OtpVersion {
    type Err = ReleaseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for OtpVersion {
    type Error = ReleaseVersionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<OtpVersion> for String {
    fn from(version: OtpVersion) -> Self {
        version.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HostSeries {
    major: u32,
    minor: u32,
}

impl fmt::Display for HostSeries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

fn parse_major_minor(value: &str) -> Option<(u32, u32)> {
    let (major, minor) = value.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

impl FromStr for HostSeries {
    type Err = ReleaseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_major_minor(s)
            .map(|(major, minor)| Self { major, minor })
            .ok_or_else(|| ReleaseVersionError::Series {
                value: s.to_owned(),
            })
    }
}

impl TryFrom<String> for HostSeries {
    type Error = ReleaseVersionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<HostSeries> for String {
    fn from(series: HostSeries) -> Self {
        series.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OtpLineVersion {
    major: ErlangMajor,
    minor: u32,
}

impl OtpLineVersion {
    #[must_use]
    pub fn major(self) -> ErlangMajor {
        self.major
    }

    fn as_release(self) -> ReleaseVersion {
        ReleaseVersion {
            segments: vec![u32::from(self.major.get()), self.minor],
        }
    }
}

impl fmt::Display for OtpLineVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for OtpLineVersion {
    type Err = ReleaseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ReleaseVersionError::Series {
            value: s.to_owned(),
        };
        let (major, minor) = parse_major_minor(s).ok_or_else(invalid)?;
        Ok(Self {
            major: ErlangMajor::try_from(major)?,
            minor,
        })
    }
}

impl TryFrom<String> for OtpLineVersion {
    type Error = ReleaseVersionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<OtpLineVersion> for String {
    fn from(line: OtpLineVersion) -> Self {
        line.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostTag {
    version: HostVersion,
    tag: TagName,
}

impl HostTag {
    pub fn parse(project: &Project, tag: TagName) -> Result<Self, ReleaseVersionError> {
        let version = HostVersion::parse(strip_tag_prefix(&project.tag_prefix, &tag)?)?;
        Ok(Self { version, tag })
    }

    #[must_use]
    pub fn tag(&self) -> &TagName {
        &self.tag
    }

    #[must_use]
    pub fn version(&self) -> &HostVersion {
        &self.version
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OtpTag {
    version: OtpVersion,
    tag: TagName,
}

impl OtpTag {
    pub fn parse(project: &Project, tag: TagName) -> Result<Self, ReleaseVersionError> {
        let version = OtpVersion::parse(strip_tag_prefix(&project.tag_prefix, &tag)?)?;
        Ok(Self { version, tag })
    }

    #[must_use]
    pub fn tag(&self) -> &TagName {
        &self.tag
    }

    #[must_use]
    pub fn version(&self) -> &OtpVersion {
        &self.version
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ErlangVersionRange {
    from: OtpVersion,
    below: OtpVersion,
}

impl ErlangVersionRange {
    #[must_use]
    pub fn contains(&self, line: OtpLineVersion) -> bool {
        let line = line.as_release();
        self.from.version <= line && line < self.below.version
    }
}

impl fmt::Display for ErlangVersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{},{})", self.from, self.below)
    }
}

impl FromStr for ErlangVersionRange {
    type Err = ReleaseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ReleaseVersionError::Range {
            value: s.to_owned(),
        };
        let (from, below) = s
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(|inner| inner.split_once(','))
            .ok_or_else(invalid)?;
        let from = OtpVersion::parse(from.trim()).map_err(|_| invalid())?;
        let below = OtpVersion::parse(below.trim()).map_err(|_| invalid())?;
        if from >= below {
            return Err(invalid());
        }
        Ok(Self { from, below })
    }
}

impl TryFrom<String> for ErlangVersionRange {
    type Error = ReleaseVersionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ErlangVersionRange> for String {
    fn from(range: ErlangVersionRange) -> Self {
        range.to_string()
    }
}
