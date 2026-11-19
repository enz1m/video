//! The subtitle, audio, speed and overflow menus: their rows from the
//! player's status, and what each row does.

use super::{App, Message};
use crate::keys::{Command, label};
use crate::model::time::{RATES, rate_label};
use crate::plex::quality::{Quality, ServerTracks};
use enzui_core::menu_model::{CheckState, MenuEntryModel};
use enzui_services::media::{SubtitleSize, VideoStatus};

fn checked(on: bool) -> CheckState {
    if on {
        CheckState::Checked
    } else {
        CheckState::Unchecked
    }
}

/// While a transcode plays, its subtitles are the server's tracks, burned
/// into the picture; otherwise they are the player's own.
pub(crate) fn subtitles(
    status: &VideoStatus,
    server: Option<&ServerTracks>,
) -> Vec<MenuEntryModel> {
    let shown = match server {
        Some(server) => server.subtitle_choice.is_some() || status.subtitle_file.is_some(),
        None => status.current_subtitle.is_some() || status.subtitle_file.is_some(),
    };
    let mut entries = vec![
        MenuEntryModel::command("off", enzui_core::localization::text("video-off-ca7981"))
            .check(checked(!shown)),
    ];
    if let Some(server) = server {
        for track in &server.subtitles {
            entries.push(
                MenuEntryModel::command(format!("server:{}", track.id), track.label.clone())
                    .check(checked(server.subtitle_choice == Some(track.id))),
            );
        }
    } else {
        for track in &status.subtitle_tracks {
            entries.push(
                MenuEntryModel::command(
                    format!("track:{}", track.index),
                    crate::l10n::track(track),
                )
                .check(checked(status.current_subtitle == Some(track.index))),
            );
        }
    }
    if let Some(file) = &status.subtitle_file {
        let name = file.file_name().map_or_else(
            || enzui_core::localization::text("video-subtitle-file-afec13"),
            |name| name.to_string_lossy().into_owned(),
        );
        entries.push(MenuEntryModel::command("file", name).check(CheckState::Checked));
    }
    entries.push(MenuEntryModel::separator("load-rule"));
    entries.push(
        MenuEntryModel::command(
            "load",
            enzui_core::localization::text("video-load-subtitle-file-8dbc1b"),
        )
        .icon("folder-open"),
    );
    entries.push(MenuEntryModel::separator("size-rule"));
    for size in SubtitleSize::ALL {
        entries.push(
            MenuEntryModel::command(
                format!("size:{}", size_key(size)),
                enzui_core::localized!("video-text-c14a18", "value0" => crate::l10n::subtitle_size(size)),
            )
            .check(checked(status.subtitle_size == size)),
        );
    }
    entries.push(MenuEntryModel::separator("delay-rule"));
    entries.push(
        MenuEntryModel::command(
            "earlier",
            enzui_core::localization::text("video-show-earlier-bf2424"),
        )
        .shortcut(label(Command::SubtitlesEarlier))
        .enabled(shown),
    );
    entries.push(
        MenuEntryModel::command(
            "later",
            enzui_core::localization::text("video-show-later-51cf12"),
        )
        .shortcut(label(Command::SubtitlesLater))
        .enabled(shown),
    );
    let delay = status.subtitle_delay_ms;
    entries.push(
        MenuEntryModel::command(
            "reset",
            enzui_core::localization::text("video-reset-timing-77ca48"),
        )
        .description(delay_label(delay))
        .enabled(delay != 0),
    );
    entries
}

pub(crate) fn delay_label(delay_ms: i64) -> String {
    let seconds = delay_ms.unsigned_abs() as f64 / 1000.0;
    match delay_ms {
        0 => enzui_core::localization::text("video-in-sync-570195"),
        d if d > 0 => {
            enzui_core::localized!("video-seconds-1-s-later-b627d3", "seconds" => crate::l10n::number(seconds, 1))
        }
        _ => {
            enzui_core::localized!("video-seconds-1-s-earlier-51640a", "seconds" => crate::l10n::number(seconds, 1))
        }
    }
}

