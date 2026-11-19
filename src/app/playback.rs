//! Driving the player: loading the current entry, turning transport
//! messages into commands, and taking in what the worker reports.

use super::{App, Message, Osd, io};
use crate::model::time::{clock, offset, rate_label, step_rate};
use crate::model::{Entry, now_ms};
use crate::persistence;
use enzui_core::url_policy::UrlPolicy;
use enzui_services::media::{LoadOptions, MediaSource, PlaybackState, SeekMode, VideoStatus};
use iced::Task;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SCRUB_SETTLE: Duration = Duration::from_millis(180);
const OSD_FOR: Duration = Duration::from_millis(1200);
const RECORD_EVERY: Duration = Duration::from_secs(15);
// "Previous" within this much of the start goes to the previous entry;
// later, it starts the current one over.
const RESTART_WITHIN: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct Recovery {
    attempts: u8,
    pending: bool,
    resume: Option<Duration>,
    start: Duration,
    rate: Option<f64>,
}

fn after(delay: Duration, message: Message) -> Task<Message> {
    crate::l10n::perform(async move { tokio::time::sleep(delay).await }, move |()| {
        message
    })
}

impl App {
    pub(crate) fn osd(&mut self, text: impl Into<String>) -> Task<Message> {
        self.osd_token = self.osd_token.wrapping_add(1);
        self.osd = Some(Osd {
            text: text.into(),
            token: self.osd_token,
        });
        after(OSD_FOR, Message::OsdExpired(self.osd_token))
    }

    pub(crate) fn osd_expired(&mut self, token: u64) {
        if self.osd.as_ref().is_some_and(|osd| osd.token == token) {
            self.osd = None;
        }
    }

    pub(crate) fn activity(&mut self) -> Task<Message> {
        self.chrome_hidden = false;
        Task::none()
    }

    pub(crate) fn hide_chrome(&mut self) {
        self.chrome_hidden = !self.over_controls;
    }

    pub(crate) fn load_current(&mut self) -> Task<Message> {
        self.surface_click = self.surface_click.wrapping_add(1);
        self.record_progress();
        self.recovery = Recovery::default();
        self.load_token = self.load_token.wrapping_add(1);
        let token = self.load_token;
        self.presenter.clear();
        self.scrub = None;
        self.scrub_dragging = false;
        self.pending_seek = None;
        self.playing = None;
        if let Ok(video) = &mut self.video {
            // Stop old audio before a potentially slow subtitle lookup.
            video.unload();
            self.status = video.status();
        }
        self.awake.set(false);
        self.refresh_menus();
        match self.playlist.current().cloned() {
            Some(Entry::File(path)) => crate::l10n::perform(
                io::disk(move || persistence::sibling_subtitle(&path)),
                move |subtitle| Message::SiblingSubtitle(token, subtitle),
            ),
            Some(Entry::Url(_)) => self.load_with(token, None),
            None => Task::none(),
        }
    }

