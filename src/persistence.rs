//! What the player remembers between runs, and the disk reads the model
//! leaves out.
//!
//! One small file: volume, subtitle preferences, repeat mode and the recent
//! list with resume positions. It is written whole and atomically, off the UI
//! thread, so a crash mid-write leaves the previous version.

use crate::model::thumbnail::Thumbnail;
use crate::model::{History, Repeat, files};
use enzui_core::atomic_state::{self, StateLoad};
use enzui_services::media::SubtitleSize;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FORMAT_ID: &str = "enzui-video-state";
const VERSION: u32 = 1;
const FILE: &str = "state.json";
const MAX_LIBRARY_SORTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) plex_server: String,
    pub(crate) plex_client_id: String,
    pub(crate) plex_user: String,
    pub(crate) plex_server_id: String,
    pub(crate) sources: Vec<crate::plex::Source>,
    pub(crate) source: usize,
    pub(crate) volume: f64,
    pub(crate) muted: bool,
    pub(crate) subtitle_size: SubtitleSizeSetting,
    pub(crate) repeat: Repeat,
    pub(crate) history: History,
    // Each library's chosen order, by `server id/library key`.
    pub(crate) library_sorts: std::collections::BTreeMap<String, crate::plex::Sort>,
    // The quality media servers stream at; the original file by default.
    pub(crate) stream_quality: crate::plex::quality::Quality,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            plex_server: "http://localhost:32400".into(),
            plex_client_id: String::new(),
            plex_user: String::new(),
            plex_server_id: String::new(),
            sources: Vec::new(),
            source: 0,
            volume: 1.0,
            muted: false,
            subtitle_size: SubtitleSizeSetting::default(),
            repeat: Repeat::Off,
            history: History::default(),
            library_sorts: std::collections::BTreeMap::new(),
            stream_quality: crate::plex::quality::Quality::Original,
        }
    }
}

impl Settings {
    fn sanitize(mut self) -> Self {
        // Legacy settings without a source list retain their Plex sign-in.
        if self.sources.is_empty() && !(self.plex_user.is_empty() && self.plex_server_id.is_empty())
        {
            self.sources.push(crate::plex::Source {
                kind: crate::plex::Kind::Plex,
                name: "Plex".into(),
                ..Default::default()
            });
        }
        self.source = self.source.min(self.sources.len().saturating_sub(1));
        self.volume = if self.volume.is_finite() {
            self.volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.history.sanitize();
        while self.library_sorts.len() > MAX_LIBRARY_SORTS {
            self.library_sorts.pop_first();
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum SubtitleSizeSetting {
    Tiny,
    ExtraSmall,
    Small,
    #[default]
    Medium,
    Large,
    ExtraLarge,
}

impl From<SubtitleSizeSetting> for SubtitleSize {
    fn from(size: SubtitleSizeSetting) -> Self {
        match size {
            SubtitleSizeSetting::Tiny => SubtitleSize::Tiny,
            SubtitleSizeSetting::ExtraSmall => SubtitleSize::ExtraSmall,
            SubtitleSizeSetting::Small => SubtitleSize::Small,
            SubtitleSizeSetting::Medium => SubtitleSize::Medium,
            SubtitleSizeSetting::Large => SubtitleSize::Large,
            SubtitleSizeSetting::ExtraLarge => SubtitleSize::ExtraLarge,
        }
    }
}

impl From<SubtitleSize> for SubtitleSizeSetting {
    fn from(size: SubtitleSize) -> Self {
        match size {
            SubtitleSize::Tiny => SubtitleSizeSetting::Tiny,
            SubtitleSize::ExtraSmall => SubtitleSizeSetting::ExtraSmall,
            SubtitleSize::Small => SubtitleSizeSetting::Small,
            SubtitleSize::Large => SubtitleSizeSetting::Large,
            SubtitleSize::ExtraLarge => SubtitleSizeSetting::ExtraLarge,
            _ => SubtitleSizeSetting::Medium,
        }
    }
}

// Test isolation may override the framework application data directory.
pub(crate) fn state_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ENZUI_VIDEO_STATE_DIR").filter(|dir| !dir.is_empty()) {
        let dir = PathBuf::from(dir);
        return dir.is_absolute().then_some(dir);
    }
    let app = enzui_core::app_data::AppId::new("enzui-video").ok()?;
    enzui_core::app_data::AppDataResolver::from_env()
        .ensure_tightening(&app)
        .ok()
        .map(|dir| dir.path().to_path_buf())
}

// Missing or unreadable preferences must not prevent playback startup.
pub(crate) fn load(dir: &Path) -> Settings {
    match atomic_state::load_with_reason(&dir.join(FILE), FORMAT_ID, VERSION).0 {
        StateLoad::Loaded(body) => serde_json::from_str::<Settings>(&body)
            .map(Settings::sanitize)
            .unwrap_or_default(),
        StateLoad::UsedDefaults { .. } => Settings::default(),
    }
}

pub(crate) fn save(dir: &Path, settings: &Settings) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let body = serde_json::to_string(settings).map_err(std::io::Error::other)?;
    atomic_state::save_atomic(&dir.join(FILE), FORMAT_ID, VERSION, &body)
}

pub(crate) fn sibling_subtitle(video: &Path) -> Option<PathBuf> {
    let dir = video.parent()?;
    let siblings: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| files::is_subtitle(path) && path.is_file())
        .collect();
    files::sibling_subtitle(video, siblings.iter().map(PathBuf::as_path)).map(Path::to_path_buf)
}

