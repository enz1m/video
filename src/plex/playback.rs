//! Fresh server selections applied once to each loaded stream generation.

use super::{Client, Item, Kind, Server, endpoint, playback_url, server_url};
use enzui_services::media::{Track, VideoService, VideoStatus};
use serde::Deserialize;
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Stream {
    pub(crate) id: u64,
    pub(crate) stream_type: u8,
    pub(crate) selected: bool,
    pub(crate) key: String,
    pub(crate) language_code: String,
    pub(crate) title: String,
    pub(crate) display_title: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Choice {
    pub(crate) ordinal: i32,
    pub(crate) language: String,
    pub(crate) title: String,
}

impl Choice {
    fn index(&self, tracks: &[Track]) -> Option<i32> {
        let ordinal = tracks.iter().find(|track| track.index == self.ordinal);
        let language = |track: &&Track| {
            !self.language.is_empty()
                && track.language.as_deref().is_some_and(|language| {
                    enzui_services::media::video_service::language_name(language)
                        == enzui_services::media::video_service::language_name(&self.language)
                        && enzui_services::media::video_service::language_name(language).is_some()
                        || language.eq_ignore_ascii_case(&self.language)
                })
        };
        ordinal
            .filter(|track| self.language.is_empty() || language(track))
            .or_else(|| {
                tracks.iter().filter(language).find(|track| {
                    !self.title.is_empty() && track.title.as_deref() == Some(self.title.as_str())
                })
            })
            .or_else(|| tracks.iter().find(language))
            .or(ordinal)
            .map(|track| track.index)
    }
}

/// Plex stream ids, in player track order, for saving a changed selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PartStreams {
    pub(crate) part: u64,
    pub(crate) audio: Vec<u64>,
    pub(crate) subtitles: Vec<u64>,
}

