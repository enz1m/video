pub(crate) mod artwork;
pub(crate) mod cast;
pub(crate) mod playback;
pub(crate) mod quality;

use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const PAGE_SIZE: usize = 24;
const SHELF_ITEMS: usize = 18;
const JSON_LIMIT: usize = 8 * 1024 * 1024;
const IMAGE_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Token(Arc<enzui_services::credential_store::Secret>);
impl Default for Token {
    fn default() -> Self {
        Self::new(String::new())
    }
}
impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token([redacted])")
    }
}
impl Token {
    pub(crate) fn new(value: String) -> Self {
        Self(Arc::new(enzui_services::credential_store::Secret::new(
            value.into_bytes(),
        )))
    }
    pub(crate) fn value(&self) -> &str {
        std::str::from_utf8(self.0.as_bytes()).expect("Token is constructed from UTF-8")
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.value().is_empty()
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Library {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) r#type: String,
}
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Item {
    pub(crate) rating_key: String,
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) r#type: String,
    pub(crate) summary: String,
    pub(crate) thumb: String,
    pub(crate) art: String,
    pub(crate) tagline: String,
    pub(crate) studio: String,
    pub(crate) audience_rating: Option<f32>,
    #[serde(rename = "Genre")]
    pub(crate) genres: Vec<Tag>,
    #[serde(rename = "Director")]
    pub(crate) directors: Vec<Tag>,
    #[serde(rename = "Role")]
    pub(crate) cast: Vec<Tag>,
    pub(crate) year: Option<u32>,
    pub(crate) index: Option<u32>,
    pub(crate) duration: u64,
    pub(crate) view_offset: u64,
    pub(crate) view_count: u32,
    pub(crate) leaf_count: Option<u32>,
    pub(crate) viewed_leaf_count: Option<u32>,
    pub(crate) content_rating: String,
    pub(crate) parent_title: String,
    pub(crate) grandparent_title: String,
    pub(crate) parent_rating_key: String,
    pub(crate) grandparent_rating_key: String,
    #[serde(rename = "librarySectionID")]
    pub(crate) library_section_id: Option<u64>,
    #[serde(rename = "Media")]
    pub(crate) media: Vec<Media>,
}
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub(crate) struct Tag {
    pub(crate) tag: String,
    pub(crate) role: String,
    pub(crate) thumb: String,
}
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub(crate) struct Media {
    #[serde(skip)]
    pub(crate) source_id: String,
    #[serde(rename = "Part")]
    pub(crate) parts: Vec<Part>,
    pub(crate) container: String,
    #[serde(rename = "videoResolution")]
    pub(crate) resolution: String,
}
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub(crate) struct Part {
    pub(crate) id: u64,
    pub(crate) key: String,
    #[serde(rename = "Stream")]
    pub(crate) streams: Vec<playback::Stream>,
}
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Container {
    total_size: Option<usize>,
    size: usize,
    friendly_name: String,
    machine_identifier: String,
    #[serde(rename = "Directory")]
    libraries: Vec<Library>,
    #[serde(rename = "Metadata")]
    items: Vec<Item>,
    #[serde(rename = "Hub")]
    hubs: Vec<Hub>,
}
#[derive(Debug, Clone, Deserialize, Default)]
struct Hub {
    #[serde(default, rename = "Metadata")]
    items: Vec<Item>,
}
#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "MediaContainer")]
    container: Container,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, Deserialize)]
