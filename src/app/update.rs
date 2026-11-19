//! The one place state changes: every message is routed from here.

use super::menus::{self, SubtitleAction};
use super::{App, Dialog, Message, Pick};
use enzui_widgets::form_dialog::FormOutcome;
use enzui_widgets::menu_button;
use enzui_widgets::toast::ToastTone;
use enzui_widgets::toast_host::ToastSpec;
use iced::Task;

impl App {
    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        enzui_core::crash_report::observe(&message);
        let task = self.route(message);
        if !self.queue_available() {
            self.show_playlist = false;
            self.queue_context.close();
        }
        self.queue_navigation.preference = if self.show_playlist {
            enzui_widgets::nav_split::SidebarPreference::Shown
        } else {
            enzui_widgets::nav_split::SidebarPreference::Hidden
        };
        let policy = if self.closing.is_some() || self.chooser_pending() {
            enzui_widgets::modal_host::alert_dismiss_policy()
        } else {
            enzui_widgets::modal_host::modal_dismiss_policy()
        };
        for key in [Dialog::OpenUrl, Dialog::Info, Dialog::Shortcuts] {
            let _ = self.dialogs.set_policy(&key, policy);
        }
        let _ = self
            .toasts
            .sync_expiry(&mut self.toast_schedule, std::time::Instant::now());
        task
    }

    pub(crate) fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(ToastSpec::new(text));
    }

    pub(crate) fn toast_error(&mut self, text: impl Into<String>) {
        self.toasts
            .push(ToastSpec::new(text).tone(ToastTone::Error));
    }

    fn route(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Plex(event) => self.plex_event(event),
            Message::Theme(event) => {
                let task = self
                    .themes
                    .update(*event)
                    .map(|event| Message::Theme(Box::new(event)));
                self.update_frame_limit();
                task
            }
            Message::VideoWake => self.video_wake(),
            Message::TimelineFrame => Task::none(),
            Message::RetryMedia(token) => self.retry_media(token),
            Message::FrameReady(ready) => {
                let had_picture = self.presenter.handle().is_some();
                let handover = self.presenter.ready(ready);
                if !had_picture && self.presenter.handle().is_some() {
                    self.refresh_menus();
                }
                handover
                    .next_upload
                    .map_or_else(Task::none, |task| task.map(Message::FrameReady))
            }

            Message::SurfacePress => self.surface_press(),
            Message::SurfaceClick(token) => {
                if token == self.surface_click {
                    self.surface_click = self.surface_click.wrapping_add(1);
                    self.toggle_play()
                } else {
                    Task::none()
                }
            }
            Message::TogglePlay => {
                self.surface_click = self.surface_click.wrapping_add(1);
                self.toggle_play()
            }
            Message::SeekTo(at) => self.seek_to(at),
            Message::Scrub(seconds) => self.scrub(seconds),
            Message::ScrubStarted => {
                self.scrub_dragging = true;
                Task::none()
            }
            Message::ScrubReleased => {
                self.scrub_dragging = false;
                self.scrub_settled(self.scrub_token)
            }
            Message::ScrubSettled(token) => self.scrub_settled(token),
            Message::SkipBy(delta) => self.skip_by(delta),
            Message::SeekFraction(fraction)
                if self.plex.visible
                    && self.plex.user.is_some()
                    && self.plex.kind == crate::plex::Kind::Plex =>
            {
                self.plex_event(crate::plex::Event::PinDigit((fraction * 10.0).round() as u8))
            }
            Message::SeekFraction(fraction) => self.seek_fraction(fraction),
            Message::Backspace => {
                if self.plex.visible
                    && self.plex.user.is_some()
                    && self.plex.kind == crate::plex::Kind::Plex
                {
                    return self.plex_event(crate::plex::Event::PinBack);
                }
                Task::none()
            }
            Message::StepFrame(forward) => self.step_frame(forward),
            Message::SetVolume(volume) => self.set_volume(volume),
            Message::VolumeBy(delta) => self.volume_by(delta),
            Message::ToggleMute => self.toggle_mute(),
            Message::RateStep(faster) => self.rate_step(faster),
            Message::Next => self.next(),
            Message::Previous => self.previous(),
            Message::CycleRepeat => self.cycle_repeat(),
            Message::PlayIndex(index) => self.play_index(index),
            Message::RemoveFromQueue(index) => self.remove_from_queue(index),
            Message::QueueDropped(target, drop) => self.reorder_dropped(target, drop),
            Message::CycleSubtitles => self.cycle_subtitles(),
            Message::CycleAudio => self.cycle_audio(),
            Message::SubtitleDelay(delta) => self.subtitle_delay(delta),

            Message::SubtitlesMenu(event) => {
                let (model, state) = &mut self.menus.subtitles;
                let chosen = menu_button::apply_event(model, state, &event)
                    .and_then(|id| menus::subtitle_action(id.as_str()));
                match chosen {
                    Some(action) => self.subtitle_action(action),
                    None => Task::none(),
                }
            }
            Message::AudioMenu(event) => {
                let (model, state) = &mut self.menus.audio;
                let Some(id) = menu_button::apply_event(model, state, &event) else {
                    return Task::none();
                };
                if let Some(track) = menus::server_track(id.as_str()) {
                    return self.choose_server_track(Some(track), None);
                }
                match (menus::track_index(id.as_str()), &self.video) {
                    (Some(index), Ok(_)) => {
                        self.select_audio(index);
                        self.note_player_tracks(Some(index), None);
                        self.plex_save_streams(Some(index), None)
                    }
                    _ => Task::none(),
                }
            }
            Message::QualityMenu(event) => {
                let (model, state) = &mut self.menus.quality;
                match menu_button::apply_event(model, state, &event)
                    .and_then(|id| crate::plex::quality::Quality::from_id(id.as_str()))
                {
                    Some(quality) => {
                        self.update(Message::Plex(crate::plex::Event::Quality(quality)))
                    }
                    None => Task::none(),
                }
            }
            Message::SpeedMenu(event) => {
                let (model, state) = &mut self.menus.speed;
                match menu_button::apply_event(model, state, &event)
                    .and_then(|id| menus::rate_for(id.as_str()))
                {
                    Some(rate) => self.set_rate(rate),
                    None => Task::none(),
                }
            }
            Message::SourceMenu(event) => {
                let (model, state) = &mut self.menus.sources;
                match menu_button::apply_event(model, state, &event)
                    .and_then(|id| menus::source_action(id.as_str()))
                {
                    Some(message) => self.update(message),
                    None => Task::none(),
                }
            }
            Message::SortMenu(event) => {
                let (model, state) = &mut self.menus.sort;
                match menu_button::apply_event(model, state, &event)
                    .and_then(|id| crate::plex::Sort::from_id(id.as_str()))
                {
                    Some(sort) => self.update(Message::Plex(crate::plex::Event::Sort(sort))),
                    None => Task::none(),
                }
            }
            Message::AccountMenu(event) => {
                let (model, state) = &mut self.menus.account;
                match menu_button::apply_event(model, state, &event)
                    .and_then(|id| menus::account_action(id.as_str()))
                {
                    Some(message) => self.update(message),
                    None => Task::none(),
                }
            }
            Message::QueueContext(entry, at) => {
                use enzui_core::context_menu::{InvocationKind, Point};
                use enzui_widgets::context_menu_host::{ContextMenuItem, FocusRestoreTarget};
                self.queue_context.open_restoring(
                    entry.clone(),
                    InvocationKind::Pointer(Point::new(at.x, at.y)),
                    vec![
                        ContextMenuItem::command(
                            super::QueueCommand::Play,
                            enzui_core::localization::text("video-play-436e61"),
                        )
                        .icon("play"),
                        ContextMenuItem::command(
                            super::QueueCommand::Remove,
                            enzui_core::localized!("video-remove-d89228", "value0" => entry.name()),
                        )
                        .icon("list-minus"),
                    ],
                    FocusRestoreTarget::Widget(iced::advanced::widget::Id::from(format!(
                        "video-queue-{}",
                        crate::model::history::thumbnail_key(&entry)
                    ))),
                );
                self.activity()
            }
            Message::QueueMenu(event) => {
                let (chosen, focus) = self.queue_context.update(&event);
                let command = chosen.and_then(|(entry, command)| {
                    self.playlist
                        .entries()
                        .iter()
                        .position(|candidate| *candidate == entry)
                        .map(|index| (index, command))
                });
                let task = match command {
                    Some((index, super::QueueCommand::Play)) => self.play_index(index),
                    Some((index, super::QueueCommand::Remove)) => self.remove_from_queue(index),
                    None => Task::none(),
                };
                Task::batch([task, focus])
            }
            Message::ToggleTitlebarMenu => {
                self.titlebar_menu = !self.titlebar_menu;
                self.activity()
            }
            Message::TitlebarMenu(open) => {
                self.titlebar_menu = open;
                self.activity()
            }

            Message::Choose(pick) => self.choose(pick),
            Message::Sleep(event) => match event {
                enzui_core::system_sleep::SleepEvent::Suspending(ack) => {
                    let paused = if self.status.is_playing() {
                        self.toggle_play()
                    } else {
                        Task::none()
                    };
                    self.record_progress();
                    self.sleep_ack = Some(ack);
                    let save = self.save_settings();
                    Task::batch([paused, save, self.finish_close_if_saved()])
                }
                enzui_core::system_sleep::SleepEvent::Resumed => Task::none(),
            },
            Message::Session(event) => {
                if event.ack.is_some() {
                    self.session_ack = event.ack;
                }
                let task =
                    if event.state != enzui_core::portal::session_monitor::SessionState::Running {
                        self.record_progress();
                        self.save_settings()
                    } else {
                        Task::none()
                    };
                Task::batch([task, self.finish_close_if_saved()])
            }
            Message::ReceiveFiles(offer) => self.receive_files(offer),
            Message::NativeFiles(delivery) => self.native_files(delivery),
            Message::MemoryPressure(level) => {
                self.thumbnails.clear();
                if level >= enzui_core::system_status::MemoryPressure::Medium {
                    let _ = self.plex.artwork.reconcile(Vec::new());
                }
                Task::none()
            }
            Message::ApplicationWake => self.application_wake(),
            Message::ChooserFallback(event) => self.chooser.handle(event, Message::Chosen),
            Message::ChooserWindow(generation, window) => self.chooser_window(generation, window),
            Message::ChooserParent(generation, reply) => self.chooser_parent(generation, reply),
            Message::ChooserWake => self.chooser.pump(Message::Chosen),
            Message::Chosen(pick, outcome) => self.chosen(pick, outcome),
            Message::OpenUrlDialog => self.open_url_dialog(),
            Message::UrlForm(event) => {
                if self.dialogs.topmost() != Some(&Dialog::OpenUrl) {
                    return Task::none();
                }
                let Some(presenter) = &mut self.url_presenter else {
                    return Task::none();
                };
                let scope = Some(enzui_widgets::modal_host::presentation_scope_id("OpenUrl"));
                let (outcome, task) = presenter.update::<Message>(event, scope);
                match outcome {
                    Some(FormOutcome::Submitted(submission)) => {
                        Task::batch([task, self.url_submitted(submission)])
                    }
                    Some(FormOutcome::Dismissed(cause)) => {
                        Task::batch([task, self.dismiss_dialog(Dialog::OpenUrl, cause)])
                    }
                    None => task,
                }
            }
            Message::OpenRecent(index) => self.open_recent(index),
            Message::ForgetRecent(index) => self.forget_recent(index),
            Message::ClearRecent => {
                self.settings.history.clear_local();
                let keep = self.settings.history.thumbnail_keys();
                self.thumbnails.retain(|key, _| keep.contains(key));
                self.save_settings()
            }
            Message::Dropped(paths) => self.dropped(paths),
            Message::Launched(opening) => self.open_arguments(opening),
            Message::DropResolved {
                token,
                order,
                videos,
                subtitle,
            } => {
                if token == self.open_token {
                    match order {
                        Some(order) => self.ordered_drop(order, videos, subtitle),
                        None => self.drop_resolved(videos, subtitle, false),
                    }
                } else {
                    Task::none()
                }
            }
            Message::SiblingSubtitle(token, subtitle) => self.load_with(token, subtitle),

            Message::Activity => self.activity(),
            Message::PointerOverControls(over) => {
                self.over_controls = over;
                Task::none()
            }
            Message::HideChrome => {
                self.hide_chrome();
                Task::none()
            }
            Message::ToggleFullscreen => {
                self.surface_click = self.surface_click.wrapping_add(1);
                self.set_fullscreen(!self.fullscreen)
            }
            Message::ExitFullscreenOrClose => {
                if self.plex.visible {
                    return self.plex_event(if self.plex.adding {
                        crate::plex::Event::CancelAdd
                    } else if self.plex.user.is_some() {
                        crate::plex::Event::PinCancel
                    } else if self.plex.manual {
                        crate::plex::Event::ShowProfiles
                    } else if self.plex.pin.is_some() {
                        crate::plex::Event::CancelSignIn
                    } else if self.plex.library.is_some()
                        || self.plex.selected.is_some()
                        || !self.plex.parents.is_empty()
                    {
                        crate::plex::Event::Back
                    } else {
                        crate::plex::Event::Close
                    });
                }
                if !self.dialogs.is_empty() {
                    self.close_dialog()
                } else if self.show_playlist && self.window_size.width < 800.0 {
                    self.show_playlist = false;
                    Task::none()
                } else if self.fullscreen {
                    self.set_fullscreen(false)
                } else {
                    Task::none()
                }
            }
            Message::Resized(size) => {
                self.window_size = size;
                self.update_frame_limit();
                Task::none()
            }
            Message::Rescaled(scale) => {
                self.scale = scale;
                self.update_frame_limit();
                Task::none()
            }
            Message::QueueNavigation(state) => {
                self.queue_navigation = state;
                self.show_playlist =
                    state.preference != enzui_widgets::nav_split::SidebarPreference::Hidden;
                self.refresh_menus();
                Task::none()
            }
            Message::TogglePlaylist => {
                self.show_playlist = !self.show_playlist && self.queue_available();
                self.refresh_menus();
                if self.show_playlist {
                    self.plex_event(crate::plex::Event::Close)
                } else {
                    Task::none()
                }
            }
            Message::ShowInfo => {
                if self.has_source() {
                    self.present(Dialog::Info)
                } else {
                    Task::none()
                }
            }
            Message::ShowShortcuts => self.present(Dialog::Shortcuts),
            Message::CloseDialog => self.close_dialog(),
            Message::DialogEvent(event) => {
                let (closed, focus) = self.dialogs.update(event);
                self.forget_dialogs(&closed);
                focus
            }
            Message::SaveFrame => self.save_frame(),
            Message::FrameSaved(result) => {
                match result {
                    Ok(path) => {
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.toast(
                            enzui_core::localized!("video-saved-name-to-pictures-4a83af", "name" => name),
                        );
                    }
                    Err(error) => self.toast_error(
                        enzui_core::localized!("video-couldn-t-save-the-frame-error-8b5a9b", "error" => error),
                    ),
                }
                Task::none()
            }
            Message::Saved(result) => self.saved(result),
            Message::ThumbnailsLoaded(loaded) => {
                for (key, thumb) in loaded {
                    self.thumbnails.entry(key).or_insert_with(|| {
                        iced::widget::image::Handle::from_rgba(
                            thumb.width,
                            thumb.height,
                            thumb.pixels,
                        )
                    });
                }
                Task::none()
            }
            Message::OsdExpired(token) => {
                self.osd_expired(token);
                Task::none()
            }
            Message::Toast(event) => {
                let _ = self.toasts.update(event);
                Task::none()
            }

            Message::Window(enzui_widgets::title_bar_gesture::WindowCommand::Close) => {
                self.update(Message::CloseWindow)
            }
            Message::Window(command) => iced::window::latest().then(move |window| match window {
                Some(window) => command.into_task(window),
                None => Task::none(),
            }),
            Message::CloseWindow => iced::window::latest().then(|window| match window {
                Some(window) => Task::done(Message::CloseRequested(window)),
                None => Task::none(),
            }),
            Message::CloseRequested(window) => {
                self.closing = Some(window);
                self.cancel_chooser(enzui_core::portal::CancelReason::ParentClosed);
                self.record_progress();
                let save = self.save_settings();
                Task::batch([save, self.finish_close_if_saved()])
            }
            Message::Flushed(window) => iced::window::close(window),
        }
    }

    fn subtitle_action(&mut self, action: SubtitleAction) -> Task<Message> {
        let Ok(video) = &self.video else {
            return Task::none();
        };
        match action {
            SubtitleAction::Off if self.plex.transcode.is_some() => {
                if self.status.subtitle_file.is_some() {
                    video.set_subtitle_file(None);
                }
                self.choose_server_track(None, Some(None))
            }
            SubtitleAction::Server(track) => {
                if self.status.subtitle_file.is_some() {
                    video.set_subtitle_file(None);
                }
                self.choose_server_track(None, Some(Some(track)))
            }
            SubtitleAction::Off => {
                if self.status.subtitle_file.is_some() {
                    video.set_subtitle_file(None);
                }
                video.select_subtitle(None);
                self.note_player_tracks(None, Some(None));
                self.plex_save_streams(None, Some(None))
            }
            SubtitleAction::Track(index) => {
                if self.status.subtitle_file.is_some() {
                    video.set_subtitle_file(None);
                }
                video.select_subtitle(Some(index));
                self.note_player_tracks(None, Some(Some(index)));
                self.plex_save_streams(None, Some(Some(index)))
            }
            SubtitleAction::Load => self.update(Message::Choose(Pick::Subtitles)),
            SubtitleAction::Size(size) => {
                video.set_subtitle_size(size);
                self.settings.subtitle_size = size.into();
                self.save_settings()
            }
            SubtitleAction::Delay(delta) => self.subtitle_delay(delta),
            SubtitleAction::ResetDelay => self.subtitle_delay(0),
        }
    }

    fn set_fullscreen(&mut self, fullscreen: bool) -> Task<Message> {
        self.fullscreen = fullscreen;
        let mode = if fullscreen {
            iced::window::Mode::Fullscreen
        } else {
            iced::window::Mode::Windowed
        };
        iced::window::latest().then(move |window| match window {
            Some(window) => iced::window::set_mode(window, mode),
            None => Task::none(),
        })
    }

    fn save_frame(&mut self) -> Task<Message> {
        let (Some((width, height)), Some(pixels)) =
            (self.presenter.size(), self.presenter.pixels())
        else {
            return Task::none();
        };
        let Some(dir) = crate::persistence::pictures_dir() else {
            self.toast_error(enzui_core::localization::text(
                "video-there-is-no-pictures-folder-to-save-to-ef62ed",
            ));
            return Task::none();
        };
        let pixels = pixels.to_vec();
        let at = crate::model::time::clock_ascii(self.status.position, false).replace(':', "-");
        let title = self.current_title().unwrap_or_else(|| "Video".to_owned());
        let stem = format!("{} {at}", crate::view::file_stem(&title));
        crate::l10n::perform(
            super::io::disk(move || {
                crate::persistence::save_frame(&dir, &stem, width, height, &pixels)
                    .map_err(|error| error.to_string())
            }),
            Message::FrameSaved,
        )
    }
}
