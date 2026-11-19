//! Getting something to play: the file chooser, a dropped file or folder,
//! a typed address, the command line, the recent list.

use super::dialogs::{self, Dialog};
use super::{App, Message, Pick, io};
use crate::model::{Entry, files};
use crate::persistence;
use enzui_core::form::FormSubmission;
use enzui_core::portal::PortalOutcome;
use enzui_core::url_policy::UrlPolicy;
use enzui_services::media::MediaSource;
use enzui_widgets::file_chooser::{ChooserOptions, FileFilter, FileResult, PortalOperation};
use iced::Task;
use std::path::PathBuf;

// Drops this close together are one drop of several files: the window
// reports each file as its own event.
const DROP_BATCH: std::time::Duration = std::time::Duration::from_millis(500);

impl App {
    pub(crate) fn open_arguments(&mut self, opening: Vec<std::ffi::OsString>) -> Task<Message> {
        if opening.is_empty() {
            return Task::none();
        }
        self.last_drop = None;
        self.drop_buffer.clear();
        self.open_token = self.open_token.wrapping_add(1);
        let token = self.open_token;
        crate::l10n::perform(
            io::disk(move || resolve_arguments(opening)),
            move |(videos, subtitle)| Message::DropResolved {
                token,
                order: None,
                videos,
                subtitle,
            },
        )
    }

    pub(crate) fn choose(&mut self, pick: Pick) -> Task<Message> {
        let videos = files::VIDEO_EXTENSIONS.iter().fold(
            FileFilter::new(enzui_core::localization::text("video-videos-c9a963")).mime("video/*"),
            |filter, extension| filter.glob(format!("*.{extension}")),
        );
        let subtitles = files::SUBTITLE_EXTENSIONS.iter().fold(
            FileFilter::new(enzui_core::localization::text("video-subtitles-0ee695")),
            |filter, extension| filter.glob(format!("*.{extension}")),
        );
        let all =
            FileFilter::new(enzui_core::localization::text("video-all-files-866956")).glob("*");
        let (operation, options) = match pick {
            Pick::Videos => (
                PortalOperation::OpenMultiple,
                ChooserOptions::new()
                    .title(enzui_core::localization::text("video-open-video-6ac7b3"))
                    .filter(videos)
                    .filter(all),
            ),
            Pick::Queue => (
                PortalOperation::OpenMultiple,
                ChooserOptions::new()
                    .title(enzui_core::localization::text("video-add-to-queue-34b25c"))
                    .filter(videos)
                    .filter(all),
            ),
            Pick::Subtitles => {
                let mut options = ChooserOptions::new()
                    .title(enzui_core::localization::text(
                        "video-load-subtitle-file-07424d",
                    ))
                    .filter(subtitles)
                    .filter(all);
                if let Some(dir) = self
                    .playlist
                    .current()
                    .and_then(Entry::path)
                    .and_then(|p| p.parent())
                {
                    options = options.current_folder(dir.to_path_buf());
                }
                (PortalOperation::OpenFile, options)
            }
        };
        self.start_chooser(pick, operation, options)
    }

    pub(crate) fn chosen(
        &mut self,
        pick: Pick,
        outcome: PortalOutcome<FileResult>,
    ) -> Task<Message> {
        if self.closing.is_some() {
            return Task::none();
        }
        let paths = match outcome {
            PortalOutcome::Completed(result) => {
                let paths = if pick == Pick::Subtitles {
                    result.first_local_path().into_iter().collect()
                } else {
                    result.local_paths()
                };
                if paths.is_empty() && !result.uris.is_empty() {
                    self.toast_error(enzui_core::localization::text(
                        "video-the-selected-item-is-not-a-local-file-use-open-network--9c2132",
                    ));
                }
                paths
            }
            outcome => {
                if let Some(message) = enzui_widgets::file_chooser::failure_text(&outcome) {
                    self.toast_error(message);
                }
                return Task::none();
            }
        };
        if paths.is_empty() {
            return Task::none();
        }
        match pick {
            Pick::Videos => {
                self.last_drop = None;
                self.drop_buffer.clear();
                self.open_token = self.open_token.wrapping_add(1);
                self.playlist
                    .replace(paths.into_iter().map(Entry::File).collect());
                self.refresh_menus();
                Task::batch([
                    self.plex_event(crate::plex::Event::Close),
                    self.load_current(),
                ])
            }
            Pick::Queue => {
                let was_empty = self.playlist.is_empty();
                if was_empty {
                    self.last_drop = None;
                    self.drop_buffer.clear();
                    self.open_token = self.open_token.wrapping_add(1);
                }
                let added = self
                    .playlist
                    .append(paths.into_iter().map(Entry::File).collect());
                self.refresh_menus();
                if was_empty {
                    return Task::batch([
                        self.plex_event(crate::plex::Event::Close),
                        self.load_current(),
                    ]);
                }
                self.toast(match added {
                    0 => enzui_core::localization::text("video-already-in-the-queue-ef3919"),
                    n => enzui_core::localized!("video-queue-added", "count" => n),
                });
                Task::none()
            }
            Pick::Subtitles => {
                if self.status.remote {
                    self.toast_error(enzui_core::localization::text(
                        "video-remote-external-subtitle",
                    ));
                    return Task::none();
                }
                if let (Ok(video), Some(path)) = (&self.video, paths.into_iter().next()) {
                    video.set_subtitle_file(Some(path));
                    return self.plex_event(crate::plex::Event::Close);
                }
                Task::none()
            }
        }
    }

