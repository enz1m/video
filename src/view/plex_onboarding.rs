//! Media server onboarding: choosing a server type, signing in, choosing a
//! profile, and choosing a server. Plex, Jellyfin and Emby share these screens.

use crate::app::{App, Message};
use crate::plex::{Event, HomeUser, Kind, Probe};
use enzui_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_widgets::icon::enz_icon;
use enzui_widgets::pressable::{PressableStyle, enz_pressable};
use enzui_widgets::scroll_view::enz_scroll_view;
use enzui_widgets::spinner::enz_spinner;
use enzui_widgets::text::{TextSize, TextTone, enz_text};
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Length};

enum Stage<'a> {
    Add,
    Restoring,
    SignIn,
    QuickConnect,
    Pin(&'a HomeUser),
    Password(&'a HomeUser),
    Manual,
    Profiles(&'a [HomeUser]),
    Connecting(&'a crate::plex::ConnectionProgress),
    Servers,
    Welcome,
    Find,
}

fn stage(app: &App) -> Option<Stage<'_>> {
    let state = &app.plex;
    let plex = state.kind == Kind::Plex;
    if state.adding {
        return Some(Stage::Add);
    }
    if state.restoring {
        return Some(Stage::Restoring);
    }
    if state.pin.is_some() {
        return Some(if plex {
            Stage::SignIn
        } else {
            Stage::QuickConnect
        });
    }
    if let Some(account) = &state.account
        && !state.profile_chosen
        && (account.users.len() > 1 || !plex)
    {
        if !plex && (state.manual || account.users.is_empty()) {
            return Some(Stage::Manual);
        }
        return Some(
            match state.user.and_then(|index| account.users.get(index)) {
                Some(user) if user.protected && plex => Stage::Pin(user),
                Some(user) if user.protected => Stage::Password(user),
                _ => Stage::Profiles(&account.users),
            },
        );
    }
    if state.connected.is_some() {
        return None;
    }
    if let Some(target) = &state.connecting {
        return Some(Stage::Connecting(target));
    }
    Some(match (&state.account, plex) {
        (Some(_), _) => Stage::Servers,
        (None, true) => Stage::Welcome,
        (None, false) => Stage::Find,
    })
}

pub(crate) fn active(app: &App) -> bool {
    stage(app).is_some()
}

pub(crate) fn screen(app: &App) -> Option<Element<'_, Message>> {
    let name = app.plex.kind.name();
    Some(match stage(app)? {
        Stage::Add => add_source(app),
        Stage::Restoring => splash(
            app,
            enzui_core::localized!("video-opening-name-596315", "name" => name),
            None,
        ),
        Stage::SignIn => sign_in(app),
        Stage::QuickConnect => quick_connect(app),
        Stage::Pin(user) => pin_pad(app, user),
        Stage::Password(user) => password(app, user),
        Stage::Manual => manual(app),
        Stage::Profiles(users) => profiles(app, users),
        Stage::Connecting(target) => {
            let label = target.label();
            let cancel = if app.plex.kind == Kind::Plex {
                Event::ChangeServer
            } else {
                Event::ShowProfiles
            };
            splash(
                app,
                label,
                Some((
                    enzui_core::localization::text("video-cancel-19766e"),
                    cancel,
                )),
            )
        }
        Stage::Servers => servers(app),
        Stage::Welcome => welcome(app),
        Stage::Find => find(app),
    })
}

pub(crate) fn mark<'a>(kind: Kind, size: f32) -> Element<'a, Message> {
    let (background, ink) = match kind {
        Kind::Plex => (
            iced::Color::from_rgb8(0xe5, 0xa0, 0x0d),
            iced::Color::from_rgb8(0x1f, 0x1f, 0x1f),
        ),
        Kind::Jellyfin => (iced::Color::from_rgb8(0x8e, 0x5c, 0xd6), iced::Color::WHITE),
        Kind::Emby => (iced::Color::from_rgb8(0x3f, 0x9e, 0x3a), iced::Color::WHITE),
    };
    let initial = match kind {
        Kind::Plex => "P",
        Kind::Jellyfin => "J",
        Kind::Emby => "E",
    };
    container(
        enz_text(initial)
            .size(if size >= 40.0 {
                TextSize::Title1
            } else {
                TextSize::Headline
            })
            .color(ink)
            .into_element(),
    )
    .center_x(size)
    .center_y(size)
    .style(move |_| container::Style {
        background: Some(background.into()),
        border: iced::Border {
            radius: (size * 0.28).into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn add_source(app: &App) -> Element<'_, Message> {
    let spacing = app.themes.theme().spacing;
    let palette = app.themes.theme().palette.clone();
    let choice = |kind: Kind, description: String| -> Element<'_, Message> {
        let added = kind == Kind::Plex
            && app
                .settings
                .sources
                .iter()
                .any(|source| source.kind == Kind::Plex);
        let surface = palette.surface_alt;
        let border = palette.border;
        let body = container(
            row![
                mark(kind, 44.0),
                column![
                    enz_text(kind.name())
                        .size(TextSize::Headline)
                        .into_element(),
                    enz_text(if added {
                        enzui_core::localization::text("video-already-added-open-it-b03915")
                    } else {
                        description
                    })
                    .size(TextSize::Subheadline)
                    .tone(TextTone::Muted)
                    .into_element(),
                ]
                .spacing(2)
                .width(Length::Fill),
                enz_icon("navigation.forward").size(16.0).into_element(),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding([14, 16])
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(surface.into()),
            border: iced::Border {
                radius: enzui_theme::text_defaults::shape_radius(|shape| shape.row).into(),
                width: 1.0,
                color: border,
            },
            ..Default::default()
        });
        enz_pressable(body.into(), Message::Plex(Event::Choose(kind)))
            .id(iced::advanced::widget::Id::from(format!(
                "video-add-source-{}",
                kind.name()
            )))
            .aria_label(enzui_core::localized!("video-add-0942e4", "value0" => kind.name()))
            .style(PressableStyle::Plain)
            .border_width(0.0)
            .radius(enzui_theme::text_defaults::shape_radius(|shape| shape.row))
            .width(Length::Fill)
            .into_element()
    };
    let mut content = column![
        heading(
            enzui_core::localization::text("video-add-a-media-server-5bca65"),
            enzui_core::localization::text(
                "video-stream-movies-and-shows-from-a-server-at-home-or-a-frie-477cb9"
            )
        ),
        column![
            choice(
                Kind::Plex,
                enzui_core::localization::text("video-sign-in-with-your-plex-account-e8bc89")
            ),
            choice(
                Kind::Jellyfin,
                enzui_core::localization::text("video-free-and-open-source-media-server-dcf71d")
            ),
            choice(
                Kind::Emby,
                enzui_core::localization::text("video-sign-in-to-an-emby-server-563c89")
            ),
        ]
        .spacing(spacing.sm()),
    ]
    .spacing(spacing.lg())
    .width(Length::Fill);
    if let Some(error) = error(app) {
        content = content.push(error);
    }
    content = content
        .push(Space::new().height(spacing.xs()))
        .push(network_header(app))
        .push(found(app, None));
    let cancel = if app.settings.sources.is_empty() {
        enzui_core::localization::text("video-not-now-a0e63d")
    } else {
        enzui_core::localization::text("video-cancel-19766e")
    };
    content = content.push(
        container(
            enz_button(cancel, Message::Plex(Event::CancelAdd))
                .id(iced::advanced::widget::Id::new("video-cancel-add-source"))
                .kind(ButtonKind::Quiet)
                .into_element(),
        )
        .center_x(Length::Fill),
    );
    frame(app, 560.0, content.into())
}

fn network_header(app: &App) -> Element<'_, Message> {
    row![
        enz_text(enzui_core::localization::text(
            "video-on-this-network-4a0254"
        ))
        .size(TextSize::Title3)
        .into_element(),
        Space::new().width(Length::Fill),
        enz_button(
            enzui_core::localization::text("video-search-again-998195"),
            Message::Plex(Event::Discover)
        )
        .id(iced::advanced::widget::Id::new("video-search-again-998195"))
        .kind(ButtonKind::Quiet)
        .icon("refresh-cw")
        .enabled(app.plex.local_servers.is_some())
        .into_element(),
    ]
    .mirrored(crate::l10n::rtl())
    .align_y(Alignment::Center)
    .into()
}

fn found(app: &App, kind: Option<Kind>) -> Element<'_, Message> {
    let spacing = app.themes.theme().spacing;
    let what = kind.map_or_else(
        || enzui_core::localization::text("video-media-721c95"),
        |kind| kind.name().to_owned(),
    );
    match &app.plex.local_servers {
        None => row![
            enz_spinner().size(18.0).into_element(),
            enz_text(
                enzui_core::localized!("video-looking-for-what-servers-98b371", "what" => what)
            )
            .tone(TextTone::Muted)
            .into_element()
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
        Some(servers) => {
            let rows: Vec<Element<'_, Message>> = servers
                .iter()
                .enumerate()
                .filter(|(_, server)| kind.is_none_or(|kind| server.kind == kind))
                .map(|(index, server)| {
                    let host = server
                        .address
                        .trim_start_matches("http://")
                        .trim_start_matches("https://")
                        .to_owned();
                    // Adding Plex goes through its sign-in, which lists these servers again.
                    let event = if kind.is_none() && server.kind == Kind::Plex {
                        Event::Choose(Kind::Plex)
                    } else {
                        Event::ConnectLocal(index)
                    };
                    let detail = if kind.is_none() {
                        format!("{}  ·  {host}", server.kind.name())
                    } else {
                        host
                    };
                    server_row(app, server.kind, &server.name, detail, Status::Local, event)
                })
                .collect();
            if rows.is_empty() {
                return enz_text(enzui_core::localized!("video-no-what-servers-answered-on-this-network-connect-by-add-88f9f9", "what" => what))
                .tone(TextTone::Muted)
                .into_element();
            }
            column(rows).spacing(spacing.sm()).into()
        }
    }
}

fn find(app: &App) -> Element<'_, Message> {
    let spacing = app.themes.theme().spacing;
    let kind = app.plex.kind;
    let mut content = column![
        container(mark(kind, 64.0)).center_x(Length::Fill),
        heading(
            match kind {
                Kind::Emby => enzui_core::localization::text("video-connect-to-emby-e28321"),
                _ => enzui_core::localization::text("video-connect-to-jellyfin-b7830a"),
            },
            enzui_core::localization::text(
                "video-choose-your-server-or-enter-the-address-it-shows-in-its-c8e1bb"
            )
        ),
    ]
    .spacing(spacing.lg())
    .width(Length::Fill);
    if let Some(error) = error(app) {
        content = content.push(error);
    }
    if let Some(warning) = insecure_warning(app) {
        content = content.push(warning);
    }
    let placeholder = "http://192.0.2.10:8096";
    content = content
        .push(network_header(app))
        .push(found(app, Some(kind)))
        .push(Space::new().height(spacing.xs()))
        .push(
            enz_text(enzui_core::localization::text(
                "video-server-address-8be897",
            ))
            .size(TextSize::Title3)
            .into_element(),
        )
        .push(
            row![
                enzui_widgets::text_field::enz_text_field(
                    "",
                    placeholder,
                    &app.plex.server_input,
                    |value| Message::Plex(Event::ServerInput(value)),
                )
                .aria_label(enzui_core::localization::text(
                    "video-server-address-8be897"
                ))
                .select_all_on_focus(true)
                .on_submit(Message::Plex(Event::Connect))
                .into_element(),
                enz_button(
                    enzui_core::localization::text("video-connect-1a2303"),
                    Message::Plex(Event::Connect)
                )
                .id(iced::advanced::widget::Id::new("video-connect-1a2303"))
                .kind(ButtonKind::Accent)
                .enabled(!app.plex.server_input.trim().is_empty())
                .into_element(),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(spacing.sm())
            .align_y(Alignment::Center),
        )
        .push(
            container(
                enz_button(
                    enzui_core::localization::text("video-choose-another-server-type-9a011e"),
                    Message::Plex(Event::AddSource),
                )
                .id(iced::advanced::widget::Id::new(
                    "video-choose-another-server-type-9a011e",
                ))
                .kind(ButtonKind::Quiet)
                .into_element(),
            )
            .center_x(Length::Fill),
        );
    frame(app, 560.0, content.into())
}

fn server_name(app: &App) -> &str {
    app.plex
        .account
        .as_ref()
        .and_then(|account| account.servers.first())
        .map_or_else(|| app.plex.kind.name(), |server| server.name.as_str())
}

fn sign_in_status(app: &App) -> Element<'_, Message> {
    let palette = app.themes.theme().palette.clone();
    match (&app.plex.pin_error, &app.plex.connecting) {
        (Some(message), _) => enz_text(crate::l10n::error(message))
            .size(TextSize::Subheadline)
            .color(palette.danger)
            .into_element(),
        (None, Some(_)) => enz_spinner().size(18.0).into_element(),
        (None, None) => Space::new().into(),
    }
}

fn password<'a>(app: &'a App, user: &'a HomeUser) -> Element<'a, Message> {
    let spacing = app.themes.theme().spacing;
    let busy = app.plex.connecting.is_some();
    let field = enzui_widgets::text_field::enz_text_field(
        "",
        enzui_core::localization::text("video-password-e7cf3e"),
        &app.plex.user_pin,
        |value| Message::Plex(Event::Password(value)),
    )
    .secure(true)
    .id(crate::app::PASSWORD_FIELD.into())
    .aria_label(enzui_core::localized!("video-user-password", "name" => user.title))
    .enabled(!busy)
    .on_submit(Message::Plex(Event::SwitchUser))
    .into_element();
    let content = column![
        avatar(app, &user.title, &user.thumb, 88.0),
        column![
            enz_text(enzui_core::localized!("video-sign-in-as-3026fd", "value0" => user.title))
                .size(TextSize::Title2)
                .into_element(),
            enz_text(server_name(app))
                .tone(TextTone::Muted)
                .into_element(),
        ]
        .spacing(4)
        .align_x(Alignment::Center),
        container(field).width(Length::Fixed(300.0)),
        container(sign_in_status(app)).height(22.0).center_y(22.0),
        enz_button(
            enzui_core::localization::text("video-sign-in-bfd402"),
            Message::Plex(Event::SwitchUser)
        )
        .id(iced::advanced::widget::Id::new("video-sign-in-bfd402"))
        .kind(ButtonKind::Accent)
        .size(ButtonSize::Lg)
        .width(Length::Fixed(300.0))
        .enabled(!busy)
        .into_element(),
        enz_button(
            enzui_core::localization::text("video-choose-another-profile-de30c2"),
            Message::Plex(Event::PinCancel)
        )
        .id(iced::advanced::widget::Id::new(
            "video-choose-another-profile-de30c2"
        ))
        .kind(ButtonKind::Quiet)
        .icon("users")
        .into_element(),
    ]
    .spacing(spacing.md())
    .align_x(Alignment::Center)
    .width(Length::Fill);
    frame(app, 420.0, content.into())
}

fn manual(app: &App) -> Element<'_, Message> {
    let spacing = app.themes.theme().spacing;
    let busy = app.plex.connecting.is_some();
    let has_profiles = app
        .plex
        .account
        .as_ref()
        .is_some_and(|account| !account.users.is_empty());
    let quick = app
        .plex
        .account
        .as_ref()
        .is_some_and(|account| account.quick_connect);
    let username = enzui_widgets::text_field::enz_text_field(
        enzui_core::localization::text("video-user-name-b82e11"),
        "",
        &app.plex.username,
        |value| Message::Plex(Event::Username(value)),
    )
    .id(crate::app::USERNAME_FIELD.into())
    .enabled(!busy)
    .on_submit(Message::Plex(Event::SwitchUser))
    .into_element();
    let password = enzui_widgets::text_field::enz_text_field(
        enzui_core::localization::text("video-password-e7cf3e"),
        "",
        &app.plex.user_pin,
        |value| Message::Plex(Event::Password(value)),
    )
    .secure(true)
    .id(crate::app::PASSWORD_FIELD.into())
    .enabled(!busy)
    .on_submit(Message::Plex(Event::SwitchUser))
    .into_element();
    let mut content = column![
        container(mark(app.plex.kind, 56.0)).center_x(Length::Fill),
        heading(
            enzui_core::localization::text("video-sign-in-bfd402"),
            enzui_core::localized!("video-use-your-account-on-f2c00a", "value0" => server_name(app))
        ),
        username,
        password,
        container(sign_in_status(app)).height(22.0).center_y(22.0),
        enz_button(
            enzui_core::localization::text("video-sign-in-bfd402"),
            Message::Plex(Event::SwitchUser)
        )
        .id(iced::advanced::widget::Id::new("video-sign-in-bfd402"))
        .kind(ButtonKind::Accent)
        .size(ButtonSize::Lg)
        .width(Length::Fill)
        .enabled(!busy && !app.plex.username.trim().is_empty())
        .into_element(),
    ]
    .spacing(spacing.md())
    .width(Length::Fill);
    if let Some(error) = error(app) {
        content = content.push(error);
    }
    let mut footer = row![].mirrored(crate::l10n::rtl()).spacing(spacing.sm());
    if quick {
        footer = footer.push(
            enz_button(
                enzui_core::localization::text("video-use-quick-connect-d52940"),
                Message::Plex(Event::SignIn),
            )
            .id(iced::advanced::widget::Id::new(
                "video-use-quick-connect-d52940",
            ))
            .kind(ButtonKind::Quiet)
            .icon("smartphone")
            .into_element(),
        );
    }
    footer = footer.push(if has_profiles {
        enz_button(
            enzui_core::localization::text("video-choose-a-profile-416c34"),
            Message::Plex(Event::ShowProfiles),
        )
        .id(iced::advanced::widget::Id::new(
            "video-choose-a-profile-416c34",
        ))
        .kind(ButtonKind::Quiet)
        .icon("users")
        .into_element()
    } else {
        enz_button(
            enzui_core::localization::text("video-choose-another-server-9403bf"),
            Message::Plex(Event::AddSource),
        )
        .id(iced::advanced::widget::Id::new(
            "video-choose-another-server-9403bf",
        ))
        .kind(ButtonKind::Quiet)
        .icon("server")
        .into_element()
    });
    content = content.push(container(footer).center_x(Length::Fill));
    frame(app, 380.0, content.into())
}