    // Load the current entry with `subtitle`, if it is still the one
    // `token` was issued for.
    pub(crate) fn load_with(&mut self, token: u64, subtitle: Option<PathBuf>) -> Task<Message> {
        if token != self.load_token {
            return Task::none();
        }
        let Some(entry) = self.playlist.current().cloned() else {
            return Task::none();
        };
        let is_plex = self
            .plex
            .playing_url
            .as_ref()
            .is_some_and(|url| entry == Entry::Url(url.clone()));
        let mut stop = Task::none();
        if !is_plex && self.plex.playing_url.take().is_some() {
            // A local file replaces the server's stream; its transcode ends.
            if let (Some(session), Some(server)) =
                (self.plex.transcode.take(), self.plex.playing_server.clone())
            {
                self.plex.stops.push((server, session));
                stop = self.drain_transcode_stops();
            }
            self.plex.server_tracks = crate::plex::quality::ServerTracks::default();
            if let Some(handle) = self.plex.next_request.take() {
                handle.abort();
            }
            self.plex.next_episode = None;
            self.plex.auto_next = false;
            self.plex.playing = None;
            self.plex.playing_server = None;
            self.video = super::create_video_service();
        }
        if !is_plex {
            self.service_selection = None;
            self.release_media_browser();
        }
        let policy = if is_plex {
            UrlPolicy {
                allow_http: true,
                allow_private_network: true,
                max_redirects: 0,
            }
        } else {
            UrlPolicy::DEFAULT
        };
        let source = match &entry {
            Entry::File(path) => MediaSource::local(path),
            Entry::Url(url) => match MediaSource::remote(url, &policy) {
                Ok(source) => source,
                Err(verdict) => {
                    self.toast_error(super::open::address_refusal(verdict));
                    return stop;
                }
            },
        };
        let resume = self.recovery.resume.take().or_else(|| {
            if is_plex {
                self.plex
                    .playing
                    .as_ref()
                    .filter(|_| self.plex_resume)
                    .map(|item| Duration::from_millis(item.view_offset))
            } else {
                self.settings.history.resume_point(&entry)
            }
        });
        let resume = resume.filter(|at| *at >= Duration::from_secs(1));
        if let Some(selection) = &mut self.service_selection {
            selection.applied = false;
        }
        self.recovery.start = resume.unwrap_or_default();
        let options = LoadOptions {
            start_at: resume.unwrap_or_default(),
            autoplay: true,
            subtitle_file: subtitle,
            volume: self.settings.volume,
            muted: self.settings.muted,
            subtitle_size: self.settings.subtitle_size.into(),
        };
        let Ok(video) = &mut self.video else {
            return stop;
        };
        video.load(source, options);
        self.status = video.status();
        self.playing = Some(entry);
        self.refresh_menus();
        self.last_record = Instant::now();
        let resumed = match resume {
            Some(at) => self.osd(
                enzui_core::localized!("video-resuming-from-58c78a", "value0" => clock(at, false)),
            ),
            None => Task::none(),
        };
        Task::batch([resumed, self.activity(), stop])
    }

    pub(crate) fn video_wake(&mut self) -> Task<Message> {
        let Ok(video) = &self.video else {
            return Task::none();
        };
        let outcomes = video.take_seek_outcomes();
        let update = video.poll();
        // A wake that only hands over a decoded frame changes nothing on
        // screen until the frame is uploaded (`FrameReady`), so it rebuilds
        // and redraws no window.
        let unseen = update.status.is_none() && outcomes.is_empty();
        let shown = update
            .frame
            .and_then(|frame| self.presenter.offer(frame))
            .map_or_else(Task::none, |task| task.map(Message::FrameReady));
        let changed = match update.status {
            Some(status) => self.status_changed(status),
            None => Task::none(),
        };
        for outcome in outcomes {
            if self.pending_seek == Some(outcome.request) {
                self.pending_seek = None;
                use enzui_services::media::video_service::SeekRejection;
                if outcome.request.generation != self.status.generation {
                    continue;
                }
                if outcome.result == Err(SeekRejection::PositionMismatch)
                    && outcome.request.mode == SeekMode::Accurate
                {
                    let retry =
                        self.video.as_ref().ok().and_then(|video| {
                            video.request_seek(outcome.target, SeekMode::Fast).ok()
                        });
                    if let Some(request) = retry {
                        self.pending_seek = Some(request);
                    } else {
                        self.toast_error(enzui_core::localization::text("video-seek-failed"));
                    }
                } else if matches!(outcome.result, Err(error) if !matches!(error, SeekRejection::Superseded | SeekRejection::SourceChanged))
                {
                    self.toast_error(enzui_core::localization::text("video-seek-failed"));
                }
                // Only the final receipt returns display ownership from the thumb to the backend.
                if self.pending_seek.is_none() && !self.scrub_dragging {
                    if let Some(actual) = outcome.actual {
                        self.status.position = actual;
                    }
                    self.scrub = None;
                    self.position_anchor = self.status.position;
                    self.position_received = Instant::now();
                }
            }
        }
        let invalidation = if unseen {
            iced::window::invalidate(iced::window::Invalidation::none())
        } else {
            Task::none()
        };
        Task::batch([shown, changed, invalidation])
    }

