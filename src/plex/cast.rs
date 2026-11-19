//! Cast photos: a bounded in-memory set of decoded images over a bounded disk cache.
//!
//! Photos are fetched through the Plex server's photo transcoder, so the access
//! token only reaches that server even for photos hosted elsewhere.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

pub(crate) const SHOWN: usize = 12;
pub(crate) const MEMORY_LIMIT: usize = 96;
pub(crate) const DISK_LIMIT: usize = 400;
pub(crate) const SIZE: u32 = 128;

#[derive(Debug, Default)]
pub(crate) struct Photos {
    handles: HashMap<String, iced::widget::image::Handle>,
    order: VecDeque<String>,
}

impl Photos {
    pub(crate) fn get(&self, thumb: &str) -> Option<&iced::widget::image::Handle> {
        self.handles.get(thumb)
    }

    pub(crate) fn contains(&self, thumb: &str) -> bool {
        self.handles.contains_key(thumb)
    }

    pub(crate) fn insert(&mut self, thumb: String, handle: iced::widget::image::Handle) {
        if self.handles.insert(thumb.clone(), handle).is_none() {
            self.order.push_back(thumb);
        }
        while self.order.len() > MEMORY_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.handles.remove(&oldest);
            }
        }
    }
}

pub(crate) fn directory() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".cache"))
        })?;
    Some(base.join("enzui-video").join("cast"))
}

fn file(directory: &Path, thumb: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    thumb.hash(&mut hasher);
    directory.join(format!("{:016x}.img", hasher.finish()))
}

pub(crate) fn read(directory: &Path, thumb: &str) -> Option<Vec<u8>> {
    std::fs::read(file(directory, thumb)).ok()
}

// Stores `bytes` and prunes the oldest files beyond `DISK_LIMIT`; failures only cost a refetch.
pub(crate) fn write(directory: &Path, thumb: &str, bytes: &[u8]) {
    if std::fs::create_dir_all(directory).is_err() {
        return;
    }
    let target = file(directory, thumb);
    let temporary = target.with_extension("tmp");
    if std::fs::write(&temporary, bytes).is_err() || std::fs::rename(&temporary, &target).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return;
    }
    prune(directory, DISK_LIMIT);
}

fn prune(directory: &Path, limit: usize) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "img")
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    if files.len() <= limit {
        return;
    }
    files.sort();
    for (_, path) in files.iter().take(files.len() - limit) {
        let _ = std::fs::remove_file(path);
    }
}