fn quick_connect(app: &App) -> Element<'_, Message> {
    let spacing = app.themes.theme().spacing;
    let palette = app.themes.theme().palette.clone();
    let code = app
        .plex
        .pin
        .as_ref()
        .map(|pin| pin.code.clone())
        .unwrap_or_default();
    let spaced = code
        .chars()
        .enumerate()
        .flat_map(|(index, digit)| (index == 3).then_some(' ').into_iter().chain(Some(digit)))
        .collect::<String>();
    let remaining = app.plex.pin.as_ref().and_then(|pin| {
        let started = app.plex.auth_started?;
        Some(std::time::Duration::from_secs(pin.expires_in).saturating_sub(started.elapsed()))
    });
    let expiry = match remaining {
        Some(left) if left.as_secs() >= 60 => {
            enzui_core::localized!("video-code-valid-for-min-9b2de1", "value0" => left.as_secs().div_ceil(60))
        }
        Some(left) => {
            enzui_core::localized!("video-code-valid-for-s-5d7215", "value0" => left.as_secs())
        }
        None => String::new(),
    };
    let code_box = container(enz_text(spaced).size(TextSize::LargeTitle).into_element())
        .padding([18, 32])
        .style(move |_| container::Style {
            background: Some(palette.accent_soft.into()),
            border: iced::Border {
                radius: enzui_theme::text_defaults::shape_radius(|shape| shape.card).into(),
                ..Default::default()
            },
            ..Default::default()
        });
    let mut content = column![
        heading(
            enzui_core::localization::text("video-approve-with-quick-connect-81e3e4"),
            enzui_core::localized!("video-on-a-phone-or-browser-signed-in-to-open-your-profile-ch-cf0e42", "value0" => server_name(app))
        ),
        container(code_box).center_x(Length::Fill),
        container(
            row![
                enz_spinner().size(16.0).into_element(),
                enz_text(enzui_core::localized!("video-waiting-for-approval-expiry-815675", "expiry" => expiry))
                    .size(TextSize::Subheadline)
                    .tone(TextTone::Muted)
                    .into_element(),
            ].mirrored(crate::l10n::rtl())
            .spacing(10)
            .align_y(Alignment::Center)
        )
        .center_x(Length::Fill),
    ]
    .spacing(spacing.lg())
    .width(Length::Fill);
    if let Some(error) = error(app) {
        content = content.push(error);
    }
    content = content.push(
        container(
            enz_button(
                enzui_core::localization::text("video-cancel-19766e"),
                Message::Plex(Event::CancelSignIn),
            )
            .id(iced::advanced::widget::Id::new("video-cancel-19766e"))
            .kind(ButtonKind::Quiet)
            .into_element(),
        )
        .center_x(Length::Fill),
    );
    frame(app, 520.0, content.into())
}

