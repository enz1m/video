//! The player's state and the one `update` that changes it.

mod chooser;
mod dialogs;
mod io;
mod menus;
mod message;
mod native_drop;
pub(crate) mod queue_drop;
pub(crate) use native_drop::{Delivery as NativeDelivery, Offer as FileOffer};
mod open;
mod playback;
mod plex;
mod sources;
pub(crate) use sources::{PASSWORD_FIELD, USERNAME_FIELD};
mod update;

#[cfg(test)]
mod tests;

pub(crate) use dialogs::Dialog;
pub(crate) use message::{Message, Pick};

use crate::model::{Entry, Playlist};
use crate::persistence::{self, Settings};
use enzui_core::menu_model::{MenuModel, MenuState};
use enzui_core::portal::inhibit::IdleInhibitor;
use enzui_core::theme_file::integration::ThemeIntegration;
use enzui_services::media::{VideoService, VideoServiceConfig, VideoStatus};
use enzui_widgets::file_chooser::ChooserSession;
use enzui_widgets::toast_host::{ToastExpirySchedule, ToastHost};
use enzui_widgets::video::LatestFramePresenter;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(crate) const DESKTOP_APP_ID: &str = "io.github.enzui.Video";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Osd {
    pub(crate) text: String,
    pub(crate) token: u64,
}

pub(crate) type Menu = (MenuModel, MenuState);

pub(crate) struct Menus {
    pub(crate) subtitles: Menu,
    pub(crate) audio: Menu,
    pub(crate) speed: Menu,
    pub(crate) account: Menu,
    pub(crate) sources: Menu,
    pub(crate) sort: Menu,
    pub(crate) quality: Menu,
}