    pub(super) fn status_changed(&mut self, status: VideoStatus) -> Task<Message> {
        if self
            .video
            .as_ref()
            .is_ok_and(|video| video.generation() != status.generation)
        {
            return Task::none();
        }
        if !status.seekable || status.error.is_some() {
            self.scrub = None;
            self.scrub_dragging = false;
        }
        let was_playing = self.status.is_playing();
        let was_ended = self.status.playback == PlaybackState::EndOfStream;
        let menus_changed = self.status.audio_tracks != status.audio_tracks
            || self.status.subtitle_tracks != status.subtitle_tracks
            || self.status.current_audio != status.current_audio
            || self.status.current_subtitle != status.current_subtitle
            || self.status.subtitle_file != status.subtitle_file
            || self.status.subtitle_size != status.subtitle_size
            || self.status.subtitle_delay_ms != status.subtitle_delay_ms
            || self.status.rate != status.rate
            || self.status.has_source() != status.has_source();
        if let (Some(selection), Ok(video)) = (&mut self.service_selection, &self.video) {
            selection.apply(video, &status);
        }
        if !status.loading
            && status.has_source()
            && status.error.is_none()
            && let Some(rate) = self.recovery.rate.take()
            && let Ok(video) = &self.video
        {
            video.set_rate(rate);
        }
        let recoverable = status.remote
            && (was_playing || self.status.loading)
            && self.closing.is_none()
            && self.recovery.attempts < 2
            && !self.recovery.pending
            && status.error.as_ref().is_some_and(|error| {
                matches!(
                    error.kind,
                    enzui_services::media::MediaErrorKind::Backend
                        | enzui_services::media::MediaErrorKind::SourceNotFound
                )
            });
        if recoverable {
            self.recovery.rate.get_or_insert(self.status.rate);
        }
        let resume = if self.status.loading {
            self.recovery.start
        } else {
            self.status.position.max(status.position)
        };
        if recoverable
            && !self.status.loading
            && let Some(selection) = &mut self.service_selection
        {
            selection.remember(&self.status);
        }
        let now = Instant::now();
        self.position_anchor = status.position;
        self.position_received = now;
        self.status = status;
        if menus_changed {
            self.refresh_menus();
        }
        self.awake
            .set(self.status.is_playing() && self.status.has_video);
        let mut tasks = Vec::new();
        if recoverable {
            self.recovery.attempts += 1;
            self.recovery.pending = true;
            self.recovery.resume = Some(resume);
            tasks.push(after(
                Duration::from_secs(u64::from(self.recovery.attempts)),
                Message::RetryMedia(self.load_token),
            ));
        }
        if self.plex.playing.is_some() && was_playing != self.status.is_playing() {
            tasks.push(self.plex_timeline());
        }
        if self.status.has_video
            && let Some(video) = self.status.natural_size
            && !self.fullscreen
            && self.fitted != Some(self.status.generation)
        {
            self.fitted = Some(self.status.generation);
            tasks.push(self.fit_window(video));
        }
        if self.status.is_playing() && !was_playing {
            tasks.push(self.activity());
        }
        if self.status.playback == PlaybackState::EndOfStream && !was_ended {
            self.record_progress();
            tasks.push(self.save_settings());
            if self.plex.playing.is_some() {
                self.plex.auto_next = true;
                tasks.push(self.next_episode());
            } else if self.playlist.after_end().is_some() {
                tasks.push(self.load_current());
            }
        } else if self.status.is_playing() && self.last_record.elapsed() >= RECORD_EVERY {
            self.record_progress_at(false);
            tasks.push(self.save_settings());
        }
        Task::batch(tasks)
    }

