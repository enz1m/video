//! Which files are videos and subtitles, and which subtitle belongs to which
//! video. Names only; the disk is read elsewhere.

use std::path::Path;

pub(crate) const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "webm", "mov", "avi", "wmv", "flv", "ogv", "mpg", "mpeg", "ts", "m2ts",
    "3gp", "3g2", "vob", "mts",
];

pub(crate) const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub"];

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
}

pub(crate) fn is_video(path: &Path) -> bool {
    extension(path).is_some_and(|extension| VIDEO_EXTENSIONS.contains(&extension.as_str()))
}

pub(crate) fn is_subtitle(path: &Path) -> bool {
    extension(path).is_some_and(|extension| SUBTITLE_EXTENSIONS.contains(&extension.as_str()))
}

// An exact stem outranks language/forced suffixes when choosing sibling subtitles.
pub(crate) fn subtitle_match(video: &Path, candidate: &Path) -> Option<u8> {
    if !is_subtitle(candidate) {
        return None;
    }
    let stem = video.file_stem()?.to_string_lossy();
    let candidate_stem = candidate.file_stem()?.to_string_lossy();
    if candidate_stem == stem {
        return Some(0);
    }
    let rest = candidate_stem
        .strip_prefix(stem.as_ref())?
        .strip_prefix('.')?;
    (!rest.is_empty() && !rest.contains('.')).then_some(1)
}

pub(crate) fn sibling_subtitle<'a>(
    video: &Path,
    siblings: impl IntoIterator<Item = &'a Path>,
) -> Option<&'a Path> {
    siblings
        .into_iter()
        .filter_map(|candidate| subtitle_match(video, candidate).map(|rank| (rank, candidate)))
        .min_by(|(a_rank, a), (b_rank, b)| a_rank.cmp(b_rank).then_with(|| a.cmp(b)))
        .map(|(_, candidate)| candidate)
}

// Bare hosts default to HTTPS; surrounding quotes permit pasted shell arguments.
pub(crate) fn normalize_url(typed: &str) -> Option<String> {
    let trimmed = typed.trim().trim_matches(['"', '\'', '<', '>']);
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return None;
    }
    if trimmed.contains("://") {
        Some(trimmed.to_owned())
    } else if trimmed.contains('.') {
        Some(format!("https://{trimmed}"))
    } else {
        None
    }
}