pub(crate) fn audio(status: &VideoStatus, server: Option<&ServerTracks>) -> Vec<MenuEntryModel> {
    if let Some(server) = server {
        return server
            .audio
            .iter()
            .map(|track| {
                MenuEntryModel::command(format!("server:{}", track.id), track.label.clone())
                    .check(checked(server.audio_choice == Some(track.id)))
            })
            .collect();
    }
    status
        .audio_tracks
        .iter()
        .map(|track| {
            let mut entry = MenuEntryModel::command(
                format!("track:{}", track.index),
                crate::l10n::track(track),
            )
            .check(checked(status.current_audio == Some(track.index)));
            if let Some(codec) = &track.codec {
                entry = entry.description(codec.clone());
            }
            entry
        })
        .collect()
}

pub(crate) fn quality(current: Quality) -> Vec<MenuEntryModel> {
    Quality::ALL
        .into_iter()
        .flat_map(|quality| {
            let entry = MenuEntryModel::command(quality.id(), quality.label())
                .check(checked(quality == current));
            // The original stands apart from the transcode presets.
            if quality.is_original() {
                vec![entry, MenuEntryModel::separator("quality-rule")]
            } else {
                vec![entry]
            }
        })
        .collect()
}

/// A server track id from its menu row.
pub(crate) fn server_track(id: &str) -> Option<u64> {
    id.strip_prefix("server:")?.parse().ok()
}

pub(crate) fn speed(rate: f64) -> Vec<MenuEntryModel> {
    RATES
        .iter()
        .map(|offered| {
            let name = if (*offered - 1.0).abs() < 1e-6 {
                enzui_core::localization::text("video-normal-a7248e")
            } else {
                rate_label(*offered)
            };
            MenuEntryModel::command(format!("rate:{offered}"), name)
                .check(checked((offered - rate).abs() < 1e-6))
        })
        .collect()
}

impl App {
    pub(crate) fn refresh_menus(&mut self) {
        let server = self.transcoded_tracks();
        let subtitles = subtitles(&self.status, server);
        let audio = audio(&self.status, server);
        let speed = speed(self.status.rate);
        let quality = quality(self.settings.stream_quality);
        self.menus.quality.0.reconcile(quality);
        let account = account(self);
        let sources = sources(self);
        self.menus.sources.0.reconcile(sources);
        self.menus.subtitles.0.reconcile(subtitles);
        self.menus.audio.0.reconcile(audio);
        self.menus.speed.0.reconcile(speed);
        self.menus.account.0.reconcile(account);
    }
}

pub(crate) enum SubtitleAction {
    Off,
    Track(i32),
    /// A server subtitle while a transcode plays.
    Server(u64),
    Load,
    Size(SubtitleSize),
    Delay(i64),
    ResetDelay,
}

fn size_key(size: SubtitleSize) -> &'static str {
    match size {
        SubtitleSize::Tiny => "Tiny",
        SubtitleSize::ExtraSmall => "Extra small",
        SubtitleSize::Small => "Small",
        SubtitleSize::Medium => "Medium",
        SubtitleSize::Large => "Large",
        SubtitleSize::ExtraLarge => "Extra large",
    }
}

pub(crate) fn subtitle_action(id: &str) -> Option<SubtitleAction> {
    match id {
        "off" => Some(SubtitleAction::Off),
        "load" => Some(SubtitleAction::Load),
        "earlier" => Some(SubtitleAction::Delay(-100)),
        "later" => Some(SubtitleAction::Delay(100)),
        "reset" => Some(SubtitleAction::ResetDelay),
        _ => {
            if let Some(index) = id.strip_prefix("track:") {
                return index.parse().ok().map(SubtitleAction::Track);
            }
            if let Some(track) = server_track(id) {
                return Some(SubtitleAction::Server(track));
            }
            let size = id.strip_prefix("size:")?;
            SubtitleSize::ALL
                .into_iter()
                .find(|candidate| size_key(*candidate) == size)
                .map(SubtitleAction::Size)
        }
    }
}