fn frame<'a>(app: &'a App, width: f32, content: Element<'a, Message>) -> Element<'a, Message> {
    let spacing = app.themes.theme().spacing;
    let body = container(container(content).width(Length::Fill.max(width)))
        .center_x(Length::Fill)
        .padding([spacing.xl(), spacing.lg()]);
    column![
        enz_scroll_view(
            container(body)
                .center_y(Length::Shrink)
                .width(Length::Fill)
                .padding(iced::Padding {
                    top: 0.0,
                    bottom: 48.0,
                    left: 0.0,
                    right: 0.0,
                })
        )
        .height(Length::Fill)
        .into_element()
    ]
    .height(Length::Fill)
    .into()
}

fn card<'a>(app: &'a App, content: Element<'a, Message>) -> Element<'a, Message> {
    let palette = app.themes.theme().palette.clone();
    container(content)
        .padding(20)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(palette.surface_alt.into()),
            border: iced::Border {
                radius: enzui_theme::text_defaults::shape_radius(|shape| shape.card).into(),
                width: 1.0,
                color: palette.border,
            },
            ..Default::default()
        })
        .into()
}

fn heading<'a>(title: impl Into<String>, subtitle: String) -> Element<'a, Message> {
    let mut heading = column![enz_text(title).size(TextSize::LargeTitle).into_element()]
        .spacing(8)
        .align_x(Alignment::Center)
        .width(Length::Fill);
    if !subtitle.is_empty() {
        heading = heading.push(
            enz_text(subtitle)
                .tone(TextTone::Muted)
                .align(enzui_widgets::text::TextAlign::Center)
                .into_element(),
        );
    }
    heading.into()
}

