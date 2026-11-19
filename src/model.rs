//! The player's domain: what is queued, what was watched and how far, and
//! the rules for files and times. Render-free and pipeline-free.

pub(crate) mod files;
pub(crate) mod history;
pub(crate) mod playlist;
pub(crate) mod thumbnail;
pub(crate) mod time;
pub(crate) mod window_fit;

pub(crate) use history::History;
pub(crate) use playlist::{Entry, Playlist, Repeat};

pub(crate) fn now_ms() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
}
