//! What was watched recently, and where to pick each one up again.

use super::Entry;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(crate) const RECENT_LIMIT: usize = 30;

const RESUME_AFTER: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Recent {
    pub(crate) entry: Entry,
    pub(crate) title: String,
    // Zero means no resume point, including completed videos.
    pub(crate) position_ms: u64,
    pub(crate) duration_ms: Option<u64>,
    // When it was last played, in Unix milliseconds.
    pub(crate) played_at: i64,
    // Distinguishes completion from near-start entries, which also have zero position.
    #[serde(default)]
    pub(crate) finished: bool,
}

impl Recent {
    pub(crate) fn progress(&self) -> Option<f32> {
        let duration = self.duration_ms.filter(|ms| *ms > 0)?;
        Some((self.position_ms as f32 / duration as f32).clamp(0.0, 1.0))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct History {
    items: Vec<Recent>,
}

impl History {
    pub(crate) fn items(&self) -> &[Recent] {
        &self.items
    }

    pub(crate) fn local_items(&self) -> impl Iterator<Item = (usize, &Recent)> + Clone {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item.entry, Entry::File(_)))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    // Positions under ten seconds or within the credits threshold are stored as zero.
    pub(crate) fn record(
        &mut self,
        entry: &Entry,
        title: &str,
        position: Duration,
        duration: Option<Duration>,
        now: i64,
    ) {
        let finished = is_finished(position, duration);
        let position = if finished || position < RESUME_AFTER {
            Duration::ZERO
        } else {
            position
        };
        self.items.retain(|item| &item.entry != entry);
        self.items.insert(
            0,
            Recent {
                entry: entry.clone(),
                title: title.to_owned(),
                position_ms: millis(position),
                duration_ms: duration.map(millis),
                played_at: now,
                finished,
            },
        );
        self.items.truncate(RECENT_LIMIT);
    }

    pub(crate) fn resume_point(&self, entry: &Entry) -> Option<Duration> {
        self.items
            .iter()
            .find(|item| &item.entry == entry)
            .map(|item| Duration::from_millis(item.position_ms))
            .filter(|position| !position.is_zero())
    }

    pub(crate) fn remove(&mut self, entry: &Entry) -> bool {
        let before = self.items.len();
        self.items.retain(|item| &item.entry != entry);
        self.items.len() != before
    }

    pub(crate) fn thumbnail_keys(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|item| thumbnail_key(&item.entry))
            .collect()
    }

    pub(crate) fn clear_local(&mut self) {
        self.items
            .retain(|item| !matches!(item.entry, Entry::File(_)));
    }

    pub(crate) fn sanitize(&mut self) {
        self.items.truncate(RECENT_LIMIT);
    }
}

// The file name an entry's thumbnail is stored under: an FNV-1a hash of
// its path or address, stable across runs and Rust versions.
pub(crate) fn thumbnail_key(entry: &Entry) -> String {
    let bytes: Vec<u8> = match entry {
        Entry::File(path) => [b"f:".as_slice(), path.as_os_str().as_encoded_bytes()].concat(),
        Entry::Url(url) => [b"u:".as_slice(), url.as_bytes()].concat(),
    };
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

// Within the last 5 % or the last 15 s, whichever is shorter: the credits.
fn is_finished(position: Duration, duration: Option<Duration>) -> bool {
    duration.is_some_and(|duration| {
        let tail = (duration / 20).min(Duration::from_secs(15));
        position.saturating_add(tail) >= duration
    })
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