    fn fit_window(&self, video: (u32, u32)) -> Task<Message> {
        let window = (self.window_size.width, self.window_size.height);
        iced::window::latest().then(move |id| {
            let Some(id) = id else {
                return Task::none();
            };
            iced::window::is_maximized(id).then(move |maximized| {
                if maximized {
                    return Task::none();
                }
                iced::window::monitor_size(id).then(move |monitor| {
                    let monitor = monitor.map(|size| (size.width, size.height));
                    match crate::model::window_fit::fitted_window(window, video, monitor) {
                        Some((width, height)) => {
                            iced::window::resize(id, iced::Size::new(width, height))
                        }
                        None => Task::none(),
                    }
                })
            })
        })
    }

    pub(crate) fn record_progress(&mut self) {
        self.record_progress_at(true);
    }

    fn record_progress_at(&mut self, leaving: bool) {
        self.last_record = Instant::now();
        let Some(entry) = &self.playing else {
            return;
        };
        // Session exports disappear on close and cannot be reopened from history.
        if entry
            .path()
            .is_some_and(|path| self.is_temporary_media(path))
        {
            return;
        }
        if self
            .plex
            .playing_url
            .as_ref()
            .is_some_and(|url| entry == &Entry::Url(url.clone()))
        {
            return;
        }
        let status = &self.status;
        if status.loading || status.error.is_some() || !status.has_source() || !status.seekable {
            return;
        }
        let position = if status.playback == PlaybackState::EndOfStream {
            status.duration.unwrap_or(status.position)
        } else {
            status.position
        };
        let title = status
            .display_title()
            .map_or_else(|| entry.name(), str::to_owned);
        let entry = entry.clone();
        let ended = status.playback == PlaybackState::EndOfStream;
        self.settings
            .history
            .record(&entry, &title, position, status.duration, now_ms());
        // The credits make a poor picture: an ended video keeps the one
        // taken while it played, if any.
        let key = crate::model::history::thumbnail_key(&entry);
        let taken = if ended {
            self.thumbnails.contains_key(&key)
        } else {
            !leaving && self.thumbed.as_ref() == Some(&key)
        };
        if taken || !self.status.has_video {
            return;
        }
        let thumb = match (self.presenter.size(), self.presenter.pixels()) {
            (Some((width, height)), Some(pixels)) => {
                crate::model::thumbnail::shrink(width, height, pixels)
            }
            _ => None,
        };
        if let Some(thumb) = thumb {
            self.thumbnails.insert(
                key.clone(),
                iced::widget::image::Handle::from_rgba(
                    thumb.width,
                    thumb.height,
                    thumb.pixels.clone(),
                ),
            );
            self.fresh_thumbnails.retain(|(fresh, _)| fresh != &key);
            self.fresh_thumbnails.push((key.clone(), thumb));
            self.thumbed = Some(key);
        }
    }

    // Write the settings, one write at a time; a request during a write
    // runs after it with the newest settings.
    pub(crate) fn save_settings(&mut self) -> Task<Message> {
        let Some(dir) = self.state_dir.clone() else {
            return Task::none();
        };
        if self.saving {
            self.save_again = true;
            return Task::none();
        }
        self.saving = true;
        let settings = self.settings.clone();
        let fresh = std::mem::take(&mut self.fresh_thumbnails);
        let keep = self.settings.history.thumbnail_keys();
        crate::l10n::perform(
            io::disk(move || {
                persistence::save(&dir, &settings).map_err(|error| error.to_string())?;
                // A thumbnail is a nicety: failing to write one is not worth
                // a notice.
                let _ = persistence::save_thumbnails(&dir, &fresh, &keep);
                Ok(())
            }),
            Message::Saved,
        )
    }