pub(crate) enum Kind {
    #[default]
    Plex,
    Jellyfin,
    Emby,
}
impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Plex => "Plex",
            Self::Jellyfin => "Jellyfin",
            Self::Emby => "Emby",
        }
    }
}
// Plex keeps its account, profile and server in the Plex settings, so its
// source only records that Plex is in use and the last server's name.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Source {
    pub(crate) kind: Kind,
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) id: String,
    pub(crate) user: String,
}
impl Source {
    fn credential(&self) -> enzui_services::credential_store::CredentialKey {
        let service = if self.kind == Kind::Emby {
            "emby"
        } else {
            "jellyfin"
        };
        credential_key(service, &format!("{}/{}", self.id, self.user))
    }
}
#[derive(Debug, Clone, Default)]
pub(crate) struct Server {
    pub(crate) kind: Kind,
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) alternatives: Vec<String>,
    pub(crate) token: Token,
    pub(crate) id: String,
    pub(crate) owned: bool,
    pub(crate) owner: String,
    pub(crate) online: bool,
    // How each address reaches the server, in `Server::address` then alternatives order.
    pub(crate) routes: Vec<Route>,
    pub(crate) user: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Local,
    Remote,
    Relay,
}
impl Route {
    pub(crate) fn label(self) -> String {
        match self {
            Self::Local => enzui_core::localization::text("video-route-local"),
            Self::Remote => enzui_core::localization::text("video-route-remote"),
            Self::Relay => enzui_core::localization::text("video-route-relay"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probe {
    Reachable {
        route: Option<Route>,
        latency_ms: u64,
    },
    Unreachable,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalServer {
    pub(crate) kind: Kind,
    pub(crate) name: String,
    pub(crate) address: String,
}
pub(crate) const INCORRECT_PIN: &str = "video-incorrect-pin";
#[derive(Debug, Clone)]
pub(crate) struct Connected {
    pub(crate) server: Server,
    pub(crate) libraries: Vec<Library>,
}
#[derive(Debug, Clone)]
pub(crate) struct Page {
    pub(crate) items: Vec<Item>,
    pub(crate) total: usize,
    pub(crate) posters: Vec<(String, iced::widget::image::Handle)>,
}
#[derive(Debug, Clone)]
pub(crate) struct LibraryPreview {
    pub(crate) library: usize,
    pub(crate) page: Page,
    pub(crate) backdrop: Option<iced::widget::image::Handle>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivitySection {
    Continue,
    Watched,
}
impl ActivitySection {
    pub(crate) fn label(self) -> String {
        enzui_core::localization::text(match self {
            Self::Continue => "video-continue-watching",
            Self::Watched => "video-recently-watched",
        })
    }
}
#[derive(Debug, Clone)]
pub(crate) struct ActivityShelf {
    pub(crate) section: ActivitySection,
    pub(crate) result: Result<Page, String>,
    pub(crate) visible_page: usize,
}
#[derive(Debug, Clone)]
pub(crate) struct Home {
    pub(crate) previews: Vec<LibraryPreview>,
    pub(crate) activity: Vec<ActivityShelf>,
}
#[derive(Debug, Clone)]
pub(crate) struct Details {
    pub(crate) item: Item,
    pub(crate) backdrop: Option<iced::widget::image::Handle>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pin {
    pub(crate) id: u64,
    pub(crate) code: String,
    pub(crate) expires_in: u64,
    pub(crate) auth_token: Option<String>,
    // The Quick Connect secret on Jellyfin, which redeems the approved code.
    #[serde(skip)]
    pub(crate) secret: String,
}
impl fmt::Debug for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PlexPin([redacted])")
    }
}
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub(crate) struct HomeUser {
    #[serde(rename = "@id", alias = "id")]
    pub(crate) id: String,
    #[serde(rename = "@title", alias = "title")]
    pub(crate) title: String,
    #[serde(rename = "@protected", alias = "protected")]
    pub(crate) protected: bool,
    #[serde(rename = "@thumb", alias = "thumb")]
    pub(crate) thumb: String,
    #[serde(rename = "@admin", alias = "admin")]
    pub(crate) admin: bool,
    #[serde(rename = "@restricted", alias = "restricted")]
    pub(crate) restricted: bool,
}
#[derive(Debug, Clone, Default)]
pub(crate) struct Account {
    pub(crate) token: Token,
    pub(crate) name: String,
    // The Plex Home id of the profile this token belongs to.
    pub(crate) user_id: String,
    pub(crate) thumb: String,
    pub(crate) servers: Vec<Server>,
    pub(crate) users: Vec<HomeUser>,
    // The server approves sign-ins from another device with a code (Jellyfin Quick Connect).
    pub(crate) quick_connect: bool,
}
impl Account {
    pub(crate) fn source_key(&self) -> String {
        match self.servers.first() {
            Some(server) if server.kind != Kind::Plex => format!("{:?}:{}", server.kind, server.id),
            _ => "Plex".into(),
        }
    }
}
impl Source {
    pub(crate) fn key(&self) -> String {
        if self.kind == Kind::Plex {
            "Plex".into()
        } else {
            format!("{:?}:{}", self.kind, self.id)
        }
    }
}
/// How a library page is ordered. Each order runs newest or best first,
/// except the title order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub(crate) enum Sort {
    #[default]
    Title,
    Added,
    Released,
    Rating,
    Watched,
}
impl Sort {
    pub(crate) const ALL: [Self; 5] = [
        Self::Title,
        Self::Added,
        Self::Released,
        Self::Rating,
        Self::Watched,
    ];
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Title => "sort:title",
            Self::Added => "sort:added",
            Self::Released => "sort:released",
            Self::Rating => "sort:rating",
            Self::Watched => "sort:watched",
        }
    }
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sort| sort.id() == id)
    }
    pub(crate) fn label(self) -> String {
        enzui_core::localization::text(match self {
            Self::Title => "video-sort-title",
            Self::Added => "video-sort-added",
            Self::Released => "video-sort-released",
            Self::Rating => "video-sort-rating",
            Self::Watched => "video-sort-watched",
        })
    }
    // Shows order by their newest episode, as Plex's own apps do.
    fn plex(self) -> &'static str {
        match self {
            Self::Title => "titleSort",
            Self::Added => "episode.addedAt:desc",
            Self::Released => "originallyAvailableAt:desc",
            Self::Rating => "audienceRating:desc",
            Self::Watched => "lastViewedAt:desc",
        }
    }
    pub(crate) fn jellyfin(self) -> (&'static str, &'static str) {
        match self {
            Self::Title => ("SortName", "Ascending"),
            Self::Added => ("DateCreated,SortName", "Descending,Ascending"),
            Self::Released => (
                "PremiereDate,ProductionYear,SortName",
                "Descending,Descending,Ascending",
            ),
            Self::Rating => ("CommunityRating,SortName", "Descending,Ascending"),
            Self::Watched => ("DatePlayed,SortName", "Descending,Ascending"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Listing {
    Library(String, Sort),
    Children(String),
    Recent(String),
    Similar(String),
    Activity(ActivitySection),
}

#[derive(Clone)]
pub(crate) enum Event {
    Open,
    Close,
    AddSource,
    CancelAdd,
    Choose(Kind),
    UseSource(usize),
    RemoveSource,
    Identified(u64, Result<Account, String>),
    Username(String),
    Password(String),
    ManualSignIn,
    ChangeServer,
    ServerInput(String),
    Connect,
    Connected(u64, Result<Connected, String>),
    Home(u64, Home),
    ActivityItem(ActivitySection, usize, bool),
    ActivityPage(ActivitySection, bool),
    BackdropAllocated(
        u64,
        iced::advanced::image::Id,
        Result<iced::widget::image::Allocation, iced::widget::image::Error>,
    ),
    Preview(usize, usize),
    QuickPlay(Option<usize>, usize),
    Details(u64, Box<Result<Details, String>>),
    ContextDetails(u64, Box<Result<Details, String>>),
    Library(usize),
    Page(u64, Result<Page, String>),
    Search(String),
    SearchGo,
    ClearSearch,
    LoadMore,
    More(u64, usize, Result<Page, String>),
    ShelfPage(usize, bool),
    LeavePlayer,
    NextEpisode(u64, String, Box<Result<Option<Item>, String>>),
    Related(u64, String, Result<Page, String>),
    OpenRelated(usize),
    PlayRelated(usize),
    PlaybackPrepared(u64, bool, Box<Result<playback::Prepared, String>>),
    Sort(Sort),
    Quality(quality::Quality),
    ServerTracksSaved(Result<(), String>),
    Refresh,
    Scan,
    Scanned(String, Result<(), String>),
    // Jump from a detail or season page to its show (`false`) or season (`true`).
    Ancestor(bool),
    CastPhoto(String, Option<iced::widget::image::Handle>),
    Avatar(String, Option<iced::widget::image::Handle>),
    PinDigit(u8),
    PinBack,
    PinCancel,
    Probed(String, Probe),
    Discover,
    Discovered(Vec<LocalServer>),
    ConnectLocal(usize),
    ShowProfiles,
    ToggleAddress,
    Select(usize),
    Back,
    Play(bool),
    SignIn,
    Pin(u64, Result<Pin, String>),
    Poll(u64),
    Polled(u64, Result<Option<Account>, String>),
    CopyLink,
    Browser,
    CancelSignIn,
    BrowserOpened(Result<(), String>),
    Server(usize),
    User(usize),
    SwitchUser,
    Switched(u64, Result<Account, String>),
    SignOut,
    Progress,
    Timeline(Result<(), String>),
    ClipboardCopied(bool),
    Restored(u64, Result<Option<Account>, String>),
    CredentialSaved(bool),
    CredentialDeleted(bool),
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PlexEvent")
            .field(&std::mem::discriminant(self))
            .finish()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectionProgress {
    Server(String),
    Switching(String),
    SigningIn(String),
}

impl ConnectionProgress {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Server(target) => {
                enzui_core::localized!("video-connecting-to-target-313d2d", "target" => target)
            }
            Self::Switching(name) => {
                enzui_core::localized!("video-switching-to-e55100", "value0" => name)
            }
            Self::SigningIn(name) => {
                enzui_core::localized!("video-signing-in-as-name-060aa0", "name" => name)
            }
        }
    }
}

pub(crate) struct State {
    pub(crate) visible: bool,
    pub(crate) server_input: String,
    pub(crate) connected: Option<Connected>,
    pub(crate) library: Option<usize>,
    pub(crate) home: Vec<LibraryPreview>,
    pub(crate) activity: Vec<ActivityShelf>,
    pub(crate) artwork: artwork::BackdropCache,
    pub(crate) backdrop: Option<(String, iced::widget::image::Handle)>,
    pub(crate) items: Vec<Item>,
    pub(crate) total: usize,
    pub(crate) offset: usize,
    pub(crate) query: String,
    pub(crate) active_query: String,
    pub(crate) selected: Option<Item>,
    pub(crate) parents: Vec<Item>,
    pub(crate) posters: HashMap<String, iced::widget::image::Handle>,
    pub(crate) busy: bool,
    pub(crate) loading_more: bool,
    pub(crate) related: (String, Vec<Item>),
    pub(crate) cast_photos: cast::Photos,
    // The visible page of each home shelf, by library index.
    pub(crate) shelves: HashMap<usize, usize>,
    pub(crate) error: Option<String>,
    pub(crate) generation: u64,
    pub(crate) request: Option<iced::task::Handle>,
    pub(crate) pin: Option<Pin>,
    pub(crate) auth_link: String,
    pub(crate) auth_qr: Option<enzui_widgets::qr::EnzQrData>,
    pub(crate) auth_started: Option<std::time::Instant>,
    pub(crate) account: Option<Account>,
    pub(crate) home_token: Token,
    pub(crate) restore_attempted: bool,
    pub(crate) user: Option<usize>,
    pub(crate) user_pin: String,
    pub(crate) pin_error: Option<String>,
    pub(crate) restoring: bool,
    pub(crate) profile_chosen: bool,
    pub(crate) probes: HashMap<String, Probe>,
    pub(crate) connecting: Option<ConnectionProgress>,
    pub(crate) local_servers: Option<Vec<LocalServer>>,
    pub(crate) avatars: cast::Photos,
    pub(crate) manual_address: bool,
    pub(crate) kind: Kind,
    pub(crate) adding: bool,
    pub(crate) manual: bool,
    pub(crate) username: String,
    // Accounts of other sources, kept while one is browsed, by `Account::source_key`.
    pub(crate) parked: HashMap<String, Account>,
    pub(crate) playing_server: Option<Server>,
    pub(crate) playing: Option<Item>,
    pub(crate) playing_url: Option<String>,
    pub(crate) play_session_id: String,
    // The playing item's server tracks, and the session of the transcode
    // playing now (`None` while the original plays).
    pub(crate) server_tracks: quality::ServerTracks,
    pub(crate) transcode: Option<String>,
    // Transcodes left behind, to stop on their servers.
    pub(crate) stops: Vec<(Server, String)>,
    pub(crate) next_episode: Option<Item>,
    pub(crate) next_request: Option<iced::task::Handle>,
    pub(crate) auto_next: bool,
}
impl State {
    pub(crate) fn new(server: String) -> Self {
        Self {
            visible: false,
            server_input: server,
            connected: None,
            library: None,
            home: vec![],
            activity: vec![],
            artwork: artwork::BackdropCache::default(),
            backdrop: None,
            items: vec![],
            total: 0,
            offset: 0,
            query: String::new(),
            active_query: String::new(),
            selected: None,
            parents: vec![],
            posters: HashMap::new(),
            busy: false,
            loading_more: false,
            related: (String::new(), vec![]),
            cast_photos: cast::Photos::default(),
            shelves: HashMap::new(),
            error: None,
            generation: 0,
            request: None,
            pin: None,
            auth_link: String::new(),
            auth_qr: None,
            auth_started: None,
            account: None,
            home_token: Token::default(),
            restore_attempted: false,
            user: None,
            user_pin: String::new(),
            pin_error: None,
            restoring: false,
            profile_chosen: false,
            probes: HashMap::new(),
            connecting: None,
            local_servers: Some(vec![]),
            avatars: cast::Photos::default(),
            manual_address: false,
            kind: Kind::Plex,
            adding: false,
            manual: false,
            username: String::new(),
            parked: HashMap::new(),
            playing_server: None,
            playing: None,
            playing_url: None,
            play_session_id: String::new(),
            server_tracks: quality::ServerTracks::default(),
            transcode: None,
            stops: Vec::new(),
            next_episode: None,
            next_request: None,
            auto_next: false,
        }
    }
    pub(crate) fn advance(&mut self) -> u64 {
        if let Some(handle) = self.request.take() {
            handle.abort();
        }
        self.generation = self.generation.wrapping_add(1);
        self.busy = true;
        self.error = None;
        self.generation
    }
}

#[derive(Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    pub(crate) client_id: String,
}
impl Client {
    pub(crate) fn new(client_id: String) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| {
                enzui_core::localization::text("video-couldn-t-create-the-plex-connection-ee37d9")
            })?;
        Ok(Self { http, client_id })
    }
    pub(crate) fn api<'a>(
        &'a self,
        server: &'a Server,
    ) -> Result<crate::jellyfin::Api<'a>, String> {
        crate::jellyfin::Api::new(&self.http, &self.client_id, server)
    }
    pub(crate) async fn identify(&self, address: &str) -> Result<Server, String> {
        crate::jellyfin::identify(&self.http, &self.client_id, address).await
    }
    fn request(&self, method: reqwest::Method, url: Url, token: &Token) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .request(method, url)
            .header("Accept", "application/json")
            .header("X-Plex-Product", "enzUI Video")
            .header("X-Plex-Version", env!("CARGO_PKG_VERSION"))
            .header("X-Plex-Platform", "Linux")
            .header("X-Plex-Client-Identifier", &self.client_id);
        if !token.is_empty() {
            let mut value = reqwest::header::HeaderValue::from_str(token.value())
                .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("invalid"));
            value.set_sensitive(true);
            request = request.header("X-Plex-Token", value);
        }
        request
    }
    async fn bytes(
        &self,
        request: reqwest::RequestBuilder,
        limit: usize,
    ) -> Result<Vec<u8>, String> {
        let response = request.send().await.map_err(|_| {
            enzui_core::localization::text(
                "video-couldn-t-reach-plex-check-the-address-and-network-then--fa5d45",
            )
        })?;
        match response.status().as_u16() {
            401 | 403 => {
                return Err(enzui_core::localization::text(
                    "video-plex-denied-access-sign-in-with-an-account-that-can-use-05c6f3",
                ));
            }
            200..=299 => (),
            300..=399 => {
                return Err(enzui_core::localization::text(
                    "video-plex-redirected-this-request-select-the-server-s-final--5a9681",
                ));
            }
            code => {
                return Err(
                    enzui_core::localized!("video-plex-returned-an-error-code-please-retry-6405df", "code" => code),
                );
            }
        }
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(enzui_core::localization::text(
                "video-plex-s-response-is-too-large-32dd6e",
            ));
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| {
                enzui_core::localization::text("video-plex-s-response-was-interrupted-a1dd39")
            })?;
            if chunk.len() > limit.saturating_sub(bytes.len()) {
                return Err(enzui_core::localization::text(
                    "video-plex-s-response-is-too-large-32dd6e",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, String> {
        let bytes = self.bytes(request, JSON_LIMIT).await?;
        serde_json::from_slice(&bytes).map_err(|_| {
            enzui_core::localization::text("video-plex-returned-an-unexpected-response-c5ff91")
        })
    }
    pub(crate) async fn connect(&self, address: &str, token: Token) -> Result<Connected, String> {
        let base = server_url(address)?;
        let identity: Envelope = self
            .json(self.request(reqwest::Method::GET, endpoint(&base, "/")?, &token))
            .await?;
        if identity.container.machine_identifier.is_empty() {
            return Err(enzui_core::localization::text(
                "video-this-address-doesn-t-identify-a-plex-media-server-4f8285",
            ));
        }
        let libraries: Envelope = self
            .json(self.request(
                reqwest::Method::GET,
                endpoint(&base, "/library/sections")?,
                &token,
            ))
            .await?;
        let name = if identity.container.friendly_name.is_empty() {
            base.host_str().unwrap_or("Plex").to_owned()
        } else {
            identity.container.friendly_name
        };
        Ok(Connected {
            server: Server {
                name,
                address: base.as_str().trim_end_matches('/').into(),
                token,
                id: identity.container.machine_identifier.clone(),
                owned: true,
                online: true,
                ..Server::default()
            },
            libraries: libraries
                .container
                .libraries
                .into_iter()
                .filter(|l| matches!(l.r#type.as_str(), "movie" | "show"))
                .collect(),
        })
    }
    pub(crate) async fn connect_server(&self, server: Server) -> Result<Connected, String> {
        if server.kind != Kind::Plex {
            let api = self.api(&server)?;
            return tokio::time::timeout(Duration::from_secs(10), api.libraries(&server))
                .await
                .map_err(|_| enzui_core::localized!("video-this-server-didn-t-respond-3a5d59", "value0" => server.kind.name()))?;
        }
        let addresses: Vec<String> = std::iter::once(server.address.clone())
            .chain(server.alternatives.iter().cloned())
            .collect();
        let attempts = addresses.into_iter().enumerate().map(|(rank, address)| {
            let token = server.token.clone();
            Box::pin(async move {
                // Remote and relay routes start later so a reachable local address wins.
                tokio::time::sleep(Duration::from_millis(250 * rank.min(4) as u64)).await;
                tokio::time::timeout(Duration::from_secs(8), self.connect(&address, token))
                    .await
                    .map_err(|_| {
                        enzui_core::localization::text(
                            "video-this-plex-server-didn-t-respond-36f2a6",
                        )
                    })?
            })
        });
        let mut connected = futures_util::future::select_ok(attempts)
            .await
            .map(|(connected, _)| connected)
            .map_err(|_| {
                enzui_core::localization::text(
                    "video-couldn-t-reach-this-plex-server-check-that-it-s-running-599e2f",
                )
            })?;
        connected.server.owned = server.owned;
        connected.server.owner = server.owner;
        if !server.id.is_empty() {
            connected.server.id = server.id;
        }
        connected.server.routes = server.routes;
        connected.server.alternatives = std::iter::once(server.address)
            .chain(server.alternatives)
            .filter(|address| address != &connected.server.address)
            .collect();
        Ok(connected)
    }
    pub(crate) async fn probe(&self, server: &Server) -> Probe {
        if server.kind != Kind::Plex {
            return match self.api(server) {
                Ok(api) => api.probe(server.routes.first().copied()).await,
                Err(_) => Probe::Unreachable,
            };
        }
        let routes = std::iter::once(&server.address)
            .chain(&server.alternatives)
            .zip(
                server
                    .routes
                    .iter()
                    .copied()
                    .map(Some)
                    .chain(std::iter::repeat(None)),
            );
        let attempts = routes.map(|(address, route)| {
            let token = server.token.clone();
            let address = address.clone();
            Box::pin(async move {
                let started = std::time::Instant::now();
                let url = endpoint(&server_url(&address)?, "/identity")?;
                tokio::time::timeout(
                    Duration::from_secs(4),
                    self.bytes(self.request(reqwest::Method::GET, url, &token), JSON_LIMIT),
                )
                .await
                .map_err(|_| String::new())??;
                Ok::<_, String>(Probe::Reachable {
                    route,
                    latency_ms: started.elapsed().as_millis() as u64,
                })
            })
        });
        futures_util::future::select_ok(attempts)
            .await
            .map_or(Probe::Unreachable, |(probe, _)| probe)
    }
    // A profile picture: from plex.tv, which serves them without a token, or
    // from the Jellyfin or Emby `server` the profile belongs to.
    pub(crate) async fn avatar(
        &self,
        server: Option<Server>,
        thumb: String,
    ) -> Option<iced::widget::image::Handle> {
        if let Some(server) = server.filter(|server| server.kind != Kind::Plex) {
            let path = thumb.split('#').next().unwrap_or_default();
            let bytes = self
                .api(&server)
                .ok()?
                .image(path, cast::SIZE, cast::SIZE)
                .await?;
            return tokio::task::spawn_blocking(move || {
                decode_picture(&bytes, cast::SIZE, cast::SIZE)
            })
            .await
            .ok()?;
        }
        let url = Url::parse(&thumb).ok()?;
        let trusted = url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|host| host == "plex.tv" || host.ends_with(".plex.tv"));
        if !trusted {
            return None;
        }
        let directory = cast::directory();
        let cached = {
            let directory = directory.clone();
            let thumb = thumb.clone();
            tokio::task::spawn_blocking(move || cast::read(directory.as_deref()?, &thumb))
                .await
                .ok()
                .flatten()
        };
        let bytes = match cached {
            Some(bytes) => bytes,
            None => {
                let bytes = self.bytes(self.http.get(url), IMAGE_LIMIT).await.ok()?;
                if let Some(directory) = directory {
                    let bytes = bytes.clone();
                    let thumb = thumb.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        cast::write(&directory, &thumb, &bytes);
                    })
                    .await;
                }
                bytes
            }
        };
        tokio::task::spawn_blocking(move || decode_picture(&bytes, cast::SIZE, cast::SIZE))
            .await
            .ok()?
    }
    async fn ordered_children(&self, server: &Server, key: &str) -> Result<Vec<Item>, String> {
        let mut result = Vec::new();
        while result.len() < 4096 {
            let (items, total) = self
                .items(
                    server,
                    &Listing::Children(key.into()),
                    result.len(),
                    "",
                    PAGE_SIZE,
                )
                .await?;
            if items.is_empty() {
                break;
            }
            result.extend(items);
            if result.len() >= total {
                break;
            }
        }
        Ok(result)
    }

    pub(crate) async fn next_episode(
        &self,
        server: &Server,
        current: &Item,
    ) -> Result<Option<Item>, String> {
        if current.r#type != "episode" || current.parent_rating_key.is_empty() {
            return Ok(None);
        }
        let mut offset = 0;
        let mut found = false;
        while offset < 4096 {
            let (items, total) = self
                .items(
                    server,
                    &Listing::Children(current.parent_rating_key.clone()),
                    offset,
                    "",
                    PAGE_SIZE,
                )
                .await?;
            if items.is_empty() {
                break;
            }
            offset += items.len();
            for item in items {
                if found && item.r#type == "episode" {
                    return Ok(Some(item));
                }
                found |= item.rating_key == current.rating_key;
            }
            if offset >= total {
                break;
            }
        }
        if !found {
            return Ok(None);
        }
        if current.grandparent_rating_key.is_empty() {
            return Ok(None);
        }
        let seasons = self
            .ordered_children(server, &current.grandparent_rating_key)
            .await?;
        let Some(index) = seasons
            .iter()
            .position(|item| item.rating_key == current.parent_rating_key)
        else {
            return Ok(None);
        };
        for season in seasons[index + 1..]
            .iter()
            .filter(|item| item.r#type == "season")
            .take(32)
        {
            if let Some(next) = self
                .ordered_children(server, &season.rating_key)
                .await?
                .into_iter()
                .find(|item| item.r#type == "episode")
            {
                return Ok(Some(next));
            }
        }
        Ok(None)
    }

    pub(crate) async fn page(
        &self,
        server: &Server,
        listing: &Listing,
        offset: usize,
        query: &str,
    ) -> Result<Page, String> {
        let limit = if matches!(
            listing,
            Listing::Similar(_) | Listing::Recent(_) | Listing::Activity(_)
        ) {
            SHELF_ITEMS
        } else {
            PAGE_SIZE
        };
        let (items, total) = self.items(server, listing, offset, query, limit).await?;
        Ok(self.with_posters(server, items, total).await)
    }
    async fn items(
        &self,
        server: &Server,
        listing: &Listing,
        offset: usize,
        query: &str,
        limit: usize,
    ) -> Result<(Vec<Item>, usize), String> {
        let limit = limit.min(PAGE_SIZE);
        if server.kind != Kind::Plex {
            return self
                .api(server)?
                .items(server, listing, offset, query, limit)
                .await;
        }
        // Seasons and episodes keep their broadcast order.
        let (path, sort) = match listing {
            Listing::Activity(ActivitySection::Continue) => {
                ("/hubs/continueWatching".into(), "lastViewedAt:desc")
            }
            Listing::Activity(ActivitySection::Watched) => {
                ("/library/all".into(), "lastViewedAt:desc")
            }
            Listing::Library(key, sort) => (format!("/library/sections/{key}/all"), sort.plex()),
            Listing::Recent(key) => (format!("/library/sections/{key}/all"), "addedAt:desc"),
            Listing::Children(key) => (format!("/library/metadata/{key}/children"), "index"),
            Listing::Similar(key) => (format!("/library/metadata/{key}/similar"), "titleSort"),
        };
        let base = server_url(&server.address)?;
        let mut url = endpoint(&base, &path)?;
        url.query_pairs_mut()
            .append_pair("X-Plex-Container-Start", &offset.to_string())
            .append_pair("X-Plex-Container-Size", &limit.to_string())
            .append_pair("sort", sort);
        if !query.is_empty() {
            url.query_pairs_mut().append_pair("title", query);
        }
        if let Listing::Activity(section) = listing {
            match section {
                ActivitySection::Continue => {
                    url.query_pairs_mut()
                        .append_pair("count", &limit.to_string());
                }
                ActivitySection::Watched => {
                    url.query_pairs_mut()
                        .append_pair("unwatched", "0")
                        .append_pair("viewOffset", "0");
                }
            }
        }
        let response: Envelope = self
            .json(
                self.request(reqwest::Method::GET, url, &server.token)
                    .header("X-Plex-Container-Start", offset)
                    .header("X-Plex-Container-Size", limit),
            )
            .await?;
        let mut items = response.container.items;
        if matches!(listing, Listing::Activity(ActivitySection::Continue)) {
            items.extend(
                response
                    .container
                    .hubs
                    .into_iter()
                    .flat_map(|hub| hub.items),
            );
        }
        let total = response
            .container
            .total_size
            .unwrap_or(response.container.size);
        if matches!(listing, Listing::Activity(_)) {
            items.retain(|item| matches!(item.r#type.as_str(), "movie" | "episode"));
        }
        Ok((items.into_iter().take(limit).collect(), total))
    }
    async fn with_posters(&self, server: &Server, items: Vec<Item>, total: usize) -> Page {
        let pictures: Vec<_> = items
            .iter()
            .filter(|item| !item.thumb.is_empty())
            .take(PAGE_SIZE)
            .map(|item| {
                (
                    item.rating_key.clone(),
                    item.thumb.clone(),
                    item.r#type == "episode",
                )
            })
            .collect();
        let posters = futures_util::stream::iter(pictures)
            .map(|(key, thumb, episode)| async move {
                let (width, height) = if episode { (480, 270) } else { (240, 360) };
                let handle = self.picture(server, &thumb, width, height).await?;
                Some((key, handle))
            })
            .buffer_unordered(4)
            .filter_map(futures_util::future::ready)
            .collect()
            .await;
        Page {
            items,
            total,
            posters,
        }
    }
    async fn picture(
        &self,
        server: &Server,
        path: &str,
        width: u32,
        height: u32,
    ) -> Option<iced::widget::image::Handle> {
        if path.is_empty() {
            return None;
        }
        let bytes = if server.kind == Kind::Plex {
            let base = server_url(&server.address).ok()?;
            endpoint(&base, path).ok()?;
            let mut url = endpoint(&base, "/photo/:/transcode").ok()?;
            url.query_pairs_mut()
                .append_pair("url", path)
                .append_pair("width", &width.to_string())
                .append_pair("height", &height.to_string())
                .append_pair("minSize", "1")
                .append_pair("upscale", "0");
            self.bytes(
                self.request(reqwest::Method::GET, url, &server.token),
                IMAGE_LIMIT,
            )
            .await
            .ok()?
        } else {
            self.api(server).ok()?.image(path, width, height).await?
        };
        tokio::task::spawn_blocking(move || decode_picture(&bytes, width, height))
            .await
            .ok()?
    }
    pub(crate) async fn home(&self, connected: &Connected) -> Home {
        let previews = futures_util::stream::iter(
            connected
                .libraries
                .iter()
                .enumerate()
                .take(8)
                .map(|(index, library)| (index, library.key.clone()))
                .collect::<Vec<_>>(),
        )
        .map(|(library, key)| {
            let client = self.clone();
            let server = connected.server.clone();
            async move {
                let page = client
                    .page(&server, &Listing::Recent(key), 0, "")
                    .await
                    .ok()?;
                let backdrop =
                    if let Some(item) = page.items.iter().find(|item| !item.art.is_empty()) {
                        client.picture(&server, &item.art, 1200, 675).await
                    } else {
                        None
                    };
                Some(LibraryPreview {
                    library,
                    page,
                    backdrop,
                })
            }
        })
        .buffer_unordered(2)
        .filter_map(futures_util::future::ready)
        .collect();
        let activity =
            futures_util::stream::iter([ActivitySection::Continue, ActivitySection::Watched])
                .map(|section| async move {
                    ActivityShelf {
                        section,
                        result: self
                            .page(&connected.server, &Listing::Activity(section), 0, "")
                            .await,
                        visible_page: 0,
                    }
                })
                .buffered(2)
                .collect();
        let (previews, activity) = futures_util::join!(previews, activity);
        Home { previews, activity }
    }
    // `thumb` is a server path or an `https` URL; the server, not this client,
    // contacts any other host.
    pub(crate) async fn cast_photo(
        &self,
        server: &Server,
        thumb: String,
    ) -> Option<iced::widget::image::Handle> {
        let directory = cast::directory();
        // Jellyfin and Emby person paths are only unique per server.
        let cache_key = if server.kind == Kind::Plex {
            thumb.clone()
        } else {
            format!("{}{thumb}", server.address)
        };
        let cached = {
            let directory = directory.clone();
            let thumb = cache_key.clone();
            tokio::task::spawn_blocking(move || cast::read(directory.as_deref()?, &thumb))
                .await
                .ok()
                .flatten()
        };
        let bytes = match cached {
            Some(bytes) => bytes,
            None if server.kind != Kind::Plex => {
                let bytes = self
                    .api(server)
                    .ok()?
                    .image(&thumb, cast::SIZE, cast::SIZE)
                    .await?;
                if let Some(directory) = directory {
                    let bytes = bytes.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        cast::write(&directory, &cache_key, &bytes);
                    })
                    .await;
                }
                bytes
            }
            None => {
                let base = server_url(&server.address).ok()?;
                let external = Url::parse(&thumb)
                    .is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some());
                if !external && endpoint(&base, &thumb).is_err() {
                    return None;
                }
                let mut url = endpoint(&base, "/photo/:/transcode").ok()?;
                let size = cast::SIZE.to_string();
                url.query_pairs_mut()
                    .append_pair("url", &thumb)
                    .append_pair("width", &size)
                    .append_pair("height", &size)
                    .append_pair("minSize", "1")
                    .append_pair("upscale", "0");
                let bytes = self
                    .bytes(
                        self.request(reqwest::Method::GET, url, &server.token),
                        IMAGE_LIMIT,
                    )
                    .await
                    .ok()?
                    .clone();
                if let Some(directory) = directory {
                    let bytes = bytes.clone();
                    let thumb = thumb.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        cast::write(&directory, &thumb, &bytes);
                    })
                    .await;
                }
                bytes
            }
        };
        tokio::task::spawn_blocking(move || decode_picture(&bytes, cast::SIZE, cast::SIZE))
            .await
            .ok()?
    }
    pub(crate) async fn related(&self, server: &Server, item: &Item) -> Result<Page, String> {
        let listing = if item.r#type == "episode" && !item.parent_rating_key.is_empty() {
            Listing::Children(item.parent_rating_key.clone())
        } else {
            Listing::Similar(item.rating_key.clone())
        };
        self.page(server, &listing, 0, "").await
    }
    pub(crate) async fn details(&self, server: &Server, item: Item) -> Result<Details, String> {
        if server.kind != Kind::Plex {
            let item = self.api(server)?.item(server, &item.rating_key).await?;
            let backdrop = self.picture(server, &item.art, 1200, 675).await;
            return Ok(Details { item, backdrop });
        }
        let base = server_url(&server.address)?;
        let path = format!("/library/metadata/{}", item.rating_key);
        let response: Envelope = self
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
        let backdrop = self.picture(server, &item.art, 1200, 675).await;
        Ok(Details { item, backdrop })
    }
    pub(crate) async fn timeline(
        &self,
        server: &Server,
        item: &Item,
        time: u64,
        duration: u64,
        state: &str,
        session: PlaySession<'_>,
    ) -> Result<(), String> {
        if server.kind != Kind::Plex {
            return self.api(server)?.report(item, time, state, session).await;
        }
        let mut url = endpoint(&server_url(&server.address)?, "/:/timeline")?;
        url.query_pairs_mut()
            .append_pair("ratingKey", &item.rating_key)
            .append_pair("key", &item.key)
            .append_pair("time", &time.to_string())
            .append_pair("duration", &duration.to_string())
            .append_pair("state", state);
        self.bytes(
            self.request(reqwest::Method::GET, url, &server.token),
            JSON_LIMIT,
        )
        .await
        .map(|_| ())
    }
    /// Ends a transcode the server is running for this app.
    pub(crate) async fn stop_transcode(
        &self,
        server: &Server,
        session: &str,
    ) -> Result<(), String> {
        if server.kind != Kind::Plex {
            return self.api(server)?.stop_transcode(session).await;
        }
        let mut url = endpoint(
            &server_url(&server.address)?,
            "/video/:/transcode/universal/stop",
        )?;
        url.query_pairs_mut().append_pair("session", session);
        self.bytes(
            self.request(reqwest::Method::GET, url, &server.token),
            JSON_LIMIT,
        )
        .await
        .map(|_| ())
    }
    /// Asks the server to look for new and changed files in one library, or
    /// in all of them. The scan runs on the server after this returns.
    pub(crate) async fn scan(&self, server: &Server, library: Option<&str>) -> Result<(), String> {
        if server.kind != Kind::Plex {
            return self.api(server)?.scan(library).await;
        }
        let key = match library {
            Some(key) if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric()) => key,
            Some(_) => {
                return Err(enzui_core::localization::text(
                    "video-the-server-returned-an-invalid-item-id-4e042b",
                ));
            }
            None => "all",
        };
        let url = endpoint(
            &server_url(&server.address)?,
            &format!("/library/sections/{key}/refresh"),
        )?;
        self.bytes(
            self.request(reqwest::Method::GET, url, &server.token),
            JSON_LIMIT,
        )
        .await
        .map(|_| ())
    }
    /// Saves the chosen audio and subtitle streams (subtitle id 0 is off) as
    /// the account's selection for every part of the item, as Plex apps do.
    pub(crate) async fn select_streams(
        &self,
        server: &Server,
        part: u64,
        audio: Option<u64>,
        subtitle: Option<u64>,
    ) -> Result<(), String> {
        if server.kind != Kind::Plex {
            return Ok(());
        }
        let mut url = endpoint(
            &server_url(&server.address)?,
            &format!("/library/parts/{part}"),
        )?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("allParts", "1");
            if let Some(audio) = audio {
                query.append_pair("audioStreamID", &audio.to_string());
            }
            if let Some(subtitle) = subtitle {
                query.append_pair("subtitleStreamID", &subtitle.to_string());
            }
        }
        self.bytes(
            self.request(reqwest::Method::PUT, url, &server.token),
            JSON_LIMIT,
        )
        .await
        .map(|_| ())
    }
    pub(crate) async fn pin(&self) -> Result<Pin, String> {
        let mut url = Url::parse("https://plex.tv/api/v2/pins").map_err(|_| {
            enzui_core::localization::text("video-invalid-plex-sign-in-address-a0c097")
        })?;
        url.query_pairs_mut().append_pair("strong", "true");
        self.json(self.request(reqwest::Method::POST, url, &Token::default()))
            .await
    }
    pub(crate) async fn poll(&self, pin: &Pin) -> Result<Option<Account>, String> {
        let mut url =
            Url::parse(&format!("https://plex.tv/api/v2/pins/{}", pin.id)).map_err(|_| {
                enzui_core::localization::text("video-invalid-plex-sign-in-address-a0c097")
            })?;
        url.query_pairs_mut().append_pair("code", &pin.code);
        let pin: Pin = self
            .json(self.request(reqwest::Method::GET, url, &Token::default()))
            .await?;
        match pin.auth_token.filter(|t| !t.is_empty()) {
            Some(token) => self.account(Token::new(token)).await.map(Some),
            None => Ok(None),
        }
    }
    pub(crate) async fn account(&self, token: Token) -> Result<Account, String> {
        #[derive(Deserialize)]
        struct User {
            #[serde(default)]
            title: String,
            #[serde(default)]
            username: String,
            #[serde(default)]
            uuid: String,
            #[serde(default)]
            thumb: String,
        }
        let user: User = self
            .json(self.request(
                reqwest::Method::GET,
                Url::parse("https://plex.tv/api/v2/user").map_err(|_| {
                    enzui_core::localization::text("video-invalid-plex-account-address-08c38d")
                })?,
                &token,
            ))
            .await?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Resource {
            name: String,
            provides: String,
            access_token: Option<String>,
            #[serde(default)]
            client_identifier: String,
            #[serde(default)]
            owned: bool,
            #[serde(default)]
            source_title: Option<String>,
            #[serde(default)]
            presence: bool,
            #[serde(default)]
            connections: Vec<Connection>,
        }
        #[derive(Deserialize)]
        struct Connection {
            uri: String,
            #[serde(default)]
            local: bool,
            #[serde(default)]
            relay: bool,
        }
        let resources: Vec<Resource> = self
            .json(
                self.request(
                    reqwest::Method::GET,
                    Url::parse("https://clients.plex.tv/api/v2/resources?includeHttps=1&includeRelay=1&includeIPv6=1")
                        .map_err(|_| enzui_core::localization::text("video-invalid-plex-resources-address-9a4f43"))?,
                    &token,
                ),
            )
            .await?;
        let mut servers = vec![];
        for resource in resources
            .into_iter()
            .filter(|r| r.provides.split(',').any(|p| p == "server"))
        {
            let server_token = resource
                .access_token
                .map_or_else(|| token.clone(), Token::new);
            let mut connections = resource.connections;
            connections.sort_by_key(|c| (c.relay, !c.local, !c.uri.starts_with("https:")));
            let (mut addresses, routes): (Vec<String>, Vec<Route>) = connections
                .into_iter()
                .filter(|c| c.local || c.uri.starts_with("https:"))
                .filter_map(|c| {
                    let route = if c.relay {
                        Route::Relay
                    } else if c.local {
                        Route::Local
                    } else {
                        Route::Remote
                    };
                    server_url(&c.uri).ok().map(|url| (url.to_string(), route))
                })
                .unzip();
            if !addresses.is_empty() {
                let address = addresses.remove(0);
                servers.push(Server {
                    name: resource.name,
                    address,
                    alternatives: addresses,
                    token: server_token,
                    id: resource.client_identifier,
                    owned: resource.owned,
                    owner: resource.source_title.unwrap_or_default(),
                    online: resource.presence,
                    routes,
                    ..Server::default()
                });
            }
        }

        let users = self.home_users(&token).await.unwrap_or_default();
        servers.sort_by_key(|server| (!server.owned, !server.online));
        Ok(Account {
            token,
            name: if user.title.is_empty() {
                user.username
            } else {
                user.title
            },
            user_id: user.uuid,
            thumb: user.thumb,
            servers,
            users,
            quick_connect: false,
        })
    }
    async fn home_users(&self, token: &Token) -> Result<Vec<HomeUser>, String> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(default)]
            users: Vec<User>,
        }
        #[derive(Deserialize)]
        struct User {
            #[serde(default)]
            uuid: String,
            id: u64,
            #[serde(default)]
            title: String,
            #[serde(default)]
            protected: bool,
            #[serde(default)]
            thumb: String,
            #[serde(default)]
            admin: bool,
            #[serde(default)]
            restricted: bool,
        }
        let response: Response = self
            .json(self.request(
                reqwest::Method::GET,
                Url::parse("https://clients.plex.tv/api/v2/home/users").map_err(|_| {
                    enzui_core::localization::text("video-invalid-plex-home-address-ccafa2")
                })?,
                token,
            ))
            .await?;
        Ok(response
            .users
            .into_iter()
            .map(|user| HomeUser {
                id: if user.uuid.is_empty() {
                    user.id.to_string()
                } else {
                    user.uuid
                },
                title: user.title,
                protected: user.protected,
                thumb: user.thumb,
                admin: user.admin,
                restricted: user.restricted,
            })
            .collect())
    }
    pub(crate) async fn switch_user(
        &self,
        token: &Token,
        user: &HomeUser,
        pin: &str,
    ) -> Result<Account, String> {
        if user.id.is_empty()
            || !user
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(enzui_core::localization::text(
                "video-invalid-plex-user-20289b",
            ));
        }
        let mut url = Url::parse(&format!(
            "https://clients.plex.tv/api/v2/home/users/{}/switch",
            user.id
        ))
        .map_err(|_| enzui_core::localization::text("video-invalid-plex-home-address-ccafa2"))?;
        if !pin.is_empty() {
            url.query_pairs_mut().append_pair("pin", pin);
        }
        let response = self
            .request(reqwest::Method::POST, url, token)
            .send()
            .await
            .map_err(|_| {
                enzui_core::localization::text(
                    "video-couldn-t-reach-plex-check-the-network-then-retry-649b68",
                )
            })?;
        match response.status().as_u16() {
            401 | 403 => return Err(INCORRECT_PIN.into()),
            200..=299 => (),
            code => {
                return Err(
                    enzui_core::localized!("video-plex-couldn-t-switch-users-code-please-retry-e55296", "code" => code),
                );
            }
        }
        let body = response.bytes().await.map_err(|_| {
            enzui_core::localization::text("video-plex-s-response-was-interrupted-a1dd39")
        })?;
        if body.len() > JSON_LIMIT {
            return Err(enzui_core::localization::text(
                "video-plex-s-response-is-too-large-32dd6e",
            ));
        }
        let response: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
            enzui_core::localization::text("video-plex-returned-an-unexpected-response-c5ff91")
        })?;
        let token = response
            .get("authToken")
            .or_else(|| response.get("authenticationToken"))
            .or_else(|| {
                response
                    .get("user")
                    .and_then(|u| u.get("authToken").or_else(|| u.get("authenticationToken")))
            })
            .and_then(serde_json::Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                enzui_core::localization::text(
                    "video-couldn-t-switch-plex-users-check-the-pin-82395e",
                )
            })?;
        self.account(Token::new(token.to_owned())).await
    }
}

