use crate::app::{App, Message};
use crate::plex::{Event, Item};
use enzui_theme::{GlassInk, MaterialStyle};
use enzui_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_widgets::clickable::enz_clickable;
use enzui_widgets::glass::enz_glass;
use enzui_widgets::grid::enz_grid;
use enzui_widgets::hover_group::{HoverGroupStyle, enz_hover_group_with};
use enzui_widgets::image_widget::{EnzImageFit, enz_image_handle};
use enzui_widgets::motion::{MotionEffect, motion};
use enzui_widgets::pressable::{PressableStyle, enz_pressable};
use enzui_widgets::scroll_view::enz_scroll_view;
use enzui_widgets::text::{TextSize, TextTone, enz_text};
use enzui_widgets::toolbar_icon::{ToolbarIconVariant, enz_toolbar_icon};
use iced::widget::{Space, column, container, row, stack};
use iced::{Element, Length};

fn button<'a>(label: impl Into<String>, event: Event) -> Element<'a, Message> {
    enz_button(label, Message::Plex(event))
        .kind(ButtonKind::Quiet)
        .into_element()
}
pub(super) fn search<'a>(app: &'a App, library: &crate::plex::Library) -> Element<'a, Message> {
    let field = enzui_widgets::search_field::enz_titlebar_search(&app.plex.query, |value| {
        Message::Plex(Event::Search(value))
    })
    .size(enzui_widgets::text_field::TextFieldSize::Md)
    .responsive(false)
    .chrome(enzui_widgets::search_field::SearchFieldChrome::Ghost)
    .radius(
        app.themes
            .theme()
            .radius
            .step(app.themes.theme().shape.button_group),
    )
    .width(Length::Fill)
    .placeholder(enzui_core::localized!("video-search-ee07a4", "value0" => library.title))
    .id("video-library-search")
    .on_submit(Message::Plex(Event::SearchGo))
    .into_element();
    container(super::titlebar::glass_chrome(field))
        .width(app.themes.theme().sizing.search_field_width)
        .into()
}