    pub(crate) fn saved(&mut self, result: Result<(), String>) -> Task<Message> {
        self.saving = false;
        if let Err(error) = result {
            self.toast_error(
                enzui_core::localized!("video-couldn-t-save-your-settings-error-28cbc7", "error" => error),
            );
        }
        if std::mem::take(&mut self.save_again) {
            return self.save_settings();
        }
        self.finish_close_if_saved()
    }

    pub(crate) fn finish_close_if_saved(&mut self) -> Task<Message> {
        if !self.saving {
            if let Some(ack) = self.sleep_ack.take() {
                ack.done();
            }
            if let Some(ack) = self.session_ack.take() {
                ack.done();
            }
        }
        match self.closing {
            Some(window) if !self.saving => Task::done(Message::Flushed(window)),
            _ => Task::none(),
        }
    }

    pub(super) fn cancel_recovery(&mut self) {
        self.recovery.pending = false;
    }

    pub(crate) fn retry_media(&mut self, token: u64) -> Task<Message> {
        if token != self.load_token || !self.recovery.pending || self.closing.is_some() {
            return Task::none();
        }
        self.recovery.pending = false;
        self.pending_seek = None;
        self.presenter.clear();
        self.load_with(token, None)
    }

    pub(crate) fn surface_press(&mut self) -> Task<Message> {
        self.surface_click = self.surface_click.wrapping_add(1);
        after(
            iced::advanced::graphics::core::preferences::get().double_click_time
                + Duration::from_millis(1),
            Message::SurfaceClick(self.surface_click),
        )
    }

    pub(crate) fn toggle_play(&mut self) -> Task<Message> {
        self.plex.auto_next = false;
        if self.recovery.pending {
            self.recovery.pending = false;
            return Task::none();
        }
        if self.status.remote && self.status.error.is_some() {
            self.recovery.pending = true;
            self.recovery.resume = Some(self.status.position.max(self.recovery.start));
            return self.retry_media(self.load_token);
        }
        let Ok(video) = &self.video else {
            return Task::none();
        };
        if !self.status.has_source() {
            return if self.playlist.is_empty() {
                self.update(Message::Choose(super::Pick::Videos))
            } else {
                Task::none()
            };
        }
        if self.status.is_playing() {
            video.pause();
        } else {
            video.play();
        }
        Task::none()
    }

    pub(crate) fn seek_to(&mut self, at: Duration) -> Task<Message> {
        self.scrub_token = self.scrub_token.wrapping_add(1);
        if !self.status.seekable || self.video.is_err() {
            self.scrub = None;
            self.scrub_dragging = false;
            return Task::none();
        }
        if let Ok(video) = &self.video {
            match video.request_seek(at, SeekMode::Accurate) {
                Ok(receipt) => {
                    self.pending_seek = Some(receipt);
                    self.scrub = Some(at);
                }
                Err(_) => {
                    if !self.scrub_dragging {
                        self.scrub = None;
                    }
                    self.toast_error(enzui_core::localization::text("video-seek-busy"));
                }
            }
        }
        Task::none()
    }

    // Dragging uses keyframes; settling requests an accurate seek with bounded receipts.
    pub(crate) fn scrub(&mut self, seconds: f32) -> Task<Message> {
        let Ok(video) = &self.video else {
            return Task::none();
        };
        if !self.status.seekable || !seconds.is_finite() {
            return Task::none();
        }
        let at = Duration::from_secs_f32(seconds.max(0.0));
        let at = self.status.duration.map_or(at, |end| at.min(end));
        self.scrub = Some(at);
        self.pending_seek = None;
        video.seek(at, SeekMode::Fast);
        self.scrub_token = self.scrub_token.wrapping_add(1);
        if self.scrub_dragging {
            Task::none()
        } else {
            after(SCRUB_SETTLE, Message::ScrubSettled(self.scrub_token))
        }
    }

