//! The added media servers: choosing a type to add, switching between them,
//! and the Jellyfin and Emby sign-in that shares the Plex screens.

use super::{App, Message};
use crate::plex::{self, Account, Event, Kind, Server, Source};
use iced::Task;

pub(crate) const PASSWORD_FIELD: &str = "source-password";
pub(crate) const USERNAME_FIELD: &str = "source-username";

impl App {
    pub(crate) fn active_source(&self) -> Option<&Source> {
        self.settings
            .sources
            .get(self.settings.source)
            .filter(|source| source.kind == self.plex.kind)
    }

    pub(crate) fn remembered_user(&self) -> String {
        if self.plex.kind == Kind::Plex {
            self.settings.plex_user.clone()
        } else {
            self.active_source()
                .map(|source| source.user.clone())
                .unwrap_or_default()
        }
    }

    pub(super) fn record_source(&mut self, server: &Server) {
        let source = Source {
            kind: server.kind,
            name: server.name.clone(),
            address: if server.kind == Kind::Plex {
                String::new()
            } else {
                server.address.clone()
            },
            id: if server.kind == Kind::Plex {
                String::new()
            } else {
                server.id.clone()
            },
            user: if server.kind == Kind::Plex {
                String::new()
            } else {
                server.user.clone()
            },
        };
        let sources = &mut self.settings.sources;
        let index = match sources.iter().position(|known| {
            known.kind == source.kind && (source.kind == Kind::Plex || known.id == source.id)
        }) {
            Some(index) => {
                sources[index] = source;
                index
            }
            None => {
                sources.push(source);
                sources.len() - 1
            }
        };
        self.settings.source = index;
    }

    pub(super) fn reset_browser(&mut self) {
        self.plex.advance();
        self.plex.busy = false;
        self.plex.connected = None;
        self.plex.connecting = None;
        self.plex.home.clear();
        self.plex.activity.clear();
        self.plex.shelves.clear();
        self.plex.backdrop = None;
        self.plex.library = None;
        self.plex.items.clear();
        self.plex.total = 0;
        self.plex.selected = None;
        self.plex.parents.clear();
        self.plex.posters.clear();
        self.plex.related = (String::new(), vec![]);
        self.plex.probes.clear();
    }

    // Frees the closed browser's artwork and pages while a local video plays; the
    // account and server stay, and the home view reloads when the page opens.
    pub(super) fn release_media_browser(&mut self) {
        if self.plex.visible {
            return;
        }
        self.plex.home.clear();
        self.plex.activity.clear();
        self.plex.shelves.clear();
        self.plex.backdrop = None;
        self.plex.library = None;
        self.plex.items.clear();
        self.plex.total = 0;
        self.plex.selected = None;
        self.plex.parents.clear();
        self.plex.posters.clear();
        self.plex.related = (String::new(), vec![]);
        self.plex.cast_photos = plex::cast::Photos::default();
    }

    fn leave_source(&mut self, kind: Kind) {
        self.reset_browser();
        if self.plex.pin.is_some() {
            self.plex.pin = None;
            self.plex.auth_link.clear();
        }
        if let Some(account) = self.plex.account.take() {
            self.plex.parked.insert(account.source_key(), account);
        }
        self.plex.kind = kind;
        self.plex.adding = false;
        self.plex.restoring = false;
        self.plex.profile_chosen = false;
        self.plex.user = None;
        self.plex.user_pin.clear();
        self.plex.pin_error = None;
        self.plex.manual = false;
        self.plex.error = None;
    }

    fn switch_source(&mut self, kind: Kind, index: Option<usize>) -> Task<Message> {
        self.leave_source(kind);
        if let Some(index) = index {
            self.settings.source = index;
        }
        let parked = index
            .and_then(|index| self.settings.sources.get(index))
            .map(Source::key)
            .or_else(|| (kind == Kind::Plex).then(|| "Plex".to_owned()))
            .and_then(|key| self.plex.parked.remove(&key));
        if let Some(account) = parked {
            self.plex.account = Some(account);
            return Task::batch([self.save_settings(), self.plex_after_account()]);
        }
        if kind == Kind::Plex {
            self.plex.restore_attempted = false;
            return Task::batch([self.save_settings(), self.plex_restore()]);
        }
        match index
            .and_then(|index| self.settings.sources.get(index))
            .cloned()
        {
            Some(source) => {
                self.plex.server_input.clone_from(&source.address);
                Task::batch([self.save_settings(), self.restore_source(source)])
            }
            None => {
                self.plex.server_input.clear();
                self.plex.local_servers = None;
                Self::plex_discover()
            }
        }
    }