impl Menus {
    pub(crate) fn any_open(&self) -> bool {
        [
            &self.subtitles,
            &self.audio,
            &self.speed,
            &self.account,
            &self.sources,
            &self.sort,
            &self.quality,
        ]
        .iter()
        .any(|(_, state)| state.is_open())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum QueueCommand {
    Play,
    Remove,
}

pub(crate) struct App {
    pub(crate) instance: crate::desktop::Instance,
    native_drop: native_drop::NativeDrop,
    pub(crate) plex: crate::plex::State,
    pub(crate) plex_resume: bool,
    pub(crate) themes: ThemeIntegration,
    pub(crate) video: Result<VideoService, String>,
    pub(crate) status: VideoStatus,
    service_selection: Option<crate::plex::playback::Selection>,
    recovery: playback::Recovery,
    pub(crate) presenter: LatestFramePresenter,
    pub(crate) playlist: Playlist,
    pub(crate) queue_drag_owner: String,
    pub(crate) queue_context: enzui_widgets::context_menu_host::ContextMenu<Entry, QueueCommand>,
    // The entry `status` describes. The queue moves on before the next
    // load, so the current entry is not always the one that played.
    playing: Option<Entry>,
    pub(crate) settings: Settings,
    pub(crate) state_dir: Option<PathBuf>,
    pub(crate) thumbnails: std::collections::HashMap<String, iced::widget::image::Handle>,
    fresh_thumbnails: Vec<(String, crate::model::thumbnail::Thumbnail)>,
    // Periodic recording skips the most recently captured entry key.
    thumbed: Option<String>,
    pub(crate) chrome_hidden: bool,
    pub(crate) over_controls: bool,
    pub(crate) titlebar_menu: bool,
    pub(crate) fullscreen: bool,
    pub(crate) window_size: iced::Size,
    // The load whose video shape the window was last fitted to; each
    // opened video fits the window once.
    fitted: Option<enzui_services::media::Generation>,
    pub(crate) scale: f32,
    pub(crate) scrub: Option<Duration>,
    scrub_dragging: bool,
    surface_click: u64,
    position_anchor: Duration,
    position_received: Instant,
    pending_seek: Option<enzui_services::media::video_service::SeekRequest>,
    scrub_token: u64,
    // Bumped per load, so a late sibling-subtitle answer for an earlier
    // video is dropped.
    load_token: u64,
    open_token: u64,
    drop_issued: usize,
    drop_next: usize,
    drop_started: bool,
    drop_buffer: std::collections::BTreeMap<usize, (Vec<Entry>, Option<PathBuf>)>,
    pub(crate) menus: Menus,
    pub(crate) dialogs: enzui_widgets::modal_host::DialogStack<Dialog>,
    pub(crate) url_presenter: Option<enzui_widgets::form_dialog::FormPresenter>,
    pub(crate) show_playlist: bool,
    pub(crate) queue_navigation: enzui_widgets::nav_split::NavSplitState,
    pub(crate) osd: Option<Osd>,
    osd_token: u64,
    pub(crate) chooser: ChooserSession<Pick>,
    chooser_parent: chooser::Parenting,
    pub(crate) awake: IdleInhibitor,
    pub(crate) toasts: ToastHost,
    pub(crate) toast_schedule: ToastExpirySchedule,
    last_record: Instant,
    // A settings write is queued or running; another waits for it.
    saving: bool,
    sleep_ack: Option<enzui_core::system_sleep::SleepAck>,
    session_ack: Option<enzui_core::portal::session_monitor::QueryEndAck>,
    save_again: bool,
    pub(crate) closing: Option<iced::window::Id>,
    last_drop: Option<Instant>,
}

impl App {
    pub(crate) fn new(opening: Vec<std::ffi::OsString>) -> (Self, iced::Task<Message>) {
        let (files, plex, server) = crate::desktop::arguments(opening);
        let (themes, boot) = ThemeIntegration::boot();
        let state_dir = persistence::state_dir();
        let settings = state_dir
            .as_deref()
            .map(persistence::load)
            .unwrap_or_default();
        let video = create_video_service();
        let mut app = Self::with(themes, video, settings, state_dir);
        let opened = app.update(Message::Launched(files));
        let plex_boot = if plex {
            app.plex.visible = true;
            if let Some(server) = server {
                app.plex.server_input = server;
                app.plex.restore_attempted = true;
                app.plex_event(crate::plex::Event::Connect)
            } else {
                app.plex_event(crate::plex::Event::Open)
            }
        } else {
            iced::Task::none()
        };
        // Frame output dimensions use physical pixels.
        let scale = iced::window::latest().then(|window| match window {
            Some(window) => iced::window::scale_factor(window).map(Message::Rescaled),
            None => iced::Task::none(),
        });
        let thumbnails = match app.state_dir.clone() {
            Some(dir) if !app.settings.history.is_empty() => {
                let keys = app.settings.history.thumbnail_keys();
                crate::l10n::perform(
                    io::disk(move || persistence::load_thumbnails(&dir, &keys)),
                    Message::ThumbnailsLoaded,
                )
            }
            _ => iced::Task::none(),
        };
        (
            app,
            iced::Task::batch([
                boot.map(|event| Message::Theme(Box::new(event))),
                opened,
                plex_boot,
                scale,
                thumbnails,
            ]),
        )
    }

    pub(crate) fn with(
        themes: ThemeIntegration,
        video: Result<VideoService, String>,
        settings: Settings,
        state_dir: Option<PathBuf>,
    ) -> Self {
        let mut playlist = Playlist::default();
        playlist.repeat = settings.repeat;
        let status = VideoStatus {
            volume: settings.volume,
            muted: settings.muted,
            subtitle_size: settings.subtitle_size.into(),
            ..VideoStatus::default()
        };
        let mut settings = settings;
        if settings.plex_client_id.is_empty() {
            std::fs::read_to_string("/proc/sys/kernel/random/uuid")
                .unwrap_or_else(|_| {
                    format!(
                        "enz-video-{}-{}",
                        std::process::id(),
                        crate::model::now_ms()
                    )
                })
                .trim()
                .clone_into(&mut settings.plex_client_id);
        }
        let mut app = Self {
            instance: crate::desktop::Instance::default(),
            native_drop: native_drop::NativeDrop::default(),
            plex: crate::plex::State::new(settings.plex_server.clone()),
            plex_resume: true,
            themes,
            video,
            status,
            service_selection: None,
            recovery: playback::Recovery::default(),
            presenter: LatestFramePresenter::new(),
            playlist,
            queue_drag_owner: format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            queue_context: enzui_widgets::context_menu_host::ContextMenu::new(),
            playing: None,
            settings,
            state_dir,
            thumbnails: std::collections::HashMap::new(),
            fresh_thumbnails: Vec::new(),
            thumbed: None,
            chrome_hidden: false,
            over_controls: false,
            titlebar_menu: false,
            fullscreen: false,
            window_size: iced::Size::new(1100.0, 680.0),
            fitted: None,
            scale: 1.0,
            scrub: None,
            scrub_dragging: false,
            surface_click: 0,
            position_anchor: Duration::ZERO,
            position_received: Instant::now(),
            pending_seek: None,
            scrub_token: 0,
            load_token: 0,
            open_token: 0,
            drop_issued: 0,
            drop_next: 0,
            drop_started: false,
            drop_buffer: std::collections::BTreeMap::new(),
            menus: Menus {
                subtitles: (MenuModel::new(Vec::new()), MenuState::closed()),
                audio: (MenuModel::new(Vec::new()), MenuState::closed()),
                speed: (MenuModel::new(Vec::new()), MenuState::closed()),
                account: (MenuModel::new(Vec::new()), MenuState::closed()),
                sources: (MenuModel::new(Vec::new()), MenuState::closed()),
                sort: (MenuModel::new(Vec::new()), MenuState::closed()),
                quality: (MenuModel::new(Vec::new()), MenuState::closed()),
            },
            dialogs: enzui_widgets::modal_host::DialogStack::new(),
            url_presenter: None,
            show_playlist: false,
            queue_navigation: enzui_widgets::nav_split::NavSplitState {
                preference: enzui_widgets::nav_split::SidebarPreference::Hidden,
                ..Default::default()
            },
            osd: None,
            osd_token: 0,
            chooser: ChooserSession::new().with_fallback(),
            chooser_parent: chooser::Parenting::default(),
            awake: IdleInhibitor::idle(enzui_core::localization::text("video-playing-reason")),
            toasts: ToastHost::new(),
            toast_schedule: ToastExpirySchedule::default(),
            last_record: Instant::now(),
            saving: false,
            sleep_ack: None,
            session_ack: None,
            save_again: false,
            closing: None,
            last_drop: None,
        };
        app.refresh_menus();
        app
    }

    pub(crate) fn theme(&self) -> iced::Theme {
        self.themes.iced_theme()
    }

    pub(crate) fn title(&self) -> String {
        if self.plex.visible {
            return enzui_core::localized!("video-window-title", "title" => self.plex.kind.name());
        }
        match self.current_title() {
            Some(title) => enzui_core::localized!("video-window-title", "title" => title),
            None => enzui_core::localization::text("video-app-name"),
        }
    }

    pub(crate) fn current_title(&self) -> Option<String> {
        if self
            .plex
            .playing_url
            .as_ref()
            .is_some_and(|url| self.playlist.current() == Some(&Entry::Url(url.clone())))
        {
            return self.plex.playing.as_ref().map(|item| item.title.clone());
        }
        self.status
            .display_title()
            .map(str::to_owned)
            .or_else(|| self.playlist.current().map(Entry::name))
    }

    pub(crate) fn has_source(&self) -> bool {
        self.video.is_ok() && (self.status.has_source() || self.playlist.current().is_some())
    }

    pub(crate) fn queue_available(&self) -> bool {
        !self.plex.visible && self.plex.playing.is_none() && !self.playlist.is_empty()
    }

    pub(crate) fn shown_position(&self) -> Duration {
        self.shown_position_at(Instant::now())
    }

    fn shown_position_at(&self, now: Instant) -> Duration {
        if let Some(scrub) = self.scrub {
            return scrub;
        }
        let position = if self.timeline_running() {
            self.position_anchor.saturating_add(
                now.saturating_duration_since(self.position_received)
                    .min(Duration::from_millis(250))
                    .mul_f64(self.status.rate.clamp(0.0, 16.0)),
            )
        } else {
            self.status.position
        };
        self.status
            .duration
            .map_or(position, |end| position.min(end))
    }

    fn timeline_running(&self) -> bool {
        self.status.is_playing()
            && !self.status.loading
            && !self.status.seeking
            && self.status.buffering.is_none()
            && self.status.error.is_none()
            && self.pending_seek.is_none()
    }

    pub(crate) fn chrome_visible(&self) -> bool {
        self.plex.visible
            || !self.has_source()
            || !self.chrome_hidden
            || !self.status.is_playing()
            || self.menus.any_open()
            || self.titlebar_menu
            || self.scrub_dragging
            || self.queue_context.is_open()
            || !self.dialogs.is_empty()
            || self.show_playlist
    }

    pub(crate) fn subscription(&self) -> iced::Subscription<Message> {
        let toasts = match self.toast_schedule.armed() {
            Some((deadline, wait)) => iced::time::every(wait)
                .with(deadline)
                .map(|(_, now)| Message::Toast(enzui_widgets::toast_host::ToastEvent::Tick(now))),
            None => iced::Subscription::none(),
        };
        let video = match &self.video {
            Ok(video) => video.subscription().map(|_| Message::VideoWake),
            Err(_) => iced::Subscription::none(),
        };
        iced::Subscription::batch([
            self.instance.subscription(),
            enzui_core::system_status::memory_subscription().map(Message::MemoryPressure),
            enzui_core::system_sleep::subscription(DESKTOP_APP_ID).map(Message::Sleep),
            enzui_core::portal::session_monitor::subscription().map(Message::Session),
            self.themes
                .subscription()
                .map(|event| Message::Theme(Box::new(event))),
            iced::window::close_requests().map(Message::CloseRequested),
            crate::keys::subscription(),
            window_events(),
            self.chooser.subscription().map(|_| Message::ChooserWake),
            video,
            if self.timeline_running()
                && !self.plex.visible
                && self.chrome_visible()
                && self.scrub.is_none()
            {
                iced::time::every(Duration::from_millis(16)).map(|_| Message::TimelineFrame)
            } else {
                iced::Subscription::none()
            },
            if self.plex.playing.is_some() {
                iced::time::every(Duration::from_secs(10))
                    .map(|_| Message::Plex(crate::plex::Event::Progress))
            } else {
                iced::Subscription::none()
            },
            toasts,
        ])
    }
}

fn create_video_service() -> Result<VideoService, String> {
    create_video_service_with(network_video_config())
}

fn network_video_config() -> VideoServiceConfig {
    use enzui_core::media::remote_gate::StreamBudget;
    VideoServiceConfig {
        preroll_timeout: Duration::from_secs(45),
        gate: enzui_core::media::remote_gate::MediaGate {
            budget: StreamBudget {
                buffer_bytes: 32 * 1024 * 1024,
                ring_buffer_bytes: 256 * 1024 * 1024,
                ..StreamBudget::DEFAULT
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(crate) fn create_video_service_with(
    config: VideoServiceConfig,
) -> Result<VideoService, String> {
    VideoService::new(config).map_err(
        |error| enzui_core::localized!("video-playback-unavailable", "error" => error.message),
    )
}

fn window_events() -> iced::Subscription<Message> {
    iced::event::listen_with(|event, _status, _window| match event {
        iced::Event::Window(iced::window::Event::Resized(size)) => Some(Message::Resized(size)),
        iced::Event::Window(iced::window::Event::Rescaled(scale)) => Some(Message::Rescaled(scale)),
        iced::Event::Window(iced::window::Event::FileDropped(path)) => {
            Some(Message::Dropped(vec![path]))
        }
        _ => None,
    })
}