pub(crate) fn videos_in(dir: &Path) -> Vec<PathBuf> {
    let mut videos: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| files::is_video(path) && path.is_file())
                .collect()
        })
        .unwrap_or_default();
    videos.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    videos
}

pub(crate) fn pictures_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_PICTURES_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let config =
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
    let named = std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|dirs| {
            dirs.lines()
                .find_map(|line| line.strip_prefix("XDG_PICTURES_DIR="))
                .map(|value| {
                    value
                        .trim_matches('"')
                        .replace("$HOME", &home.to_string_lossy())
                })
        })
        .map(PathBuf::from);
    Some(named.unwrap_or_else(|| home.join("Pictures")))
}

// Save an RGBA frame as a PNG, checking for an unused name before atomic publication.
// The name check does not reserve the path against concurrent saves.
pub(crate) fn save_frame(
    dir: &Path,
    stem: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut path = dir.join(format!("{stem}.png"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).png"));
        n += 1;
    }
    use image::ImageEncoder;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(pixels, width, height, image::ExtendedColorType::Rgba8)
        .map_err(std::io::Error::other)?;
    atomic_state::save_atomic_raw(&path, &png)?;
    Ok(path)
}

fn thumbnail_dir(dir: &Path) -> PathBuf {
    dir.join("thumbnails")
}