pub(crate) fn server_url(input: &str) -> Result<Url, String> {
    let url = Url::parse(input.trim())
        .map_err(|_| enzui_core::localization::text("video-enter-server-address"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(enzui_core::localization::text(
            "video-use-the-server-s-http-or-https-address-without-a-path-o-7fb92a",
        ));
    }
    Ok(url)
}
fn endpoint(base: &Url, path: &str) -> Result<Url, String> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\\')
        || path.contains('?')
        || path.contains('#')
    {
        return Err(enzui_core::localization::text(
            "video-plex-returned-an-unsafe-resource-path-9f8a77",
        ));
    }
    let url = base
        .join(path)
        .map_err(|_| enzui_core::localization::text("video-invalid-plex-resource-path-df44af"))?;
    if url.origin() != base.origin() {
        return Err(enzui_core::localization::text(
            "video-plex-returned-a-resource-on-a-different-server-067fb9",
        ));
    }
    Ok(url)
}
pub(crate) fn playback_url(server: &Server, item: &Item) -> Result<String, String> {
    if server.kind != Kind::Plex {
        return crate::jellyfin::stream(server, item);
    }
    let parts = item
        .media
        .first()
        .map(|m| m.parts.as_slice())
        .unwrap_or_default();
    if parts.len() != 1 {
        return Err(if parts.is_empty() {
            enzui_core::localization::text(
                "video-this-item-has-no-playable-video-select-a-movie-or-episo-938ddf",
            )
        } else {
            enzui_core::localization::text(
                "video-multipart-videos-aren-t-supported-yet-open-this-item-in-2c6cec",
            )
        });
    }
    let mut url = endpoint(&server_url(&server.address)?, &parts[0].key)?;
    if !server.token.is_empty() {
        url.query_pairs_mut()
            .append_pair("X-Plex-Token", server.token.value());
    }
    Ok(url.to_string())
}
/// The playback a progress report belongs to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaySession<'a> {
    pub(crate) id: &'a str,
    pub(crate) transcoding: bool,
}
pub(crate) fn auth_link(client_id: &str, pin: &Pin) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("clientID", client_id)
        .append_pair("code", &pin.code)
        .append_pair("context[device][product]", "enzUI Video")
        .finish();
    format!("https://app.plex.tv/auth#?{query}")
}
pub(crate) fn shelf_columns(width: f32) -> usize {
    ((width - 32.0) / 170.0).floor().clamp(2.0, 8.0) as usize
}
pub(crate) fn decode_picture(
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Option<iced::widget::image::Handle> {
    if !matches!(
        image::guess_format(bytes),
        Ok(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
    ) {
        return None;
    }
    let limits = enzui_core::decode_limits::DecodeLimits {
        max_encoded_bytes: IMAGE_LIMIT as u64,
        max_width: 2048,
        max_height: 2048,
        max_pixels: 2048 * 2048,
        max_rgba_bytes: 16 * 1024 * 1024,
    };
    enzui_resources::decode_static_resized(bytes, &limits, width, height, || false)
        .ok()
        .map(|asset| asset.handle)
}

fn credential_key(service: &str, account: &str) -> enzui_services::credential_store::CredentialKey {
    enzui_services::credential_store::CredentialKey::new("io.github.enzui.Video", service, account)
}
fn plex_credential() -> enzui_services::credential_store::CredentialKey {
    credential_key("plex", "account-token")
}
pub(crate) async fn discover_local() -> Vec<LocalServer> {
    let wait = Duration::from_millis(1500);
    let (plex, others) = futures_util::future::join(
        tokio::task::spawn_blocking(move || discover_local_blocking(wait)),
        tokio::task::spawn_blocking(move || crate::jellyfin::discover_blocking(wait)),
    )
    .await;
    let mut found = plex.unwrap_or_default();
    found.extend(others.unwrap_or_default());
    found
}
fn discover_local_blocking(wait: Duration) -> Vec<LocalServer> {
    let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") else {
        return vec![];
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
    let query = b"M-SEARCH * HTTP/1.1\r\n\r\n";
    for target in ["239.0.0.250:32414", "255.255.255.255:32414"] {
        let _ = socket.send_to(query, target);
    }
    let started = std::time::Instant::now();
    let mut found: Vec<LocalServer> = Vec::new();
    let mut buffer = [0_u8; 2048];
    while started.elapsed() < wait {
        let Ok((length, from)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        if let Some(server) = parse_gdm(&buffer[..length], from.ip())
            && !found.iter().any(|known| known.address == server.address)
        {
            found.push(server);
        }
    }
    found
}
fn parse_gdm(reply: &[u8], from: std::net::IpAddr) -> Option<LocalServer> {
    let text = std::str::from_utf8(reply).ok()?;
    if !text.starts_with("HTTP/1.0 200") && !text.starts_with("HTTP/1.1 200") {
        return None;
    }
    let field = |name: &str| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    if field("Content-Type").is_some_and(|kind| kind != "plex/media-server") {
        return None;
    }
    let port: u16 = field("Port")?.parse().ok()?;
    let host = match from {
        std::net::IpAddr::V4(ip) => ip.to_string(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
    };
    Some(LocalServer {
        kind: Kind::Plex,
        name: field("Name").unwrap_or_else(|| host.clone()),
        address: format!("http://{host}:{port}"),
    })
}
pub(crate) async fn restored_account(client: Client) -> Result<Option<Account>, String> {
    use enzui_services::credential_store::{
        CredentialOutcome, secret_service::SecretServiceTransport,
    };
    let store = SecretServiceTransport::connect_or_unavailable().await;
    // A keyring that is locked, missing or slow means no saved sign-in, not a failure.
    match tokio::time::timeout(Duration::from_secs(6), store.get(plex_credential())).await {
        Ok(CredentialOutcome::Retrieved(secret)) => {
            let token = String::from_utf8(secret.as_bytes().to_vec())
                .map_err(|_| enzui_core::localization::text("video-restore-invalid"))?;
            client.account(Token::new(token)).await.map(Some)
        }
        Ok(
            CredentialOutcome::NotFound
            | CredentialOutcome::Unavailable
            | CredentialOutcome::UnlockCancelled,
        )
        | Err(_) => Ok(None),
        _ => Err(enzui_core::localization::text("video-restore-failed")),
    }
}
struct CredentialVault {
    _owner: enzui_services::credential_store::mutation_owner::CredentialMutationOwner,
    store: enzui_services::credential_store::mutation_owner::CredentialMutationStore,
}
async fn credential_vault() -> Option<&'static CredentialVault> {
    static VAULT: tokio::sync::OnceCell<Option<CredentialVault>> =
        tokio::sync::OnceCell::const_new();
    VAULT.get_or_init(||async {enzui_services::credential_store::secret_service::SecretServiceTransport::connect_with_mutation_owner().await.ok().map(|(owner,store)|CredentialVault {_owner:owner,store})}).await.as_ref()
}
pub(crate) async fn remember_token(token: Token) -> bool {
    remember(plex_credential(), token).await
}
pub(crate) async fn forget_token() -> bool {
    forget(plex_credential()).await
}
pub(crate) async fn remember_source(source: Source, token: Token) -> bool {
    remember(source.credential(), token).await
}
pub(crate) async fn forget_source(source: Source) -> bool {
    forget(source.credential()).await
}
pub(crate) async fn restore_source(
    client: Client,
    source: Source,
) -> Result<Option<Account>, String> {
    use enzui_services::credential_store::{
        CredentialOutcome, secret_service::SecretServiceTransport,
    };
    let mut server = Server {
        kind: source.kind,
        name: source.name.clone(),
        address: source.address.clone(),
        id: source.id.clone(),
        owned: true,
        online: true,
        routes: vec![crate::jellyfin::route_for(&source.address)],
        ..Server::default()
    };
    let (users, quick_connect) = client.api(&server)?.profiles().await?;
    let mut account = Account {
        servers: vec![server.clone()],
        users,
        quick_connect,
        ..Account::default()
    };
    if source.user.is_empty() {
        return Ok(Some(account));
    }
    let store = SecretServiceTransport::connect_or_unavailable().await;
    let Ok(CredentialOutcome::Retrieved(secret)) =
        tokio::time::timeout(Duration::from_secs(6), store.get(source.credential())).await
    else {
        return Ok(Some(account));
    };
    let Ok(token) = String::from_utf8(secret.as_bytes().to_vec()) else {
        return Ok(Some(account));
    };
    server.token = Token::new(token);
    server.user = source.user;
    if let Some(resumed) = client.api(&server)?.resume(&server).await? {
        account = Account {
            users: account.users,
            quick_connect,
            ..resumed
        };
    }
    Ok(Some(account))
}
async fn remember(key: enzui_services::credential_store::CredentialKey, token: Token) -> bool {
    use enzui_services::credential_store::{CredentialOutcome, CredentialStore, Secret};
    let Some(vault) = credential_vault().await else {
        return false;
    };
    let store = &vault.store;
    matches!(
        tokio::time::timeout(
            Duration::from_secs(15),
            store.set(key, Secret::new(token.value().as_bytes().to_vec()))
        )
        .await,
        Ok(CredentialOutcome::Stored)
    )
}
async fn forget(key: enzui_services::credential_store::CredentialKey) -> bool {
    use enzui_services::credential_store::{CredentialOutcome, CredentialStore};
    let Some(vault) = credential_vault().await else {
        return false;
    };
    let store = &vault.store;
    matches!(
        tokio::time::timeout(Duration::from_secs(15), store.delete(key)).await,
        Ok(CredentialOutcome::Deleted)
    )
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(handle) = self.next_request.take() {
            handle.abort();
        }
        if let Some(handle) = self.request.take() {
            handle.abort();
        }
    }
}

pub(crate) async fn open_browser(link: String) -> Result<(), String> {
    use enzui_core::portal::{
        CancelReason, PortalOutcome,
        open_uri::{OpenUri, OpenUriRequest},
        parent::PortalParent,
    };
    let mut backend = enzui_core::portal::open_uri::desktop_launcher::host_open_uri_backend();
    let error = || enzui_core::localization::text("video-browser-failed");
    let uri = OpenUri::parse(link).map_err(|_| error())?;
    let mut pending = OpenUriRequest::new(uri, PortalParent::None)
        .start(backend.as_mut(), |_| true)
        .map_err(|_| error())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let completion = backend.open_uri_completion_signal();
        let generation = completion
            .as_ref()
            .map(enzui_core::portal::CompletionSignal::generation);
        if let Some(outcome) = pending.poll(backend.as_mut()) {
            return if matches!(outcome, PortalOutcome::Completed(())) {
                Ok(())
            } else {
                Err(error())
            };
        }
        if std::time::Instant::now() >= deadline {
            let _ = pending.cancel(CancelReason::Application);
            let _ = pending.settle(backend.as_mut());
            return Err(error());
        }
        if let (Some(signal), Some(generation)) = (completion, generation) {
            let _ = tokio::time::timeout(
                deadline.saturating_duration_since(std::time::Instant::now()),
                signal.wait_after(generation),
            )
            .await;
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    use std::io::{Read, Write};
    fn server(
        responses: Vec<(u16, String, &'static str)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let mut requests = vec![];
            for (status, headers, body) in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = vec![];
                let mut chunk = [0_u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut chunk).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                }
                let end = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let request_headers = String::from_utf8_lossy(&request[..end]);
                let length: usize = request_headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, value)| value.trim().parse().ok())
                    })
                    .unwrap_or(0);
                while request.len() < end + length {
                    let n = socket.read(&mut chunk).unwrap();
                    assert!(n > 0, "request body ended early");
                    request.extend_from_slice(&chunk[..n]);
                }
                requests.push(String::from_utf8(request).unwrap());
                write!(socket,"HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",body.len()).unwrap();
            }
            requests
        });
        (address, thread)
    }

    #[tokio::test]
    async fn plex_discovery_browsing_and_search_follow_the_provider_protocol() {
        {
            let (address, requests) = server(vec![
                (
                    200,
                    String::new(),
                    r#"{"MediaContainer":{"friendlyName":"Cinema","machineIdentifier":"server-id"}}"#,
                ),
                (
                    200,
                    String::new(),
                    r#"{"MediaContainer":{"Directory":[{"key":"1","title":"Movies","type":"movie"},{"key":"2","title":"TV","type":"show"},{"key":"3","title":"Music","type":"artist"}]}}"#,
                ),
            ]);
            let c = Client::new("stable-id".into()).unwrap();
            let connected = c.connect(&address, Token::default()).await.unwrap();
            assert_eq!(connected.server.name, "Cinema");
            assert_eq!(connected.libraries.len(), 2);
            let requests = requests.join().unwrap();
            assert!(
                requests[0].contains("x-plex-client-identifier: stable-id"),
                "anonymous connection discovers only video libraries"
            );
            assert!(
                !requests[0].contains("x-plex-token:"),
                "anonymous connection discovers only video libraries"
            );
        }
        {
            let (address, thread) = server(vec![(
                200,
                String::new(),
                r#"{"MediaContainer":{"size":1,"totalSize":50,"Metadata":[{"ratingKey":"12","title":"Film"}]}}"#,
            )]);
            let server = Server {
                address,
                alternatives: vec![],
                name: "Test".into(),
                token: Token::new("secret".into()),
                ..Default::default()
            };
            let page = Client::new("id".into())
                .unwrap()
                .page(
                    &server,
                    &Listing::Library("1".into(), Sort::Added),
                    24,
                    "a & b",
                )
                .await
                .unwrap();
            assert_eq!(page.total, 50);
            assert_eq!(page.items.len(), 1);
            let requests = thread.join().unwrap();
            assert!(
                requests[0].contains("X-Plex-Container-Start=24"),
                "paging search and auth use expected query and header"
            );
            assert!(
                requests[0].contains("title=a+%26+b"),
                "paging search and auth use expected query and header"
            );
            assert!(
                requests[0].contains("x-plex-token: secret"),
                "paging search and auth use expected query and header"
            );
            assert!(
                !requests[0].lines().next().unwrap().contains("secret"),
                "paging search and auth use expected query and header"
            );
            assert!(
                requests[0].contains("sort=episode.addedAt%3Adesc"),
                "the chosen order reaches the server"
            );
        }
        {
            // Scans start on the server for one library or all of them, and
            // keep the credential out of the request line.
            for kind in [Kind::Plex, Kind::Jellyfin, Kind::Emby] {
                let (address, thread) =
                    server(vec![(200, String::new(), ""), (204, String::new(), "")]);
                let server = Server {
                    kind,
                    address,
                    token: Token::new("secret".into()),
                    ..Default::default()
                };
                let client = Client::new("id".into()).unwrap();
                client.scan(&server, Some("1")).await.unwrap();
                client.scan(&server, None).await.unwrap();
                assert!(client.scan(&server, Some("../1")).await.is_err());
                let requests = thread.join().unwrap();
                let lines: Vec<&str> = requests
                    .iter()
                    .map(|request| request.lines().next().unwrap_or_default())
                    .collect();
                let expected = match kind {
                    Kind::Plex => [
                        "GET /library/sections/1/refresh ",
                        "GET /library/sections/all/refresh ",
                    ],
                    Kind::Jellyfin => ["POST /Items/1/Refresh?", "POST /Library/Refresh "],
                    Kind::Emby => ["POST /emby/Items/1/Refresh?", "POST /emby/Library/Refresh "],
                };
                for (line, expected) in lines.iter().zip(expected) {
                    assert!(line.starts_with(expected), "{line}");
                    assert!(!line.contains("secret"));
                }
            }
        }
    }
    #[tokio::test]
    async fn plex_requests_bound_responses_and_keep_credentials_on_the_selected_origin() {
        {
            let redirect_target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            redirect_target.set_nonblocking(true).unwrap();
            let redirect = format!(
                "Location: http://{}/\r\n",
                redirect_target.local_addr().unwrap()
            );
            for (status, headers, body) in [
                (401, String::new(), ""),
                (302, redirect, ""),
                (200, String::new(), "not json"),
            ] {
                let (address, thread) = server(vec![(status, headers, body)]);
                let error = Client::new("id".into())
                    .unwrap()
                    .connect(&address, Token::new("secret".into()))
                    .await
                    .unwrap_err();
                assert!(!error.is_empty(), "protocol failures need a diagnostic");
                assert!(
                    !error.contains("secret"),
                    "unauthorized redirect and invalid json have actionable errors"
                );
                thread.join().unwrap();
            }
            assert!(
                matches!(redirect_target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
                "the selected server must not redirect requests to another origin"
            );
        }
        {
            for value in [
                "file:///tmp/a",
                "http://user:pass@host:32400",
                "http://host:32400/?token=x",
            ] {
                assert!(server_url(value).is_err(), "invalid server {value}");
            }
            let base = server_url("http://127.0.0.1:32400").unwrap();
            for path in [
                "//example.com/secret",
                "https://example.com",
                "/\\evil/a",
                "/a?token=x",
            ] {
                assert!(endpoint(&base, path).is_err(), "unsafe resource {path}");
            }
            assert_eq!(
                endpoint(&base, "/library/parts/1/file.mkv")
                    .unwrap()
                    .host_str(),
                Some("127.0.0.1")
            );
            assert!(
                !format!("{:?}", Token::new("secret".into())).contains("secret"),
                "diagnostics redact credentials"
            );
            use image::ImageEncoder;
            for (width, accepted) in [(16, true), (2049, false)] {
                let mut encoded = Vec::new();
                image::codecs::png::PngEncoder::new(&mut encoded)
                    .write_image(
                        &vec![0; width as usize * 4],
                        width,
                        1,
                        image::ExtendedColorType::Rgba8,
                    )
                    .unwrap();
                assert_eq!(
                    decode_picture(&encoded, 16, 16).is_some(),
                    accepted,
                    "remote artwork dimension admission for {width}px"
                );
            }
            assert!(
                decode_picture(b"invalid image", 16, 16).is_none(),
                "malformed artwork is refused"
            );
        }
    }
    #[tokio::test]
    async fn plex_playback_preserves_profile_resume_and_stream_capabilities() {
        {
            for kind in [Kind::Plex, Kind::Jellyfin, Kind::Emby] {
                let responses = if kind == Kind::Plex {
                    vec![(
                        200,
                        String::new(),
                        r#"{"MediaContainer":{"Metadata":[{"ratingKey":"movie","type":"movie","Media":[{"Part":[{"key":"/video.mkv","Stream":[{"streamType":2,"languageCode":"eng"},{"streamType":2,"languageCode":"fra","selected":true},{"streamType":3,"key":"/captions.srt","selected":true}]}]}]}]}}"#,
                    )]
                } else {
                    vec![
                        (
                            200,
                            String::new(),
                            r#"{"Id":"movie","Type":"Movie","MediaSources":[{"Id":"stale-source"}]}"#,
                        ),
                        (
                            200,
                            String::new(),
                            r#"{"MediaSources":[{"Id":"mediasource_11","DefaultAudioStreamIndex":2,"DefaultSubtitleStreamIndex":3,"MediaStreams":[{"Type":"Video","Index":0},{"Type":"Audio","Index":1,"Language":"eng"},{"Type":"Audio","Index":2,"Language":"fra"},{"Type":"Subtitle","Index":3,"IsExternal":true,"DeliveryUrl":"/captions.srt"}]}]}"#,
                        ),
                    ]
                };
                let (address, thread) = server(responses);
                let server = Server {
                    kind,
                    address,
                    user: "active-profile".into(),
                    ..Default::default()
                };
                let prepared = Client::new("test-client".into())
                    .unwrap()
                    .prepare_playback(
                        server,
                        Item {
                            rating_key: "movie".into(),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
                let audio = prepared.selection.audio.as_ref().unwrap();
                assert_eq!((audio.ordinal, audio.language.as_str()), (1, "fra"));
                if kind != Kind::Plex {
                    assert!(prepared.url.contains("MediaSourceId=mediasource_11"));
                }
                assert!(prepared.warning.is_some());
                assert!(prepared.selection.subtitle.is_none());
                let requests = thread.join().unwrap();
                if kind != Kind::Plex {
                    assert!(requests[1].contains("UserId=active-profile"));
                }
            }
        }
        {
            // A track change made in the player is saved back to the server's
            // ids for the same part, in player track order.
            let (address, thread) = server(vec![
                (
                    200,
                    String::new(),
                    r#"{"MediaContainer":{"Metadata":[{"ratingKey":"episode","type":"episode","Media":[{"Part":[{"id":19,"key":"/video.mkv","Stream":[{"id":1,"streamType":1},{"id":2,"streamType":2,"languageCode":"hun"},{"id":3,"streamType":2,"languageCode":"eng","selected":true},{"id":4,"streamType":3,"languageCode":"hun"},{"id":5,"streamType":3,"key":"/library/streams/5","languageCode":"eng"},{"id":6,"streamType":3,"languageCode":"eng"}]}]}]}]}}"#,
                ),
                (200, String::new(), ""),
            ]);
            let server = Server {
                kind: Kind::Plex,
                address,
                token: Token::new("secret".into()),
                ..Default::default()
            };
            let client = Client::new("id".into()).unwrap();
            let prepared = client
                .prepare_playback(
                    server.clone(),
                    Item {
                        rating_key: "episode".into(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            let streams = prepared.selection.streams.unwrap();
            assert_eq!(
                streams,
                playback::PartStreams {
                    part: 19,
                    audio: vec![2, 3],
                    subtitles: vec![4, 6],
                }
            );
            let track = |index| enzui_services::media::Track {
                index,
                language: None,
                title: None,
                codec: None,
            };
            let status = enzui_services::media::VideoStatus {
                audio_tracks: vec![track(0), track(1)],
                subtitle_tracks: vec![track(0), track(1)],
                ..Default::default()
            };
            assert_eq!(
                streams.ids(&status, None, Some(Some(1))),
                Some((None, Some(6)))
            );
            assert_eq!(
                streams.ids(&status, Some(0), Some(None)),
                Some((Some(2), Some(0)))
            );
            let mismatched = enzui_services::media::VideoStatus {
                subtitle_tracks: vec![track(0)],
                ..status.clone()
            };
            assert_eq!(
                streams.ids(&mismatched, None, Some(Some(0))),
                None,
                "tracks that do not line up with the server are not saved"
            );
            client
                .select_streams(&server, streams.part, None, Some(6))
                .await
                .unwrap();
            let requests = thread.join().unwrap();
            assert!(
                requests[1].starts_with("PUT /library/parts/19?allParts=1&subtitleStreamID=6 "),
                "{}",
                requests[1].lines().next().unwrap_or_default()
            );
            assert!(
                !requests[1]
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .contains("secret")
            );
        }
        {
            for kind in [Kind::Plex, Kind::Jellyfin, Kind::Emby] {
                let (resume, watched) = if kind == Kind::Plex {
                    (
                        r#"{"MediaContainer":{"Hub":[{"Metadata":[{"ratingKey":"resume","type":"movie","viewOffset":60000}]}]}}"#,
                        r#"{"MediaContainer":{"Metadata":[{"ratingKey":"watched","type":"episode","viewCount":1}]}}"#,
                    )
                } else {
                    (
                        r#"{"Items":[{"Id":"resume","Type":"Movie","MediaSources":[{"Id":"opaque-source","Container":"mkv"}],"UserData":{"PlaybackPositionTicks":600000000}}]}"#,
                        r#"{"Items":[{"Id":"watched","Type":"Episode","UserData":{"Played":true,"PlayCount":1}}]}"#,
                    )
                };
                let (address, thread) = server(vec![
                    (200, String::new(), resume),
                    (200, String::new(), watched),
                    (204, String::new(), ""),
                    (204, String::new(), ""),
                ]);
                let server = Server {
                    kind,
                    address,
                    user: "profile".into(),
                    token: Token::new("secret".into()),
                    ..Default::default()
                };
                let client = Client::new("id".into()).unwrap();
                let resume = client
                    .page(
                        &server,
                        &Listing::Activity(ActivitySection::Continue),
                        0,
                        "",
                    )
                    .await
                    .unwrap();
                let watched = client
                    .page(&server, &Listing::Activity(ActivitySection::Watched), 0, "")
                    .await
                    .unwrap();
                assert_eq!(
                    (
                        resume.items[0].rating_key.as_str(),
                        resume.items[0].view_offset
                    ),
                    ("resume", 60_000)
                );
                assert_eq!(watched.items[0].rating_key, "watched");
                if kind != Kind::Plex {
                    let stream =
                        Url::parse(&playback_url(&server, &resume.items[0]).unwrap()).unwrap();
                    assert_eq!(
                        stream
                            .query_pairs()
                            .find(|(key, _)| key == "MediaSourceId")
                            .unwrap()
                            .1,
                        "opaque-source"
                    );
                }
                client
                    .timeline(
                        &server,
                        &resume.items[0],
                        90_000,
                        600_000,
                        "paused",
                        PlaySession {
                            id: "test-play-session",
                            transcoding: false,
                        },
                    )
                    .await
                    .unwrap();
                client
                    .timeline(
                        &server,
                        &resume.items[0],
                        91_000,
                        600_000,
                        "stopped",
                        PlaySession {
                            id: "test-play-session",
                            transcoding: false,
                        },
                    )
                    .await
                    .unwrap();
                let requests = thread.join().unwrap();
                if kind != Kind::Plex {
                    for request in &requests[2..] {
                        let body: serde_json::Value =
                            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1)
                                .unwrap();
                        assert_eq!(body["PlaySessionId"], "test-play-session");
                        assert_eq!(body["ItemId"], "resume");
                    }
                }
                for (index, request) in requests.iter().take(2).enumerate() {
                    let target = request
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap();
                    assert!(!target.contains("secret"));
                    assert!(request.contains("secret"));
                    let url = Url::parse(&format!("http://fixture{target}")).unwrap();
                    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
                    match kind {
                        Kind::Plex if index == 0 => {
                            assert_eq!(url.path(), "/hubs/continueWatching");
                        }
                        Kind::Plex => {
                            assert_eq!(query.get("sort").unwrap(), "lastViewedAt:desc");
                            assert_eq!(query.get("unwatched").unwrap(), "0");
                        }
                        Kind::Jellyfin if index == 0 => {
                            assert_eq!(url.path(), "/Users/profile/Items/Resume");
                        }
                        _ => {
                            assert_eq!(
                                url.path(),
                                if kind == Kind::Emby {
                                    "/emby/Users/profile/Items"
                                } else {
                                    "/Users/profile/Items"
                                }
                            );
                            assert_eq!(
                                query.get("Filters").unwrap(),
                                if index == 0 {
                                    "IsResumable"
                                } else {
                                    "IsPlayed"
                                }
                            );
                            assert_eq!(query.get("SortBy").unwrap(), "DatePlayed");
                        }
                    }
                }
            }
        }
        {
            let server = Server {
                name: "Test".into(),
                alternatives: vec![],
                address: "http://127.0.0.1:32400".into(),
                token: Token::new("a&b".into()),
                ..Default::default()
            };
            let mut item = Item::default();
            assert!(
                playback_url(&server, &item).is_err(),
                "unplayable items are refused"
            );
            item.media.push(Media {
                parts: vec![Part {
                    key: "/library/parts/1/file.mkv".into(),
                    ..Default::default()
                }],
                ..Default::default()
            });
            let url = playback_url(&server, &item).unwrap();
            assert!(
                url.contains("X-Plex-Token=a%26b"),
                "media transport encodes credentials"
            );
            assert_eq!(
                crate::model::Entry::Url(url).name(),
                "file.mkv",
                "display names exclude transport credentials"
            );
            item.media[0].parts.push(Part::default());
            assert!(
                playback_url(&server, &item).is_err(),
                "single part playback and multi part refusal"
            );
        }
    }
    #[tokio::test]
    async fn next_episode_follows_provider_order_across_seasons_and_stops_at_series_end() {
        for kind in [Kind::Plex, Kind::Jellyfin, Kind::Emby] {
            let (episodes, seasons, following) = if kind == Kind::Plex {
                (
                    r#"{"MediaContainer":{"totalSize":2,"Metadata":[{"ratingKey":"e1","type":"episode"},{"ratingKey":"e2","type":"episode"}]}}"#,
                    r#"{"MediaContainer":{"totalSize":2,"Metadata":[{"ratingKey":"s1","type":"season"},{"ratingKey":"s2","type":"season"}]}}"#,
                    r#"{"MediaContainer":{"totalSize":1,"Metadata":[{"ratingKey":"e3","type":"episode"}]}}"#,
                )
            } else {
                (
                    r#"{"TotalRecordCount":2,"Items":[{"Id":"e1","Type":"Episode"},{"Id":"e2","Type":"Episode"}]}"#,
                    r#"{"TotalRecordCount":2,"Items":[{"Id":"s1","Type":"Season"},{"Id":"s2","Type":"Season"}]}"#,
                    r#"{"TotalRecordCount":1,"Items":[{"Id":"e3","Type":"Episode"}]}"#,
                )
            };
            let (address, requests) = server(vec![
                (200, String::new(), episodes),
                (200, String::new(), episodes),
                (200, String::new(), seasons),
                (200, String::new(), following),
                (200, String::new(), following),
                (200, String::new(), seasons),
            ]);
            let server = Server {
                address,
                kind,
                user: "viewer".into(),
                ..Default::default()
            };
            let client = Client::new("next-episode-test".into()).unwrap();
            let mut current = Item {
                rating_key: "e1".into(),
                r#type: "episode".into(),
                parent_rating_key: "s1".into(),
                grandparent_rating_key: "show".into(),
                ..Default::default()
            };
            assert_eq!(
                client
                    .next_episode(&server, &current)
                    .await
                    .unwrap()
                    .unwrap()
                    .rating_key,
                "e2"
            );
            current.rating_key = "e2".into();
            assert_eq!(
                client
                    .next_episode(&server, &current)
                    .await
                    .unwrap()
                    .unwrap()
                    .rating_key,
                "e3"
            );
            current.rating_key = "e3".into();
            current.parent_rating_key = "s2".into();
            assert!(
                client
                    .next_episode(&server, &current)
                    .await
                    .unwrap()
                    .is_none()
            );
            let requests = requests.join().unwrap();
            assert_eq!(requests.len(), 6);
            assert!(
                requests
                    .iter()
                    .all(|request| !request.contains("/thumb") && !request.contains("/art"))
            );
        }
    }
    #[tokio::test]
    async fn next_episode_crosses_pages_without_fetching_the_rest_of_a_long_season() {
        let (address, requests) = server(vec![
            (
                200,
                String::new(),
                r#"{"MediaContainer":{"totalSize":100,"Metadata":[{"ratingKey":"e0","type":"episode"},{"ratingKey":"e1","type":"episode"}]}}"#,
            ),
            (
                200,
                String::new(),
                r#"{"MediaContainer":{"totalSize":100,"Metadata":[{"ratingKey":"e2","type":"episode"},{"ratingKey":"e3","type":"episode"}]}}"#,
            ),
        ]);
        let server = Server {
            address,
            ..Default::default()
        };
        let current = Item {
            rating_key: "e1".into(),
            r#type: "episode".into(),
            parent_rating_key: "season".into(),
            ..Default::default()
        };
        let next = Client::new("paged-next".into())
            .unwrap()
            .next_episode(&server, &current)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.rating_key, "e2");
        let requests = requests.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains("X-Plex-Container-Start=2"));
    }
}