fn insecure_warning(app: &App) -> Option<Element<'_, Message>> {
    let (_, address) = app.plex.insecure_address.as_ref()?;
    let palette = app.themes.theme().palette.clone();
    let spacing = app.themes.theme().spacing;
    Some(
        container(
            column![
                row![
                    enz_icon("triangle-alert")
                        .size(16.0)
                        .tint(palette.warning)
                        .into_element(),
                    enz_text(enzui_core::localized!(
                        "video-address-isn-t-encrypted-and-isn-t-on-your-network-580178",
                        "address" => address.as_str()
                    ))
                    .into_element(),
                ]
                .mirrored(crate::l10n::rtl())
                .spacing(10)
                .align_y(Alignment::Center),
                container(
                    enz_button(
                        enzui_core::localization::text("video-connect-anyway-3e3d60"),
                        Message::Plex(Event::ConnectInsecure),
                    )
                    .id(iced::advanced::widget::Id::new(
                        "video-connect-anyway-3e3d60"
                    ))
                    .kind(ButtonKind::Secondary)
                    .into_element(),
                )
                .align_right(Length::Fill),
            ]
            .spacing(spacing.sm()),
        )
        .padding([10, 14])
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(palette.warning_soft.into()),
            border: iced::Border {
                radius: enzui_theme::text_defaults::shape_radius(|shape| shape.card).into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into(),
    )
}

