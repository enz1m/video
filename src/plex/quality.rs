//! Stream quality: the original file, or a server transcode at a preset.
//!
//! A transcode is an HLS stream the server encodes on demand to H.264 and
//! AAC at a bitrate and size cap. It carries one audio track and burns the
//! chosen subtitles into the picture, so while transcoding the audio and
//! subtitle choices are the server's tracks and changing one restarts the
//! stream at the same position.

use super::playback::Choice;
use super::{Item, Kind, Server, endpoint, server_url};
use serde::{Deserialize, Serialize};

/// The streaming quality a media server is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum Quality {
    /// The file as stored on the server, played directly.
    #[default]
    Original,
    Mbps20,
    Mbps12,
    Mbps8,
    Mbps4,
    Mbps2,
    Kbps1500,
    Kbps720,
}

impl Quality {
    pub(crate) const ALL: [Self; 8] = [
        Self::Original,
        Self::Mbps20,
        Self::Mbps12,
        Self::Mbps8,
        Self::Mbps4,
        Self::Mbps2,
        Self::Kbps1500,
        Self::Kbps720,
    ];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Original => "quality:original",
            Self::Mbps20 => "quality:20000",
            Self::Mbps12 => "quality:12000",
            Self::Mbps8 => "quality:8000",
            Self::Mbps4 => "quality:4000",
            Self::Mbps2 => "quality:2000",
            Self::Kbps1500 => "quality:1500",
            Self::Kbps720 => "quality:720",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|quality| quality.id() == id)
    }

    /// The video bitrate cap in kbps; `None` plays the original.
    pub(crate) fn video_kbps(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::Mbps20 => Some(20_000),
            Self::Mbps12 => Some(12_000),
            Self::Mbps8 => Some(8_000),
            Self::Mbps4 => Some(4_000),
            Self::Mbps2 => Some(2_000),
            Self::Kbps1500 => Some(1_500),
            Self::Kbps720 => Some(720),
        }
    }

    /// The largest picture the transcode may send, as width and height.
    pub(crate) fn max_size(self) -> (u32, u32) {
        match self {
            Self::Original | Self::Mbps20 | Self::Mbps12 | Self::Mbps8 => (1920, 1080),
            Self::Mbps4 | Self::Mbps2 => (1280, 720),
            Self::Kbps1500 => (854, 480),
            Self::Kbps720 => (640, 360),
        }
    }

    pub(crate) fn is_original(self) -> bool {
        self == Self::Original
    }

    /// The menu entry: "Original", or the rate and height, "4 Mbps · 720p".
    pub(crate) fn label(self) -> String {
        let Some(kbps) = self.video_kbps() else {
            return enzui_core::localization::text("video-quality-original");
        };
        let rate = if kbps < 1_000 {
            enzui_core::localized!("video-quality-kbps", "value" => kbps.to_string())
        } else if kbps % 1_000 == 0 {
            enzui_core::localized!("video-quality-mbps", "value" => (kbps / 1_000).to_string())
        } else {
            let value = format!("{}.{}", kbps / 1_000, kbps % 1_000 / 100);
            enzui_core::localized!("video-quality-mbps", "value" => value)
        };
        enzui_core::localized!(
            "video-quality-preset",
            "rate" => rate,
            "height" => self.max_size().1.to_string()
        )
    }

    /// The control's caption: "Original", or the height, "720p".
    pub(crate) fn short_label(self) -> String {
        if self.is_original() {
            return enzui_core::localization::text("video-quality-original");
        }
        enzui_core::localized!("video-quality-height", "height" => self.max_size().1.to_string())
    }
}

/// One audio or subtitle track as the server lists it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ServerTrack {
    /// Plex's stream id, or the Jellyfin/Emby stream index.
    pub(crate) id: u64,
    pub(crate) label: String,
    pub(crate) language: String,
    pub(crate) title: String,
    /// The track's place among the file's own tracks of its kind, which is
    /// the player's track index when the original plays; `None` for an
    /// external subtitle file.
    pub(crate) ordinal: Option<i32>,
}

impl ServerTrack {
    fn choice(&self) -> Option<Choice> {
        Some(Choice {
            ordinal: self.ordinal?,
            language: self.language.clone(),
            title: self.title.clone(),
        })
    }
}

/// The server's audio and subtitle tracks and which of them are chosen.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ServerTracks {
    pub(crate) audio: Vec<ServerTrack>,
    pub(crate) subtitles: Vec<ServerTrack>,
    pub(crate) audio_choice: Option<u64>,
    pub(crate) subtitle_choice: Option<u64>,
}

impl ServerTracks {
    /// The player-side choices for the original file.
    pub(crate) fn choices(&self) -> (Option<Choice>, Option<Choice>) {
        let find = |tracks: &[ServerTrack], id: Option<u64>| {
            tracks
                .iter()
                .find(|track| Some(track.id) == id)
                .and_then(ServerTrack::choice)
        };
        (
            find(&self.audio, self.audio_choice),
            find(&self.subtitles, self.subtitle_choice),
        )
    }
}

/// Asks Plex to describe the stream it may send: HLS in MPEG-TS with H.264
/// video and AAC or MP3 audio, which GStreamer plays everywhere.
const PLEX_PROFILE: &str = "add-transcode-target(type=videoProfile&context=streaming&protocol=hls\
                            &container=mpegts&videoCodec=h264&audioCodec=aac,mp3)";