pub(crate) fn track_index(id: &str) -> Option<i32> {
    id.strip_prefix("track:")?.parse().ok()
}

pub(crate) fn rate_for(id: &str) -> Option<f64> {
    id.strip_prefix("rate:")?.parse().ok()
}

pub(crate) fn sources(app: &App) -> Vec<MenuEntryModel> {
    let active = app.active_source();
    let mut entries: Vec<MenuEntryModel> = app
        .settings
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            MenuEntryModel::command(format!("source:{index}"), source.name.clone())
                .description(source.kind.name())
                .check(checked(active == Some(source)))
        })
        .collect();
    entries.push(MenuEntryModel::separator("sources-rule"));
    entries.push(
        MenuEntryModel::command(
            "add",
            enzui_core::localization::text("video-add-media-server-2c398d"),
        )
        .icon("plus"),
    );
    if let Some(source) = active {
        entries.push(
            MenuEntryModel::command(
                "remove",
                enzui_core::localized!("video-remove-d89228", "value0" => source.name),
            )
            .icon("trash-2")
            .destructive(true),
        );
    }
    entries
}

pub(crate) fn source_action(id: &str) -> Option<Message> {
    use crate::plex::Event;
    Some(Message::Plex(match id {
        "add" => Event::AddSource,
        "remove" => Event::RemoveSource,
        _ => Event::UseSource(id.strip_prefix("source:")?.parse().ok()?),
    }))
}

pub(crate) fn account(app: &App) -> Vec<MenuEntryModel> {
    if app.plex.kind != crate::plex::Kind::Plex {
        let mut entries = vec![
            MenuEntryModel::command(
                "profiles",
                enzui_core::localization::text("video-switch-user-4f8991"),
            )
            .icon("users"),
        ];
        if app
            .plex
            .account
            .as_ref()
            .is_some_and(|account| !account.token.is_empty())
        {
            entries.push(MenuEntryModel::separator("signout-rule"));
            entries.push(
                MenuEntryModel::command(
                    "signout",
                    enzui_core::localization::text("video-sign-out-48f0d3"),
                )
                .icon("log-out"),
            );
        }
        return entries;
    }
    let Some(account) = &app.plex.account else {
        return vec![
            MenuEntryModel::command(
                "signin",
                enzui_core::localization::text("video-sign-in-to-plex-477f1f"),
            )
            .icon("log-in"),
            MenuEntryModel::command(
                "server",
                enzui_core::localization::text("video-change-server-b7ee1d"),
            )
            .icon("server"),
        ];
    };
    let mut entries = Vec::new();
    if account.users.len() > 1 {
        entries.push(
            MenuEntryModel::command(
                "profiles",
                enzui_core::localization::text("video-switch-profile-c1e0da"),
            )
            .icon("users"),
        );
    }
    entries.push(
        MenuEntryModel::command(
            "server",
            enzui_core::localization::text("video-change-server-b7ee1d"),
        )
        .icon("server"),
    );
    entries.push(MenuEntryModel::separator("signout-rule"));
    entries.push(
        MenuEntryModel::command(
            "signout",
            enzui_core::localization::text("video-sign-out-48f0d3"),
        )
        .icon("log-out"),
    );
    entries
}

pub(crate) fn sort(app: &App) -> Vec<MenuEntryModel> {
    let current = app.library_sort();
    crate::plex::Sort::ALL
        .into_iter()
        .map(|sort| {
            MenuEntryModel::command(sort.id(), sort.label()).check(checked(sort == current))
        })
        .collect()
}

pub(crate) fn account_action(id: &str) -> Option<Message> {
    use crate::plex::Event;
    Some(Message::Plex(match id {
        "signin" => Event::SignIn,
        "profiles" => Event::ShowProfiles,
        "server" => Event::ChangeServer,
        "signout" => Event::SignOut,
        _ => return None,
    }))
}