fn error<'a>(app: &'a App) -> Option<Element<'a, Message>> {
    let message = app.plex.error.as_deref()?;
    let palette = app.themes.theme().palette.clone();
    Some(
        container(
            row![
                enz_icon("circle-alert")
                    .size(16.0)
                    .tint(palette.danger)
                    .into_element(),
                enz_text(crate::l10n::error(message)).into_element(),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([10, 14])
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(palette.danger_soft.into()),
            border: iced::Border {
                radius: enzui_theme::text_defaults::shape_radius(|shape| shape.card).into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into(),
    )
}

fn splash<'a>(
    app: &'a App,
    label: String,
    action: Option<(String, Event)>,
) -> Element<'a, Message> {
    let mut content = column![
        enz_spinner().size(36.0).into_element(),
        enz_text(label).size(TextSize::Title3).into_element(),
    ]
    .spacing(18)
    .align_x(Alignment::Center)
    .width(Length::Fill);
    if let Some((label, event)) = action {
        content = content.push(
            enz_button(label, Message::Plex(event))
                .kind(ButtonKind::Quiet)
                .into_element(),
        );
    }
    frame(
        app,
        420.0,
        container(content)
            .padding(iced::Padding {
                top: 120.0,
                ..Default::default()
            })
            .into(),
    )
}

fn welcome(app: &App) -> Element<'_, Message> {
    let spacing = app.themes.theme().spacing;
    let mut content = column![
        container(mark(Kind::Plex, 64.0)).center_x(Length::Fill),
        heading(
            enzui_core::localization::text("video-watch-your-plex-library-74e736"),
            enzui_core::localization::text(
                "video-sign-in-to-stream-the-movies-and-shows-on-your-plex-ser-b92e2d"
            )
        ),
        container(
            enz_button(
                enzui_core::localization::text("video-sign-in-with-plex-bf7f8a"),
                Message::Plex(Event::SignIn)
            )
            .id(iced::advanced::widget::Id::new(
                "video-sign-in-with-plex-bf7f8a"
            ))
            .kind(ButtonKind::Accent)
            .size(ButtonSize::Lg)
            .width(Length::Fixed(260.0))
            .into_element()
        )
        .center_x(Length::Fill),
    ]
    .spacing(spacing.lg())
    .width(Length::Fill);
    if let Some(error) = error(app) {
        content = content.push(error);
    }
    if let Some(warning) = insecure_warning(app) {
        content = content.push(warning);
    }
    content = content
        .push(Space::new().height(spacing.md()))
        .push(network_header(app))
        .push(found(app, Some(Kind::Plex)))
        .push(
            row![
                address_entry(app),
                Space::new().width(Length::Fill),
                enz_button(
                    enzui_core::localization::text("video-other-server-types-d84a5f"),
                    Message::Plex(Event::AddSource)
                )
                .id(iced::advanced::widget::Id::new(
                    "video-other-server-types-d84a5f"
                ))
                .kind(ButtonKind::Quiet)
                .into_element(),
            ]
            .mirrored(crate::l10n::rtl())
            .align_y(Alignment::Center),
        );
    frame(app, 520.0, content.into())
}

fn address_entry(app: &App) -> Element<'_, Message> {
    let spacing = app.themes.theme().spacing;
    if !app.plex.manual_address {
        return enz_button(
            enzui_core::localization::text("video-connect-by-address-c2bf1a"),
            Message::Plex(Event::ToggleAddress),
        )
        .id(iced::advanced::widget::Id::new(
            "video-connect-by-address-c2bf1a",
        ))
        .kind(ButtonKind::Quiet)
        .icon("link")
        .into_element();
    }
    row![
        enzui_widgets::text_field::enz_text_field(
            "",
            "http://192.0.2.10:32400",
            &app.plex.server_input,
            |value| Message::Plex(Event::ServerInput(value)),
        )
        .aria_label(enzui_core::localization::text(
            "video-server-address-8be897"
        ))
        .select_all_on_focus(true)
        .on_submit(Message::Plex(Event::Connect))
        .into_element(),
        enz_button(
            enzui_core::localization::text("video-connect-1a2303"),
            Message::Plex(Event::Connect)
        )
        .id(iced::advanced::widget::Id::new("video-connect-1a2303"))
        .kind(ButtonKind::Secondary)
        .into_element(),
    ]
    .mirrored(crate::l10n::rtl())
    .spacing(spacing.sm())
    .align_y(Alignment::Center)
    .into()
}

