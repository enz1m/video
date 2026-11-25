use super::{App, Message};
use crate::plex::{self, Event, Kind, Listing, Token};
use iced::Task;
use std::time::{Duration, Instant};

impl App {
    pub(super) fn clear_plex_playback(&mut self) {
        if self.plex.playing.is_some() {
            self.queue_transcode_stop();
            self.plex.server_tracks = plex::quality::ServerTracks::default();
            self.plex.playing = None;
            self.plex.playing_server = None;
            self.plex.playing_url = None;
            self.plex.play_session_id.clear();
            if let Some(handle) = self.plex.next_request.take() {
                handle.abort();
            }
            self.plex.next_episode = None;
            self.plex.auto_next = false;
            self.service_selection = None;
            self.playlist.replace(vec![]);
            self.presenter.clear();
            self.pending_seek = None;
            self.video = super::create_video_service();
            self.status = Default::default();
            self.playing = None;
            self.awake.set(false);
        }
    }
    fn find_next_episode(&mut self) -> Task<Message> {
        let (Some(item), Some(server), Ok(client)) = (
            self.plex.playing.clone(),
            self.plex.playing_server.clone(),
            self.plex_client(),
        ) else {
            return Task::none();
        };
        if item.r#type != "episode" {
            return Task::none();
        }
        let token = self.load_token;
        let key = item.rating_key.clone();
        let (task, handle) = crate::l10n::perform(
            async move { client.next_episode(&server, &item).await },
            move |result| Message::Plex(Event::NextEpisode(token, key, Box::new(result))),
        )
        .abortable();
        if let Some(previous) = self.plex.next_request.replace(handle) {
            previous.abort();
        }
        task
    }

    pub(super) fn next_episode(&mut self) -> Task<Message> {
        let Some(next) = self.plex.next_episode.take() else {
            return Task::none();
        };
        if self.plex.busy || self.plex.visible {
            self.plex.next_episode = Some(next);
            return Task::none();
        }
        self.plex.auto_next = false;
        self.plex.selected = Some(next);
        self.plex_route(Event::Play(false))
    }

    fn plex_home(&mut self) -> Task<Message> {
        let Some(connected) = self.plex.connected.clone() else {
            return Task::none();
        };
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        let generation = self.plex.advance();
        let (task, handle) = crate::l10n::perform(
            async move { client.home(&connected).await },
            move |previews| Message::Plex(Event::Home(generation, previews)),
        )
        .abortable();
        self.plex.request = Some(handle);
        task
    }
    fn plex_details(&mut self) -> Task<Message> {
        let Some(item) = self.plex.selected.clone() else {
            return Task::none();
        };
        let Some(server) = self
            .plex
            .connected
            .as_ref()
            .map(|connected| connected.server.clone())
        else {
            return Task::none();
        };
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        let generation = self.plex.advance();
        self.plex.backdrop = None;
        let key = item.rating_key.clone();
        let related = {
            let client = client.clone();
            let server = server.clone();
            let item = item.clone();
            crate::l10n::perform(
                async move { client.related(&server, &item).await },
                move |result| Message::Plex(Event::Related(generation, key, result)),
            )
        };
        Task::batch([
            crate::l10n::perform(
                async move { client.details(&server, item).await },
                move |details| Message::Plex(Event::Details(generation, Box::new(details))),
            ),
            related,
            Self::plex_to_top(),
        ])
    }
    fn plex_context(&self) -> Task<Message> {
        let (Some(parent), Some(connected), Ok(client)) = (
            self.plex.parents.last().cloned(),
            self.plex.connected.as_ref(),
            self.plex_client(),
        ) else {
            return Task::none();
        };
        let server = connected.server.clone();
        let generation = self.plex.generation;
        crate::l10n::perform(
            async move { client.details(&server, parent).await },
            move |result| Message::Plex(Event::ContextDetails(generation, Box::new(result))),
        )
    }

    fn plex_to_top() -> Task<Message> {
        iced::widget::operation::snap_to(
            "plex-library",
            iced::widget::scrollable::RelativeOffset::START,
            iced::widget::operation::Animation::Instant,
        )
    }
    pub(super) fn plex_client(&self) -> Result<plex::Client, String> {
        plex::Client::new(self.settings.plex_client_id.clone())
    }
    pub(crate) fn plex_event(&mut self, event: Event) -> Task<Message> {
        let request = matches!(
            event,
            Event::Open
                | Event::Connect
                | Event::ConnectInsecure
                | Event::Server(_)
                | Event::Library(_)
                | Event::Sort(_)
                | Event::Refresh
                | Event::SearchGo
                | Event::ClearSearch
                | Event::Select(_)
                | Event::ActivityItem(_, _, _)
                | Event::Preview(_, _)
                | Event::QuickPlay(_, _)
                | Event::Back
                | Event::Ancestor(_)
                | Event::OpenRelated(_)
                | Event::PlayRelated(_)
                | Event::SignIn
                | Event::Poll(_)
                | Event::SwitchUser
        );
        let task = self.plex_route(event);
        self.menus.account.0.reconcile(super::menus::account(self));
        self.menus.sources.0.reconcile(super::menus::sources(self));
        self.menus.sort.0.reconcile(super::menus::sort(self));
        let task = if request {
            let (task, handle) = task.abortable();
            self.plex.request = Some(handle);
            task
        } else {
            task
        };
        let handles = if self.plex.visible && self.plex.connected.is_some() {
            self.plex
                .backdrop
                .iter()
                .map(|(_, image)| image.clone())
                .chain(
                    self.plex
                        .home
                        .iter()
                        .filter_map(|preview| preview.backdrop.clone()),
                )
                .collect()
        } else {
            Vec::new()
        };
        let uploads = self
            .plex
            .artwork
            .reconcile(handles)
            .into_iter()
            .map(|(ticket, handle)| {
                let id = handle.id();
                iced::widget::image::allocate(handle)
                    .map(move |result| Message::Plex(Event::BackdropAllocated(ticket, id, result)))
            });
        // Stops are batched outside the abortable request so they still run.
        Task::batch([task, Task::batch(uploads), self.drain_transcode_stops()])
    }
    /// The playing transcode's session, kept to stop on the server.
    fn queue_transcode_stop(&mut self) {
        if let (Some(session), Some(server)) =
            (self.plex.transcode.take(), self.plex.playing_server.clone())
        {
            self.plex.stops.push((server, session));
        }
    }
    /// Asks the servers to end the transcodes this app no longer plays.
    pub(super) fn drain_transcode_stops(&mut self) -> Task<Message> {
        let stops = std::mem::take(&mut self.plex.stops);
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        Task::batch(stops.into_iter().map(|(server, session)| {
            let client = client.clone();
            crate::l10n::perform(
                async move { client.stop_transcode(&server, &session).await },
                |result| Message::Plex(Event::Timeline(result)),
            )
        }))
    }
    fn new_play_session(&self) -> String {
        format!(
            "{}-{}-{}",
            std::process::id(),
            crate::model::now_ms(),
            self.load_token
        )
    }
    /// The server's tracks while a transcode plays, which then stand in
    /// for the player's own.
    pub(crate) fn transcoded_tracks(&self) -> Option<&plex::quality::ServerTracks> {
        self.plex
            .transcode
            .as_ref()
            .map(|_| &self.plex.server_tracks)
    }
    /// Streams the playing item again at the current quality and track
    /// choices, from the position it is at.
    pub(crate) fn restart_service_stream(&mut self) -> Task<Message> {
        let (Some(server), Some(item)) =
            (self.plex.playing_server.clone(), self.plex.playing.clone())
        else {
            return Task::none();
        };
        let position = self.status.position;
        self.queue_transcode_stop();
        let quality = self.settings.stream_quality;
        let session = self.new_play_session();
        let url = match plex::quality::stream_url(
            &server,
            &item,
            quality,
            &self.plex.server_tracks,
            &session,
            &self.settings.plex_client_id,
        ) {
            Ok(url) => url,
            Err(error) => {
                self.toast_error(error);
                return self.drain_transcode_stops();
            }
        };
        self.plex.play_session_id.clone_from(&session);
        self.plex.transcode = (!quality.is_original()).then_some(session);
        let (audio, subtitle) = self.plex.server_tracks.choices();
        if let Some(selection) = &mut self.service_selection {
            selection.audio = audio;
            selection.subtitle = subtitle;
            selection.transcoded = !quality.is_original();
        }
        if let Some(playing) = &mut self.plex.playing {
            playing.view_offset = u64::try_from(position.as_millis()).unwrap_or(u64::MAX);
        }
        self.plex_resume = true;
        self.plex.playing_url = Some(url.clone());
        self.playlist.replace(vec![crate::model::Entry::Url(url)]);
        let load = self.load_current();
        Task::batch([self.drain_transcode_stops(), load])
    }
    /// Chooses a server track while a transcode plays: Plex keeps the
    /// choice on the item first, Jellyfin and Emby take it in the address.
    pub(crate) fn choose_server_track(
        &mut self,
        audio: Option<u64>,
        subtitle: Option<Option<u64>>,
    ) -> Task<Message> {
        let tracks = &mut self.plex.server_tracks;
        if let Some(audio) = audio {
            tracks.audio_choice = Some(audio);
        }
        if let Some(subtitle) = subtitle {
            tracks.subtitle_choice = subtitle;
        }
        self.refresh_menus();
        let Some(server) = self.plex.playing_server.clone() else {
            return Task::none();
        };
        let part = self
            .service_selection
            .as_ref()
            .and_then(|selection| selection.streams.as_ref())
            .map(|streams| streams.part);
        match (server.kind, part) {
            (Kind::Plex, Some(part)) => {
                let Ok(client) = self.plex_client() else {
                    return Task::none();
                };
                // Subtitle id 0 turns them off.
                let subtitle = subtitle.map(|choice| choice.unwrap_or(0));
                crate::l10n::perform(
                    async move { client.select_streams(&server, part, audio, subtitle).await },
                    |result| Message::Plex(Event::ServerTracksSaved(result)),
                )
            }
            _ => self.restart_service_stream(),
        }
    }
    /// Keeps the server-side choice in step with a track the player picked,
    /// so a later transcode starts with it.
    pub(crate) fn note_player_tracks(&mut self, audio: Option<i32>, subtitle: Option<Option<i32>>) {
        let tracks = &mut self.plex.server_tracks;
        let by_ordinal = |list: &[plex::quality::ServerTrack], index: i32| {
            list.iter()
                .find(|track| track.ordinal == Some(index))
                .map(|track| track.id)
        };
        if let Some(index) = audio
            && let Some(id) = by_ordinal(&tracks.audio, index)
        {
            tracks.audio_choice = Some(id);
        }
        match subtitle {
            Some(Some(index)) => {
                if let Some(id) = by_ordinal(&tracks.subtitles, index) {
                    tracks.subtitle_choice = Some(id);
                }
            }
            Some(None) => tracks.subtitle_choice = None,
            None => {}
        }
    }
    pub(super) fn plex_route(&mut self, event: Event) -> Task<Message> {
        match event {
            event @ (Event::AddSource
            | Event::CancelAdd
            | Event::Choose(_)
            | Event::UseSource(_)
            | Event::RemoveSource
            | Event::Identified(..)
            | Event::Username(_)
            | Event::Password(_)
            | Event::ManualSignIn) => self.source_route(event),
            Event::BackdropAllocated(ticket, id, result) => {
                self.plex.artwork.complete(ticket, id, result);
                Task::none()
            }
            Event::Open => {
                self.plex.visible = true;
                let settled = self.plex.connected.is_some()
                    || self.plex.account.is_some()
                    || self.plex.restoring
                    || self.plex.adding
                    || self.plex.pin.is_some()
                    || self.plex.connecting.is_some();
                if settled {
                    let released = self.plex.connected.is_some()
                        && self.plex.home.is_empty()
                        && self.plex.library.is_none()
                        && self.plex.selected.is_none()
                        && !self.plex.busy;
                    return if released {
                        self.plex_home()
                    } else {
                        self.activity()
                    };
                }
                match self.settings.sources.get(self.settings.source).cloned() {
                    None if self.plex.kind == Kind::Plex && !self.plex.restore_attempted => {
                        self.source_route(Event::AddSource)
                    }
                    Some(source) if source.kind != Kind::Plex => {
                        self.plex.kind = source.kind;
                        self.plex.server_input.clone_from(&source.address);
                        self.restore_source(source)
                    }
                    Some(_) => {
                        self.plex.kind = Kind::Plex;
                        self.plex_restore()
                    }
                    None => self.plex_restore(),
                }
            }
            Event::Discover => {
                self.plex.local_servers = None;
                Self::plex_discover()
            }
            Event::Discovered(servers) => {
                self.plex.local_servers = Some(servers);
                Task::none()
            }
            Event::ConnectLocal(index) => {
                let Some(server) = self
                    .plex
                    .local_servers
                    .as_ref()
                    .and_then(|servers| servers.get(index))
                    .cloned()
                else {
                    return Task::none();
                };
                self.plex.connecting = Some(plex::ConnectionProgress::Server(server.name));
                self.connect_address(server.kind, server.address)
            }
            Event::ToggleAddress => {
                self.plex.manual_address = !self.plex.manual_address;
                Task::none()
            }
            Event::ShowProfiles => {
                self.plex.profile_chosen = false;
                self.plex.manual = false;
                if self.plex.connecting.take().is_some() {
                    self.plex.advance();
                    self.plex.busy = false;
                }
                self.plex.user = None;
                self.plex.user_pin.clear();
                self.plex.pin_error = None;
                self.plex_avatars()
            }
            Event::Avatar(thumb, Some(handle)) => {
                self.plex.avatars.insert(thumb, handle);
                Task::none()
            }
            Event::Probed(id, probe) => {
                self.plex.probes.insert(id, probe);
                Task::none()
            }
            Event::PinDigit(digit) => {
                if self.plex.user.is_none() || self.plex.user_pin.len() >= 4 || digit > 9 {
                    return Task::none();
                }
                self.plex.pin_error = None;
                self.plex.user_pin.push(char::from(b'0' + digit));
                if self.plex.user_pin.len() == 4 {
                    return self.plex_route(Event::SwitchUser);
                }
                Task::none()
            }
            Event::PinBack => {
                self.plex.user_pin.pop();
                Task::none()
            }
            Event::PinCancel => {
                self.plex.user = None;
                self.plex.user_pin.clear();
                self.plex.pin_error = None;
                Task::none()
            }
            Event::ChangeServer => {
                self.plex.advance();
                self.plex.busy = false;
                self.plex.connecting = None;
                self.plex.connected = None;
                self.plex.home.clear();
                self.plex.activity.clear();
                self.plex.backdrop = None;
                self.plex.library = None;
                self.plex.items.clear();
                self.plex.selected = None;
                Task::batch([self.plex_choose_server(false), Self::plex_discover()])
            }
            Event::Close => {
                self.plex.advance();
                self.plex.busy = false;
                self.plex.visible = false;
                self.plex.adding = false;
                // A sign-in waiting for approval stops polling once its page is left.
                if self.plex.pin.is_some() {
                    self.plex.pin = None;
                    self.plex.auth_link.clear();
                }
                self.activity()
            }
            Event::ServerInput(value) => {
                self.plex.insecure_address = None;
                self.plex.server_input = value;
                Task::none()
            }
            Event::ConnectInsecure => {
                let Some((kind, address)) = self.plex.insecure_address.take() else {
                    return Task::none();
                };
                if kind != Kind::Plex {
                    self.plex.connecting = Some(plex::ConnectionProgress::Server(address.clone()));
                }
                self.connect_accepted_address(kind, address)
            }
            Event::Connect => {
                let address = self.plex.server_input.trim().to_owned();
                if self.plex.kind != Kind::Plex {
                    self.plex.connecting = Some(plex::ConnectionProgress::Server(address.clone()));
                }
                self.connect_address(self.plex.kind, address)
            }
            Event::Server(index) => {
                let server = self
                    .plex
                    .account
                    .as_ref()
                    .and_then(|a| a.servers.get(index))
                    .cloned();
                match server {
                    Some(server) => {
                        self.plex.connecting =
                            Some(plex::ConnectionProgress::Server(server.name.clone()));
                        let generation = self.plex.advance();
                        let Ok(client) = self.plex_client() else {
                            return Task::none();
                        };
                        crate::l10n::perform(
                            async move { client.connect_server(server).await },
                            move |result| Message::Plex(Event::Connected(generation, result)),
                        )
                    }
                    None => Task::none(),
                }
            }
            Event::Connected(generation, result) if generation == self.plex.generation => {
                self.plex.busy = false;
                self.plex.connecting = None;
                match result {
                    Ok(connected) => {
                        if !connected.server.id.is_empty() {
                            self.settings
                                .plex_server_id
                                .clone_from(&connected.server.id);
                        }
                        self.plex.profile_chosen = true;
                        self.record_source(&connected.server);
                        if connected.server.kind == Kind::Plex {
                            self.settings
                                .plex_server
                                .clone_from(&connected.server.address);
                        }
                        self.plex.server_input.clone_from(&connected.server.address);
                        self.plex.connected = Some(connected);
                        self.plex.home.clear();
                        self.plex.activity.clear();
                        self.plex.backdrop = None;
                        self.plex.library = None;
                        self.plex.items.clear();
                        self.plex.selected = None;
                        self.plex.parents.clear();
                        self.plex.posters.clear();
                        Task::batch([self.save_settings(), self.plex_home()])
                    }
                    Err(error) => {
                        // A Jellyfin or Emby token the server revoked asks for the profile again.
                        if self.plex.kind != Kind::Plex && error == crate::jellyfin::SESSION_EXPIRED
                        {
                            self.plex.profile_chosen = false;
                        }
                        self.plex.error = Some(error);
                        Task::none()
                    }
                }
            }
            Event::Home(generation, mut home) if generation == self.plex.generation => {
                self.plex.busy = false;
                home.previews.sort_by_key(|preview| preview.library);
                self.plex.home = home.previews;
                self.plex.activity = home.activity;
                Task::none()
            }
            Event::ActivityPage(section, forward) => {
                let columns = plex::shelf_columns(self.window_size.width);
                if let Some(shelf) = self
                    .plex
                    .activity
                    .iter_mut()
                    .find(|shelf| shelf.section == section)
                    && let Ok(page) = &shelf.result
                {
                    let last = page.items.len().saturating_sub(1) / columns;
                    shelf.visible_page = if forward {
                        (shelf.visible_page + 1).min(last)
                    } else {
                        shelf.visible_page.saturating_sub(1).min(last)
                    };
                }
                Task::none()
            }
            Event::ActivityItem(section, index, play) => {
                let Some(page) = self
                    .plex
                    .activity
                    .iter()
                    .find(|shelf| shelf.section == section)
                    .and_then(|shelf| shelf.result.as_ref().ok())
                else {
                    return Task::none();
                };
                let Some(item) = page.items.get(index).cloned() else {
                    return Task::none();
                };
                self.plex.posters.extend(page.posters.clone());
                self.plex.selected = Some(item);
                if play {
                    self.plex_route(Event::Play(true))
                } else {
                    self.plex_details()
                }
            }
            Event::Preview(library, index) => {
                let Some(preview) = self
                    .plex
                    .home
                    .iter()
                    .find(|preview| preview.library == library)
                else {
                    return Task::none();
                };
                let Some(item) = preview.page.items.get(index).cloned() else {
                    return Task::none();
                };
                self.plex.posters.extend(preview.page.posters.clone());
                if matches!(item.r#type.as_str(), "show" | "season") {
                    self.plex.library = Some(library);
                    self.plex.parents = vec![item];
                    self.plex.offset = 0;
                    self.plex.active_query.clear();
                    self.plex.query.clear();
                    self.plex_page()
                } else {
                    self.plex.selected = Some(item);
                    self.plex_details()
                }
            }
            Event::QuickPlay(library, index) => {
                let item = if let Some(library) = library {
                    self.plex
                        .home
                        .iter()
                        .find(|preview| preview.library == library)
                        .and_then(|preview| preview.page.items.get(index))
                } else {
                    self.plex.items.get(index)
                }
                .cloned();
                let Some(item) = item else {
                    return Task::none();
                };
                if matches!(item.r#type.as_str(), "show" | "season") {
                    return self.plex_route(library.map_or(Event::Select(index), |library| {
                        Event::Preview(library, index)
                    }));
                }
                self.plex.advance();
                self.plex.busy = false;
                self.plex.selected = Some(item);
                let task = self.plex_route(Event::Play(true));
                if !self.plex.visible {
                    self.plex.selected = None;
                }
                task
            }
            Event::Details(generation, result) if generation == self.plex.generation => {
                self.plex.busy = false;
                match *result {
                    Ok(details) => {
                        self.plex.backdrop = details
                            .backdrop
                            .map(|image| (details.item.rating_key.clone(), image));
                        let photos = self.cast_photos(&details.item);
                        self.plex.selected = Some(details.item);
                        return photos;
                    }
                    Err(error) => self.plex.error = Some(error),
                }
                Task::none()
            }
            Event::CastPhoto(thumb, Some(handle)) => {
                self.plex.cast_photos.insert(thumb, handle);
                Task::none()
            }
            Event::ContextDetails(generation, result) if generation == self.plex.generation => {
                let Ok(details) = *result else {
                    return Task::none();
                };
                let Some(parent) = self
                    .plex
                    .parents
                    .last_mut()
                    .filter(|parent| parent.rating_key == details.item.rating_key)
                else {
                    return Task::none();
                };
                *parent = details.item;
                self.plex.backdrop = details
                    .backdrop
                    .map(|image| (parent.rating_key.clone(), image));
                let parent = parent.clone();
                self.cast_photos(&parent)
            }
            Event::Library(index) => {
                if !self
                    .plex
                    .connected
                    .as_ref()
                    .is_some_and(|c| c.libraries.get(index).is_some())
                {
                    return Task::none();
                }
                self.plex.library = Some(index);
                self.plex.offset = 0;
                self.plex.query.clear();
                self.plex.active_query.clear();
                self.plex.parents.clear();
                self.plex.selected = None;
                self.plex.posters.clear();
                self.plex_page()
            }
            Event::Page(generation, result) if generation == self.plex.generation => {
                self.plex.busy = false;
                self.plex.loading_more = false;
                match result {
                    Ok(page) => {
                        self.plex.items = page.items;
                        self.plex.total = page.total;
                        if self.plex.posters.len() > 72 {
                            self.plex.posters.clear();
                        }
                        self.plex.posters.extend(page.posters);
                        self.plex.selected = None;
                        iced::widget::operation::snap_to(
                            "plex-library",
                            iced::widget::scrollable::RelativeOffset::START,
                            iced::widget::operation::Animation::Instant,
                        )
                    }
                    Err(error) => {
                        self.plex.error = Some(error);
                        Task::none()
                    }
                }
            }
            Event::Search(value) => {
                self.plex.query = value;
                Task::none()
            }
            Event::SearchGo => {
                self.plex.active_query = self.plex.query.trim().to_owned();
                self.plex.offset = 0;
                self.plex_page()
            }
            Event::ClearSearch => {
                self.plex.query.clear();
                self.plex.active_query.clear();
                self.plex.offset = 0;
                self.plex_page()
            }
            Event::LoadMore => self.plex_more(),
            Event::More(generation, offset, result)
                if generation == self.plex.generation && offset == self.plex.items.len() =>
            {
                self.plex.loading_more = false;
                match result {
                    Ok(page) => {
                        let known: std::collections::HashSet<_> = self
                            .plex
                            .items
                            .iter()
                            .map(|item| item.rating_key.clone())
                            .collect();
                        self.plex.items.extend(
                            page.items
                                .into_iter()
                                .filter(|item| !known.contains(&item.rating_key)),
                        );
                        self.plex.total = page.total;
                        self.plex.posters.extend(page.posters);
                    }
                    Err(error) => self.plex.error = Some(error),
                }
                Task::none()
            }
            Event::ShelfPage(library, forward) => {
                let Some(preview) = self.plex.home.iter().find(|p| p.library == library) else {
                    return Task::none();
                };
                let columns = plex::shelf_columns(self.window_size.width);
                let last = preview.page.items.len().saturating_sub(1) / columns;
                let page = self.plex.shelves.entry(library).or_default();
                *page = if forward {
                    (*page + 1).min(last)
                } else {
                    page.saturating_sub(1).min(last)
                };
                Task::none()
            }
            Event::Related(generation, key, result) if generation == self.plex.generation => {
                if let Ok(page) = result {
                    self.plex.posters.extend(page.posters);
                    self.plex.related = (key, page.items);
                }
                Task::none()
            }
            Event::OpenRelated(index) => {
                let Some(item) = self.plex.related.1.get(index).cloned() else {
                    return Task::none();
                };
                self.plex.selected = Some(item);
                self.plex_details()
            }
            Event::PlayRelated(index) => {
                let Some(item) = self.plex.related.1.get(index).cloned() else {
                    return Task::none();
                };
                self.plex.selected = Some(item);
                self.plex_route(Event::Play(true))
            }
            Event::Ancestor(season) => self.plex_ancestor(season),
            Event::LeavePlayer => {
                self.cancel_recovery();
                self.plex.advance();
                self.plex.busy = false;
                if let Ok(video) = &self.video {
                    video.pause();
                }
                self.plex.auto_next = false;
                self.record_progress();
                self.plex.visible = true;
                self.surface_click = self.surface_click.wrapping_add(1);
                self.plex.selected = None;
                self.plex.backdrop = None;
                if let Some(episode) = self
                    .plex
                    .playing
                    .as_ref()
                    .filter(|item| item.r#type == "episode")
                    && self
                        .plex
                        .parents
                        .last()
                        .is_none_or(|parent| parent.rating_key != episode.parent_rating_key)
                {
                    self.plex.selected = Some(episode.clone());
                    return self.plex_ancestor(true);
                }
                Task::batch([self.activity(), self.plex_context()])
            }
            Event::NextEpisode(token, key, result)
                if token == self.load_token
                    && self
                        .plex
                        .playing
                        .as_ref()
                        .is_some_and(|item| item.rating_key == key) =>
            {
                self.plex.next_request = None;
                self.plex.next_episode = result.ok().flatten();
                if self.plex.auto_next
                    && self.status.playback == enzui_services::media::PlaybackState::EndOfStream
                    && !self.plex.visible
                {
                    self.next_episode()
                } else {
                    Task::none()
                }
            }
            Event::Select(index) => {
                let Some(item) = self.plex.items.get(index).cloned() else {
                    return Task::none();
                };
                if matches!(item.r#type.as_str(), "show" | "season") {
                    self.plex.parents.push(item);
                    self.plex.offset = 0;
                    self.plex.active_query.clear();
                    self.plex.query.clear();
                    self.plex_page()
                } else {
                    self.plex.selected = Some(item);
                    self.plex_details()
                }
            }
            Event::Back => {
                if self.plex.selected.take().is_some() {
                    self.plex.advance();
                    self.plex.busy = false;
                    self.plex.backdrop = None;
                    return Task::batch([Self::plex_to_top(), self.plex_context()]);
                }
                if self.plex.parents.pop().is_some() {
                    self.plex.offset = 0;
                    return self.plex_page();
                }
                if self.plex.library.is_some() && !self.plex.active_query.is_empty() {
                    return self.plex_route(Event::ClearSearch);
                }
                self.plex.library = None;
                self.plex.items.clear();
                if self.plex.home.is_empty() {
                    self.plex_home()
                } else {
                    Self::plex_to_top()
                }
            }
            Event::Play(resume) => {
                let Some(item) = self.plex.selected.clone() else {
                    return Task::none();
                };
                let Some(server) = self.plex.connected.as_ref().map(|c| c.server.clone()) else {
                    return Task::none();
                };
                let Ok(client) = self.plex_client() else {
                    return Task::none();
                };
                let generation = self.plex.advance();
                let (task, handle) = crate::l10n::perform(
                    async move { client.prepare_playback(server, item).await },
                    move |result| {
                        Message::Plex(Event::PlaybackPrepared(
                            generation,
                            resume,
                            Box::new(result),
                        ))
                    },
                )
                .abortable();
                self.plex.request = Some(handle);
                task
            }
            Event::PlaybackPrepared(generation, resume, result)
                if generation == self.plex.generation =>
            {
                self.plex.busy = false;
                if self.closing.is_some() || !self.dialogs.is_empty() || self.chooser_pending() {
                    return Task::none();
                }
                match *result {
                    Ok(prepared) => {
                        self.record_progress();
                        let plex::playback::Prepared {
                            server,
                            item,
                            url,
                            mut selection,
                            tracks,
                            warning,
                        } = prepared;
                        let quality = self.settings.stream_quality;
                        let session = self.new_play_session();
                        let url = if quality.is_original() {
                            url
                        } else {
                            match plex::quality::stream_url(
                                &server,
                                &item,
                                quality,
                                &tracks,
                                &session,
                                &self.settings.plex_client_id,
                            ) {
                                Ok(url) => url,
                                Err(error) => {
                                    self.plex.error = Some(error);
                                    return Task::none();
                                }
                            }
                        };
                        // The episode it replaces stops transcoding.
                        self.queue_transcode_stop();
                        selection.transcoded = !quality.is_original();
                        self.plex.server_tracks = tracks;
                        self.plex.transcode = (!quality.is_original()).then(|| session.clone());
                        if let Some(handle) = self.plex.next_request.take() {
                            handle.abort();
                        }
                        self.plex.next_episode = None;
                        self.plex.auto_next = false;
                        self.plex.playing_server = Some(server);
                        self.plex.playing = Some(item);
                        self.plex.playing_url = Some(url.clone());
                        self.plex.play_session_id = session;
                        let mut config = super::network_video_config();
                        config.gate.policy = enzui_core::url_policy::UrlPolicy {
                            allow_http: true,
                            allow_private_network: true,
                            max_redirects: 0,
                        };
                        self.video = super::create_video_service_with(config);
                        self.update_frame_limit();
                        self.plex.visible = false;
                        self.plex_resume = resume;
                        self.playlist.replace(vec![crate::model::Entry::Url(url)]);
                        self.service_selection = Some(selection);
                        if let Some(warning) = warning {
                            self.toast(warning);
                        }
                        let load = self.load_current();
                        Task::batch([load, self.find_next_episode()])
                    }
                    Err(error) => {
                        self.plex.error = Some(error);
                        Task::none()
                    }
                }
            }
            Event::SignIn => {
                let generation = self.plex.advance();
                let client = match self.plex_client() {
                    Ok(c) => c,
                    Err(e) => {
                        self.plex.error = Some(e);
                        self.plex.busy = false;
                        return Task::none();
                    }
                };
                if self.plex.kind != Kind::Plex {
                    let Some(server) = self
                        .plex
                        .account
                        .as_ref()
                        .and_then(|a| a.servers.first().cloned())
                    else {
                        return Task::none();
                    };
                    return crate::l10n::perform(
                        async move { client.api(&server)?.quick_connect().await },
                        move |result| Message::Plex(Event::Pin(generation, result)),
                    );
                }
                crate::l10n::perform(async move { client.pin().await }, move |result| {
                    Message::Plex(Event::Pin(generation, result))
                })
            }
            Event::Pin(generation, result) if generation == self.plex.generation => {
                self.plex.busy = false;
                match result {
                    Ok(pin) => {
                        if self.plex.kind == Kind::Plex {
                            self.plex.auth_link =
                                plex::auth_link(&self.settings.plex_client_id, &pin);
                            self.plex.auth_qr =
                                enzui_widgets::qr::EnzQrData::try_new(self.plex.auth_link.clone())
                                    .ok();
                        }
                        self.plex.auth_started = Some(Instant::now());
                        self.plex.pin = Some(pin);
                        Task::done(Message::Plex(Event::Poll(generation)))
                    }
                    Err(error) => {
                        self.plex.error = Some(error);
                        Task::none()
                    }
                }
            }
            Event::Poll(generation) if generation == self.plex.generation => {
                let Some(pin) = self.plex.pin.clone() else {
                    return Task::none();
                };
                if self
                    .plex
                    .auth_started
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(pin.expires_in.min(900)))
                {
                    self.plex.pin = None;
                    self.plex.auth_link.clear();
                    self.plex.error = Some(enzui_core::localization::text(
                        "video-this-sign-in-link-has-expired-sign-in-again-for-a-new-l-d10fb8",
                    ));
                    return Task::none();
                }
                let Ok(client) = self.plex_client() else {
                    return Task::none();
                };
                if self.plex.kind != Kind::Plex {
                    let Some(server) = self
                        .plex
                        .account
                        .as_ref()
                        .and_then(|a| a.servers.first().cloned())
                    else {
                        return Task::none();
                    };
                    return crate::l10n::perform(
                        async move {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            client.api(&server)?.quick_connect_poll(&server, &pin).await
                        },
                        move |result| Message::Plex(Event::Polled(generation, result)),
                    );
                }
                crate::l10n::perform(
                    async move {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        client.poll(&pin).await
                    },
                    move |result| Message::Plex(Event::Polled(generation, result)),
                )
            }
            Event::Polled(generation, result) if generation == self.plex.generation => match result
            {
                Ok(Some(account)) if self.plex.kind != Kind::Plex => {
                    self.plex.pin = None;
                    self.plex_route(Event::Switched(generation, Ok(account)))
                }
                Ok(Some(account)) => {
                    self.clear_plex_playback();
                    self.plex.pin = None;
                    self.plex.auth_link.clear();
                    let token = account.token.clone();
                    self.plex.home_token = token.clone();
                    self.plex.account = Some(account);
                    self.plex.connected = None;
                    self.plex.home.clear();
                    self.plex.activity.clear();
                    self.plex.backdrop = None;
                    self.plex.library = None;
                    self.plex.items.clear();
                    self.plex.selected = None;
                    self.plex.user = None;
                    Task::batch([
                        crate::l10n::perform(plex::remember_token(token), |stored| {
                            Message::Plex(Event::CredentialSaved(stored))
                        }),
                        self.plex_after_account(),
                    ])
                }
                Ok(None) => Task::done(Message::Plex(Event::Poll(generation))),
                Err(error) => {
                    self.plex.auth_link.clear();
                    self.plex.error = Some(error);
                    self.plex.pin = None;
                    Task::none()
                }
            },
            Event::CancelSignIn => {
                self.plex.advance();
                self.plex.busy = false;
                self.plex.pin = None;
                self.plex.auth_link.clear();
                Task::none()
            }
            Event::CopyLink => iced::clipboard::write(self.plex.auth_link.clone())
                .map(|result| Message::Plex(Event::ClipboardCopied(result.is_ok()))),
            Event::Browser => {
                crate::l10n::perform(plex::open_browser(self.plex.auth_link.clone()), |result| {
                    Message::Plex(Event::BrowserOpened(result))
                })
            }
            Event::BrowserOpened(result) => {
                if let Err(error) = result {
                    self.plex.error = Some(error);
                }
                Task::none()
            }
            Event::User(index) => {
                let Some(user) = self
                    .plex
                    .account
                    .as_ref()
                    .and_then(|account| account.users.get(index))
                    .cloned()
                else {
                    return Task::none();
                };
                self.plex.user = Some(index);
                self.plex.user_pin.clear();
                self.plex.pin_error = None;
                let current = self.plex.account.as_ref().is_some_and(|account| {
                    account.user_id == user.id && !account.user_id.is_empty()
                });
                if current {
                    self.plex.user = None;
                    self.plex.profile_chosen = true;
                    self.settings.plex_user.clone_from(&user.id);
                    return Task::batch([self.save_settings(), self.plex_choose_server(true)]);
                }
                if user.protected {
                    return if self.plex.kind == Kind::Plex {
                        Task::none()
                    } else {
                        iced::widget::operation::focus(super::sources::PASSWORD_FIELD)
                    };
                }
                self.plex_route(Event::SwitchUser)
            }
            Event::SwitchUser if self.plex.kind != Kind::Plex => self.source_sign_in(),
            Event::SwitchUser => {
                let Some(account) = self.plex.account.as_ref() else {
                    return Task::none();
                };
                let Some(user) = self.plex.user.and_then(|i| account.users.get(i)).cloned() else {
                    return Task::none();
                };
                if user.protected && self.plex.user_pin.len() != 4 {
                    return Task::none();
                }
                let token = self.plex.home_token.clone();
                let pin = std::mem::take(&mut self.plex.user_pin);
                self.plex.connecting =
                    Some(plex::ConnectionProgress::Switching(user.title.clone()));
                let generation = self.plex.advance();
                let Ok(client) = self.plex_client() else {
                    return Task::none();
                };
                crate::l10n::perform(
                    async move { client.switch_user(&token, &user, &pin).await },
                    move |result| Message::Plex(Event::Switched(generation, result)),
                )
            }
            Event::Switched(generation, result) if generation == self.plex.generation => {
                self.plex.busy = false;
                self.plex.connecting = None;
                match result {
                    Ok(mut account) => {
                        self.clear_plex_playback();
                        if let Some(previous) = &self.plex.account {
                            account.users.clone_from(&previous.users);
                            account.quick_connect = previous.quick_connect;
                        }
                        let mut remember = Task::none();
                        if self.plex.kind == Kind::Plex {
                            if let Some(user) = self.plex.user.and_then(|i| account.users.get(i)) {
                                account.user_id.clone_from(&user.id);
                                if account.thumb.is_empty() {
                                    account.thumb.clone_from(&user.thumb);
                                }
                            }
                            self.settings.plex_user.clone_from(&account.user_id);
                        } else if let Some(server) = account.servers.first() {
                            let source = plex::Source {
                                kind: server.kind,
                                id: server.id.clone(),
                                user: server.user.clone(),
                                ..Default::default()
                            };
                            remember = crate::l10n::perform(
                                plex::remember_source(source, account.token.clone()),
                                |stored| Message::Plex(Event::CredentialSaved(stored)),
                            );
                            self.plex.manual = false;
                            self.plex.username.clear();
                        }
                        self.plex.account = Some(account);
                        self.plex.connected = None;
                        self.plex.home.clear();
                        self.plex.activity.clear();
                        self.plex.backdrop = None;
                        self.plex.library = None;
                        self.plex.user = None;
                        self.plex.pin_error = None;
                        self.plex.profile_chosen = true;
                        self.plex.items.clear();
                        self.plex.selected = None;
                        self.plex.parents.clear();
                        self.plex.posters.clear();
                        Task::batch([
                            remember,
                            self.save_settings(),
                            self.plex_avatars(),
                            self.plex_choose_server(true),
                        ])
                    }
                    Err(error)
                        if (error == plex::INCORRECT_PIN
                            || error == crate::jellyfin::INCORRECT_PASSWORD)
                            && (self.plex.user.is_some() || self.plex.manual) =>
                    {
                        self.plex.pin_error = Some(error);
                        if self.plex.kind == Kind::Plex {
                            Task::none()
                        } else {
                            iced::widget::operation::focus(super::sources::PASSWORD_FIELD)
                        }
                    }
                    Err(error) => {
                        self.plex.error = Some(error);
                        Task::none()
                    }
                }
            }
            Event::SignOut if self.plex.kind != Kind::Plex => {
                self.clear_plex_playback();
                self.reset_browser();
                if let Some(account) = self.plex.account.as_mut() {
                    account.token = Token::default();
                    account.user_id.clear();
                    account.name.clear();
                    account.thumb.clear();
                    for server in &mut account.servers {
                        server.token = Token::default();
                        server.user.clear();
                    }
                }
                self.plex.profile_chosen = false;
                self.plex.user = None;
                self.plex.user_pin.clear();
                self.plex.pin_error = None;
                let Some(index) = self.active_source().map(|_| self.settings.source) else {
                    return Task::none();
                };
                let source = self.settings.sources[index].clone();
                self.settings.sources[index].user.clear();
                Task::batch([
                    self.save_settings(),
                    self.plex_avatars(),
                    crate::l10n::perform(plex::forget_source(source), |deleted| {
                        Message::Plex(Event::CredentialDeleted(deleted))
                    }),
                ])
            }
            Event::SignOut => {
                self.clear_plex_playback();
                self.plex.advance();
                self.plex.busy = false;
                self.plex.home_token = Token::default();
                self.plex.account = None;
                self.plex.pin = None;
                self.plex.auth_link.clear();
                self.plex.connected = None;
                self.plex.home.clear();
                self.plex.activity.clear();
                self.plex.backdrop = None;
                self.plex.library = None;
                self.plex.selected = None;
                self.plex.items.clear();
                self.plex.posters.clear();
                self.plex.user = None;
                self.plex.user_pin.clear();
                self.plex.pin_error = None;
                self.plex.profile_chosen = false;
                self.plex.probes.clear();
                self.plex.connecting = None;
                self.settings.plex_user.clear();
                self.plex.busy = false;
                crate::l10n::perform(plex::forget_token(), |deleted| {
                    Message::Plex(Event::CredentialDeleted(deleted))
                })
            }
            Event::Restored(generation, result) if generation == self.plex.generation => {
                self.plex.busy = false;
                self.plex.restoring = false;
                match result {
                    Ok(Some(account)) => {
                        self.plex.home_token = account.token.clone();
                        self.plex.account = Some(account);
                        return self.plex_after_account();
                    }
                    Ok(None) => (),
                    Err(error) => self.plex.error = Some(error),
                }
                Task::none()
            }
            Event::ClipboardCopied(success) => {
                if success {
                    self.toast(enzui_core::localization::text(
                        "video-plex-sign-in-link-copied-7847d8",
                    ));
                } else {
                    self.toast_error(enzui_core::localization::text(
                        "video-couldn-t-copy-the-plex-sign-in-link-a63df0",
                    ));
                }
                Task::none()
            }
            Event::Progress => self.plex_timeline(),
            Event::Timeline(_result) => Task::none(),
            Event::Sort(sort) => {
                let Some(key) = self.library_sort_key() else {
                    return Task::none();
                };
                if sort == plex::Sort::default() {
                    self.settings.library_sorts.remove(&key);
                } else {
                    self.settings.library_sorts.insert(key, sort);
                }
                self.plex.offset = 0;
                self.plex.posters.clear();
                Task::batch([self.save_settings(), self.plex_page()])
            }
            Event::Quality(quality) => {
                if quality == self.settings.stream_quality {
                    return Task::none();
                }
                self.settings.stream_quality = quality;
                self.refresh_menus();
                let save = self.save_settings();
                if self.plex.playing.is_some() && !self.plex.visible {
                    return Task::batch([save, self.restart_service_stream()]);
                }
                save
            }
            Event::ServerTracksSaved(result) => match result {
                Ok(()) if self.plex.transcode.is_some() => self.restart_service_stream(),
                Ok(()) => Task::none(),
                Err(error) => {
                    self.toast_error(error);
                    Task::none()
                }
            },
            Event::Refresh => self.plex_refresh(),
            Event::Scan => self.plex_scan(),
            Event::Scanned(name, result) => {
                match result {
                    Ok(()) => {
                        self.toast(enzui_core::localized!("video-scan-started", "name" => name));
                    }
                    Err(error) => self.toast_error(error),
                }
                Task::none()
            }
            Event::CredentialSaved(stored) => {
                if !stored {
                    self.toast(enzui_core::localized!("video-is-signed-in-for-this-session-the-system-keyring-couldn-804b17", "value0" => self.plex.kind.name()));
                }
                Task::none()
            }
            Event::CredentialDeleted(deleted) => {
                if !deleted {
                    self.toast_error(enzui_core::localized!("video-signed-out-of-this-session-but-the-saved-sign-in-couldn-5ad08a", "value0" => self.plex.kind.name()));
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }
    pub(crate) fn plex_timeline(&mut self) -> Task<Message> {
        let Some(item) = self.plex.playing.as_mut() else {
            return Task::none();
        };
        if self.status.loading || !self.status.has_source() {
            return Task::none();
        }
        let Some(server) = self.plex.playing_server.clone() else {
            return Task::none();
        };
        let state = if self.status.playback == enzui_services::media::PlaybackState::EndOfStream {
            "stopped"
        } else if self.status.is_playing() {
            "playing"
        } else {
            "paused"
        };
        let time = u64::try_from(self.status.position.as_millis()).unwrap_or(u64::MAX);
        item.view_offset = if state == "stopped" { 0 } else { time };
        if self
            .plex
            .connected
            .as_ref()
            .is_some_and(|connected| connected.server.address == server.address)
        {
            if let Some(selected) = self
                .plex
                .selected
                .as_mut()
                .filter(|selected| selected.rating_key == item.rating_key)
            {
                selected.view_offset = item.view_offset;
            }
            for cached in self
                .plex
                .items
                .iter_mut()
                .filter(|cached| cached.rating_key == item.rating_key)
            {
                cached.view_offset = item.view_offset;
            }
        }
        let item = item.clone();
        let duration =
            u64::try_from(self.status.duration.unwrap_or_default().as_millis()).unwrap_or(u64::MAX);
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        let session = self.plex.play_session_id.clone();
        let transcoding = self.plex.transcode.is_some();
        crate::l10n::perform(
            async move {
                let session = plex::PlaySession {
                    id: &session,
                    transcoding,
                };
                client
                    .timeline(&server, &item, time, duration, state, session)
                    .await
            },
            |result| Message::Plex(Event::Timeline(result)),
        )
    }
    /// Saves a track change the viewer made back to Plex, so the next play on
    /// any client starts with the same audio and subtitles.
    pub(crate) fn plex_save_streams(
        &self,
        audio: Option<i32>,
        subtitle: Option<Option<i32>>,
    ) -> Task<Message> {
        let (Some(server), Some(streams)) = (
            self.plex.playing_server.clone(),
            self.service_selection
                .as_ref()
                .and_then(|selection| selection.streams.as_ref()),
        ) else {
            return Task::none();
        };
        let Some((audio, subtitle)) = streams.ids(&self.status, audio, subtitle) else {
            return Task::none();
        };
        let part = streams.part;
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        crate::l10n::perform(
            async move { client.select_streams(&server, part, audio, subtitle).await },
            |result| Message::Plex(Event::Timeline(result)),
        )
    }
    // The open library's saved-order key: the server and the library.
    fn library_sort_key(&self) -> Option<String> {
        let connected = self.plex.connected.as_ref()?;
        let library = connected.libraries.get(self.plex.library?)?;
        Some(format!("{}/{}", connected.server.id, library.key))
    }
    pub(crate) fn library_sort(&self) -> plex::Sort {
        self.library_sort_key()
            .and_then(|key| self.settings.library_sorts.get(&key).copied())
            .unwrap_or_default()
    }
    // Reloads what is on screen: the open title, the library page, or home.
    fn plex_refresh(&mut self) -> Task<Message> {
        if self.plex.connected.is_none() {
            return Task::none();
        }
        if self.plex.selected.is_some() {
            return self.plex_details();
        }
        if self.plex.library.is_some() || !self.plex.parents.is_empty() {
            self.plex.offset = 0;
            self.plex.posters.clear();
            return self.plex_page();
        }
        self.plex_home()
    }
    // Asks the server to scan the open library, or every library from home.
    fn plex_scan(&mut self) -> Task<Message> {
        let Some(connected) = &self.plex.connected else {
            return Task::none();
        };
        let library = self
            .plex
            .library
            .and_then(|index| connected.libraries.get(index));
        let name = library.map_or_else(|| connected.server.name.clone(), |l| l.title.clone());
        let key = library.map(|library| library.key.clone());
        let server = connected.server.clone();
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        crate::l10n::perform(
            async move { client.scan(&server, key.as_deref()).await },
            move |result| Message::Plex(Event::Scanned(name, result)),
        )
    }
    pub(super) fn plex_restore(&mut self) -> Task<Message> {
        if self.plex.kind != Kind::Plex
            || self.plex.restore_attempted
            || self.plex.connected.is_some()
        {
            return self.activity();
        }
        self.plex.restore_attempted = true;
        let generation = self.plex.advance();
        let Ok(client) = self.plex_client() else {
            return self.activity();
        };
        self.plex.restoring = true;
        Task::batch([
            self.save_settings(),
            crate::l10n::perform(plex::restored_account(client), move |result| {
                Message::Plex(Event::Restored(generation, result))
            }),
            Self::plex_discover(),
        ])
    }
    pub(super) fn plex_connect(&mut self, address: String, token: Token) -> Task<Message> {
        self.plex.pin = None;
        self.plex.auth_link.clear();
        let generation = self.plex.advance();
        self.plex.items.clear();
        self.plex.total = 0;
        self.plex.selected = None;
        match self.plex_client() {
            Ok(client) => crate::l10n::perform(
                async move { client.connect(&address, token).await },
                move |result| Message::Plex(Event::Connected(generation, result)),
            ),
            Err(error) => {
                self.plex.error = Some(error);
                self.plex.busy = false;
                Task::none()
            }
        }
    }
    fn plex_ancestor(&mut self, season: bool) -> Task<Message> {
        let source = self
            .plex
            .selected
            .clone()
            .or_else(|| self.plex.parents.last().cloned());
        let Some(source) = source else {
            return Task::none();
        };
        let (show_key, show_title, season_key, season_title) = match source.r#type.as_str() {
            "episode" => (
                source.grandparent_rating_key.clone(),
                source.grandparent_title.clone(),
                source.parent_rating_key.clone(),
                source.parent_title.clone(),
            ),
            "season" => (
                source.parent_rating_key.clone(),
                source.parent_title.clone(),
                source.rating_key.clone(),
                source.title.clone(),
            ),
            _ => return Task::none(),
        };
        if show_key.is_empty() || (season && season_key.is_empty()) {
            return Task::none();
        }
        let library = source
            .library_section_id
            .and_then(|id| {
                self.plex
                    .connected
                    .as_ref()?
                    .libraries
                    .iter()
                    .position(|library| library.key == id.to_string())
            })
            .or(self.plex.library);
        let known = |key: &str| {
            self.plex
                .parents
                .iter()
                .find(|item| item.rating_key == key)
                .cloned()
        };
        let show = known(&show_key).unwrap_or_else(|| plex::Item {
            rating_key: show_key.clone(),
            title: show_title,
            r#type: "show".into(),
            ..Default::default()
        });
        let mut parents = vec![show];
        if season {
            parents.push(known(&season_key).unwrap_or_else(|| plex::Item {
                rating_key: season_key.clone(),
                title: season_title,
                r#type: "season".into(),
                parent_title: parents[0].title.clone(),
                parent_rating_key: show_key.clone(),
                ..Default::default()
            }));
        }
        self.plex.library = library;
        self.plex.parents = parents;
        self.plex.selected = None;
        self.plex.offset = 0;
        self.plex.active_query.clear();
        self.plex.query.clear();
        Task::batch([self.plex_page(), Self::plex_to_top()])
    }
    fn cast_photos(&self, item: &plex::Item) -> Task<Message> {
        let (Some(connected), Ok(client)) = (self.plex.connected.as_ref(), self.plex_client())
        else {
            return Task::none();
        };
        Task::batch(
            item.cast
                .iter()
                .take(plex::cast::SHOWN)
                .filter(|person| {
                    !person.thumb.is_empty() && !self.plex.cast_photos.contains(&person.thumb)
                })
                .map(|person| {
                    let client = client.clone();
                    let server = connected.server.clone();
                    let thumb = person.thumb.clone();
                    crate::l10n::perform(
                        async move {
                            let photo = client.cast_photo(&server, thumb.clone()).await;
                            (thumb, photo)
                        },
                        |(thumb, photo)| Message::Plex(Event::CastPhoto(thumb, photo)),
                    )
                }),
        )
    }
    pub(super) fn plex_discover() -> Task<Message> {
        crate::l10n::perform(plex::discover_local(), |servers| {
            Message::Plex(Event::Discovered(servers))
        })
    }
    pub(super) fn plex_avatars(&self) -> Task<Message> {
        let (Some(account), Ok(client)) = (self.plex.account.as_ref(), self.plex_client()) else {
            return Task::none();
        };
        let server = account
            .servers
            .first()
            .filter(|server| server.kind != Kind::Plex)
            .cloned();
        let thumbs: std::collections::BTreeSet<String> = account
            .users
            .iter()
            .map(|user| user.thumb.clone())
            .chain(std::iter::once(account.thumb.clone()))
            .filter(|thumb| !thumb.is_empty() && !self.plex.avatars.contains(thumb))
            .collect();
        Task::batch(thumbs.into_iter().map(|thumb| {
            let client = client.clone();
            let server = server.clone();
            crate::l10n::perform(
                async move {
                    let photo = client.avatar(server, thumb.clone()).await;
                    (thumb, photo)
                },
                |(thumb, photo)| Message::Plex(Event::Avatar(thumb, photo)),
            )
        }))
    }
    pub(super) fn plex_after_account(&mut self) -> Task<Message> {
        let Some(account) = self.plex.account.as_ref() else {
            return Task::none();
        };
        let avatars = self.plex_avatars();
        let remembered = self.remembered_user();
        let users = &account.users;
        if self.plex.kind != Kind::Plex {
            // Jellyfin and Emby profiles need a sign-in unless a saved token restored one.
            if !account.token.is_empty() {
                self.plex.profile_chosen = true;
                return Task::batch([avatars, self.plex_choose_server(true)]);
            }
            self.plex.profile_chosen = false;
            return match users
                .iter()
                .position(|user| !remembered.is_empty() && user.id == remembered)
            {
                Some(index) if users[index].protected => {
                    self.plex.user = Some(index);
                    self.plex.user_pin.clear();
                    Task::batch([
                        avatars,
                        iced::widget::operation::focus(super::sources::PASSWORD_FIELD),
                    ])
                }
                Some(index) => Task::batch([avatars, self.plex_route(Event::User(index))]),
                None => avatars,
            };
        }
        if users.len() <= 1 || (!remembered.is_empty() && remembered == account.user_id) {
            self.plex.profile_chosen = true;
            return Task::batch([avatars, self.plex_choose_server(true)]);
        }
        self.plex.profile_chosen = false;
        let Some(index) = users
            .iter()
            .position(|user| !remembered.is_empty() && user.id == remembered)
        else {
            return avatars;
        };
        if users[index].protected {
            self.plex.user = Some(index);
            self.plex.user_pin.clear();
            return avatars;
        }
        Task::batch([avatars, self.plex_route(Event::User(index))])
    }
    pub(super) fn plex_choose_server(&mut self, automatic: bool) -> Task<Message> {
        if self.plex.connected.is_some() {
            return Task::none();
        }
        let (Some(account), Ok(client)) = (self.plex.account.as_ref(), self.plex_client()) else {
            return Task::none();
        };
        self.plex.probes.clear();
        let probes = Task::batch(account.servers.iter().map(|server| {
            let client = client.clone();
            let server = server.clone();
            crate::l10n::perform(
                async move {
                    let probe = client.probe(&server).await;
                    (server.id, probe)
                },
                |(id, probe)| Message::Plex(Event::Probed(id, probe)),
            )
        }));
        let pick = if automatic {
            account
                .servers
                .iter()
                .position(|server| {
                    !self.settings.plex_server_id.is_empty()
                        && server.id == self.settings.plex_server_id
                })
                .or_else(|| (account.servers.len() == 1).then_some(0))
        } else {
            None
        };
        match pick {
            Some(index) => Task::batch([probes, self.plex_route(Event::Server(index))]),
            None => probes,
        }
    }
    fn plex_more(&mut self) -> Task<Message> {
        if self.plex.busy
            || self.plex.loading_more
            || self.plex.selected.is_some()
            || self.plex.items.is_empty()
            || self.plex.items.len() >= self.plex.total
        {
            return Task::none();
        }
        let sort = self.library_sort();
        let Some(connected) = &self.plex.connected else {
            return Task::none();
        };
        let listing = match self.plex.parents.last() {
            Some(parent) => Listing::Children(parent.rating_key.clone()),
            None => {
                let Some(library) = self.plex.library.and_then(|i| connected.libraries.get(i))
                else {
                    return Task::none();
                };
                Listing::Library(library.key.clone(), sort)
            }
        };
        let server = connected.server.clone();
        let offset = self.plex.items.len();
        let query = self.plex.active_query.clone();
        let generation = self.plex.generation;
        match self.plex_client() {
            Ok(client) => {
                self.plex.loading_more = true;
                crate::l10n::perform(
                    async move { client.page(&server, &listing, offset, &query).await },
                    move |result| Message::Plex(Event::More(generation, offset, result)),
                )
            }
            Err(error) => {
                self.plex.error = Some(error);
                Task::none()
            }
        }
    }
    fn plex_page(&mut self) -> Task<Message> {
        let sort = self.library_sort();
        let Some(connected) = self.plex.connected.as_ref() else {
            return Task::none();
        };
        let listing = match self.plex.parents.last() {
            Some(parent) => Listing::Children(parent.rating_key.clone()),
            None => {
                let Some(library) = self.plex.library.and_then(|i| connected.libraries.get(i))
                else {
                    return Task::none();
                };
                Listing::Library(library.key.clone(), sort)
            }
        };
        let server = connected.server.clone();
        let offset = self.plex.offset;
        let query = self.plex.active_query.clone();
        let generation = self.plex.advance();
        self.plex.items.clear();
        self.plex.total = 0;
        self.plex.selected = None;
        match self.plex_client() {
            Ok(client) => {
                let context = self.plex_context();
                Task::batch([
                    crate::l10n::perform(
                        async move { client.page(&server, &listing, offset, &query).await },
                        move |result| Message::Plex(Event::Page(generation, result)),
                    ),
                    context,
                ])
            }
            Err(error) => {
                self.plex.error = Some(error);
                self.plex.busy = false;
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let themes =
            enzui_core::theme_file::integration::ThemeIntegration::unmanaged_compiled_theme(
                enzui_theme::EnzTheme::dark(),
            )
            .0;
        App::with(
            themes,
            Err("no player".into()),
            crate::persistence::Settings::default(),
            None,
        )
    }
    fn titled(key: &str) -> plex::Item {
        plex::Item {
            rating_key: key.into(),
            title: key.into(),
            ..Default::default()
        }
    }
    #[test]
    fn a_quality_change_restreams_from_the_position_and_stops_the_old_transcode() {
        use plex::quality::{Quality, ServerTrack, ServerTracks};
        let mut app = app();
        let server = plex::Server {
            kind: Kind::Jellyfin,
            address: "http://192.168.1.9:8096".into(),
            token: Token::new("secret".into()),
            ..Default::default()
        };
        let mut item = titled("movie-1");
        item.media = vec![plex::Media {
            source_id: "source-1".into(),
            parts: vec![plex::Part::default()],
            ..Default::default()
        }];
        let track = |id, ordinal| ServerTrack {
            id,
            label: format!("Track {id}"),
            ordinal: Some(ordinal),
            ..Default::default()
        };
        app.plex.playing_server = Some(server);
        app.plex.playing = Some(item);
        app.plex.visible = false;
        app.plex.server_tracks = ServerTracks {
            audio: vec![track(1, 0), track(2, 1)],
            subtitles: vec![track(3, 0)],
            audio_choice: Some(1),
            subtitle_choice: None,
        };
        app.service_selection = Some(plex::playback::Selection::default());
        app.status.position = std::time::Duration::from_secs(90);

        let _ = app.plex_event(Event::Quality(Quality::Mbps4));
        assert_eq!(app.settings.stream_quality, Quality::Mbps4);
        let url = app.plex.playing_url.clone().expect("a transcode address");
        assert!(url.contains("/Videos/movie-1/master.m3u8"), "{url}");
        assert!(url.contains("AudioStreamIndex=1"), "{url}");
        let first = app.plex.transcode.clone().expect("a transcode session");
        assert_eq!(first, app.plex.play_session_id);
        assert_eq!(app.plex.playing.as_ref().unwrap().view_offset, 90_000);
        assert!(app.service_selection.as_ref().unwrap().transcoded);

        // A server track while transcoding streams again with it.
        let _ = app.choose_server_track(Some(2), Some(Some(3)));
        let url = app.plex.playing_url.clone().unwrap();
        assert!(url.contains("AudioStreamIndex=2") && url.contains("SubtitleStreamIndex=3"));
        assert_ne!(app.plex.transcode.as_ref(), Some(&first), "a new session");

        // The original plays the file again with the chosen tracks.
        let _ = app.plex_event(Event::Quality(Quality::Original));
        let url = app.plex.playing_url.clone().unwrap();
        assert!(url.contains("/Videos/movie-1/stream"), "{url}");
        assert!(app.plex.transcode.is_none());
        assert!(app.plex.stops.is_empty(), "stops were handed to the server");
        let selection = app.service_selection.as_ref().unwrap();
        assert!(!selection.transcoded);
        assert_eq!(
            selection.audio.as_ref().map(|choice| choice.ordinal),
            Some(1)
        );
        assert_eq!(
            selection.subtitle.as_ref().map(|choice| choice.ordinal),
            Some(0)
        );
    }
    #[test]
    fn service_navigation_rejects_stale_pages_and_cross_profile_history() {
        {
            let mut app = app();
            app.plex.generation = 3;
            app.plex.items = vec![titled("a"), titled("b")];
            app.plex.total = 5;
            app.plex.loading_more = true;
            let page = |keys: &[&str]| {
                Ok(plex::Page {
                    items: keys.iter().map(|key| titled(key)).collect(),
                    total: 5,
                    posters: vec![],
                })
            };
            let _ = app.plex_event(Event::More(3, 1, page(&["x"])));
            let _ = app.plex_event(Event::More(2, 2, page(&["y"])));
            assert_eq!(app.plex.items.len(), 2);
            assert!(
                app.plex.loading_more,
                "more results append new titles and ignore stale offsets"
            );
            let _ = app.plex_event(Event::More(3, 2, page(&["b", "c", "d"])));
            let keys: Vec<_> = app
                .plex
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect();
            assert_eq!(keys, ["a", "b", "c", "d"]);
            assert!(
                !app.plex.loading_more,
                "more results append new titles and ignore stale offsets"
            );
        }
        {
            let mut app = app();
            app.plex.generation = 3;
            app.plex.connected = Some(plex::Connected {
                server: plex::Server::default(),
                libraries: vec![],
            });
            app.plex.library = Some(0);
            app.plex.selected = Some(titled("movie"));
            app.plex.parents = vec![titled("series")];
            app.plex.query = "query".into();
            let local = crate::model::Entry::File("local.mkv".into());
            app.playlist.replace(vec![local]);
            app.plex.visible = false;
            let _ = app.update(Message::TogglePlaylist);
            assert!(app.show_playlist);
            let _ = app.update(Message::Plex(Event::Open));
            assert!(
                !app.show_playlist,
                "entering a service closes the local queue"
            );
            let _ = app.update(Message::TogglePlaylist);
            assert!(
                !app.show_playlist,
                "service browsing refuses the queue shortcut"
            );
            app.plex.visible = false;
            app.plex.playing = Some(titled("movie"));
            let entry = crate::model::Entry::Url("https://example.com/movie".into());
            app.playlist.replace(vec![entry.clone()]);
            let _ = app.update(Message::TogglePlaylist);
            assert!(
                !app.show_playlist,
                "service playback refuses the local queue"
            );
            let generation = app.plex.generation;
            let history = || plex::Home {
                previews: vec![],
                activity: vec![plex::ActivityShelf {
                    section: plex::ActivitySection::Continue,
                    result: Ok(plex::Page {
                        items: vec![titled("movie")],
                        total: 1,
                        posters: vec![],
                    }),
                    visible_page: 0,
                }],
            };
            let _ = app.plex_event(Event::Home(generation, history()));
            assert_eq!(app.plex.activity.len(), 1);
            let _ = app.plex_event(Event::LeavePlayer);
            assert!(app.plex.visible && app.plex.selected.is_none());
            assert_eq!(app.plex.library, Some(0));
            assert_eq!(app.plex.parents[0].rating_key, "series");
            assert_eq!(app.plex.query, "query");
            assert_eq!(app.playlist.current(), Some(&entry));
            assert_eq!(app.plex.playing.as_ref().unwrap().rating_key, "movie");
            app.reset_browser();
            let _ = app.plex_event(Event::Home(generation, history()));
            assert!(app.plex.activity.is_empty());
            let _ = app.plex_event(Event::ActivityItem(
                plex::ActivitySection::Continue,
                0,
                true,
            ));
            assert!(app.plex.selected.is_none());
            assert_eq!(app.playlist.current(), Some(&entry));
        }
    }

    #[test]
    fn account_transitions_retire_old_playback_and_preserve_profile_authentication() {
        {
            let mut app = app();
            app.plex.generation = 4;
            app.plex.auth_link = "https://app.plex.tv/auth#?code=test".into();
            let _ = app.plex_event(Event::CancelSignIn);
            assert_eq!(app.plex.generation, 5);
            assert!(
                app.plex.auth_link.is_empty(),
                "cancel sign in invalidates poll and removes link"
            );
            let _ = app.plex_event(Event::Polled(
                4,
                Ok(Some(plex::Account {
                    token: Token::new("secret".into()),
                    name: "Old account".into(),
                    servers: vec![],
                    users: vec![],
                    ..Default::default()
                })),
            ));
            assert!(
                app.plex.account.is_none(),
                "cancel sign in invalidates poll and removes link"
            );
        }
        {
            let mut app = app();
            app.plex.playing = Some(plex::Item::default());
            app.plex.playing_url = Some("http://127.0.0.1/video?X-Plex-Token=secret".into());
            app.playlist.replace(vec![crate::model::Entry::Url(
                app.plex.playing_url.clone().unwrap(),
            )]);
            let _ = app.plex_event(Event::Switched(
                0,
                Ok(plex::Account {
                    token: Token::new("child-token".into()),
                    name: "Child".into(),
                    servers: vec![],
                    users: vec![],
                    ..Default::default()
                }),
            ));
            assert!(
                app.playlist.current().is_none(),
                "changing users stops the previous profiles stream"
            );
            assert!(
                app.plex.playing.is_none(),
                "changing users stops the previous profiles stream"
            );
            assert!(
                app.plex.playing_url.is_none(),
                "changing users stops the previous profiles stream"
            );
        }
        {
            let mut app = app();
            app.settings.plex_user = "child".into();
            app.plex.account = Some(plex::Account {
                token: Token::new("owner".into()),
                user_id: "owner".into(),
                users: vec![
                    plex::HomeUser {
                        id: "owner".into(),
                        ..Default::default()
                    },
                    plex::HomeUser {
                        id: "child".into(),
                        protected: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            });
            let _ = app.plex_after_account();
            assert!(
                !app.plex.profile_chosen && app.plex.connecting.is_none(),
                "remembered profiles still require authorization"
            );
            assert_eq!(app.plex.user, Some(1));
        }
    }
    #[test]
    fn leaving_episode_playback_preserves_the_season_listing_and_back_stack() {
        let mut app = app();
        app.plex.library = Some(2);
        app.plex.parents = vec![titled("show"), titled("season")];
        app.plex.items = vec![titled("episode-1"), titled("episode-2")];
        app.plex.offset = 24;
        app.plex.selected = Some(titled("episode-1"));
        app.plex.playing = Some(plex::Item {
            rating_key: "episode-1".into(),
            r#type: "episode".into(),
            parent_rating_key: "season".into(),
            grandparent_rating_key: "show".into(),
            ..Default::default()
        });
        let _ = app.plex_route(Event::LeavePlayer);
        assert!(app.plex.visible);
        assert!(app.plex.selected.is_none());
        assert_eq!(app.plex.library, Some(2));
        assert_eq!(app.plex.offset, 24);
        assert_eq!(app.plex.items[0].rating_key, "episode-1");
        assert_eq!(app.plex.parents.len(), 2);
        let _ = app.plex_route(Event::Back);
        assert_eq!(app.plex.parents.len(), 1);
        assert_eq!(app.plex.parents[0].rating_key, "show");
        app.plex.parents.clear();
        app.plex.library = None;
        let old_generation = app.plex.generation;
        let _ = app.plex_route(Event::LeavePlayer);
        assert_eq!(app.plex.parents.len(), 2);
        assert_eq!(app.plex.parents[1].rating_key, "season");
        assert!(app.plex.library.is_none());
        assert!(app.plex.selected.is_none());
        let _ = app.plex_route(Event::PlaybackPrepared(
            old_generation,
            true,
            Box::new(Err("retired playback".into())),
        ));
        assert!(app.plex.error.is_none());
        app.plex.playing = None;
        let _ = app.plex_route(Event::NextEpisode(
            app.load_token,
            "episode-1".into(),
            Box::new(Ok(Some(titled("stale")))),
        ));
        assert!(app.plex.next_episode.is_none());
    }

    #[test]
    fn plain_http_beyond_the_lan_waits_for_consent_before_connecting() {
        let mut app = app();
        app.plex.kind = Kind::Jellyfin;
        for local in [
            "192.168.1.10:8096",
            "http://[::1]:8096",
            "https://media.example.com",
        ] {
            let _ = app.plex_event(Event::ServerInput(local.into()));
            let _ = app.plex_event(Event::Connect);
            assert!(app.plex.insecure_address.is_none(), "{local}");
        }
        let _ = app.plex_event(Event::ServerInput("media.example.com:8096".into()));
        let _ = app.plex_event(Event::Connect);
        assert_eq!(
            app.plex.insecure_address,
            Some((Kind::Jellyfin, "media.example.com:8096".to_owned()))
        );
        assert!(app.plex.connecting.is_none());
        let _ = app.plex_event(Event::ConnectInsecure);
        assert!(app.plex.insecure_address.is_none());
        assert!(app.plex.connecting.is_some());
    }
}