    pub(crate) fn dropped(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        if !self.dialogs.is_empty() || self.closing.is_some() || self.chooser_pending() {
            return Task::none();
        }
        let now = std::time::Instant::now();
        let append = self
            .last_drop
            .is_some_and(|at| now.saturating_duration_since(at) < DROP_BATCH);
        self.last_drop = Some(now);
        if !append {
            self.open_token = self.open_token.wrapping_add(1);
            self.drop_issued = 0;
            self.drop_next = 0;
            self.drop_started = false;
            self.drop_buffer.clear();
        }
        let order = self.drop_issued;
        self.drop_issued += 1;
        let token = self.open_token;
        crate::l10n::perform(
            io::disk(move || resolve(paths)),
            move |(videos, subtitle)| Message::DropResolved {
                token,
                order: Some(order),
                videos,
                subtitle,
            },
        )
    }

    pub(crate) fn ordered_drop(
        &mut self,
        order: usize,
        videos: Vec<Entry>,
        subtitle: Option<PathBuf>,
    ) -> Task<Message> {
        self.drop_buffer.insert(order, (videos, subtitle));
        let mut tasks = Vec::new();
        while let Some((videos, subtitle)) = self.drop_buffer.remove(&self.drop_next) {
            self.drop_next += 1;
            let append = self.drop_started;
            self.drop_started |= !videos.is_empty();
            tasks.push(self.drop_resolved(videos, subtitle, append));
        }
        Task::batch(tasks)
    }

    pub(crate) fn drop_resolved(
        &mut self,
        videos: Vec<Entry>,
        subtitle: Option<PathBuf>,
        append: bool,
    ) -> Task<Message> {
        if videos.is_empty() {
            if subtitle.is_some() && self.status.remote {
                self.toast_error(enzui_core::localization::text(
                    "video-remote-external-subtitle",
                ));
                return Task::none();
            }
            match (subtitle, &self.video) {
                (Some(path), Ok(video)) if self.status.has_source() => {
                    video.set_subtitle_file(Some(path));
                    return self.plex_event(crate::plex::Event::Close);
                }
                (Some(_), _) => self.toast(enzui_core::localization::text(
                    "video-open-a-video-before-adding-subtitles-335402",
                )),
                (None, _) => self.toast_error(enzui_core::localization::text(
                    "video-nothing-there-can-be-played-d7abe1",
                )),
            }
            return Task::none();
        }
        if append && !self.playlist.is_empty() {
            self.playlist.append(videos);
            self.refresh_menus();
            return Task::none();
        }
        self.playlist.replace(videos);
        self.refresh_menus();
        let leave_browser = self.plex_event(crate::plex::Event::Close);
        let load = match subtitle {
            // A video dropped with its subtitles: use those, not a sibling.
            Some(path) => {
                self.record_progress();
                self.load_token = self.load_token.wrapping_add(1);
                self.presenter.clear();
                self.load_with(self.load_token, Some(path))
            }
            None => self.load_current(),
        };
        Task::batch([leave_browser, load])
    }

    pub(crate) fn open_url_dialog(&mut self) -> Task<Message> {
        if !self.dialogs.is_open(&Dialog::OpenUrl) {
            self.url_presenter = Some(dialogs::open_url());
        }
        self.present(Dialog::OpenUrl)
    }

    pub(crate) fn present(&mut self, dialog: Dialog) -> Task<Message> {
        self.queue_context.close();
        self.titlebar_menu = false;
        let invoker = if self.dialogs.topmost() == Some(&Dialog::OpenUrl) {
            Some(enzui_widgets::form_dialog::scoped_field_widget_id(
                self.url_presenter
                    .as_ref()
                    .and_then(|presenter| presenter.field_id_scope()),
                &dialogs::field(dialogs::URL),
            ))
        } else if let Some(covered) = self.dialogs.topmost() {
            Some(covered.close_button_id())
        } else {
            Some(iced::advanced::widget::Id::new(
                "video-titlebar-actions-overflow-trigger",
            ))
        };
        self.dialogs.open(dialog, invoker)
    }

    pub(crate) fn forget_dialogs(&mut self, closed: &[Dialog]) {
        if closed.contains(&Dialog::OpenUrl) {
            self.url_presenter = None;
        }
    }

    pub(crate) fn dismiss_dialog(
        &mut self,
        key: Dialog,
        cause: enzui_core::modal::DismissCause,
    ) -> Task<Message> {
        let (closed, focus) = self.dialogs.dismiss(&key, cause);
        self.forget_dialogs(&closed);
        focus
    }