    pub(crate) fn scrub_settled(&mut self, token: u64) -> Task<Message> {
        if token != self.scrub_token || self.scrub_dragging {
            return Task::none();
        }
        match self.scrub {
            Some(at) => self.seek_to(at),
            None => Task::none(),
        }
    }

    pub(crate) fn skip_by(&mut self, delta_ms: i64) -> Task<Message> {
        if !self.status.seekable {
            return Task::none();
        }
        let target = offset(self.shown_position(), delta_ms, self.status.duration);
        self.scrub = None;
        let seek = self.seek_to(target);
        let long = self
            .status
            .duration
            .is_some_and(|d| d >= Duration::from_secs(3600));
        let key = if delta_ms < 0 {
            "video-skip-back"
        } else {
            "video-skip-forward"
        };
        let osd = self.osd(enzui_core::localized!(key, "time" => clock(target, long)));
        Task::batch([seek, osd])
    }

    pub(crate) fn seek_fraction(&mut self, fraction: f32) -> Task<Message> {
        match self.status.duration {
            Some(duration) if self.status.seekable => {
                self.seek_to(duration.mul_f32(fraction.clamp(0.0, 1.0)))
            }
            _ => Task::none(),
        }
    }

    pub(crate) fn step_frame(&mut self, forward: bool) -> Task<Message> {
        if let Ok(video) = &self.video {
            video.step_frame(forward);
        }
        Task::none()
    }

    pub(crate) fn set_volume(&mut self, volume: f64) -> Task<Message> {
        let volume = volume.clamp(0.0, 1.0);
        if let Ok(video) = &self.video {
            video.set_volume(volume);
            if self.status.muted && volume > 0.0 {
                video.set_mute(false);
            }
        }
        self.settings.volume = volume;
        if volume > 0.0 {
            self.settings.muted = false;
        }
        self.save_settings()
    }

    pub(crate) fn volume_by(&mut self, delta: f64) -> Task<Message> {
        let volume = (self.settings.volume + delta).clamp(0.0, 1.0);
        let save = self.set_volume(volume);
        let osd = self.osd(
            enzui_core::localized!("video-volume-d1ab0a", "value0" => (volume * 100.0).round()),
        );
        Task::batch([save, osd])
    }

    pub(crate) fn toggle_mute(&mut self) -> Task<Message> {
        let muted = !self.status.muted;
        if let Ok(video) = &self.video {
            video.set_mute(muted);
        }
        self.settings.muted = muted;
        let save = self.save_settings();
        let osd = self.osd(if muted {
            enzui_core::localization::text("video-muted-2346f2")
        } else {
            enzui_core::localized!("video-volume-d1ab0a", "value0" => (self.settings.volume * 100.0).round())
        });
        Task::batch([save, osd])
    }

    pub(crate) fn set_rate(&mut self, rate: f64) -> Task<Message> {
        if let Ok(video) = &self.video {
            video.set_rate(rate);
        }
        self.osd(if (rate - 1.0).abs() < 1e-6 {
            enzui_core::localization::text("video-normal-speed-17744a")
        } else {
            enzui_core::localized!("video-speed-db74c0", "value0" => rate_label(rate))
        })
    }

    pub(crate) fn rate_step(&mut self, faster: bool) -> Task<Message> {
        match step_rate(self.status.rate, faster) {
            Some(rate) => self.set_rate(rate),
            None => Task::none(),
        }
    }

    pub(crate) fn next(&mut self) -> Task<Message> {
        if self.plex.playing.is_some() {
            return self.next_episode();
        }
        if self.playlist.next().is_some() {
            self.load_current()
        } else {
            Task::none()
        }
    }

    pub(crate) fn previous(&mut self) -> Task<Message> {
        if self.status.position > RESTART_WITHIN || !self.playlist.has_previous() {
            return self.seek_to(Duration::ZERO);
        }
        self.playlist.previous();
        self.load_current()
    }