// Write `fresh` thumbnails, then delete any not named in `keep`, so a
// forgotten or cleared entry leaves no picture of it behind.
pub(crate) fn save_thumbnails(
    dir: &Path,
    fresh: &[(String, Thumbnail)],
    keep: &[String],
) -> std::io::Result<()> {
    let thumbs = thumbnail_dir(dir);
    if !fresh.is_empty() {
        std::fs::create_dir_all(&thumbs)?;
    }
    for (key, thumb) in fresh.iter().filter(|(key, _)| keep.contains(key)) {
        use image::ImageEncoder;
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &thumb.pixels,
                thumb.width,
                thumb.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(std::io::Error::other)?;
        atomic_state::save_atomic_raw(&thumbs.join(format!("{key}.png")), &png)?;
    }
    let Ok(listing) = std::fs::read_dir(&thumbs) else {
        return Ok(());
    };
    for file in listing.flatten() {
        let path = file.path();
        let kept = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| keep.iter().any(|key| key == stem));
        if !kept {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

pub(crate) fn load_thumbnails(dir: &Path, keys: &[String]) -> Vec<(String, Thumbnail)> {
    let thumbs = thumbnail_dir(dir);
    keys.iter()
        .filter_map(|key| {
            let mut reader = image::ImageReader::open(thumbs.join(format!("{key}.png"))).ok()?;
            reader.set_format(image::ImageFormat::Png);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(crate::model::thumbnail::WIDTH);
            limits.max_image_height = Some(crate::model::thumbnail::WIDTH);
            limits.max_alloc = Some(u64::from(crate::model::thumbnail::WIDTH).pow(2) * 4);
            reader.limits(limits);
            let image = reader.decode().ok()?.into_rgba8();
            let (width, height) = image.dimensions();
            Some((
                key.clone(),
                Thumbnail {
                    width,
                    height,
                    pixels: image.into_raw(),
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Entry;
    use enzui_test_support::test_scratch::ScratchDir;
    use std::time::Duration;

    #[test]
    fn saved_state_and_exported_frames_preserve_data_and_destination_authority() {
        {
            let scratch = ScratchDir::new("video-settings");
            let dir = scratch.path();
            atomic_state::save_atomic(&dir.join(FILE), FORMAT_ID, VERSION,
            r#"{"volume":0.4,"plex_user":"owner","history":{"items":[{"entry":{"File":"/v/old.mkv"},"title":"Old","position_ms":20000,"duration_ms":120000,"played_at":1}]}}"#,
        ).unwrap();
            let mut settings = load(dir);
            assert_eq!(settings.volume, 0.4);
            assert_eq!(settings.sources[0].kind, crate::plex::Kind::Plex);
            assert_eq!(
                settings
                    .history
                    .resume_point(&Entry::File("/v/old.mkv".into())),
                Some(Duration::from_secs(20))
            );
            settings.volume = 0.6;
            settings.muted = true;
            settings.repeat = Repeat::All;
            settings
                .library_sorts
                .insert("server/2".into(), crate::plex::Sort::Added);
            save(dir, &settings).expect("save");
            assert_eq!(load(dir), settings);
            for index in 0..MAX_LIBRARY_SORTS + 5 {
                settings
                    .library_sorts
                    .insert(format!("server/{index:03}"), crate::plex::Sort::Rating);
            }
            save(dir, &settings).expect("save");
            assert_eq!(
                load(dir).library_sorts.len(),
                MAX_LIBRARY_SORTS,
                "saved library orders stay bounded"
            );
            std::fs::write(dir.join(FILE), "garbage").expect("write");
            assert_eq!(load(dir), Settings::default());
        }
        {
            let scratch = ScratchDir::new("video-frames");
            let dir = scratch.path();
            let pixels = vec![255u8; 4 * 4 * 4];
            let stem = crate::view::file_stem("../outside/Clip");
            let first = save_frame(dir, &stem, 4, 4, &pixels).expect("first");
            let second = save_frame(dir, &stem, 4, 4, &[0; 64]).expect("second");
            assert_ne!(first, second);
            assert_eq!(first.parent(), Some(dir));
            assert_eq!(
                image::open(&first).unwrap().to_rgba8().get_pixel(0, 0).0,
                [255; 4]
            );
        }
    }

    #[test]
    fn thumbnail_cache_roundtrips_bounded_images_and_discards_invalid_entries() {
        {
            let scratch = ScratchDir::new("video-thumbnails");
            let dir = scratch.path();
            let thumb = |shade: u8| Thumbnail {
                width: 2,
                height: 1,
                pixels: vec![shade, 0, 0, 255, 0, shade, 0, 255],
            };
            let keep = ["a".to_owned(), "b".to_owned()];
            save_thumbnails(
                dir,
                &[("a".into(), thumb(10)), ("b".into(), thumb(20))],
                &keep,
            )
            .expect("save");
            let loaded = load_thumbnails(dir, &keep);
            assert_eq!(
                loaded,
                vec![("a".into(), thumb(10)), ("b".into(), thumb(20))]
            );
            save_thumbnails(dir, &[("c".into(), thumb(30))], &keep[1..]).expect("prune");
            assert_eq!(load_thumbnails(dir, &keep), vec![("b".into(), thumb(20))]);
            assert_eq!(
                std::fs::read_dir(thumbnail_dir(dir)).expect("dir").count(),
                1,
                "only b.png is left"
            );
        }
        {
            let scratch = ScratchDir::new("video-thumbnail-limits");
            let dir = scratch.path();
            let thumbs = thumbnail_dir(dir);
            std::fs::create_dir_all(&thumbs).unwrap();
            image::save_buffer_with_format(
                thumbs.join("oversized.png"),
                &[0; 193 * 4],
                193,
                1,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )
            .unwrap();
            std::fs::write(thumbs.join("corrupt.png"), "not a PNG").unwrap();
            assert!(
                load_thumbnails(dir, &["oversized".into(), "corrupt".into()]).is_empty(),
                "oversized and corrupt recent thumbnails are skipped"
            );
        }
    }
}