    pub(super) fn restore_source(&mut self, source: Source) -> Task<Message> {
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        let generation = self.plex.advance();
        self.plex.restoring = true;
        crate::l10n::perform(plex::restore_source(client, source), move |result| {
            Message::Plex(Event::Restored(generation, result))
        })
    }

    fn identify(&mut self, address: String) -> Task<Message> {
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        let generation = self.plex.advance();
        self.plex
            .connecting
            .get_or_insert_with(|| plex::ConnectionProgress::Server(address.clone()));
        crate::l10n::perform(
            async move {
                let server = client.identify(&address).await?;
                let (users, quick_connect) = client.api(&server)?.profiles().await?;
                Ok(Account {
                    servers: vec![server],
                    users,
                    quick_connect,
                    ..Account::default()
                })
            },
            move |result| Message::Plex(Event::Identified(generation, result)),
        )
    }

    pub(super) fn connect_address(&mut self, kind: Kind, address: String) -> Task<Message> {
        self.plex.insecure_address = None;
        if crate::jellyfin::cleartext_beyond_lan(&address) {
            self.plex.connecting = None;
            self.plex.insecure_address = Some((kind, address));
            return Task::none();
        }
        self.connect_accepted_address(kind, address)
    }

    pub(super) fn connect_accepted_address(
        &mut self,
        kind: Kind,
        address: String,
    ) -> Task<Message> {
        if kind == Kind::Plex {
            let token = self
                .plex
                .account
                .as_ref()
                .map(|account| account.token.clone())
                .unwrap_or_default();
            return self.plex_connect(address, token);
        }
        if self.plex.kind != kind || self.plex.connected.is_some() {
            let connecting = self.plex.connecting.take();
            self.leave_source(kind);
            self.plex.connecting = connecting;
        }
        self.identify(address)
    }

    pub(super) fn source_sign_in(&mut self) -> Task<Message> {
        let Some(account) = self.plex.account.as_ref() else {
            return Task::none();
        };
        let Some(server) = account.servers.first().cloned() else {
            return Task::none();
        };
        let name = if self.plex.manual {
            self.plex.username.trim().to_owned()
        } else {
            match self.plex.user.and_then(|index| account.users.get(index)) {
                Some(user) => user.title.clone(),
                None => return Task::none(),
            }
        };
        if name.is_empty() {
            return Task::none();
        }
        let password = std::mem::take(&mut self.plex.user_pin);
        self.plex.pin_error = None;
        self.plex.connecting = Some(plex::ConnectionProgress::SigningIn(name.clone()));
        let generation = self.plex.advance();
        let Ok(client) = self.plex_client() else {
            return Task::none();
        };
        crate::l10n::perform(
            async move {
                client
                    .api(&server)?
                    .sign_in(&server, &name, &password)
                    .await
            },
            move |result| Message::Plex(Event::Switched(generation, result)),
        )
    }