pub(crate) fn page<'a>(
    app: &'a App,
    geometry: &enzui_widgets::app_shell::ShellGeometry,
) -> Element<'a, Message> {
    if let Some(screen) = super::plex_onboarding::screen(app) {
        return screen;
    }
    let top_inset = geometry.content_inset[0];
    let state = &app.plex;
    let spacing = app.themes.theme().spacing;
    let mut body = column![].spacing(spacing.lg()).width(Length::Fill);
    if let Some(error) = &state.error {
        body = body.push(
            enz_text(crate::l10n::error(error))
                .tone(TextTone::Muted)
                .into_element(),
        );
    }
    if let Some(item) = &state.selected {
        return detail(app, body, item, top_inset);
    }
    let Some(connected) = &state.connected else {
        return framed(app, None, body.into(), top_inset);
    };
    let library = state.library.and_then(|i| connected.libraries.get(i));
    let parent = state.parents.last();
    if library.is_none() && parent.is_none() {
        return home(app, body, connected, top_inset);
    }
    let library_title = library.map_or("", |library| library.title.as_str());
    let episodes = parent.is_some_and(|parent| parent.r#type == "season");
    let hero = match parent {
        None if state.active_query.is_empty() => {
            let preview = state
                .home
                .iter()
                .find(|preview| Some(preview.library) == state.library);
            Some(banner(
                app,
                preview.and_then(|preview| preview.backdrop.as_ref()),
                library_title,
                enzui_core::localization::text("video-library-dc20b3").into(),
                "",
                vec![],
                200.0,
            ))
        }
        None => None,
        Some(parent) => {
            let image = state
                .backdrop
                .as_ref()
                .filter(|(key, _)| key == &parent.rating_key)
                .map(|(_, image)| image);
            let crumbs = if parent.r#type == "season" && !parent.parent_title.is_empty() {
                vec![(parent.parent_title.clone(), Event::Ancestor(false))]
            } else {
                vec![]
            };
            let eyebrow = if parent.r#type == "season" {
                String::new()
            } else {
                metadata(parent)
            };
            Some(banner(
                app,
                image,
                &parent.title,
                Eyebrow {
                    crumbs,
                    text: eyebrow,
                },
                &parent.summary,
                vec![],
                260.0,
            ))
        }
    };
    if parent.is_none() {
        let count = if state.total > 0 {
            enzui_core::localized!("video-title-count", "count" => state.total)
        } else {
            String::new()
        };
        body = body.push(
            row![
                sort_control(app),
                Space::new().width(Length::Fill),
                enz_text(count).tone(TextTone::Muted).into_element(),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(spacing.md())
            .align_y(iced::Alignment::Center),
        );
        if !state.active_query.is_empty() {
            body = body.push(
                row![
                    enz_text(enzui_core::localized!("video-results-for-in-390399", "value0" => state.active_query, "value1" => library_title))
                        .size(TextSize::Title3)
                        .into_element(),
                    Space::new().width(Length::Fill),
                    button(enzui_core::localization::text("video-clear-search-3b7ea5"), Event::ClearSearch),
                ].mirrored(crate::l10n::rtl())
                .align_y(iced::Alignment::Center),
            );
        }
    } else {
        body = body.push(section_title(if episodes {
            enzui_core::localization::text("video-episodes-83f972")
        } else {
            enzui_core::localization::text("video-seasons-c15527")
        }));
    }
    if !state.busy && state.items.is_empty() {
        body = body.push(
            enz_text(enzui_core::localization::text(
                "video-no-titles-found-60ce88",
            ))
            .tone(TextTone::Muted)
            .into_element(),
        );
    }
    let (min_width, max_columns, aspect) = if episodes {
        (280.0, 5.0, 9.0 / 16.0)
    } else {
        (170.0, 8.0, 1.5)
    };
    let columns = ((app.window_size.width - 32.0) / min_width)
        .floor()
        .clamp(if episodes { 1.0 } else { 2.0 }, max_columns) as usize;
    let art_height = card_width(app, columns) * aspect;
    let cards = state
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            art_tile(
                app,
                item,
                state.posters.get(&item.rating_key),
                art_height,
                Event::Select(index),
                Event::QuickPlay(None, index),
            )
        })
        .collect();
    body = body.push(
        enz_grid(columns, cards)
            .gap(spacing.md())
            .stretch_cells(true)
            .into_element(),
    );
    if let Some(show) = parent.filter(|parent| parent.r#type == "show")
        && let Some(cast) = cast_section(app, show)
    {
        body = body.push(cast);
    }
    if state.items.len() < state.total {
        body = body.push(
            iced::widget::sensor(Space::new().height(1.0))
                .key(state.items.len())
                .anticipate(900.0)
                .on_show(|_| Message::Plex(Event::LoadMore)),
        );
    }
    if state.loading_more {
        body = body.push(
            container(
                enz_text(enzui_core::localization::text("video-loading-more-964e5f"))
                    .tone(TextTone::Muted)
                    .into_element(),
            )
            .center_x(Length::Fill),
        );
    } else if !state.busy && state.items.len() < state.total {
        body = body.push(
            container(button(
                enzui_core::localization::text("video-load-more-ac8991"),
                Event::LoadMore,
            ))
            .center_x(Length::Fill),
        );
    }
    framed(app, hero, body.into(), top_inset)
}
fn source_control<'a>(app: &'a App, connected: &'a crate::plex::Connected) -> Element<'a, Message> {
    row![
        crate::view::plex_onboarding::mark(connected.server.kind, 22.0),
        enzui_widgets::menu_button::enz_menu_button(
            connected.server.name.as_str(),
            &app.menus.sources.0,
            &app.menus.sources.1,
            Message::SourceMenu,
        )
        .kind(enzui_widgets::menu_button::MenuButtonKind::Quiet(
            enzui_widgets::button::ButtonSize::Md,
        ))
        .menu_id(iced::advanced::widget::Id::new("video-source-menu"))
        .aria_label(enzui_core::localization::text("video-media-server-b44e32"))
        .alignment(enzui_core::placement::Alignment::End)
        .into_element(),
    ]
    .mirrored(crate::l10n::rtl())
    .spacing(2)
    .align_y(iced::Alignment::Center)
    .into()
}
fn sort_control(app: &App) -> Element<'_, Message> {
    enzui_widgets::menu_button::enz_menu_button(
        enzui_core::localized!("video-sort-by", "order" => app.library_sort().label()),
        &app.menus.sort.0,
        &app.menus.sort.1,
        Message::SortMenu,
    )
    .kind(enzui_widgets::menu_button::MenuButtonKind::Quiet(
        enzui_widgets::button::ButtonSize::Md,
    ))
    .menu_id(iced::advanced::widget::Id::new("video-sort-menu"))
    .aria_label(enzui_core::localization::text("video-sort-library"))
    .into_element()
}
fn account_control(app: &App) -> Element<'_, Message> {
    let (name, thumb) = match &app.plex.account {
        Some(account) => (account.name.clone(), account.thumb.as_str()),
        None => (enzui_core::localization::text("video-account-7e1b0d"), ""),
    };
    let mut face = enzui_widgets::avatar::enz_avatar(&name).avatar_size(28.0);
    if let Some(photo) = app.plex.avatars.get(thumb) {
        face = face.image(photo.clone());
    }
    row![
        face.into_element(),
        enzui_widgets::menu_button::enz_menu_button(
            name,
            &app.menus.account.0,
            &app.menus.account.1,
            Message::AccountMenu,
        )
        .kind(enzui_widgets::menu_button::MenuButtonKind::Quiet(
            enzui_widgets::button::ButtonSize::Md,
        ))
        .menu_id(iced::advanced::widget::Id::new("video-account-menu"))
        .aria_label(enzui_core::localization::text("video-account-7e1b0d"))
        .alignment(enzui_core::placement::Alignment::End)
        .into_element(),
    ]
    .mirrored(crate::l10n::rtl())
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}
fn plex_shelf_columns(app: &App) -> usize {
    crate::plex::shelf_columns(app.window_size.width)
}
fn state_shelf(app: &App, library: usize) -> usize {
    app.plex.shelves.get(&library).copied().unwrap_or(0)
}
fn section_title<'a>(title: impl Into<String>) -> Element<'a, Message> {
    enz_text(title).size(TextSize::Title2).into_element()
}
fn card_width(app: &App, columns: usize) -> f32 {
    let spacing = app.themes.theme().spacing;
    ((app.window_size.width
        - 2.0 * spacing.lg()
        - (columns.saturating_sub(1) as f32) * spacing.md())
        / columns as f32)
        .max(90.0)
}
fn runtime(milliseconds: u64) -> String {
    let minutes = milliseconds / 60_000;
    if minutes == 0 {
        enzui_core::localized!("video-s-af37e5", "value0" => milliseconds.div_ceil(1000))
    } else if minutes >= 60 {
        enzui_core::localized!("video-h-min-7b3016", "value0" => minutes / 60, "value1" => minutes % 60)
    } else {
        enzui_core::localized!("video-minutes-min-e1bded", "minutes" => minutes)
    }
}
fn resolution(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "4k" => "4K".into(),
        "sd" => "SD".into(),
        digits if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
            format!("{digits}p")
        }
        other => other.to_uppercase(),
    }
}
fn metadata(item: &Item) -> String {
    let mut parts = Vec::new();
    if let Some(year) = item.year {
        parts.push(year.to_string());
    }
    if !item.content_rating.is_empty() {
        parts.push(item.content_rating.clone());
    }
    if item.duration > 0 {
        parts.push(runtime(item.duration));
    }
    if let Some(rating) = item
        .audience_rating
        .filter(|rating| rating.is_finite() && (0.0..=10.0).contains(rating))
    {
        parts.push(enzui_core::localized!("video-rating-1-008555", "rating" => crate::l10n::number(f64::from(rating), 1)));
    }
    parts.join("  ·  ")
}
fn banner<'a>(
    app: &'a App,
    image: Option<&iced::widget::image::Handle>,
    title: &str,
    eyebrow: Eyebrow,
    summary: &str,
    actions: Vec<Element<'a, Message>>,
    height: f32,
) -> Element<'a, Message> {
    let compact = app.window_size.width < 700.0;
    let white = iced::Color::WHITE;
    const TRAIL_HEIGHT: f32 = 26.0;
    // Links carry a hover inset; the hero starts that much further left so their text aligns.
    const CRUMB_INSET: f32 = 8.0;
    let linked = !eyebrow.crumbs.is_empty();
    let segment = |element: Element<'a, Message>| {
        container(element)
            .height(TRAIL_HEIGHT)
            .align_y(iced::Alignment::Center)
    };
    let Eyebrow {
        crumbs,
        text: eyebrow,
    } = eyebrow;
    let mut segments: Vec<Element<'a, Message>> = crumbs
        .into_iter()
        .map(|(label, event)| {
            enz_pressable(
                segment(
                    enz_text(label)
                        .size(TextSize::Subheadline)
                        .color(white)
                        .single_line_elided()
                        .into_element(),
                )
                .into(),
                Message::Plex(event),
            )
            .style(PressableStyle::Plain)
            .border_width(0.0)
            .radius(app.themes.theme().radius.step(app.themes.theme().shape.row))
            .padding([0.0, CRUMB_INSET])
            .width(Length::Shrink)
            .into_element()
        })
        .collect();
    if !eyebrow.is_empty() {
        segments.push(
            segment(
                enz_text(eyebrow)
                    .size(TextSize::Subheadline)
                    .color(iced::Color { a: 0.82, ..white })
                    .single_line_elided()
                    .into_element(),
            )
            .padding([0.0, if linked { CRUMB_INSET } else { 0.0 }])
            .into(),
        );
    }
    let mut trail = row![]
        .mirrored(crate::l10n::rtl())
        .spacing(2)
        .align_y(iced::Alignment::Center);
    for (index, part) in segments.into_iter().enumerate() {
        if index > 0 {
            trail = trail.push(segment(
                enzui_widgets::icon::enz_icon("navigation.forward")
                    .size(12.0)
                    .tint(iced::Color { a: 0.7, ..white })
                    .into_element(),
            ));
        }
        trail = trail.push(part);
    }
    let indent = if linked { CRUMB_INSET } else { 0.0 };
    let indented = |element: Element<'a, Message>| {
        container(element).padding(iced::Padding {
            left: if crate::l10n::rtl() { 0.0 } else { indent },
            right: if crate::l10n::rtl() { indent } else { 0.0 },
            ..Default::default()
        })
    };
    let mut content = column![
        trail,
        indented(
            enz_text(title)
                .size(if compact || height < 180.0 {
                    TextSize::Title2
                } else {
                    TextSize::LargeTitle
                })
                .lines(2)
                .color(white)
                .into_element()
        )
    ]
    .spacing(8);
    if !summary.is_empty() {
        content = content.push(indented(
            enz_text(summary)
                .lines(if compact { 2 } else { 3 })
                .color(iced::Color { a: 0.9, ..white })
                .into_element(),
        ));
    }
    if !actions.is_empty() {
        content = content.push(indented(
            row(actions)
                .mirrored(crate::l10n::rtl())
                .spacing(10)
                .align_y(iced::Alignment::Center)
                .into(),
        ));
    }
    let foreground =
        container(container(content).width(Length::Fill.max(if compact { 430.0 } else { 530.0 })))
            .padding(iced::Padding {
                top: 20.0,
                right: if crate::l10n::rtl() {
                    24.0 - indent
                } else {
                    24.0
                },
                bottom: 24.0,
                left: if crate::l10n::rtl() {
                    24.0
                } else {
                    24.0 - indent
                },
            })
            .width(Length::Fill)
            .height(height)
            .align_y(iced::Alignment::End)
            .style(move |_| container::Style {
                background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                    iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
                        .add_stop(0.0, iced::Color::from_rgba(0.03, 0.04, 0.06, 0.94))
                        .add_stop(0.55, iced::Color::from_rgba(0.03, 0.04, 0.06, 0.68))
                        .add_stop(1.0, iced::Color::from_rgba(0.03, 0.04, 0.06, 0.10)),
                ))),
                ..Default::default()
            });
    let background: Element<'a, Message> = match image {
        Some(image) => enz_image_handle(image.clone())
            .fit(EnzImageFit::Cover)
            .radius(0.0)
            .alt_text(enzui_core::localized!("video-title-artwork-c26eca", "title" => title))
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into_element(),
        _ => container(Space::new())
            .width(Length::Fill)
            .height(height)
            .style(move |_| container::Style {
                background: Some(app.themes.theme().palette.surface_alt.into()),
                ..Default::default()
            })
            .into(),
    };
    stack![background, foreground].into()
}
struct Eyebrow {
    crumbs: Vec<(String, Event)>,
    text: String,
}
impl<'s> From<&'s str> for Eyebrow {
    fn from(text: &'s str) -> Self {
        Self {
            crumbs: Vec::new(),
            text: text.to_owned(),
        }
    }
}
impl From<String> for Eyebrow {
    fn from(text: String) -> Self {
        Self {
            crumbs: Vec::new(),
            text,
        }
    }
}
// The banner's main action: a solid button, so it leads over the artwork.
// The banner keeps its dark scrim in both schemes, so the button is white.
fn hero_play<'a>(app: &App, label: impl Into<String>, event: Event) -> Element<'a, Message> {
    let mut tokens = enzui_widgets::tokens::ButtonTokens::from_palette(&app.themes.theme().palette);
    tokens.primary_bg = iced::Color::WHITE;
    tokens.primary_hover_bg = iced::Color::from_rgb(0.88, 0.88, 0.9);
    tokens.primary_pressed_bg = iced::Color::from_rgb(0.78, 0.78, 0.8);
    tokens.primary_fg = iced::Color::from_rgb(0.07, 0.07, 0.08);
    tokens.primary_border = iced::Color::TRANSPARENT;
    tokens.primary_border_width = 0.0;
    enz_button(label, Message::Plex(event))
        .kind(ButtonKind::Primary)
        .tokens(tokens)
        .size(ButtonSize::Lg)
        .icon("play")
        .into_element()
}
fn glass_action<'a>(label: impl Into<String>, icon: &str, event: Event) -> Element<'a, Message> {
    enz_glass(
        enz_button(label, Message::Plex(event))
            .kind(ButtonKind::Quiet)
            .size(ButtonSize::Lg)
            .icon(icon.to_owned())
            .into_element(),
    )
    .style(MaterialStyle::Frosted)
    .ink(GlassInk::Theme)
    .radius(enzui_theme::text_defaults::shape_radius(|shape| {
        shape.button
    }))
    .into_element()
}
fn home<'a>(
    app: &'a App,
    mut body: iced::widget::Column<'a, Message>,
    connected: &'a crate::plex::Connected,
    top_inset: f32,
) -> Element<'a, Message> {
    let spacing = app.themes.theme().spacing;
    let hero = if let Some(preview) = app
        .plex
        .home
        .iter()
        .find(|preview| !preview.page.items.is_empty())
    {
        let featured = preview
            .page
            .items
            .iter()
            .position(|item| !item.art.is_empty())
            .unwrap_or(0);
        let item = &preview.page.items[featured];
        Some(banner(
            app,
            preview.backdrop.as_ref(),
            &item.title,
            enzui_core::localized!("video-recently-added-eb1d19", "value0" => metadata(item))
                .as_str()
                .into(),
            &item.summary,
            vec![
                hero_play(
                    app,
                    enzui_core::localization::text("video-play-436e61"),
                    Event::QuickPlay(Some(preview.library), featured),
                ),
                glass_action(
                    enzui_core::localization::text("video-more-info-7dd4d9"),
                    "info",
                    Event::Preview(preview.library, featured),
                ),
            ],
            if app.window_size.width < 700.0 {
                260.0
            } else {
                350.0
            },
        ))
    } else {
        Some(banner(
            app,
            None,
            &connected.server.name,
            enzui_core::localized!("video-your-collection-f7e4a4", "value0" => connected.server.kind.name())
                .as_str()
                .into(),
            "",
            vec![],
            210.0,
        ))
    };
    let heading = enz_text(enzui_core::localization::text(
        "video-your-libraries-ad51aa",
    ))
    .size(TextSize::Title2)
    .into_element();
    let controls = row![source_control(app, connected), account_control(app)]
        .mirrored(crate::l10n::rtl())
        .spacing(spacing.sm())
        .align_y(iced::Alignment::Center);
    let libraries_header: Element<'_, Message> = if app.window_size.width < 560.0 {
        column![heading, controls]
            .spacing(spacing.sm())
            .width(Length::Fill)
            .into()
    } else {
        row![heading, Space::new().width(Length::Fill), controls]
            .mirrored(crate::l10n::rtl())
            .spacing(spacing.sm())
            .align_y(iced::Alignment::Center)
            .into()
    };
    body = body.push(libraries_header);
    if connected.libraries.is_empty() {
        body = body.push(
            enz_text(enzui_core::localization::text(
                "video-this-server-has-no-accessible-video-libraries-1b716e",
            ))
            .into_element(),
        );
    }
    let tiles = connected
        .libraries
        .iter()
        .enumerate()
        .map(|(index, library)| {
            let preview = app
                .plex
                .home
                .iter()
                .find(|preview| preview.library == index);
            let subtitle = preview.map_or_else(
                || {
                    if library.r#type == "show" {
                        enzui_core::localization::text("video-shows-seasons-and-episodes-a7b5b5")
                    } else {
                        enzui_core::localization::text("video-movies-in-your-collection-6e33b4")
                    }
                },
                |preview| {
                    enzui_core::localized!(if library.r#type == "show" { "video-show-count" } else { "video-movie-count" }, "count" => preview.page.total)
                },
            );
            enz_pressable(
                banner(
                    app,
                    preview.and_then(|preview| preview.backdrop.as_ref()),
                    &library.title,
                    subtitle.as_str().into(),
                    "",
                    vec![],
                    132.0,
                ),
                Message::Plex(Event::Library(index)),
            )
            .id(iced::advanced::widget::Id::from(format!("video-library-{}", library.key)))
            .aria_label(enzui_core::localized!("video-open-202010", "value0" => library.title))
            .style(PressableStyle::Plain)
            .border_width(0.0)
            .radius(app.themes.theme().radius.step(app.themes.theme().shape.card))
            .into_element()
        })
        .collect();
    body = body.push(
        enz_grid(if app.window_size.width < 560.0 { 1 } else { 2 }, tiles)
            .gap(spacing.md())
            .stretch_cells(true)
            .into_element(),
    );
    for shelf in &app.plex.activity {
        body = body.push(activity_shelf(app, shelf));
    }
    for preview in &app.plex.home {
        let Some(library) = connected.libraries.get(preview.library) else {
            continue;
        };
        if preview.page.items.is_empty() {
            continue;
        }
        let columns = plex_shelf_columns(app);
        let pages = preview.page.items.len().div_ceil(columns).max(1);
        let page = state_shelf(app, preview.library).min(pages - 1);
        let mut header = row![
            enz_text(enzui_core::localized!("video-new-in-106a25", "value0" => library.title))
                .size(TextSize::Title2)
                .into_element(),
            Space::new().width(Length::Fill),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(spacing.xs())
        .align_y(iced::Alignment::Center);
        if pages > 1 {
            header = header
                .push(
                    enz_toolbar_icon(
                        "navigation.back",
                        Message::Plex(Event::ShelfPage(preview.library, false)),
                    )
                    .label(
                        enzui_core::localized!("video-previous-3c699e", "value0" => library.title),
                    )
                    .enabled(page > 0)
                    .into_element(),
                )
                .push(
                    enz_toolbar_icon(
                        "navigation.forward",
                        Message::Plex(Event::ShelfPage(preview.library, true)),
                    )
                    .label(enzui_core::localized!("video-next-6a93bd", "value0" => library.title))
                    .enabled(page + 1 < pages)
                    .into_element(),
                );
        }
        body = body.push(header.push(button(
            enzui_core::localization::text("video-see-all-d7a8c4"),
            Event::Library(preview.library),
        )));
        let poster_height = card_width(app, columns) * 1.5;
        let mut cards: Vec<Element<'_, Message>> = preview
            .page
            .items
            .iter()
            .enumerate()
            .skip(page * columns)
            .take(columns)
            .map(|(index, item)| {
                let handle = preview
                    .page
                    .posters
                    .iter()
                    .find(|(key, _)| key == &item.rating_key)
                    .map(|(_, image)| image);
                art_tile(
                    app,
                    item,
                    handle,
                    poster_height,
                    Event::Preview(preview.library, index),
                    Event::QuickPlay(Some(preview.library), index),
                )
            })
            .collect();
        // A short last page keeps full-size cards instead of stretching them.
        while cards.len() < columns {
            cards.push(Space::new().width(Length::Fill).into());
        }
        body = body.push(
            enz_grid(columns, cards)
                .gap(spacing.md())
                .stretch_cells(true)
                .into_element(),
        );
    }
    framed(app, hero, body.into(), top_inset)
}
fn activity_shelf<'a>(app: &'a App, shelf: &'a crate::plex::ActivityShelf) -> Element<'a, Message> {
    let section = shelf.section;
    let columns = plex_shelf_columns(app);
    let mut header = row![
        section_title(section.label()),
        Space::new().width(Length::Fill)
    ]
    .align_y(iced::Alignment::Center)
    .spacing(4);
    let content = match &shelf.result {
        Err(error) => enz_text(enzui_core::localized!("video-history-unavailable", "error" => crate::l10n::error(error))).tone(TextTone::Muted).into_element(),
        Ok(page) if page.items.is_empty() => enz_text(enzui_core::localization::text("video-history-empty")).tone(TextTone::Muted).into_element(),
        Ok(page) => {
            let last = page.items.len().saturating_sub(1) / columns;
            let visible_page = shelf.visible_page.min(last);
            if last > 0 {
                header = header.push(enz_toolbar_icon("navigation.back", Message::Plex(Event::ActivityPage(section, false)))
                    .label(enzui_core::localized!("video-previous-3c699e", "value0" => section.label())).enabled(visible_page > 0).into_element())
                    .push(enz_toolbar_icon("navigation.forward", Message::Plex(Event::ActivityPage(section, true)))
                    .label(enzui_core::localized!("video-next-6a93bd", "value0" => section.label())).enabled(visible_page < last).into_element());
            }
            let height = card_width(app, columns) * 1.5;
            let mut cards: Vec<_> = page.items.iter().enumerate().skip(visible_page * columns).take(columns)
                .map(|(index, item)| art_tile(app, item, page.posters.iter().find(|(key, _)| key == &item.rating_key).map(|(_, image)| image), height,
                    Event::ActivityItem(section, index, false), Event::ActivityItem(section, index, true))).collect();
            while cards.len() < columns { cards.push(Space::new().width(Length::Fill).into()); }
            enz_grid(columns, cards).gap(app.themes.theme().spacing.md()).stretch_cells(true).into_element()
        }
    };
    column![header, content]
        .spacing(app.themes.theme().spacing.md())
        .into()
}
fn framed<'a>(
    app: &'a App,
    hero: Option<Element<'a, Message>>,
    content: Element<'a, Message>,
    top_inset: f32,
) -> Element<'a, Message> {
    let body = column![
        container(content)
            .padding(app.themes.theme().spacing.lg())
            .width(Length::Fill)
    ];
    let body: Element<'a, Message> = if let Some(hero) = hero {
        column![hero, body].into()
    } else {
        container(body)
            .padding(iced::Padding {
                top: top_inset,
                ..Default::default()
            })
            .into()
    };
    enz_scroll_view(body)
        .id(iced::advanced::widget::Id::new("plex-library"))
        .height(Length::Fill)
        .into_element()
}
fn art_tile<'a>(
    app: &'a App,
    item: &'a Item,
    image: Option<&iced::widget::image::Handle>,
    height: f32,
    details: Event,
    play: Event,
) -> Element<'a, Message> {
    let image = image.cloned();
    let radius = app
        .themes
        .theme()
        .radius
        .step(app.themes.theme().shape.card);
    let progress = (item.view_offset > 0 && item.duration > 0)
        .then(|| (item.view_offset as f32 / item.duration as f32).clamp(0.0, 1.0));
    let artwork = enz_hover_group_with(move |hovered| {
        let picture = match &image {
            Some(image) => enz_image_handle(image.clone())
                .fit(EnzImageFit::Cover)
                .radius(radius)
                .alt_text(item.title.clone())
                .width(Length::Fill)
                .height(Length::Fixed(height))
                .into_element(),
            _ => poster(app, item, height),
        };
        let is_series = matches!(item.r#type.as_str(), "show" | "season");
        let play_button = enz_glass(
            enz_toolbar_icon(
                if is_series { "layers" } else { "play" },
                Message::Plex(play.clone()),
            )
            .size(52.0, 22.0)
            .variant(ToolbarIconVariant::InkOnly)
            .aria_label(if is_series {
                enzui_core::localized!("video-browse-episodes-title", "title" => item.title)
            } else {
                enzui_core::localized!("video-play-5cd3b2", "value0" => item.title)
            })
            .into_element(),
        )
        .style(MaterialStyle::Frosted)
        .ink(GlassInk::Theme)
        .radius(
            app.themes
                .theme()
                .radius
                .step(app.themes.theme().shape.button),
        );
        let scrim = container(Space::new())
            .width(Length::Fill)
            .height(height)
            .style(move |_| container::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.32).into()),
                border: iced::Border {
                    radius: radius.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        let revealed = hovered || image.is_none();
        // Glass inside the fading group would sample the group, not the artwork beneath it.
        let play = revealed.then(|| {
            container(play_button)
                .center_x(Length::Fill)
                .center_y(height)
        });
        let mut layers = stack![picture, motion(scrim, hovered, MotionEffect::Fade)]
            .extend(play.map(Element::from));
        if let Some(progress) = progress {
            layers = layers.push(
                container(
                    enzui_widgets::progress_bar::enz_progress_bar(progress)
                        .animated(false)
                        .into_element(),
                )
                .padding(iced::Padding {
                    left: 10.0,
                    right: 10.0,
                    bottom: 10.0,
                    top: 0.0,
                })
                .width(Length::Fill)
                .height(height)
                .align_y(iced::Alignment::End),
            );
        }
        enz_clickable(layers, Message::Plex(details.clone()))
            .aria_label(
                enzui_core::localized!("video-open-details-for-5a17c2", "value0" => item.title),
            )
            .description(enzui_core::localization::text(
                "video-open-details-to-play-or-choose-an-episode-02299e",
            ))
            .width(Length::Fill)
            .height(height)
            .radius(radius)
            .into_element()
    })
    .width(Length::Fill)
    .height(height)
    .style(|_, _| HoverGroupStyle::NONE)
    .into_element();
    let (title, subtitle) = caption(item);
    column![
        artwork,
        column![
            enz_text(title)
                .size(TextSize::Subheadline)
                .single_line_elided()
                .into_element(),
            enz_text(subtitle)
                .size(TextSize::Caption)
                .tone(TextTone::Muted)
                .single_line_elided()
                .into_element(),
        ]
        .spacing(2)
        .padding(iced::Padding {
            left: 2.0,
            right: 2.0,
            top: 0.0,
            bottom: 0.0,
        }),
    ]
    .spacing(8)
    .width(Length::Fill)
    .into()
}
fn caption(item: &Item) -> (String, String) {
    match item.r#type.as_str() {
        "episode" => {
            let title = match item.index {
                Some(index) => {
                    enzui_core::localized!("video-numbered-title", "index" => index, "title" => item.title)
                }
                None => item.title.clone(),
            };
            let mut details = Vec::new();
            if item.duration > 0 {
                details.push(if item.view_offset > 0 {
                    enzui_core::localized!("video-left-061a33", "value0" => runtime(item.duration.saturating_sub(item.view_offset)))
                } else {
                    runtime(item.duration)
                });
            }
            if item.view_count > 0 && item.view_offset == 0 {
                details.push(enzui_core::localization::text("video-watched-1ca8c1"));
            }
            (title, details.join("  ·  "))
        }
        "season" | "show" => {
            let detail = match (item.leaf_count, item.viewed_leaf_count) {
                (Some(total), Some(seen)) if seen >= total && total > 0 => {
                    enzui_core::localization::text("video-watched-1ca8c1")
                }
                (Some(total), _) if total > 0 => {
                    enzui_core::localized!("video-episode-count", "count" => total)
                }
                _ if item.r#type == "show" => {
                    item.year.map(|year| year.to_string()).unwrap_or_default()
                }
                _ => String::new(),
            };
            (item.title.clone(), detail)
        }
        _ => {
            let mut details: Vec<String> =
                item.year.map(|year| year.to_string()).into_iter().collect();
            if item.view_count > 0 && item.view_offset == 0 {
                details.push(enzui_core::localization::text("video-watched-1ca8c1"));
            }
            (item.title.clone(), details.join("  ·  "))
        }
    }
}
fn poster<'a>(app: &'a App, item: &'a Item, height: f32) -> Element<'a, Message> {
    let radius = app
        .themes
        .theme()
        .radius
        .step(app.themes.theme().shape.card);
    match app.plex.posters.get(&item.rating_key) {
        Some(handle) => enz_image_handle(handle.clone())
            .fit(EnzImageFit::Cover)
            .radius(radius)
            .alt_text(item.title.clone())
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into_element(),
        _ => {
            let palette = &app.themes.theme().palette;
            container(
                enzui_widgets::icon::enz_icon("film")
                    .size(40.0)
                    .tint(palette.text_muted)
                    .into_element(),
            )
            .center_x(Length::Fill)
            .center_y(height)
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(palette.surface_alt.into()),
                border: iced::Border {
                    radius: radius.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        }
    }
}
fn detail<'a>(
    app: &'a App,
    mut body: iced::widget::Column<'a, Message>,
    item: &'a Item,
    top_inset: f32,
) -> Element<'a, Message> {
    let spacing = app.themes.theme().spacing;
    let backdrop = app
        .plex
        .backdrop
        .as_ref()
        .filter(|(key, _)| key == &item.rating_key)
        .map(|(_, image)| image);
    let mut actions = Vec::new();
    if item.view_offset > 0 {
        actions.push(hero_play(
                    app,
            &enzui_core::localized!("video-resume-from-30a94f", "value0" => crate::model::time::clock(
                std::time::Duration::from_millis(item.view_offset),
                false
            )),
            Event::Play(true),
        ));
        actions.push(glass_action(
            enzui_core::localization::text("video-play-from-start-99d3c4"),
            "rotate-ccw",
            Event::Play(false),
        ));
    } else {
        actions.push(hero_play(
            app,
            enzui_core::localization::text("video-play-436e61"),
            Event::Play(false),
        ));
    }
    let mut crumbs = Vec::new();
    let eyebrow = if item.r#type == "episode" {
        if !item.grandparent_title.is_empty() {
            crumbs.push((item.grandparent_title.clone(), Event::Ancestor(false)));
        }
        if !item.parent_title.is_empty() {
            crumbs.push((item.parent_title.clone(), Event::Ancestor(true)));
        }
        let mut parts = Vec::new();
        if let Some(index) = item.index {
            parts.push(enzui_core::localized!("video-episode-index-dbb178", "index" => index));
        }
        let details = metadata(item);
        if !details.is_empty() {
            parts.push(details);
        }
        parts.join("  ·  ")
    } else {
        metadata(item)
    };
    let hero = Some(banner(
        app,
        backdrop,
        &item.title,
        Eyebrow {
            crumbs,
            text: eyebrow,
        },
        &item.tagline,
        actions,
        if app.window_size.width < 700.0 {
            280.0
        } else {
            380.0
        },
    ));
    let mut facts: Vec<(String, String)> = Vec::new();
    for (label, tags, limit) in [
        (
            enzui_core::localization::text("video-genres-3561ab"),
            &item.genres,
            4,
        ),
        (
            enzui_core::localization::text("video-director-c149bc"),
            &item.directors,
            3,
        ),
    ] {
        let names = tags
            .iter()
            .take(limit)
            .map(|tag| tag.tag.as_str())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !names.is_empty() {
            facts.push((label, names));
        }
    }
    if !item.studio.is_empty() {
        facts.push((
            enzui_core::localization::text("video-studio-0aa91a"),
            item.studio.clone(),
        ));
    }
    if let Some(media) = item.media.first() {
        let video = [
            resolution(&media.resolution),
            media.container.to_uppercase(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  ·  ");
        if !video.is_empty() {
            facts.push((enzui_core::localization::text("video-app-name"), video));
        }
    }
    let mut information = column![].spacing(spacing.lg()).width(Length::Fill);
    if !item.summary.is_empty() {
        information = information.push(
            enz_text(item.summary.as_str())
                .size(TextSize::Body)
                .into_element(),
        );
    }
    let mut fact_list = column![].spacing(spacing.sm());
    for (label, value) in facts {
        fact_list = fact_list.push(
            row![
                container(
                    enz_text(label)
                        .size(TextSize::Subheadline)
                        .tone(TextTone::Muted)
                        .into_element()
                )
                .width(96.0),
                enz_text(value).size(TextSize::Subheadline).into_element(),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(spacing.md()),
        );
    }
    information = information.push(fact_list);
    let wide = app.window_size.width >= 760.0;
    let (width, height) = if wide { (220.0, 330.0) } else { (130.0, 195.0) };
    let poster_height = if item.r#type == "episode" {
        width * 9.0 / 16.0
    } else {
        height
    };
    let resume = item.view_offset > 0;
    let cover = enz_hover_group_with(move |hovered| {
        let play = hovered.then(|| {
            container(
                enz_glass(
                    enz_toolbar_icon("play", Message::Plex(Event::Play(resume)))
                        .size(52.0, 22.0)
                        .variant(ToolbarIconVariant::InkOnly)
                        .aria_label(
                            enzui_core::localized!("video-play-5cd3b2", "value0" => item.title),
                        )
                        .into_element(),
                )
                .style(MaterialStyle::Frosted)
                .ink(GlassInk::Theme)
                .radius(
                    app.themes
                        .theme()
                        .radius
                        .step(app.themes.theme().shape.button),
                ),
            )
            .center_x(Length::Fill)
            .center_y(poster_height)
        });
        enz_clickable(
            stack![poster(app, item, poster_height)].extend(play.map(Element::from)),
            Message::Plex(Event::Play(resume)),
        )
        .aria_label(enzui_core::localized!("video-play-5cd3b2", "value0" => item.title))
        .width(Length::Fill)
        .height(poster_height)
        .radius(
            app.themes
                .theme()
                .radius
                .step(app.themes.theme().shape.card),
        )
        .into_element()
    })
    .width(Length::Fill)
    .height(poster_height)
    .style(|_, _| HoverGroupStyle::NONE)
    .into_element();
    body = body.push(
        row![container(cover).width(width), information]
            .mirrored(crate::l10n::rtl())
            .spacing(spacing.xl()),
    );
    let related = &app.plex.related;
    if related.0 == item.rating_key && !related.1.is_empty() {
        let episodes = item.r#type == "episode";
        let (min_width, aspect, limit) = if episodes {
            (280.0, 9.0 / 16.0, usize::MAX)
        } else {
            (
                170.0,
                1.5,
                crate::plex::shelf_columns(app.window_size.width),
            )
        };
        let columns = ((app.window_size.width - 32.0) / min_width)
            .floor()
            .clamp(if episodes { 1.0 } else { 2.0 }, 8.0) as usize;
        let columns = columns.min(limit);
        let art_height = card_width(app, columns) * aspect;
        let mut cards: Vec<Element<'a, Message>> = related
            .1
            .iter()
            .enumerate()
            .filter(|(_, other)| other.rating_key != item.rating_key)
            .take(limit)
            .map(|(index, other)| {
                art_tile(
                    app,
                    other,
                    app.plex.posters.get(&other.rating_key),
                    art_height,
                    Event::OpenRelated(index),
                    Event::PlayRelated(index),
                )
            })
            .collect();
        while cards.len() < columns {
            cards.push(Space::new().width(Length::Fill).into());
        }
        let heading = if episodes {
            if item.parent_title.is_empty() {
                enzui_core::localization::text("video-more-episodes-09ed24")
            } else {
                enzui_core::localized!("video-more-from-df8f20", "value0" => item.parent_title)
            }
        } else {
            enzui_core::localization::text("video-more-like-this-ee548a")
        };
        body = body
            .push(enz_text(heading).size(TextSize::Title2).into_element())
            .push(
                enz_grid(columns, cards)
                    .gap(spacing.md())
                    .stretch_cells(true)
                    .into_element(),
            );
    }
    if let Some(cast) = cast_section(app, item) {
        body = body.push(cast);
    }
    framed(app, hero, body.into(), top_inset)
}

fn cast_section<'a>(app: &'a App, item: &'a Item) -> Option<Element<'a, Message>> {
    let spacing = app.themes.theme().spacing;
    let cast: Vec<Element<'a, Message>> = item
        .cast
        .iter()
        .filter(|person| !person.tag.is_empty())
        .take(crate::plex::cast::SHOWN)
        .map(|person| {
            let mut face = enzui_widgets::avatar::enz_avatar(&person.tag).avatar_size(48.0);
            if let Some(photo) = app.plex.cast_photos.get(&person.thumb) {
                face = face.image(photo.clone());
            }
            row![
                face.into_element(),
                column![
                    enz_text(person.tag.as_str())
                        .size(TextSize::Subheadline)
                        .single_line_elided()
                        .into_element(),
                    enz_text(person.role.as_str())
                        .size(TextSize::Caption)
                        .tone(TextTone::Muted)
                        .single_line_elided()
                        .into_element(),
                ]
                .spacing(2),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(spacing.sm())
            .align_y(iced::Alignment::Center)
            .into()
        })
        .collect();
    if cast.is_empty() {
        return None;
    }
    let columns = ((app.window_size.width - 32.0) / 220.0)
        .floor()
        .clamp(1.0, 6.0) as usize;
    Some(
        column![
            section_title(enzui_core::localization::text("video-cast-150040")),
            enz_grid(columns, cast)
                .gap(spacing.md())
                .stretch_cells(true)
                .into_element(),
        ]
        .spacing(spacing.lg())
        .into(),
    )
}
