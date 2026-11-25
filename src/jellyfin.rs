//! Jellyfin and Emby: sign-in, catalog, artwork and playback reports.
//!
//! Both servers share one API lineage. Responses are mapped onto the Plex item
//! model so the library browser stays server neutral. Emby serves the API under
//! `/emby` and reads the client identity from `X-Emby-Authorization`.

use crate::plex::{
    Account, ActivitySection, Connected, HomeUser, Item, Kind, Library, Listing, Media, Part, Pin,
    Probe, Route, Server, Tag, Token,
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;
use url::Url;

const JSON_LIMIT: usize = 8 * 1024 * 1024;
const IMAGE_LIMIT: usize = 2 * 1024 * 1024;
const LIST_FIELDS: &str = "Overview,Genres,ProductionYear,OfficialRating,CommunityRating,RecursiveItemCount,ChildCount,MediaSources,ParentId";
pub(crate) const INCORRECT_PASSWORD: &str = "video-incorrect-password";
pub(crate) const SESSION_EXPIRED: &str = "video-session-expired";
// Quick Connect codes stay valid on the server for ten minutes.
const QUICK_CONNECT_SECONDS: u64 = 600;

pub(crate) struct Api<'a> {
    http: &'a reqwest::Client,
    client_id: &'a str,
    kind: Kind,
    base: Url,
    token: &'a Token,
}

impl<'a> Api<'a> {
    pub(crate) fn new(
        http: &'a reqwest::Client,
        client_id: &'a str,
        server: &'a Server,
    ) -> Result<Self, String> {
        Ok(Self {
            http,
            client_id,
            kind: server.kind,
            base: crate::plex::server_url(&server.address)?,
            token: &server.token,
        })
    }