impl PartStreams {
    /// The stream ids to save for a track change, or `None` when the player's
    /// tracks do not line up with the server's list. Subtitle id 0 means off.
    pub(crate) fn ids(
        &self,
        status: &VideoStatus,
        audio: Option<i32>,
        subtitle: Option<Option<i32>>,
    ) -> Option<(Option<u64>, Option<u64>)> {
        let pick = |ids: &[u64], tracks: &[Track], index: i32| {
            (ids.len() == tracks.len())
                .then(|| usize::try_from(index).ok().and_then(|index| ids.get(index)))
                .flatten()
                .copied()
        };
        let audio = match audio {
            Some(index) => Some(pick(&self.audio, &status.audio_tracks, index)?),
            None => None,
        };
        let subtitle = match subtitle {
            Some(Some(index)) => Some(pick(&self.subtitles, &status.subtitle_tracks, index)?),
            Some(None) => Some(0),
            None => None,
        };
        (audio.is_some() || subtitle.is_some()).then_some((audio, subtitle))
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Selection {
    pub(crate) audio: Option<Choice>,
    pub(crate) subtitle: Option<Choice>,
    pub(crate) applied: bool,
    pub(crate) streams: Option<PartStreams>,
    /// A transcode carries the chosen tracks already, as its only ones.
    pub(crate) transcoded: bool,
}

impl Selection {
    pub(crate) fn remember(&mut self, status: &VideoStatus) {
        let choice = |track: &Track| Choice {
            ordinal: track.index,
            language: track.language.clone().unwrap_or_default(),
            title: track.title.clone().unwrap_or_default(),
        };
        self.audio = status
            .audio_tracks
            .iter()
            .find(|track| Some(track.index) == status.current_audio)
            .map(choice);
        self.subtitle = status
            .subtitle_tracks
            .iter()
            .find(|track| Some(track.index) == status.current_subtitle)
            .map(choice);
    }

    pub(crate) fn apply(&mut self, video: &VideoService, status: &VideoStatus) {
        if self.applied || status.loading || !status.has_source() || status.error.is_some() {
            return;
        }
        if self.transcoded {
            self.applied = true;
            return;
        }
        if let Some(index) = self
            .audio
            .as_ref()
            .and_then(|choice| choice.index(&status.audio_tracks))
        {
            video.select_audio(index);
        }
        video.select_subtitle(
            self.subtitle
                .as_ref()
                .and_then(|choice| choice.index(&status.subtitle_tracks)),
        );
        self.applied = true;
    }
}

#[derive(Clone)]
pub(crate) struct Prepared {
    pub(crate) server: Server,
    pub(crate) item: Item,
    pub(crate) url: String,
    pub(crate) selection: Selection,
    pub(crate) tracks: super::quality::ServerTracks,
    pub(crate) warning: Option<String>,
}

impl Client {
    pub(crate) async fn prepare_playback(
        &self,
        server: Server,
        item: Item,
    ) -> Result<Prepared, String> {
        let (item, selection, tracks, external) = if server.kind == Kind::Plex {
            let base = server_url(&server.address)?;
            let path = format!("/library/metadata/{}", item.rating_key);
            let response: super::Envelope = self
                .json(self.request(reqwest::Method::GET, endpoint(&base, &path)?, &server.token))
                .await?;
            let item = response
                .container
                .items
                .into_iter()
                .find(|detail| detail.rating_key == item.rating_key)
                .ok_or_else(|| {
                    enzui_core::localization::text(
                        "video-this-title-is-no-longer-available-on-plex-85558f",
                    )
                })?;
            let part = item.media.first().and_then(|media| media.parts.first());
            let streams = part.map(|part| part.streams.as_slice()).unwrap_or_default();
            let ids = |kind| {
                streams
                    .iter()
                    .filter(|stream| stream.stream_type == kind && stream.key.is_empty())
                    .map(|stream| stream.id)
                    .collect::<Vec<_>>()
            };
            let part_streams = part.filter(|part| part.id != 0).map(|part| PartStreams {
                part: part.id,
                audio: ids(2),
                subtitles: ids(3),
            });
            let selected = |kind| {
                streams
                    .iter()
                    .filter(|stream| stream.stream_type == kind)
                    .enumerate()
                    .find(|(_, stream)| stream.selected)
            };
            let choice = |(ordinal, stream): (usize, &Stream)| Choice {
                ordinal: ordinal as i32,
                language: stream.language_code.clone(),
                title: stream.title.clone(),
            };
            let audio = selected(2).map(choice);
            let subtitle = selected(3);
            let external = subtitle.is_some_and(|(_, stream)| !stream.key.is_empty());
            let subtitle = streams
                .iter()
                .filter(|stream| stream.stream_type == 3 && stream.key.is_empty())
                .enumerate()
                .find(|(_, stream)| stream.selected)
                .map(choice);
            let tracks = server_tracks(streams);
            (
                item,
                Selection {
                    audio,
                    subtitle,
                    streams: part_streams,
                    ..Default::default()
                },
                tracks,
                external,
            )
        } else {
            self.api(&server)?
                .playback(&server, &item.rating_key)
                .await?
        };
        let warning =
            external.then(|| enzui_core::localization::text("video-remote-external-subtitle"));
        let url = playback_url(&server, &item)?;
        Ok(Prepared {
            server,
            item,
            url,
            selection,
            tracks,
            warning,
        })
    }
}

/// Plex's audio and subtitle streams for a transcode's track choices.
fn server_tracks(streams: &[Stream]) -> super::quality::ServerTracks {
    use super::quality::{ServerTrack, ServerTracks};
    let list = |kind: u8| {
        let mut ordinal = 0;
        streams
            .iter()
            .filter(|stream| stream.stream_type == kind)
            .enumerate()
            .map(|(position, stream)| {
                let internal = stream.key.is_empty();
                let track = ServerTrack {
                    id: stream.id,
                    label: track_label(
                        &stream.display_title,
                        &stream.title,
                        &stream.language_code,
                        position,
                    ),
                    language: stream.language_code.clone(),
                    title: stream.title.clone(),
                    ordinal: internal.then_some(ordinal),
                };
                if internal {
                    ordinal += 1;
                }
                track
            })
            .collect::<Vec<_>>()
    };
    let chosen = |kind: u8| {
        streams
            .iter()
            .find(|stream| stream.stream_type == kind && stream.selected)
            .map(|stream| stream.id)
    };
    ServerTracks {
        audio: list(2),
        subtitles: list(3),
        audio_choice: chosen(2),
        subtitle_choice: chosen(3),
    }
}

/// A server track's menu name: the server's own, or its language, title or
/// position when the server gives none.
pub(crate) fn track_label(display: &str, title: &str, language: &str, position: usize) -> String {
    [display, title]
        .into_iter()
        .find(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            enzui_services::media::video_service::language_name(language).map(str::to_owned)
        })
        .unwrap_or_else(
            || enzui_core::localized!("video-track-number", "number" => (position + 1).to_string()),
        )
}
