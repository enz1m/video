//! The queue: what plays now, what plays next, and what happens at the end.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Entry {
    File(#[serde(with = "native_path")] PathBuf),
    // A stream address, as the user typed it.
    Url(String),
}

impl Entry {
    pub(crate) fn name(&self) -> String {
        match self {
            Entry::File(path) => path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
            Entry::Url(url) => {
                let without_query = url.split(['?', '#']).next().unwrap_or(url);
                without_query
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .filter(|segment| !segment.is_empty() && !segment.contains(':'))
                    .map_or_else(|| url.clone(), str::to_owned)
            }
        }
    }

    pub(crate) fn path(&self) -> Option<&std::path::Path> {
        match self {
            Entry::File(path) => Some(path),
            Entry::Url(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub(crate) fn cycle(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            Repeat::Off => enzui_core::localization::text("video-repeat-off-0c1fe3"),
            Repeat::All => enzui_core::localization::text("video-repeat-all-5cf6d2"),
            Repeat::One => enzui_core::localization::text("video-repeat-one-b3ca2e"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Playlist {
    revision: u64,
    entries: Vec<Entry>,
    current: Option<usize>,
    pub(crate) repeat: Repeat,
}

impl Playlist {
    pub(crate) fn replace(&mut self, entries: Vec<Entry>) {
        self.entries = dedup(entries);
        self.revision = self.revision.wrapping_add(1);
        self.current = (!self.entries.is_empty()).then_some(0);
    }

    pub(crate) fn append(&mut self, entries: Vec<Entry>) -> usize {
        let before = self.entries.len();
        let mut queued: std::collections::HashSet<Entry> = self.entries.iter().cloned().collect();
        self.entries.extend(
            entries
                .into_iter()
                .filter(|entry| queued.insert(entry.clone())),
        );
        if self.current.is_none() && !self.entries.is_empty() {
            self.current = Some(0);
        }
        let added = self.entries.len() - before;
        if added > 0 {
            self.revision = self.revision.wrapping_add(1);
        }
        added
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn reorder_before(&mut self, entry: &Entry, before: Option<&Entry>) -> bool {
        let Some(source) = self.entries.iter().position(|item| item == entry) else {
            return false;
        };
        let target = match before {
            Some(target) => match self.entries.iter().position(|item| item == target) {
                Some(index) => index,
                None => return false,
            },
            None => self.entries.len(),
        };
        let target = target.saturating_sub(usize::from(source < target));
        if source == target {
            return false;
        }
        let playing = self.current().cloned();
        let entry = self.entries.remove(source);
        self.entries.insert(target, entry);
        self.current =
            playing.and_then(|entry| self.entries.iter().position(|item| item == &entry));
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub(crate) fn current(&self) -> Option<&Entry> {
        self.entries.get(self.current?)
    }

    pub(crate) fn has_next(&self) -> bool {
        self.current
            .is_some_and(|index| index + 1 < self.entries.len() || self.repeat == Repeat::All)
            && self.entries.len() > 1
    }

    pub(crate) fn has_previous(&self) -> bool {
        self.current
            .is_some_and(|index| index > 0 || self.repeat == Repeat::All)
            && self.entries.len() > 1
    }

    pub(crate) fn select(&mut self, index: usize) -> Option<&Entry> {
        (index < self.entries.len()).then(|| {
            self.current = Some(index);
            &self.entries[index]
        })
    }

    pub(crate) fn next(&mut self) -> Option<&Entry> {
        let index = self.current?;
        let next = if index + 1 < self.entries.len() {
            index + 1
        } else if self.repeat == Repeat::All && self.entries.len() > 1 {
            0
        } else {
            return None;
        };
        self.select(next)
    }

    pub(crate) fn previous(&mut self) -> Option<&Entry> {
        let index = self.current?;
        let previous = if index > 0 {
            index - 1
        } else if self.repeat == Repeat::All && self.entries.len() > 1 {
            self.entries.len() - 1
        } else {
            return None;
        };
        self.select(previous)
    }

    pub(crate) fn after_end(&mut self) -> Option<&Entry> {
        if self.repeat == Repeat::One {
            return self.current();
        }
        self.next()
    }

    // Removing the current entry selects its successor, or its predecessor at the end.
    pub(crate) fn remove(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.entries.remove(index);
        self.revision = self.revision.wrapping_add(1);
        self.current = match self.current {
            _ if self.entries.is_empty() => None,
            Some(current) if current > index => Some(current - 1),
            Some(current) => Some(current.min(self.entries.len() - 1)),
            None => None,
        };
        true
    }
}

fn dedup(entries: Vec<Entry>) -> Vec<Entry> {
    let mut seen = std::collections::HashSet::with_capacity(entries.len());
    entries
        .into_iter()
        .filter(|entry| seen.insert(entry.clone()))
        .collect()
}

mod native_path {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::path::{Path, PathBuf};

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum StoredPath {
        Text(String),
        #[cfg(unix)]
        Bytes {
            unix_bytes: Vec<u8>,
        },
    }

    pub(crate) fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(text) = path.to_str() {
            return text.serialize(serializer);
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            StoredPath::Bytes {
                unix_bytes: path.as_os_str().as_bytes().to_vec(),
            }
            .serialize(serializer)
        }
        #[cfg(not(unix))]
        Err(serde::ser::Error::custom("path is not Unicode"))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PathBuf, D::Error> {
        match StoredPath::deserialize(deserializer)? {
            StoredPath::Text(text) => Ok(PathBuf::from(text)),
            #[cfg(unix)]
            StoredPath::Bytes { unix_bytes } => {
                use std::os::unix::ffi::OsStringExt;
                Ok(PathBuf::from(std::ffi::OsString::from_vec(unix_bytes)))
            }
        }
    }
}
