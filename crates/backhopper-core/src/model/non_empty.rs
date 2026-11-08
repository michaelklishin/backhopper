// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A list that always has a first element.

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("a non-empty list was expected, got an empty one")]
pub struct EmptyListError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "Vec<T>")]
pub struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    #[must_use]
    pub fn new(first: T, rest: impl IntoIterator<Item = T>) -> Self {
        let mut items = vec![first];
        items.extend(rest);
        Self(items)
    }

    pub fn push(&mut self, item: T) {
        self.0.push(item);
    }

    #[must_use]
    pub fn first(&self) -> &T {
        &self.0[0]
    }

    #[must_use]
    pub fn last(&self) -> &T {
        &self.0[self.0.len() - 1]
    }

    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    #[must_use]
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> NonEmpty<U> {
        NonEmpty(self.0.into_iter().map(f).collect())
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    /// Sorts and removes duplicates; at least one element always remains.
    pub fn sort_dedup(&mut self)
    where
        T: Ord,
    {
        self.0.sort();
        self.0.dedup();
    }
}

impl<T> TryFrom<Vec<T>> for NonEmpty<T> {
    type Error = EmptyListError;

    fn try_from(items: Vec<T>) -> Result<Self, EmptyListError> {
        if items.is_empty() {
            Err(EmptyListError)
        } else {
            Ok(Self(items))
        }
    }
}

impl<T> From<NonEmpty<T>> for Vec<T> {
    fn from(list: NonEmpty<T>) -> Self {
        list.0
    }
}

impl<T> AsRef<[T]> for NonEmpty<T> {
    fn as_ref(&self) -> &[T] {
        &self.0
    }
}

impl<T> IntoIterator for NonEmpty<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a NonEmpty<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: Serialize> Serialize for NonEmpty<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