    fn name(&self) -> &'static str {
        self.kind.name()
    }

    fn url(&self, path: &str, query: &[(&str, &str)]) -> Result<Url, String> {
        api_url(self.kind, &self.base, path, query)
    }

    fn request(&self, method: reqwest::Method, url: Url) -> reqwest::RequestBuilder {
        let identity = format!(
            "Client=\"enzUI Video\", Device=\"Linux\", DeviceId=\"{}\", Version=\"{}\"",
            self.client_id,
            env!("CARGO_PKG_VERSION")
        );
        let mut request = self
            .http
            .request(method, url)
            .header("Accept", "application/json");
        let token = self.token.value();
        match self.kind {
            Kind::Emby => {
                request = request.header("X-Emby-Authorization", format!("Emby {identity}"));
                if !token.is_empty() {
                    request = request.header("X-Emby-Token", sensitive(token));
                }
            }
            _ => {
                let mut value = format!("MediaBrowser {identity}");
                if !token.is_empty() {
                    let _ = write!(value, ", Token=\"{token}\"");
                }
                request = request.header("Authorization", sensitive(&value));
            }
        }
        request
    }

    async fn bytes(
        &self,
        request: reqwest::RequestBuilder,
        limit: usize,
    ) -> Result<Vec<u8>, String> {
        let name = self.name();
        let response = request.send().await.map_err(|_| {
            enzui_core::localized!("video-couldn-t-reach-name-check-the-address-and-network-then--5fbb0c", "name" => name)
        })?;
        match response.status().as_u16() {
            401 => return Err(SESSION_EXPIRED.into()),
            403 => return Err(enzui_core::localized!("video-access-denied", "server" => name)),
            200..=299 => (),
            300..=399 => {
                return Err(
                    enzui_core::localized!("video-name-redirected-this-request-use-the-server-s-final-add-85ec8e", "name" => name),
                );
            }
            code => {
                return Err(
                    enzui_core::localized!("video-name-returned-an-error-code-please-retry-b92a37", "name" => name, "code" => code),
                );
            }
        }
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(enzui_core::localized!("video-response-too-large", "server" => name));
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(
                |_| enzui_core::localized!("video-response-interrupted", "server" => name),
            )?;
            if chunk.len() > limit.saturating_sub(bytes.len()) {
                return Err(enzui_core::localized!("video-response-too-large", "server" => name));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, String> {
        let url = self.url(path, query)?;
        let bytes = self
            .bytes(self.request(reqwest::Method::GET, url), JSON_LIMIT)
            .await?;
        serde_json::from_slice(&bytes).map_err(
            |_| enzui_core::localized!("video-response-unexpected", "server" => self.name()),
        )
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, String> {
        let url = self.url(path, &[])?;
        let request = self.request(reqwest::Method::POST, url).json(body);
        let bytes = self.bytes(request, JSON_LIMIT).await?;
        serde_json::from_slice(&bytes).map_err(
            |_| enzui_core::localized!("video-response-unexpected", "server" => self.name()),
        )
    }

    pub(crate) async fn profiles(&self) -> Result<(Vec<HomeUser>, bool), String> {
        let users: Vec<UserDto> = self.get("/Users/Public", &[]).await?;
        let quick = if self.kind == Kind::Jellyfin {
            self.get::<bool>("/QuickConnect/Enabled", &[])
                .await
                .unwrap_or(false)
        } else {
            false
        };
        Ok((users.into_iter().map(UserDto::profile).collect(), quick))
    }

    // Signs in by name; a wrong password returns `INCORRECT_PASSWORD`.
    pub(crate) async fn sign_in(
        &self,
        server: &Server,
        username: &str,
        password: &str,
    ) -> Result<Account, String> {
        let url = self.url("/Users/AuthenticateByName", &[])?;
        let response = self
            .request(reqwest::Method::POST, url)
            .json(&serde_json::json!({ "Username": username, "Pw": password }))
            .send()
            .await
            .map_err(|_| enzui_core::localized!("video-couldn-t-reach-check-the-network-then-retry-64c134", "value0" => self.name()))?;
        match response.status().as_u16() {
            200..=299 => (),
            401 | 403 => return Err(INCORRECT_PASSWORD.into()),
            code => {
                return Err(
                    enzui_core::localized!("video-couldn-t-sign-in-code-please-retry-0c9366", "value0" => self.name(), "code" => code),
                );
            }
        }
        let body = response.bytes().await.map_err(
            |_| enzui_core::localized!("video-response-interrupted", "server" => self.name()),
        )?;
        let session: SessionDto = serde_json::from_slice(&body).map_err(
            |_| enzui_core::localized!("video-response-unexpected", "server" => self.name()),
        )?;
        Ok(session.account(server))
    }

    pub(crate) async fn quick_connect(&self) -> Result<Pin, String> {
        let quick: QuickDto = self
            .post("/QuickConnect/Initiate", &serde_json::Value::Null)
            .await?;
        Ok(Pin {
            id: 0,
            code: quick.code,
            expires_in: QUICK_CONNECT_SECONDS,
            auth_token: None,
            secret: quick.secret,
        })
    }

    pub(crate) async fn quick_connect_poll(
        &self,
        server: &Server,
        pin: &Pin,
    ) -> Result<Option<Account>, String> {
        let quick: QuickDto = self
            .get("/QuickConnect/Connect", &[("secret", pin.secret.as_str())])
            .await?;
        if !quick.authenticated {
            return Ok(None);
        }
        let session: SessionDto = self
            .post(
                "/Users/AuthenticateWithQuickConnect",
                &serde_json::json!({ "Secret": pin.secret }),
            )
            .await?;
        Ok(Some(session.account(server)))
    }

    pub(crate) async fn resume(&self, server: &Server) -> Result<Option<Account>, String> {
        let url = self.url(&format!("/Users/{}", segment(&server.user)?), &[])?;
        match self
            .bytes(self.request(reqwest::Method::GET, url), JSON_LIMIT)
            .await
        {
            Ok(bytes) => {
                let user: UserDto = serde_json::from_slice(&bytes).map_err(
                    |_| enzui_core::localized!("video-response-unexpected", "server" => self.name()),
                )?;
                let mut server = server.clone();
                server.user.clone_from(&user.id);
                Ok(Some(Account {
                    token: server.token.clone(),
                    name: user.name.clone(),
                    user_id: user.id.clone(),
                    thumb: user.thumb(),
                    servers: vec![server],
                    ..Account::default()
                }))
            }
            Err(error) if error == SESSION_EXPIRED => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn libraries(&self, server: &Server) -> Result<Connected, String> {
        let views: ItemsDto = self
            .get(&format!("/Users/{}/Views", server.user), &[])
            .await?;
        let libraries = views
            .items
            .into_iter()
            .filter_map(|view| {
                let r#type = match view.collection_type.as_str() {
                    "movies" => "movie",
                    "tvshows" => "show",
                    _ => return None,
                };
                Some(Library {
                    key: view.id,
                    title: view.name,
                    r#type: r#type.into(),
                })
            })
            .collect();
        Ok(Connected {
            server: Server {
                online: true,
                ..server.clone()
            },
            libraries,
        })
    }

    pub(crate) async fn probe(&self, route: Option<Route>) -> Probe {
        let started = std::time::Instant::now();
        let reached = tokio::time::timeout(
            Duration::from_secs(4),
            self.get::<serde_json::Value>("/System/Info/Public", &[]),
        )
        .await;
        match reached {
            Ok(Ok(_)) => Probe::Reachable {
                route,
                latency_ms: started.elapsed().as_millis() as u64,
            },
            _ => Probe::Unreachable,
        }
    }

    pub(crate) async fn items(
        &self,
        server: &Server,
        listing: &Listing,
        offset: usize,
        query: &str,
        limit: usize,
    ) -> Result<(Vec<Item>, usize), String> {
        let offset = offset.to_string();
        let limit = limit.to_string();
        let user = server.user.as_str();
        let mut params: Vec<(&str, &str)> = vec![
            ("Fields", LIST_FIELDS),
            ("EnableImageTypes", "Primary,Backdrop,Thumb"),
            ("ImageTypeLimit", "1"),
            ("StartIndex", &offset),
            ("Limit", &limit),
        ];
        if !query.is_empty() {
            params.push(("SearchTerm", query));
        }
        params.push(("EnableUserData", "true"));
        let response: ItemsDto = match listing {
            Listing::Activity(section) => {
                params.extend([
                    ("Recursive", "true"),
                    ("IncludeItemTypes", "Movie,Episode"),
                    ("SortBy", "DatePlayed"),
                    ("SortOrder", "Descending"),
                ]);
                if *section == ActivitySection::Continue && server.kind == Kind::Jellyfin {
                    self.get(&format!("/Users/{user}/Items/Resume"), &params)
                        .await?
                } else {
                    params.push((
                        "Filters",
                        if *section == ActivitySection::Continue {
                            "IsResumable"
                        } else {
                            "IsPlayed"
                        },
                    ));
                    self.get(&format!("/Users/{user}/Items"), &params).await?
                }
            }
            Listing::Library(key, _) | Listing::Recent(key) => {
                params.extend([
                    ("ParentId", key.as_str()),
                    ("Recursive", "true"),
                    ("IncludeItemTypes", "Movie,Series"),
                ]);
                if matches!(listing, Listing::Recent(_)) {
                    params.extend([
                        ("SortBy", "DateCreated,SortName"),
                        ("SortOrder", "Descending"),
                    ]);
                } else if let Listing::Library(_, sort) = listing {
                    let (by, order) = sort.jellyfin();
                    params.extend([("SortBy", by), ("SortOrder", order)]);
                }
                self.get(&format!("/Users/{user}/Items"), &params).await?
            }
            Listing::Children(key) => {
                params.extend([
                    ("ParentId", key.as_str()),
                    ("SortBy", "ParentIndexNumber,IndexNumber,SortName"),
                    ("SortOrder", "Ascending"),
                ]);
                self.get(&format!("/Users/{user}/Items"), &params).await?
            }
            Listing::Similar(key) => {
                params.push(("UserId", user));
                self.get(&format!("/Items/{}/Similar", segment(key)?), &params)
                    .await?
            }
        };
        let total = response.total_record_count.unwrap_or(response.items.len());
        Ok((
            response.items.into_iter().map(ItemDto::item).collect(),
            total,
        ))
    }

    pub(crate) async fn item(&self, server: &Server, id: &str) -> Result<Item, String> {
        let dto: ItemDto = self
            .get(
                &format!("/Users/{}/Items/{}", server.user, segment(id)?),
                &[],
            )
            .await?;
        Ok(dto.item())
    }

    pub(crate) async fn image(&self, path: &str, width: u32, height: u32) -> Option<Vec<u8>> {
        let width = width.to_string();
        let height = height.to_string();
        let url = self
            .url(
                path,
                &[
                    ("maxWidth", &width),
                    ("maxHeight", &height),
                    ("quality", "90"),
                ],
            )
            .ok()?;
        self.bytes(self.request(reqwest::Method::GET, url), IMAGE_LIMIT)
            .await
            .ok()
    }

    pub(crate) async fn playback(
        &self,
        server: &Server,
        id: &str,
    ) -> Result<
        (
            Item,
            crate::plex::playback::Selection,
            crate::plex::quality::ServerTracks,
            bool,
        ),
        String,
    > {
        use crate::plex::playback::{Choice, Selection};
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Playback {
            media_sources: Vec<SourceDto>,
        }
        let mut item = self.item(server, id).await?;
        let id = segment(id)?;
        let info: Playback = self
            .get(
                &format!("/Items/{id}/PlaybackInfo"),
                &[("UserId", &server.user)],
            )
            .await?;
        let source = info.media_sources.into_iter().next().ok_or_else(|| {
            enzui_core::localization::text(
                "video-this-item-has-no-playable-video-select-a-movie-or-episo-938ddf",
            )
        })?;
        let selected = |kind: &str, index: Option<i32>| {
            source
                .media_streams
                .iter()
                .filter(|stream| stream.r#type == kind && !stream.is_external)
                .enumerate()
                .find(|(_, stream)| Some(stream.index) == index)
                .map(|(ordinal, stream)| Choice {
                    ordinal: ordinal as i32,
                    language: stream.language.clone(),
                    title: stream.title.clone(),
                })
        };
        let audio = selected("Audio", source.default_audio_stream_index);
        let subtitle = selected("Subtitle", source.default_subtitle_stream_index);
        let external = source.media_streams.iter().any(|stream| {
            stream.r#type == "Subtitle"
                && stream.is_external
                && Some(stream.index) == source.default_subtitle_stream_index
        });
        let list = |kind: &str| {
            let mut ordinal = 0;
            source
                .media_streams
                .iter()
                .filter(|stream| stream.r#type == kind)
                .enumerate()
                .filter_map(|(position, stream)| {
                    let internal = !stream.is_external;
                    let track = crate::plex::quality::ServerTrack {
                        id: u64::try_from(stream.index).ok()?,
                        label: crate::plex::playback::track_label(
                            &stream.display_title,
                            &stream.title,
                            &stream.language,
                            position,
                        ),
                        language: stream.language.clone(),
                        title: stream.title.clone(),
                        ordinal: internal.then_some(ordinal),
                    };
                    if internal {
                        ordinal += 1;
                    }
                    Some(track)
                })
                .collect::<Vec<_>>()
        };
        let tracks = crate::plex::quality::ServerTracks {
            audio: list("Audio"),
            subtitles: list("Subtitle"),
            audio_choice: source
                .default_audio_stream_index
                .and_then(|index| u64::try_from(index).ok()),
            subtitle_choice: source
                .default_subtitle_stream_index
                .and_then(|index| u64::try_from(index).ok()),
        };
        if let Some(media) = item.media.first_mut() {
            media.source_id = source.id;
        }
        Ok((
            item,
            Selection {
                audio,
                subtitle,
                ..Default::default()
            },
            tracks,
            external,
        ))
    }

    /// Starts a scan of one library folder, or of every library.
    pub(crate) async fn scan(&self, library: Option<&str>) -> Result<(), String> {
        let url = match library {
            Some(id) => self.url(
                &format!("/Items/{}/Refresh", segment(id)?),
                &[
                    ("Recursive", "true"),
                    ("MetadataRefreshMode", "Default"),
                    ("ImageRefreshMode", "Default"),
                    ("ReplaceAllMetadata", "false"),
                    ("ReplaceAllImages", "false"),
                ],
            )?,
            None => self.url("/Library/Refresh", &[])?,
        };
        self.bytes(self.request(reqwest::Method::POST, url), JSON_LIMIT)
            .await
            .map(|_| ())
    }

    /// Ends the transcode `session` started from this device.
    pub(crate) async fn stop_transcode(&self, session: &str) -> Result<(), String> {
        let url = self.url(
            "/Videos/ActiveEncodings",
            &[("DeviceId", self.client_id), ("PlaySessionId", session)],
        )?;
        self.bytes(self.request(reqwest::Method::DELETE, url), JSON_LIMIT)
            .await
            .map(|_| ())
    }

    pub(crate) async fn report(
        &self,
        item: &Item,
        time_ms: u64,
        state: &str,
        session: crate::plex::PlaySession<'_>,
    ) -> Result<(), String> {
        let play_session_id = session.id;
        let ticks = time_ms.saturating_mul(10_000);
        let (path, body) = if state == "stopped" {
            (
                "/Sessions/Playing/Stopped",
                serde_json::json!({ "ItemId": item.rating_key, "PlaySessionId": play_session_id, "PositionTicks": ticks }),
            )
        } else {
            (
                "/Sessions/Playing/Progress",
                serde_json::json!({
                    "ItemId": item.rating_key, "PlaySessionId": play_session_id,
                    "PositionTicks": ticks,
                    "IsPaused": state == "paused",
                    "CanSeek": true,
                    "PlayMethod": if session.transcoding { "Transcode" } else { "DirectStream" },
                }),
            )
        };
        let url = self.url(path, &[])?;
        self.bytes(
            self.request(reqwest::Method::POST, url).json(&body),
            JSON_LIMIT,
        )
        .await
        .map(|_| ())
    }
}

fn api_url(kind: Kind, base: &Url, path: &str, query: &[(&str, &str)]) -> Result<Url, String> {
    let prefix = if kind == Kind::Emby { "/emby" } else { "" };
    let mut url = base.join(&format!("{prefix}{path}")).map_err(
        |_| enzui_core::localized!("video-invalid-address-8851a1", "value0" => kind.name()),
    )?;
    if url.origin() != base.origin() {
        return Err(
            enzui_core::localized!("video-returned-a-resource-on-a-different-server-22c8bf", "value0" => kind.name()),
        );
    }
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Ok(url)
}

// The direct-stream address of a movie or episode, carrying the token for the player.
pub(crate) fn stream(server: &Server, item: &Item) -> Result<String, String> {
    if item.media.is_empty() {
        return Err(enzui_core::localization::text(
            "video-this-item-has-no-playable-video-select-a-movie-or-episo-938ddf",
        ));
    }
    let id = segment(&item.rating_key)?;
    let mut params = vec![("static", "true"), ("api_key", server.token.value())];
    if let Some(source) = item
        .media
        .first()
        .filter(|media| !media.source_id.is_empty())
    {
        params.push(("MediaSourceId", source.source_id.as_str()));
    }
    let url = api_url(
        server.kind,
        &crate::plex::server_url(&server.address)?,
        &format!("/Videos/{id}/stream"),
        &params,
    )?;
    Ok(url.to_string())
}

/// The HLS transcode of a movie or episode at a `kbps` video cap, with the
/// chosen audio and burned-in subtitles; `session` names it for progress
/// reports and for stopping it.
pub(crate) fn transcode(
    server: &Server,
    item: &Item,
    quality: crate::plex::quality::Quality,
    kbps: u32,
    tracks: &crate::plex::quality::ServerTracks,
    session: &str,
    client_id: &str,
) -> Result<String, String> {
    if item.media.is_empty() {
        return Err(enzui_core::localization::text(
            "video-this-item-has-no-playable-video-select-a-movie-or-episo-938ddf",
        ));
    }
    let id = segment(&item.rating_key)?;
    let (width, height) = quality.max_size();
    let bitrate = (u64::from(kbps) * 1_000).to_string();
    let (width, height) = (width.to_string(), height.to_string());
    let audio = tracks.audio_choice.map(|index| index.to_string());
    let subtitle = tracks.subtitle_choice.map(|index| index.to_string());
    let mut params = vec![
        ("DeviceId", client_id),
        ("PlaySessionId", session),
        ("VideoCodec", "h264"),
        ("AudioCodec", "aac"),
        ("VideoBitrate", bitrate.as_str()),
        ("AudioBitrate", "192000"),
        ("MaxWidth", width.as_str()),
        ("MaxHeight", height.as_str()),
        ("TranscodingMaxAudioChannels", "2"),
        ("SegmentContainer", "ts"),
        ("SubtitleMethod", "Encode"),
    ];
    if let Some(source) = item
        .media
        .first()
        .filter(|media| !media.source_id.is_empty())
    {
        params.push(("MediaSourceId", source.source_id.as_str()));
    }
    if let Some(audio) = &audio {
        params.push(("AudioStreamIndex", audio));
    }
    if let Some(subtitle) = &subtitle {
        params.push(("SubtitleStreamIndex", subtitle));
    }
    // Jellyfin's HLS endpoints take only the newer key name; Emby the older.
    params.push((
        if server.kind == Kind::Emby {
            "api_key"
        } else {
            "ApiKey"
        },
        server.token.value(),
    ));
    let url = api_url(
        server.kind,
        &crate::plex::server_url(&server.address)?,
        &format!("/Videos/{id}/master.m3u8"),
        &params,
    )?;
    Ok(url.to_string())
}

pub(crate) async fn identify(
    http: &reqwest::Client,
    client_id: &str,
    address: &str,
) -> Result<Server, String> {
    let mut server = Server {
        address: origin(address)?,
        owned: true,
        online: true,
        ..Server::default()
    };
    for kind in [Kind::Jellyfin, Kind::Emby] {
        server.kind = kind;
        let api = Api::new(http, client_id, &server)?;
        let Ok(info) = tokio::time::timeout(
            Duration::from_secs(6),
            api.get::<InfoDto>("/System/Info/Public", &[]),
        )
        .await
        .unwrap_or_else(|_| Err(String::new())) else {
            continue;
        };
        if info.id.is_empty() {
            continue;
        }
        // Emby answers unprefixed paths too, but never names itself Jellyfin.
        let jellyfin = info.product_name.contains("Jellyfin");
        if kind == Kind::Jellyfin && !jellyfin {
            continue;
        }
        server.name = if info.server_name.is_empty() {
            kind.name().to_owned()
        } else {
            info.server_name
        };
        server.id = info.id;
        server.routes = vec![route_for(&server.address)];
        return Ok(server);
    }
    Err(enzui_core::localization::text(
        "video-this-address-doesn-t-answer-as-a-jellyfin-or-emby-serve-0bfeba",
    ))
}

fn origin(input: &str) -> Result<String, String> {
    let input = input.trim();
    let full = if input.contains("://") {
        input.to_owned()
    } else {
        format!("http://{input}")
    };
    let url = Url::parse(&full)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .ok_or_else(|| {
            enzui_core::localization::text(
                "video-enter-the-server-s-address-such-as-192-168-1-10-8096-7b1133",
            )
        })?;
    let origin = url.origin().ascii_serialization();
    crate::plex::server_url(&origin)?;
    Ok(origin)
}

// Tokens and passwords sent to such an address cross networks the user does not control in clear.
pub(crate) fn cleartext_beyond_lan(address: &str) -> bool {
    origin(address)
        .is_ok_and(|origin| origin.starts_with("http://") && route_for(&origin) == Route::Remote)
}

pub(crate) fn route_for(address: &str) -> Route {
    let local = Url::parse(address).ok().and_then(|url| match url.host()? {
        url::Host::Ipv4(ip) => Some(ip.is_private() || ip.is_loopback() || ip.is_link_local()),
        url::Host::Ipv6(ip) => {
            Some(ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local())
        }
        url::Host::Domain(name) => {
            Some(name == "localhost" || name.ends_with(".local") || name.ends_with(".lan"))
        }
    });
    if local.unwrap_or(false) {
        Route::Local
    } else {
        Route::Remote
    }
}

fn sensitive(value: &str) -> reqwest::header::HeaderValue {
    let mut value = reqwest::header::HeaderValue::from_str(value)
        .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("invalid"));
    value.set_sensitive(true);
    value
}

// An id used as one path segment; server ids are hexadecimal GUIDs.
fn segment(id: &str) -> Result<&str, String> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        Ok(id)
    } else {
        Err(enzui_core::localization::text(
            "video-the-server-returned-an-invalid-item-id-4e042b",
        ))
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct InfoDto {
    server_name: String,
    id: String,
    product_name: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct UserDto {
    id: String,
    name: String,
    has_password: bool,
    primary_image_tag: Option<String>,
    policy: PolicyDto,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct PolicyDto {
    is_administrator: bool,
}

impl UserDto {
    fn thumb(&self) -> String {
        match &self.primary_image_tag {
            Some(tag) if segment(&self.id).is_ok() => {
                format!("/Users/{}/Images/Primary#{tag}", self.id)
            }
            _ => String::new(),
        }
    }

    fn profile(self) -> HomeUser {
        HomeUser {
            thumb: self.thumb(),
            id: self.id,
            title: self.name,
            protected: self.has_password,
            admin: self.policy.is_administrator,
            restricted: false,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct SessionDto {
    access_token: String,
    user: UserDto,
}

impl SessionDto {
    fn account(self, server: &Server) -> Account {
        let token = Token::new(self.access_token);
        let server = Server {
            token: token.clone(),
            user: self.user.id.clone(),
            ..server.clone()
        };
        Account {
            token,
            name: self.user.name.clone(),
            user_id: self.user.id.clone(),
            thumb: self.user.thumb(),
            servers: vec![server],
            ..Account::default()
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct QuickDto {
    secret: String,
    code: String,
    authenticated: bool,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct ItemsDto {
    items: Vec<ItemDto>,
    total_record_count: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct ItemDto {
    id: String,
    name: String,
    r#type: String,
    collection_type: String,
    overview: String,
    taglines: Vec<String>,
    studios: Vec<NamedDto>,
    community_rating: Option<f32>,
    genres: Vec<String>,
    people: Vec<PersonDto>,
    production_year: Option<u32>,
    index_number: Option<u32>,
    run_time_ticks: Option<u64>,
    user_data: UserDataDto,
    recursive_item_count: Option<u32>,
    child_count: Option<u32>,
    official_rating: String,
    season_name: String,
    series_name: String,
    season_id: String,
    series_id: String,
    parent_id: String,
    image_tags: HashMap<String, String>,
    backdrop_image_tags: Vec<String>,
    parent_backdrop_item_id: String,
    parent_backdrop_image_tags: Vec<String>,
    media_sources: Vec<SourceDto>,
    container: String,
    height: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct NamedDto {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct PersonDto {
    id: String,
    name: String,
    role: String,
    r#type: String,
    primary_image_tag: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct UserDataDto {
    playback_position_ticks: u64,
    play_count: u32,
    played: bool,
    unplayed_item_count: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct SourceDto {
    id: String,
    container: String,
    media_streams: Vec<StreamDto>,
    default_audio_stream_index: Option<i32>,
    default_subtitle_stream_index: Option<i32>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct StreamDto {
    r#type: String,
    height: Option<u32>,
    index: i32,
    language: String,
    title: String,
    display_title: String,
    is_external: bool,
}

impl ItemDto {
    fn item(self) -> Item {
        let id_ok = segment(&self.id).is_ok();
        let r#type = match self.r#type.as_str() {
            "Movie" => "movie",
            "Series" => "show",
            "Season" => "season",
            "Episode" => "episode",
            "Video" => "clip",
            other => other,
        }
        .to_owned();
        let thumb = if id_ok && self.image_tags.contains_key("Primary") {
            format!("/Items/{}/Images/Primary", self.id)
        } else {
            String::new()
        };
        let art = if id_ok && !self.backdrop_image_tags.is_empty() {
            format!("/Items/{}/Images/Backdrop/0", self.id)
        } else if segment(&self.parent_backdrop_item_id).is_ok()
            && !self.parent_backdrop_image_tags.is_empty()
        {
            format!("/Items/{}/Images/Backdrop/0", self.parent_backdrop_item_id)
        } else {
            String::new()
        };
        let person = |person: &PersonDto| Tag {
            tag: person.name.clone(),
            role: person.role.clone(),
            thumb: match (&person.primary_image_tag, segment(&person.id)) {
                (Some(_), Ok(id)) => format!("/Items/{id}/Images/Primary"),
                _ => String::new(),
            },
        };
        let height = self
            .media_sources
            .first()
            .and_then(|source| {
                source
                    .media_streams
                    .iter()
                    .find(|stream| stream.r#type == "Video")?
                    .height
            })
            .or(self.height);
        let container = self
            .media_sources
            .first()
            .map(|source| source.container.clone())
            .filter(|container| !container.is_empty())
            .unwrap_or(self.container);
        let playable = matches!(r#type.as_str(), "movie" | "episode" | "clip");
        let leaf_count = match r#type.as_str() {
            "show" => self.recursive_item_count,
            "season" => self.child_count.or(self.recursive_item_count),
            _ => None,
        };
        let viewed_leaf_count = leaf_count
            .zip(self.user_data.unplayed_item_count)
            .map(|(total, unplayed)| total.saturating_sub(unplayed));
        let (parent_title, grandparent_title, parent_key, grandparent_key) = match r#type.as_str() {
            "episode" => (
                self.season_name,
                self.series_name,
                self.season_id,
                self.series_id,
            ),
            "season" => {
                let show = if self.series_id.is_empty() {
                    self.parent_id
                } else {
                    self.series_id
                };
                (self.series_name, String::new(), show, String::new())
            }
            _ => Default::default(),
        };
        Item {
            key: self.id.clone(),
            rating_key: self.id,
            title: self.name,
            r#type,
            summary: self.overview,
            thumb,
            art,
            tagline: self.taglines.into_iter().next().unwrap_or_default(),
            studio: self
                .studios
                .into_iter()
                .next()
                .map(|s| s.name)
                .unwrap_or_default(),
            audience_rating: self.community_rating,
            genres: self
                .genres
                .into_iter()
                .map(|tag| Tag {
                    tag,
                    ..Tag::default()
                })
                .collect(),
            directors: self
                .people
                .iter()
                .filter(|p| p.r#type == "Director")
                .map(person)
                .collect(),
            cast: self
                .people
                .iter()
                .filter(|p| p.r#type == "Actor" || p.r#type == "GuestStar")
                .map(person)
                .collect(),
            year: self.production_year,
            index: self.index_number,
            duration: self.run_time_ticks.unwrap_or(0) / 10_000,
            view_offset: self.user_data.playback_position_ticks / 10_000,
            view_count: if self.user_data.played {
                self.user_data.play_count.max(1)
            } else {
                0
            },
            leaf_count,
            viewed_leaf_count,
            content_rating: self.official_rating,
            parent_title,
            grandparent_title,
            parent_rating_key: parent_key,
            grandparent_rating_key: grandparent_key,
            library_section_id: None,
            media: if playable {
                vec![Media {
                    source_id: self
                        .media_sources
                        .first()
                        .map(|source| source.id.clone())
                        .unwrap_or_default(),
                    parts: vec![Part::default()],
                    container,
                    resolution: height.map(resolution).unwrap_or_default(),
                }]
            } else {
                vec![]
            },
        }
    }
}

fn resolution(height: u32) -> String {
    match height {
        2000.. => "4k".into(),
        1000.. => "1080".into(),
        700.. => "720".into(),
        470.. => "480".into(),
        _ => "sd".into(),
    }
}

pub(crate) fn discover_blocking(wait: Duration) -> Vec<crate::plex::LocalServer> {
    let probes: Vec<(Kind, std::net::UdpSocket)> = [
        (Kind::Jellyfin, &b"who is JellyfinServer?"[..]),
        (Kind::Emby, &b"who is EmbyServer?"[..]),
    ]
    .into_iter()
    .filter_map(|(kind, query)| {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.set_broadcast(true).ok()?;
        socket.set_nonblocking(true).ok()?;
        socket.send_to(query, "255.255.255.255:7359").ok()?;
        Some((kind, socket))
    })
    .collect();
    let started = std::time::Instant::now();
    let mut found: Vec<crate::plex::LocalServer> = Vec::new();
    let mut buffer = [0_u8; 2048];
    while started.elapsed() < wait && !probes.is_empty() {
        let mut idle = true;
        for (kind, socket) in &probes {
            if let Ok((length, _)) = socket.recv_from(&mut buffer) {
                idle = false;
                if let Some(server) = parse_discovery(*kind, &buffer[..length])
                    && !found.iter().any(|known| known.address == server.address)
                {
                    found.push(server);
                }
            }
        }
        if idle {
            std::thread::sleep(Duration::from_millis(40));
        }
    }
    found
}

fn parse_discovery(kind: Kind, reply: &[u8]) -> Option<crate::plex::LocalServer> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Reply {
        address: String,
        #[serde(default)]
        name: String,
    }
    let reply: Reply = serde_json::from_slice(reply).ok()?;
    let address = crate::plex::server_url(&reply.address)
        .or_else(|_| crate::plex::server_url(&format!("{}/", reply.address.trim_end_matches('/'))))
        .ok()?;
    Some(crate::plex::LocalServer {
        kind,
        name: if reply.name.is_empty() {
            kind.name().to_owned()
        } else {
            reply.name
        },
        address: address.as_str().trim_end_matches('/').to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_server_identifiers_and_timestamps_follow_the_provider_contract() {
        {
            let dto: ItemDto = serde_json::from_str(
                r#"{"Id":"../../x","Type":"Movie","ImageTags":{"Primary":"a"}}"#,
            )
            .unwrap();
            assert!(
                dto.item().thumb.is_empty(),
                "hostile ids never become paths"
            );
            assert!(segment("a/b").is_err(), "hostile ids never become paths");
        }
        {
            let dto: ItemDto = serde_json::from_str(
                r#"{"Id":"ep1","Name":"Pilot","Type":"Episode","RunTimeTicks":27000000000,
               "UserData":{"PlaybackPositionTicks":600000000}}"#,
            )
            .unwrap();
            let item = dto.item();
            assert_eq!(item.r#type, "episode");
            assert_eq!(item.duration, 2_700_000);
            assert_eq!(item.view_offset, 60_000);
        }
    }
}