    pub(crate) fn close_dialog(&mut self) -> Task<Message> {
        self.dialogs
            .topmost()
            .copied()
            .map_or_else(Task::none, |key| {
                self.dismiss_dialog(key, enzui_core::modal::DismissCause::CloseButton)
            })
    }

    pub(crate) fn url_submitted(&mut self, submission: FormSubmission) -> Task<Message> {
        let typed = submission
            .text(&dialogs::field(dialogs::URL))
            .unwrap_or_default()
            .to_owned();
        let refusal = match files::normalize_url(&typed) {
            None => Some(enzui_core::localization::text("video-bad-address")),
            Some(url) => MediaSource::remote(&url, &UrlPolicy::DEFAULT)
                .err()
                .map(address_refusal),
        };
        if let Some(refusal) = refusal {
            let presenter = dialogs::url_refused(submission, refusal);
            let focus = enzui_widgets::form_dialog::scoped_focus_first_invalid::<Message>(
                presenter.field_id_scope(),
                presenter.session(),
            );
            self.url_presenter = Some(presenter);
            return focus.map_or_else(Task::none, iced::advanced::widget::operate);
        }
        let Some(url) = files::normalize_url(&typed) else {
            return Task::none();
        };
        let focus = self.dismiss_dialog(
            Dialog::OpenUrl,
            enzui_core::modal::DismissCause::DefaultAction,
        );
        self.last_drop = None;
        self.drop_buffer.clear();
        self.open_token = self.open_token.wrapping_add(1);
        self.playlist.replace(vec![Entry::Url(url)]);
        self.refresh_menus();
        Task::batch([
            focus,
            self.plex_event(crate::plex::Event::Close),
            self.load_current(),
        ])
    }

    pub(crate) fn open_recent(&mut self, index: usize) -> Task<Message> {
        let Some(recent) = self.settings.history.items().get(index) else {
            return Task::none();
        };
        self.last_drop = None;
        self.drop_buffer.clear();
        self.open_token = self.open_token.wrapping_add(1);
        self.playlist.replace(vec![recent.entry.clone()]);
        self.refresh_menus();
        Task::batch([
            self.plex_event(crate::plex::Event::Close),
            self.load_current(),
        ])
    }

    pub(crate) fn forget_recent(&mut self, index: usize) -> Task<Message> {
        let Some(entry) = self
            .settings
            .history
            .items()
            .get(index)
            .map(|r| r.entry.clone())
        else {
            return Task::none();
        };
        self.settings.history.remove(&entry);
        self.thumbnails
            .remove(&crate::model::history::thumbnail_key(&entry));
        self.save_settings()
    }
}

// Individual files reach GStreamer regardless of extension; folders use the video filter.
pub(super) fn resolve(paths: Vec<PathBuf>) -> (Vec<Entry>, Option<PathBuf>) {
    let mut videos = Vec::new();
    let mut subtitle = None;
    for path in paths {
        if path.is_dir() {
            videos.extend(persistence::videos_in(&path).into_iter().map(Entry::File));
        } else if files::is_subtitle(&path) {
            subtitle.get_or_insert(path);
        } else if path.is_file() {
            videos.push(Entry::File(path));
        }
    }
    (videos, subtitle)
}

fn resolve_arguments(arguments: Vec<std::ffi::OsString>) -> (Vec<Entry>, Option<PathBuf>) {
    let mut videos = Vec::new();
    let mut subtitle = None;
    for argument in arguments {
        let path = match argument.to_str() {
            Some(uri)
                if uri
                    .get(..5)
                    .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:")) =>
            {
                match enzui_core::portal::uri::file_uri_to_path(uri) {
                    Ok(path) => path,
                    Err(_) => continue,
                }
            }
            Some(url) if url.contains("://") => {
                if MediaSource::remote(url, &UrlPolicy::DEFAULT).is_ok() {
                    videos.push(Entry::Url(url.to_owned()));
                }
                continue;
            }
            _ => PathBuf::from(argument),
        };
        let (entries, found_subtitle) = resolve(vec![path]);
        videos.extend(entries);
        if subtitle.is_none() {
            subtitle = found_subtitle;
        }
    }
    (videos, subtitle)
}

pub(crate) fn address_refusal(verdict: enzui_core::url_policy::UrlVerdict) -> String {
    use enzui_core::url_policy::UrlVerdict;
    match verdict {
        UrlVerdict::SchemeRefused { .. } => {
            enzui_core::localization::text("video-use-an-https-video-address-801817")
        }
        UrlVerdict::CredentialsInUrl => enzui_core::localization::text(
            "video-remove-the-username-and-password-from-the-address-167cb4",
        ),
        UrlVerdict::CleartextRefused => {
            enzui_core::localization::text("video-use-a-secure-https-video-address-fc62d4")
        }
        UrlVerdict::MissingHost | UrlVerdict::Malformed => enzui_core::localization::text(
            "video-enter-a-complete-video-address-with-a-hostname-f39d3c",
        ),
        _ => enzui_core::localization::text(
            "video-that-address-cannot-be-used-for-a-network-stream-361347",
        ),
    }
}