fn sign_in(app: &App) -> Element<'_, Message> {
    let state = &app.plex;
    let spacing = app.themes.theme().spacing;
    let remaining = state.pin.as_ref().and_then(|pin| {
        let started = state.auth_started?;
        let total = std::time::Duration::from_secs(pin.expires_in.min(900));
        Some(total.saturating_sub(started.elapsed()))
    });
    let expiry = match remaining {
        Some(left) if left.as_secs() >= 60 => {
            enzui_core::localized!("video-link-valid-for-min-90218e", "value0" => left.as_secs().div_ceil(60))
        }
        Some(left) => {
            enzui_core::localized!("video-link-valid-for-s-77c5db", "value0" => left.as_secs())
        }
        None => String::new(),
    };
    let qr: Element<'_, Message> = match &state.auth_qr {
        Some(data) => container(
            enzui_widgets::qr::enz_qr(data)
                .total_size(212.0)
                .into_element(),
        )
        .padding(6)
        .into(),
        None => Space::new().into(),
    };
    let actions = column![
        enz_text(enzui_core::localization::text(
            "video-scan-with-your-phone-or-continue-on-this-computer-566afb"
        ))
        .size(TextSize::Headline)
        .into_element(),
        enz_text(enzui_core::localization::text(
            "video-approve-the-request-on-plex-and-this-window-continues-a-4a9c75"
        ))
        .tone(TextTone::Muted)
        .into_element(),
        row![
            enz_button(
                enzui_core::localization::text("video-open-sign-in-page-d39769"),
                Message::Plex(Event::Browser)
            )
            .id(iced::advanced::widget::Id::new(
                "video-open-sign-in-page-d39769"
            ))
            .kind(ButtonKind::Accent)
            .icon("external-link")
            .into_element(),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(spacing.sm()),
        enz_button(
            enzui_core::localization::text("video-copy-link-dbf362"),
            Message::Plex(Event::CopyLink)
        )
        .id(iced::advanced::widget::Id::new("video-copy-link-dbf362"))
        .kind(ButtonKind::Quiet)
        .icon("copy")
        .into_element(),
        row![
            enz_spinner().size(16.0).into_element(),
            enz_text(enzui_core::localized!("video-waiting-for-approval-expiry-815675", "expiry" => expiry))
                .size(TextSize::Subheadline)
                .tone(TextTone::Muted)
                .into_element(),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(spacing.md())
    .width(Length::Fill);
    let mut content = column![
        heading(
            enzui_core::localization::text("video-sign-in-to-plex-477f1f"),
            enzui_core::localization::text(
                "video-use-your-plex-account-to-see-your-servers-cdf100"
            )
        ),
        card(
            app,
            row![qr, actions]
                .mirrored(crate::l10n::rtl())
                .spacing(spacing.xl())
                .align_y(Alignment::Center)
                .into()
        ),
    ]
    .spacing(spacing.lg())
    .width(Length::Fill);
    if let Some(error) = error(app) {
        content = content.push(error);
    }
    content = content.push(
        container(
            enz_button(
                enzui_core::localization::text("video-cancel-19766e"),
                Message::Plex(Event::CancelSignIn),
            )
            .id(iced::advanced::widget::Id::new("video-cancel-19766e"))
            .kind(ButtonKind::Quiet)
            .into_element(),
        )
        .center_x(Length::Fill),
    );
    frame(app, 640.0, content.into())
}

fn avatar<'a>(app: &'a App, name: &str, thumb: &str, size: f32) -> Element<'a, Message> {
    let mut avatar = enzui_widgets::avatar::enz_avatar(name).avatar_size(size);
    if let Some(photo) = app.plex.avatars.get(thumb) {
        avatar = avatar.image(photo.clone());
    }
    avatar.into_element()
}

fn profiles<'a>(app: &'a App, users: &'a [HomeUser]) -> Element<'a, Message> {
    let spacing = app.themes.theme().spacing;
    let remembered = app.remembered_user();
    let tiles = users.iter().enumerate().map(|(index, user)| {
        let mut badges = row![]
            .mirrored(crate::l10n::rtl())
            .spacing(6)
            .align_y(Alignment::Center);
        if user.protected {
            badges = badges.push(enz_icon("lock").size(12.0).into_element());
        }
        if user.admin {
            badges = badges.push(
                enz_text(enzui_core::localization::text("video-admin-c1c224"))
                    .size(TextSize::Caption)
                    .tone(TextTone::Muted)
                    .into_element(),
            );
        } else if user.restricted {
            badges = badges.push(
                enz_text(enzui_core::localization::text("video-managed-8f2de6"))
                    .size(TextSize::Caption)
                    .tone(TextTone::Muted)
                    .into_element(),
            );
        }
        let current = app
            .plex
            .account
            .as_ref()
            .is_some_and(|account| account.user_id == user.id)
            || user.id == remembered;
        let accent = app.themes.theme().palette.accent;
        let tile = column![
            container(avatar(app, &user.title, &user.thumb, 96.0))
                .padding(4)
                .style(move |_| container::Style {
                    border: iced::Border {
                        radius: 52.0.into(),
                        width: if current { 3.0 } else { 0.0 },
                        color: accent,
                    },
                    ..Default::default()
                }),
            enz_text(user.title.as_str())
                .size(TextSize::Headline)
                .single_line_elided()
                .into_element(),
            container(badges).height(16.0),
        ]
        .spacing(10)
        .align_x(Alignment::Center)
        .width(Length::Fixed(132.0));
        enz_pressable(tile.into(), Message::Plex(Event::User(index)))
            .aria_label(if user.protected {
                let lock = if app.plex.kind == Kind::Plex {
                    enzui_core::localization::text("video-a-pin-cf9d57")
                } else {
                    enzui_core::localization::text("video-a-password-afd136")
                };
                enzui_core::localized!("video-protected-profile", "name" => user.title, "lock" => lock)
            } else {
                user.title.clone()
            })
            .style(PressableStyle::Plain)
            .border_width(0.0)
            .radius(app.themes.theme().radius.step(app.themes.theme().shape.card))
            .padding(12)
            .width(Length::Shrink)
            .into_element()
    });
    let mut grid = row![].mirrored(crate::l10n::rtl()).spacing(spacing.md());
    for tile in tiles {
        grid = grid.push(tile);
    }
    let plex = app.plex.kind == Kind::Plex;
    if !plex {
        // Accounts hidden from the profile list sign in by name.
        let palette = app.themes.theme().palette.clone();
        let other = column![
            container(
                container(enz_icon("user-plus").size(34.0).into_element())
                    .center_x(96.0)
                    .center_y(96.0)
                    .style(move |_| container::Style {
                        background: Some(palette.surface_alt.into()),
                        border: iced::Border {
                            radius: 48.0.into(),
                            width: 1.0,
                            color: palette.border,
                        },
                        ..Default::default()
                    })
            )
            .padding(4),
            enz_text(enzui_core::localization::text("video-another-user-9cd8b0"))
                .size(TextSize::Headline)
                .into_element(),
            container(Space::new()).height(16.0),
        ]
        .spacing(10)
        .align_x(Alignment::Center)
        .width(Length::Fixed(132.0));
        grid = grid.push(
            enz_pressable(other.into(), Message::Plex(Event::ManualSignIn))
                .aria_label(enzui_core::localization::text(
                    "video-sign-in-as-another-user-d416de",
                ))
                .style(PressableStyle::Plain)
                .border_width(0.0)
                .radius(
                    app.themes
                        .theme()
                        .radius
                        .step(app.themes.theme().shape.card),
                )
                .padding(12)
                .width(Length::Shrink)
                .into_element(),
        );
    }
    let subtitle = if plex {
        enzui_core::localization::text(
            "video-choose-a-profile-for-recommendations-and-watch-progress-2c4f03",
        )
    } else {
        enzui_core::localized!("video-choose-your-profile-on-0a6dbc", "value0" => server_name(app))
    };
    let mut content = column![
        heading(
            enzui_core::localization::text("video-who-s-watching-d0ec3b"),
            subtitle
        ),
        container(
            grid.wrap()
                .vertical_spacing(spacing.md())
                .align_x(iced::alignment::Horizontal::Center)
        )
        .center_x(Length::Fill),
    ]
    .spacing(spacing.xl())
    .width(Length::Fill);
    if let Some(error) = error(app) {
        content = content.push(error);
    }
    let mut footer = row![].mirrored(crate::l10n::rtl()).spacing(spacing.sm());
    if plex {
        footer = footer.push(
            enz_button(
                enzui_core::localization::text("video-sign-out-48f0d3"),
                Message::Plex(Event::SignOut),
            )
            .id(iced::advanced::widget::Id::new("video-sign-out-48f0d3"))
            .kind(ButtonKind::Quiet)
            .icon("log-out")
            .into_element(),
        );
    } else {
        if app
            .plex
            .account
            .as_ref()
            .is_some_and(|account| account.quick_connect)
        {
            footer = footer.push(
                enz_button(
                    enzui_core::localization::text("video-use-quick-connect-d52940"),
                    Message::Plex(Event::SignIn),
                )
                .id(iced::advanced::widget::Id::new(
                    "video-use-quick-connect-d52940",
                ))
                .kind(ButtonKind::Quiet)
                .icon("smartphone")
                .into_element(),
            );
        }
        footer = footer.push(
            enz_button(
                enzui_core::localization::text("video-choose-another-server-9403bf"),
                Message::Plex(Event::AddSource),
            )
            .id(iced::advanced::widget::Id::new(
                "video-choose-another-server-9403bf",
            ))
            .kind(ButtonKind::Quiet)
            .icon("server")
            .into_element(),
        );
    }
    content = content.push(container(footer).center_x(Length::Fill));
    frame(app, 760.0, content.into())
}