    pub(super) fn source_route(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::AddSource => {
                self.plex.adding = true;
                self.plex.visible = true;
                self.plex.error = None;
                self.plex.local_servers = None;
                Self::plex_discover()
            }
            Event::CancelAdd => {
                self.plex.adding = false;
                if self.settings.sources.is_empty() && self.plex.connected.is_none() {
                    self.plex.visible = false;
                }
                self.activity()
            }
            Event::Choose(kind) => {
                let existing = (kind == Kind::Plex)
                    .then(|| {
                        self.settings
                            .sources
                            .iter()
                            .position(|s| s.kind == Kind::Plex)
                    })
                    .flatten();
                self.switch_source(kind, existing)
            }
            Event::UseSource(index) => match self.settings.sources.get(index) {
                Some(source)
                    if index == self.settings.source
                        && source.kind == self.plex.kind
                        && !self.plex.adding =>
                {
                    Task::none()
                }
                Some(source) => {
                    let kind = source.kind;
                    self.switch_source(kind, Some(index))
                }
                None => Task::none(),
            },
            Event::RemoveSource => {
                let Some(source) = self.active_source().cloned() else {
                    return Task::none();
                };
                self.settings.sources.retain(|known| known != &source);
                self.settings.source = 0;
                self.plex.parked.remove(&source.key());
                // The removed source's sign-in is forgotten, not parked.
                self.plex.account = None;
                let forget = if source.kind == Kind::Plex {
                    self.settings.plex_user.clear();
                    self.settings.plex_server_id.clear();
                    self.plex.home_token = plex::Token::default();
                    crate::l10n::perform(plex::forget_token(), |deleted| {
                        Message::Plex(Event::CredentialDeleted(deleted))
                    })
                } else {
                    crate::l10n::perform(plex::forget_source(source), |deleted| {
                        Message::Plex(Event::CredentialDeleted(deleted))
                    })
                };
                let next = match self.settings.sources.first() {
                    Some(next) => {
                        let kind = next.kind;
                        self.switch_source(kind, Some(0))
                    }
                    None => {
                        self.reset_browser();
                        self.plex.account = None;
                        self.source_route(Event::AddSource)
                    }
                };
                Task::batch([forget, next, self.save_settings()])
            }
            Event::Identified(generation, result) if generation == self.plex.generation => {
                self.plex.busy = false;
                self.plex.connecting = None;
                match result {
                    Ok(mut account) => {
                        // The same server reached again keeps a sign-in still held this session.
                        if let Some(mut parked) = self.plex.parked.remove(&account.source_key()) {
                            let address =
                                account.servers.first().map(|server| server.address.clone());
                            if let (Some(server), Some(address)) =
                                (parked.servers.first_mut(), address)
                            {
                                server.address = address;
                            }
                            parked.users = account.users;
                            parked.quick_connect = account.quick_connect;
                            account = parked;
                        }
                        if let Some(server) = account.servers.first() {
                            self.plex.kind = server.kind;
                            self.plex.server_input.clone_from(&server.address);
                            // A known server keeps its remembered profile.
                            if let Some(index) = self.settings.sources.iter().position(|source| {
                                source.kind == server.kind && source.id == server.id
                            }) {
                                self.settings.source = index;
                            }
                        }
                        self.plex.account = Some(account);
                        self.plex.profile_chosen = false;
                        self.plex_after_account()
                    }
                    Err(error) => {
                        self.plex.error = Some(error);
                        Task::none()
                    }
                }
            }
            Event::Username(value) => {
                self.plex.username = value;
                Task::none()
            }
            Event::Password(value) => {
                self.plex.pin_error = None;
                self.plex.user_pin = value;
                Task::none()
            }
            Event::ManualSignIn => {
                self.plex.manual = true;
                self.plex.user = None;
                self.plex.user_pin.clear();
                self.plex.pin_error = None;
                iced::widget::operation::focus(USERNAME_FIELD)
            }
            _ => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plex::Token;

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

    fn source(kind: Kind, id: &str) -> Source {
        Source {
            kind,
            name: id.to_uppercase(),
            address: format!("http://{id}.lan:8096"),
            id: id.into(),
            user: "u".into(),
        }
    }

    #[test]
    fn switching_sources_parks_the_signed_in_account() {
        let mut app = app();
        app.settings.sources = vec![source(Kind::Jellyfin, "den"), source(Kind::Emby, "attic")];
        app.plex.kind = Kind::Jellyfin;
        app.plex.account = Some(Account {
            token: Token::new("t".into()),
            user_id: "u".into(),
            servers: vec![Server {
                kind: Kind::Jellyfin,
                id: "den".into(),
                address: app.settings.sources[0].address.clone(),
                ..Default::default()
            }],
            ..Account::default()
        });
        app.plex.profile_chosen = true;
        let _ = app.plex_event(Event::UseSource(1));
        assert_eq!(app.plex.kind, Kind::Emby);
        assert!(
            app.plex.account.is_none() && app.plex.restoring,
            "switching sources parks the signed in account"
        );
        let _ = app.plex_event(Event::UseSource(0));
        assert_eq!(app.plex.kind, Kind::Jellyfin);
        assert_eq!(app.plex.account.as_ref().unwrap().token.value(), "t");
        assert!(
            app.plex.profile_chosen,
            "restoring this source retains its signed-in profile"
        );
    }
}
