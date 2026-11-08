// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::slice;

use thiserror::Error;

use crate::model::non_empty::NonEmpty;
use crate::model::release_version::{HostSeries, HostTag, HostVersion};

pub trait HasHostTag {
    fn host_tag(&self) -> &HostTag;
}

impl HasHostTag for HostTag {
    fn host_tag(&self) -> &HostTag {
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SeriesReleasesError {
    #[error("no releases")]
    Empty,

    #[error("release {0} is listed twice")]
    Duplicate(HostVersion),

    #[error("releases of series {first} and {other} are mixed")]
    MixedSeries {
        first: HostSeries,
        other: HostSeries,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesReleases<R>(NonEmpty<R>);

impl<R: HasHostTag> SeriesReleases<R> {
    pub fn new(mut releases: Vec<R>) -> Result<Self, SeriesReleasesError> {
        releases.sort_by(|a, b| a.host_tag().version().cmp(b.host_tag().version()));
        let releases = NonEmpty::try_from(releases).map_err(|_| SeriesReleasesError::Empty)?;
        let series = releases.first().host_tag().version().series();
        for pair in releases.as_slice().windows(2) {
            let (earlier, later) = (pair[0].host_tag().version(), pair[1].host_tag().version());
            if earlier == later {
                return Err(SeriesReleasesError::Duplicate(later.clone()));
            }
        }
        if let Some(other) = releases
            .iter()
            .map(|release| release.host_tag().version().series())
            .find(|other| *other != series)
        {
            return Err(SeriesReleasesError::MixedSeries {
                first: series,
                other,
            });
        }
        Ok(Self(releases))
    }

    #[must_use]
    pub fn build(&self) -> &R {
        self.0.last()
    }

    #[must_use]
    pub fn first(&self) -> &R {
        self.0.first()
    }

    #[must_use]
    pub fn series(&self) -> HostSeries {
        self.0.first().host_tag().version().series()
    }

    pub fn iter(&self) -> slice::Iter<'_, R> {
        self.0.iter()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[R] {
        self.0.as_slice()
    }

    /// The mapped value carries its own tag, so the order is checked again.
    pub fn map<U: HasHostTag>(
        self,
        f: impl FnMut(R) -> U,
    ) -> Result<SeriesReleases<U>, SeriesReleasesError> {
        SeriesReleases::new(self.0.into_iter().map(f).collect())
    }
}

impl<'a, R> IntoIterator for &'a SeriesReleases<R> {
    type Item = &'a R;
    type IntoIter = slice::Iter<'a, R>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