    pub(crate) fn play_index(&mut self, index: usize) -> Task<Message> {
        if self.playlist.current_index() == Some(index) {
            return Task::none();
        }
        if self.playlist.select(index).is_some() {
            self.load_current()
        } else {
            Task::none()
        }
    }

    pub(crate) fn remove_from_queue(&mut self, index: usize) -> Task<Message> {
        let was_current = self.playlist.current_index() == Some(index);
        if !self.playlist.remove(index) {
            return Task::none();
        }
        if self.playlist.is_empty() {
            self.show_playlist = false;
        }
        self.refresh_menus();
        if was_current {
            self.load_current()
        } else {
            Task::none()
        }
    }

    pub(crate) fn cycle_repeat(&mut self) -> Task<Message> {
        self.playlist.repeat = self.playlist.repeat.cycle();
        self.settings.repeat = self.playlist.repeat;
        self.refresh_menus();
        let save = self.save_settings();
        let osd = self.osd(self.playlist.repeat.label());
        Task::batch([save, osd])
    }

    // Off → each embedded track → off; a subtitle file counts as on.
    pub(crate) fn cycle_subtitles(&mut self) -> Task<Message> {
        let Ok(video) = &self.video else {
            return Task::none();
        };
        if self.status.subtitle_file.is_some() {
            video.set_subtitle_file(None);
            return self.osd(enzui_core::localization::text("video-subtitles-off-6a5216"));
        }
        let tracks = &self.status.subtitle_tracks;
        let next = match self.status.current_subtitle {
            None => tracks.first(),
            Some(current) => tracks
                .iter()
                .skip_while(|track| track.index != current)
                .nth(1),
        };
        match next {
            Some(track) => {
                video.select_subtitle(Some(track.index));
                let save = self.plex_save_streams(None, Some(Some(track.index)));
                let label = crate::l10n::track(track);
                let osd = self
                    .osd(enzui_core::localized!("video-subtitles-label-7dd65c", "label" => label));
                Task::batch([save, osd])
            }
            None if tracks.is_empty() => {
                self.osd(enzui_core::localization::text("video-no-subtitles-68ded0"))
            }
            None => {
                video.select_subtitle(None);
                let save = self.plex_save_streams(None, Some(None));
                let osd = self.osd(enzui_core::localization::text("video-subtitles-off-6a5216"));
                Task::batch([save, osd])
            }
        }
    }

    pub(crate) fn cycle_audio(&mut self) -> Task<Message> {
        if self.video.is_err() {
            return Task::none();
        }
        if let Some(server) = self.transcoded_tracks() {
            let tracks = &server.audio;
            if tracks.len() < 2 {
                return self.osd(enzui_core::localization::text(
                    "video-only-one-audio-track-35bbb4",
                ));
            }
            let next = server
                .audio_choice
                .and_then(|current| tracks.iter().position(|track| track.id == current))
                .map_or(0, |position| (position + 1) % tracks.len());
            let (id, label) = (tracks[next].id, tracks[next].label.clone());
            let osd =
                self.osd(enzui_core::localized!("video-audio-label-4b8be0", "label" => label));
            return Task::batch([self.choose_server_track(Some(id), None), osd]);
        }
        let tracks = &self.status.audio_tracks;
        if tracks.len() < 2 {
            return self.osd(if tracks.is_empty() {
                enzui_core::localization::text("video-no-sound-0c08de")
            } else {
                enzui_core::localization::text("video-only-one-audio-track-35bbb4")
            });
        }
        let next = self
            .status
            .current_audio
            .and_then(|current| tracks.iter().position(|track| track.index == current))
            .map_or(0, |position| (position + 1) % tracks.len());
        let track = &tracks[next];
        let index = track.index;
        let label = crate::l10n::track(track);
        self.select_audio(index);
        self.note_player_tracks(Some(index), None);
        let save = self.plex_save_streams(Some(index), None);
        let osd = self.osd(enzui_core::localized!("video-audio-label-4b8be0", "label" => label));
        Task::batch([save, osd])
    }