fn pin_pad<'a>(app: &'a App, user: &'a HomeUser) -> Element<'a, Message> {
    let spacing = app.themes.theme().spacing;
    let palette = app.themes.theme().palette.clone();
    let entered = app.plex.user_pin.len();
    let failed = app.plex.pin_error.is_some();
    let dots = row((0..4).map(|index| {
        let filled = index < entered;
        let palette = palette.clone();
        container(Space::new())
            .width(16.0)
            .height(16.0)
            .style(move |_| {
                let ink = if failed { palette.danger } else { palette.text };
                container::Style {
                    background: filled.then(|| ink.into()),
                    border: iced::Border {
                        radius: 8.0.into(),
                        width: 2.0,
                        color: if failed {
                            palette.danger
                        } else {
                            palette.border_strong
                        },
                    },
                    ..Default::default()
                }
            })
            .into()
    }))
    .spacing(18);
    let key = |label: &'a str, event: Event| -> Element<'a, Message> {
        enz_button(label, Message::Plex(event))
            .kind(ButtonKind::Secondary)
            .size(ButtonSize::Lg)
            .width(Length::Fixed(76.0))
            .into_element()
    };
    let mut keypad = column![].spacing(12).align_x(Alignment::Center);
    for row_digits in [[1_u8, 2, 3], [4, 5, 6], [7, 8, 9]] {
        keypad = keypad.push(
            row(row_digits.map(|digit| key(DIGITS[digit as usize], Event::PinDigit(digit))))
                .spacing(12),
        );
    }
    keypad = keypad.push(
        row![
            Space::new().width(Length::Fixed(76.0)),
            key("0", Event::PinDigit(0)),
            enz_button("", Message::Plex(Event::PinBack))
                .kind(ButtonKind::Quiet)
                .size(ButtonSize::Lg)
                .icon("delete")
                .width(Length::Fixed(76.0))
                .into_element(),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(12),
    );
    let status: Element<'_, Message> = match (&app.plex.pin_error, &app.plex.connecting) {
        (Some(message), _) => enz_text(crate::l10n::error(message))
            .size(TextSize::Subheadline)
            .color(palette.danger)
            .into_element(),
        (None, Some(_)) => enz_spinner().size(18.0).into_element(),
        (None, None) => enz_text(enzui_core::localization::text(
            "video-type-the-pin-or-use-the-keypad-33fe16",
        ))
        .size(TextSize::Subheadline)
        .tone(TextTone::Muted)
        .into_element(),
    };
    let content = column![
        avatar(app, &user.title, &user.thumb, 88.0),
        enz_text(enzui_core::localized!("video-enter-s-pin-1f259a", "value0" => user.title))
            .size(TextSize::Title2)
            .into_element(),
        dots,
        container(status).height(22.0).center_y(22.0),
        keypad,
        enz_button(
            enzui_core::localization::text("video-choose-another-profile-de30c2"),
            Message::Plex(Event::PinCancel)
        )
        .id(iced::advanced::widget::Id::new(
            "video-choose-another-profile-de30c2"
        ))
        .kind(ButtonKind::Quiet)
        .icon("users")
        .into_element(),
    ]
    .spacing(spacing.lg())
    .align_x(Alignment::Center)
    .width(Length::Fill);
    frame(app, 420.0, content.into())
}

const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

enum Status {
    Local,
    Checking,
    Probed(Probe),
    Offline,
}