/// The address that plays `item`: the original file, or a transcode at
/// `quality` identified by `session`.
pub(crate) fn stream_url(
    server: &Server,
    item: &Item,
    quality: Quality,
    tracks: &ServerTracks,
    session: &str,
    client_id: &str,
) -> Result<String, String> {
    let Some(kbps) = quality.video_kbps() else {
        return super::playback_url(server, item);
    };
    if server.kind != Kind::Plex {
        return crate::jellyfin::transcode(server, item, quality, kbps, tracks, session, client_id);
    }
    // The single-part check and the playable-item error match the original.
    super::playback_url(server, item)?;
    let (width, height) = quality.max_size();
    let mut url = endpoint(
        &server_url(&server.address)?,
        "/video/:/transcode/universal/start.m3u8",
    )?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("path", &format!("/library/metadata/{}", item.rating_key))
            .append_pair("mediaIndex", "0")
            .append_pair("partIndex", "0")
            .append_pair("protocol", "hls")
            .append_pair("fastSeek", "1")
            .append_pair("directPlay", "0")
            .append_pair("directStream", "1")
            .append_pair("directStreamAudio", "1")
            .append_pair("videoQuality", "100")
            .append_pair("maxVideoBitrate", &kbps.to_string())
            .append_pair("videoResolution", &format!("{width}x{height}"))
            .append_pair("subtitles", "burn")
            .append_pair("session", session)
            .append_pair("X-Plex-Session-Identifier", session)
            .append_pair("X-Plex-Client-Identifier", client_id)
            .append_pair("X-Plex-Product", "enzUI Video")
            .append_pair("X-Plex-Version", env!("CARGO_PKG_VERSION"))
            .append_pair("X-Plex-Platform", "Generic")
            .append_pair("X-Plex-Client-Profile-Extra", PLEX_PROFILE);
        if !server.token.is_empty() {
            query.append_pair("X-Plex-Token", server.token.value());
        }
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plex::{Media, Part, Token};

    fn movie() -> Item {
        Item {
            rating_key: "42".into(),
            media: vec![Media {
                source_id: "source-42".into(),
                parts: vec![Part {
                    key: "/library/parts/7/file.mkv".into(),
                    ..Part::default()
                }],
                ..Media::default()
            }],
            ..Item::default()
        }
    }

    fn server(kind: Kind) -> Server {
        Server {
            kind,
            address: "http://192.168.1.5:32400".into(),
            token: Token::new("secret".into()),
            ..Server::default()
        }
    }

    #[test]
    fn original_plays_the_file_and_presets_ask_for_a_capped_transcode() {
        let tracks = ServerTracks {
            audio_choice: Some(2),
            subtitle_choice: Some(4),
            ..ServerTracks::default()
        };
        let plex = server(Kind::Plex);
        let original = stream_url(&plex, &movie(), Quality::Original, &tracks, "s1", "c1").unwrap();
        assert!(original.contains("/library/parts/7/file.mkv"), "{original}");

        let transcode = stream_url(&plex, &movie(), Quality::Mbps4, &tracks, "s1", "c1").unwrap();
        let url = url::Url::parse(&transcode).unwrap();
        assert_eq!(url.path(), "/video/:/transcode/universal/start.m3u8");
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["path"], "/library/metadata/42");
        assert_eq!(query["maxVideoBitrate"], "4000");
        assert_eq!(query["videoResolution"], "1280x720");
        assert_eq!(query["session"], "s1");
        assert_eq!(query["X-Plex-Client-Identifier"], "c1");
        assert!(query["X-Plex-Client-Profile-Extra"].contains("protocol=hls"));

        for (kind, prefix, key) in [
            (Kind::Jellyfin, "", "ApiKey"),
            (Kind::Emby, "/emby", "api_key"),
        ] {
            let transcode = stream_url(
                &server(kind),
                &movie(),
                Quality::Kbps720,
                &tracks,
                "s2",
                "c2",
            )
            .unwrap();
            let url = url::Url::parse(&transcode).unwrap();
            assert_eq!(url.path(), format!("{prefix}/Videos/42/master.m3u8"));
            let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(query["MediaSourceId"], "source-42");
            assert_eq!(query["PlaySessionId"], "s2");
            assert_eq!(query["DeviceId"], "c2");
            assert_eq!(query["VideoBitrate"], "720000");
            assert_eq!(query["MaxHeight"], "360");
            assert_eq!(query["AudioStreamIndex"], "2");
            assert_eq!(query["SubtitleStreamIndex"], "4");
            assert_eq!(query[key], "secret");
        }
    }

    #[test]
    fn presets_round_trip_their_ids_and_name_rate_and_height() {
        for quality in Quality::ALL {
            assert_eq!(Quality::from_id(quality.id()), Some(quality));
        }
        assert_eq!(Quality::from_id("quality:nope"), None);
        assert!(Quality::Original.video_kbps().is_none());
        assert!(
            Quality::ALL
                .windows(2)
                .skip(1)
                .all(|pair| pair[0].video_kbps() > pair[1].video_kbps()),
            "presets run from the highest rate down"
        );
    }
}