    /// Plays the audio track at `index`. enzUI Alpha 0.2 applies a switch
    /// once data flows again, so a paused video prerolls at the same frame
    /// to show the new track's choice at once.
    pub(crate) fn select_audio(&self, index: i32) {
        let Ok(video) = &self.video else {
            return;
        };
        video.select_audio(index);
        if self.status.playback == PlaybackState::Paused && !self.status.loading {
            video.seek(self.status.position, SeekMode::Accurate);
        }
    }

    // Shift subtitles by `delta_ms`; zero puts them back in sync.
    pub(crate) fn subtitle_delay(&mut self, delta_ms: i64) -> Task<Message> {
        let Ok(video) = &self.video else {
            return Task::none();
        };
        let delay = if delta_ms == 0 {
            0
        } else {
            self.status.subtitle_delay_ms.saturating_add(delta_ms)
        };
        video.set_subtitle_delay(Duration::from_millis(delay.unsigned_abs()), delay > 0);
        self.osd(if delay == 0 {
            enzui_core::localization::text("video-subtitle-synced")
        } else {
            enzui_core::localized!("video-subtitles-b7e8c8", "value0" => super::menus::delay_label(delay))
        })
    }

    pub(crate) fn update_frame_limit(&self) {
        if let Ok(video) = &self.video {
            let scale = if self.scale.is_finite() && self.scale > 0.0 {
                self.scale
            } else {
                1.0
            };
            let width = (self.window_size.width * scale).ceil();
            let height = (self.window_size.height * scale).ceil();
            if width >= 1.0 && height >= 1.0 {
                video.set_frame_limit(Some((width as u32, height as u32)));
                let spacing = self.themes.theme().spacing;
                let inset =
                    (crate::view::PLAYBACK_CONTROLS_HEIGHT + spacing.lg() + spacing.md()) * scale;
                video.set_subtitle_bottom_inset(inset.ceil() as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_recovery_is_bounded_and_new_sources_refuse_late_retries() {
        let themes =
            enzui_core::theme_file::integration::ThemeIntegration::unmanaged_compiled_theme(
                enzui_theme::EnzTheme::dark(),
            )
            .0;
        let mut app = App::with(
            themes,
            Err("no decoder required".into()),
            Default::default(),
            None,
        );
        app.playlist
            .replace(vec![Entry::Url("https://example.test/movie.mkv".into())]);
        for attempt in 1..=3 {
            app.status = VideoStatus {
                remote: true,
                playback: PlaybackState::Playing,
                position: Duration::from_secs(42),
                ..Default::default()
            };
            let failed = VideoStatus {
                remote: true,
                error: Some(enzui_services::media::MediaError {
                    kind: enzui_services::media::MediaErrorKind::Backend,
                    message: "interrupted".into(),
                }),
                ..Default::default()
            };
            let _ = app.status_changed(failed);
            assert_eq!(app.recovery.pending, attempt <= 2);
            if attempt <= 2 {
                assert_eq!(app.recovery.resume, Some(Duration::from_secs(42)));
                let _ = app.retry_media(app.load_token);
            }
        }
        let _ = app.load_current();
        app.status = VideoStatus {
            remote: true,
            loading: true,
            ..Default::default()
        };
        app.recovery.start = Duration::from_secs(18);
        let _ = app.status_changed(VideoStatus {
            remote: true,
            error: Some(enzui_services::media::MediaError {
                kind: enzui_services::media::MediaErrorKind::SourceNotFound,
                message: "disconnected".into(),
            }),
            ..Default::default()
        });
        assert_eq!(app.recovery.resume, Some(Duration::from_secs(18)));
        let old = app.load_token;
        let _ = app.load_current();
        let _ = app.retry_media(old);
        assert!(!app.recovery.pending);
        assert_eq!(app.recovery.attempts, 0);
    }
}