fn server_row<'a>(
    app: &'a App,
    kind: Kind,
    name: &str,
    detail: String,
    status: Status,
    event: Event,
) -> Element<'a, Message> {
    let palette = app.themes.theme().palette.clone();
    let (dot, state) = match status {
        Status::Local => (
            Some(palette.success),
            enzui_core::localization::text("video-available-e67444"),
        ),
        Status::Checking => (
            None,
            enzui_core::localization::text("video-checking-ec963f"),
        ),
        Status::Probed(Probe::Reachable { route, latency_ms }) => (
            Some(palette.success),
            match route {
                Some(route) => {
                    enzui_core::localized!("video-latency-ms-ms-3012d6", "value0" => route.label(), "latency_ms" => latency_ms)
                }
                None => {
                    enzui_core::localized!("video-latency-ms-ms-923db6", "latency_ms" => latency_ms)
                }
            },
        ),
        Status::Probed(Probe::Unreachable) => (
            Some(palette.danger),
            enzui_core::localization::text("video-not-reachable-5ad992"),
        ),
        Status::Offline => (
            Some(palette.text_subtle),
            enzui_core::localization::text("video-offline-a17947"),
        ),
    };
    let indicator: Element<'_, Message> = match dot {
        Some(color) => container(Space::new())
            .width(8.0)
            .height(8.0)
            .style(move |_| container::Style {
                background: Some(color.into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into(),
        None => enz_spinner().size(12.0).into_element(),
    };
    let surface = palette.surface_alt;
    let border = palette.border;
    let content = container(
        row![
            mark(kind, 36.0),
            column![
                enz_text(name.to_owned())
                    .size(TextSize::Headline)
                    .single_line_elided()
                    .into_element(),
                enz_text(detail)
                    .size(TextSize::Subheadline)
                    .tone(TextTone::Muted)
                    .single_line_elided()
                    .into_element(),
            ]
            .spacing(2)
            .width(Length::Fill),
            row![
                indicator,
                enz_text(state)
                    .size(TextSize::Subheadline)
                    .tone(TextTone::Muted)
                    .into_element()
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            enz_icon("navigation.forward").size(16.0).into_element(),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(surface.into()),
        border: iced::Border {
            radius: enzui_theme::text_defaults::shape_radius(|shape| shape.row).into(),
            width: 1.0,
            color: border,
        },
        ..Default::default()
    });
    enz_pressable(content.into(), Message::Plex(event))
        .aria_label(enzui_core::localized!("video-connect-to-name-150444", "name" => name))
        .style(PressableStyle::Plain)
        .border_width(0.0)
        .radius(enzui_theme::text_defaults::shape_radius(|shape| shape.row))
        .width(Length::Fill)
        .into_element()
}

fn servers(app: &App) -> Element<'_, Message> {
    let state = &app.plex;
    let spacing = app.themes.theme().spacing;
    let Some(account) = &state.account else {
        return welcome(app);
    };
    let multiple_profiles = account.users.len() > 1;
    let plex = app.plex.kind == Kind::Plex;
    let mut list = column![].spacing(spacing.sm()).width(Length::Fill);
    for (index, server) in account.servers.iter().enumerate() {
        let owner = if server.owned {
            enzui_core::localization::text("video-your-server-c03391").clone()
        } else if server.owner.is_empty() {
            enzui_core::localization::text("video-shared-with-you-b44712").clone()
        } else {
            enzui_core::localized!("video-shared-by-645565", "value0" => server.owner)
        };
        let status = match state.probes.get(&server.id) {
            Some(probe @ Probe::Reachable { .. }) => Status::Probed(probe.clone()),
            _ if !server.online => Status::Offline,
            Some(probe) => Status::Probed(probe.clone()),
            None => Status::Checking,
        };
        list = list.push(server_row(
            app,
            server.kind,
            &server.name,
            owner,
            status,
            Event::Server(index),
        ));
    }
    if account.servers.is_empty() {
        list = list.push(card(
            app,
            column![
                enz_text(enzui_core::localization::text(
                    "video-no-servers-on-this-account-161ffb"
                ))
                .size(TextSize::Headline)
                .into_element(),
                enz_text(enzui_core::localization::text(
                    "video-set-up-plex-media-server-ask-a-friend-to-share-theirs-o-bc4ab5"
                ))
                .tone(TextTone::Muted)
                .into_element(),
            ]
            .spacing(6)
            .into(),
        ));
    }
    let who = row![
        avatar(app, &account.name, &account.thumb, 28.0),
        enz_text(enzui_core::localized!("video-signed-in-as-9573b8", "value0" => account.name))
            .tone(TextTone::Muted)
            .into_element(),
    ]
    .mirrored(crate::l10n::rtl())
    .spacing(10)
    .align_y(Alignment::Center);
    let mut content = column![
        heading(
            if plex {
                enzui_core::localization::text("video-choose-a-server-89d099")
            } else {
                enzui_core::localization::text("video-your-server-c03391")
            },
            String::new()
        ),
        container(who).center_x(Length::Fill),
        list,
    ]
    .spacing(spacing.lg())
    .width(Length::Fill);
    if let Some(error) = error(app) {
        content = content.push(error);
    }
    let mut footer = row![]
        .mirrored(crate::l10n::rtl())
        .spacing(spacing.sm())
        .align_y(Alignment::Center);
    if plex {
        footer = footer.push(address_entry(app));
    }
    footer = footer.push(Space::new().width(Length::Fill));
    if multiple_profiles || !plex {
        footer = footer.push(
            enz_button(
                enzui_core::localization::text("video-switch-profile-c1e0da"),
                Message::Plex(Event::ShowProfiles),
            )
            .id(iced::advanced::widget::Id::new(
                "video-switch-profile-c1e0da",
            ))
            .kind(ButtonKind::Quiet)
            .icon("users")
            .into_element(),
        );
    }
    footer = footer.push(
        enz_button(
            enzui_core::localization::text("video-sign-out-48f0d3"),
            Message::Plex(Event::SignOut),
        )
        .id(iced::advanced::widget::Id::new("video-sign-out-48f0d3"))
        .kind(ButtonKind::Quiet)
        .icon("log-out")
        .into_element(),
    );
    content = content.push(footer);
    frame(app, 640.0, content.into())
}
