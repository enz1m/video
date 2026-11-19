//! What fills the window: the start page, or the picture and the states
//! that stand in for it (opening, buffering, failed, sound only).

use super::osd;
use crate::app::{App, Message, Pick};
use crate::model::Entry;
use crate::model::history::{self, Recent};
use crate::model::time::clock;
use enzui_core::Action;
use enzui_core::live_region::Announcement;
use enzui_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_widgets::clamp::{ClampSize, enz_clamp};
use enzui_widgets::icon::enz_icon;
use enzui_widgets::image_widget::{EnzImageFit, enz_image_handle};
use enzui_widgets::list_row_surface::{TrailingVisibility, enz_list_row_surface};
use enzui_widgets::scroll_view::enz_scroll_view;
use enzui_widgets::semantics_container::SemanticsContainer;
use enzui_widgets::spinner::enz_spinner;
use enzui_widgets::status_page::{StatusPageTone, enz_status_page};
use enzui_widgets::text::{TextAlign, TextSize, TextTone, enz_text};
use enzui_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Space, column, container, row};
use iced::{Element, Length};
use std::time::Duration;

pub(crate) fn start_page<'a>(
    app: &'a App,
    geometry: &enzui_widgets::app_shell::ShellGeometry,
) -> Element<'a, Message> {
    let sp = app.themes.theme().spacing;
    let page = match &app.video {
        Err(reason) => enz_status_page(enzui_core::localization::text("video-videos-can-t-be-played-2af62d"))
            .icon("circle-alert")
            .tone(StatusPageTone::Error)
            .description(enzui_core::localized!("video-reason-install-gstreamer-with-its-base-and-good-plugins-ba6474", "reason" => reason)),
        Ok(_) => enz_status_page(enzui_core::localization::text("video-watch-a-video-c6bc64"))
            .icon("clapperboard")
            .description(enzui_core::localization::text("video-open-a-video-drop-one-onto-this-window-or-play-a-stream-a3fc1e"))
            .primary_action(
                Action::new("open", Message::Choose(Pick::Videos)).with_label(enzui_core::localization::text("video-open-video-a98884")),
            )
            .secondary_action(
                Action::new("url", Message::OpenUrlDialog).with_label(enzui_core::localization::text("video-open-network-stream-fd5713")),
            ),
    };
    let history = &app.settings.history;
    let recent = history.local_items();
    let has_recent = recent.clone().next().is_some() && app.video.is_ok();
    let mut body = column![
        page.into_element(),
        enz_button(browse_label(app), Message::Plex(crate::plex::Event::Open))
            .id(iced::advanced::widget::Id::new("video-browse-library"))
            .kind(ButtonKind::Quiet)
            .icon(if app.settings.sources.is_empty() {
                "server"
            } else {
                "library"
            })
            .into_element()
    ]
    .spacing(sp.xl())
    .width(Length::Fill)
    .align_x(iced::Alignment::Center);
    if !has_recent {
        let [top, right, bottom, left] = geometry.content_inset;
        return container(body)
            .center(Length::Fill)
            .padding(iced::Padding {
                top: top + sp.lg(),
                right: right + sp.lg(),
                bottom: bottom + sp.lg(),
                left: left + sp.lg(),
            })
            .into();
    }
    let rows = recent
        .clone()
        .map(|(index, item)| recent_row(app, index, item));
    body = body.push(
        column![
            row![
                enz_text(if recent.clone().any(|(_, item)| item.position_ms > 0) {
                    enzui_core::localization::text("video-continue-watching-81ebbe")
                } else {
                    enzui_core::localization::text("video-recently-watched-eb33c3")
                })
                .size(TextSize::Title3)
                .into_element(),
                Space::new().width(Length::Fill),
                enz_button(
                    enzui_core::localization::text("video-clear-list-a2a6ef"),
                    Message::ClearRecent
                )
                .id(iced::advanced::widget::Id::new("video-clear-list-a2a6ef"))
                .kind(ButtonKind::Quiet)
                .size(ButtonSize::Sm)
                .icon("trash-2")
                .into_element(),
            ]
            .mirrored(crate::l10n::rtl())
            .align_y(iced::Alignment::Center)
            .padding(iced::Padding {
                left: ROW_INSET,
                ..Default::default()
            }),
            column(rows).spacing(2),
        ]
        .spacing(sp.sm())
        .width(Length::Fill),
    );
    let page = enz_clamp(
        container(body)
            .padding([sp.xxl(), sp.lg()])
            .width(Length::Fill),
        ClampSize::Default,
    );
    let [top, right, bottom, left] = geometry.content_inset;
    enz_scroll_view(
        container(page)
            .center_x(Length::Fill)
            .padding(iced::Padding {
                top,
                right,
                bottom,
                left,
            }),
    )
    .scrollbar_insets(geometry.scrollbar_safe_area())
    .height(Length::Fill)
    .into_element()
}

const ROW_INSET: f32 = 4.0 + 8.0;
const THUMB: (f32, f32) = (96.0, 54.0);

fn recent_row<'a>(app: &'a App, index: usize, item: &'a Recent) -> Element<'a, Message> {
    let palette = &app.themes.theme().palette;
    let radius = app
        .themes
        .theme()
        .radius
        .step(app.themes.theme().shape.media);
    let compact = app.window_size.width < 560.0;
    let picture: Element<'a, Message> =
        match app.thumbnails.get(&history::thumbnail_key(&item.entry)) {
            Some(handle) => enz_image_handle(handle.clone())
                .fit(EnzImageFit::Cover)
                .radius(radius)
                .alt_text("")
                .width(Length::Fixed(THUMB.0))
                .height(Length::Fixed(THUMB.1))
                .into_element(),
            None => {
                let well = palette.surface_alt;
                container(
                    enz_icon(match item.entry {
                        Entry::File(_) => "film",
                        Entry::Url(_) => "globe",
                    })
                    .tint(palette.text_muted)
                    .size(20.0)
                    .into_element(),
                )
                .center_x(Length::Fixed(THUMB.0))
                .center_y(Length::Fixed(THUMB.1))
                .style(move |_| container::Style {
                    background: Some(well.into()),
                    border: iced::Border {
                        radius: radius.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into()
            }
        };
    let place = match &item.entry {
        Entry::File(path) => path
            .parent()
            .map_or_else(String::new, |dir| dir.display().to_string()),
        Entry::Url(url) => url.clone(),
    };
    let mut lines = column![
        enz_text(item.title.clone())
            .size(TextSize::Headline)
            .single_line_elided()
            .into_element(),
        enz_text(place)
            .size(TextSize::Footnote)
            .tone(TextTone::Muted)
            .single_line_elided()
            .into_element(),
    ]
    .spacing(2)
    .width(Length::Fill);
    if let Some(progress) = item.progress().filter(|_| item.position_ms > 0) {
        lines = lines.push(
            container(
                enzui_widgets::progress_bar::enz_progress_bar(progress)
                    .animated(false)
                    .bar_height(4.0)
                    .into_element(),
            )
            .padding(iced::Padding {
                top: 4.0,
                ..Default::default()
            })
            .width(Length::Fill),
        );
    }
    let status: Element<'a, Message> = if item.finished {
        row![
            enz_icon("circle-check")
                .tint(palette.success)
                .size(16.0)
                .into_element(),
            enz_text(enzui_core::localization::text("video-watched-1ca8c1"))
                .size(TextSize::Footnote)
                .tone(TextTone::Muted)
                .into_element(),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        let text = match (item.position_ms, item.duration_ms) {
            (0, Some(total)) => clock(Duration::from_millis(total), false),
            (0, None) => String::new(),
            (at, Some(total)) if total > at => {
                enzui_core::localized!("video-left-061a33", "value0" => clock(Duration::from_millis(total - at), false))
            }
            (at, _) => {
                enzui_core::localized!("video-at-49f214", "value0" => clock(Duration::from_millis(at), false))
            }
        };
        enz_text(text)
            .size(TextSize::Footnote)
            .tone(TextTone::Muted)
            .into_element()
    };
    let details: Element<'a, Message> = if compact {
        column![lines, status].spacing(4).width(Length::Fill).into()
    } else {
        row![lines, status]
            .mirrored(crate::l10n::rtl())
            .spacing(12)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .into()
    };
    let content = row![picture, details]
        .mirrored(crate::l10n::rtl())
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .padding([6.0, 0.0]);
    let forget = enz_toolbar_icon("x", Message::ForgetRecent(index))
        .id(iced::advanced::widget::Id::from(format!(
            "video-forget-{}",
            history::thumbnail_key(&item.entry)
        )))
        .label(enzui_core::localized!("video-remove-from-the-list-aced07", "value0" => item.title))
        .into_element();
    enz_list_row_surface(content, Message::OpenRecent(index))
        .content_height(true)
        .content_padding(8.0)
        .trailing(forget)
        .trailing_visibility(TrailingVisibility::OnHoverOrFocus)
        .label(item.title.clone())
        .id_prefix(format!("recent-{index}"))
        .into_element()
}

pub(crate) fn picture(app: &App) -> Element<'_, Message> {
    let status = &app.status;
    let title = app.current_title().unwrap_or_default();
    let inner: Element<'_, Message> = if let Some(error) = &status.error {
        dark_notice(
            "circle-alert",
            enzui_core::localization::text("video-this-video-can-t-be-played-3418ea"),
            Some(error.message.clone()),
        )
    } else if let Some(handle) = app.presenter.handle() {
        enz_image_handle(handle.clone())
            .fit(EnzImageFit::Contain)
            .placeholder_background(osd::BACKDROP)
            .alt_text(enzui_core::localized!("video-video-title-833ed1", "title" => title))
            .width(Length::Fill)
            .height(Length::Fill)
            .into_element()
    } else if status.loading || app.playlist.current().is_some() && !status.has_source() {
        container(
            column![
                enz_spinner().into_element(),
                enz_text(enzui_core::localized!("video-opening-title-75b6e3", "title" => title))
                    .color(osd::MUTED)
                    .into_element(),
            ]
            .spacing(12)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    } else if !status.has_video || !status.loading && !status.missing_decoders.is_empty() {
        let missing = status.missing_decoders.first().map(|decoder| {
            enzui_core::localized!("video-a-required-decoder-is-not-installed-decoder-install-the-30c050", "decoder" => decoder)
        });
        dark_notice(
            if missing.is_some() {
                "circle-alert"
            } else {
                "audio-lines"
            },
            if missing.is_some() {
                enzui_core::localization::text("video-no-picture-6e9a17")
            } else {
                enzui_core::localization::text("video-sound-only-cabb0b")
            },
            missing.or_else(|| Some(title.clone())),
        )
    } else {
        Space::new().width(Length::Fill).height(Length::Fill).into()
    };
    let mut layers = vec![inner];
    if let Some(percent) = status.buffering {
        layers.push(notice_at(
            &enzui_core::localized!("video-buffering-percent-725981", "percent" => percent),
            true,
        ));
    }
    container(iced::widget::stack(layers))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(osd::BACKDROP.into()),
            ..Default::default()
        })
        .into()
}

fn dark_notice<'a>(
    icon: &str,
    title: impl Into<String>,
    detail: Option<String>,
) -> Element<'a, Message> {
    let mut lines = column![
        enzui_widgets::icon::enz_icon(icon)
            .tint(osd::MUTED)
            .size(40.0)
            .into_element(),
        enz_text(title.into())
            .size(TextSize::Title2)
            .color(osd::TEXT)
            .into_element(),
    ]
    .spacing(12)
    .width(Length::Fill)
    .align_x(iced::Alignment::Center);
    if let Some(detail) = detail.filter(|detail| !detail.is_empty()) {
        lines = lines.push(
            enz_text(detail)
                .color(osd::MUTED)
                .align(TextAlign::Center)
                .into_element(),
        );
    }
    container(enz_clamp(lines, ClampSize::Narrow))
        .center(Length::Fill)
        .padding(24)
        .into()
}

pub(crate) fn notice(text: &str) -> Element<'_, Message> {
    notice_at(text, false)
}

fn notice_at<'a>(text: &str, lower: bool) -> Element<'a, Message> {
    let radius = enzui_theme::text_defaults::shape_radius(|shape| shape.media);
    let pill = container(
        enz_text(text.to_owned())
            .size(TextSize::Headline)
            .color(osd::TEXT)
            .announce(false)
            .into_element(),
    )
    .padding([8, 16])
    .style(move |_| container::Style {
        background: Some(osd::PILL.into()),
        border: iced::Border {
            radius: radius.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let pill: Element<'a, Message> =
        SemanticsContainer::leaf(pill, Announcement::Status.region(text.to_owned())).into();
    let placed = container(pill).center_x(Length::Fill);
    if lower {
        placed.align_bottom(Length::Fill).padding([120, 0]).into()
    } else {
        placed.center_y(Length::Fill).into()
    }
}

fn browse_label(app: &App) -> String {
    match app.settings.sources.as_slice() {
        [] => enzui_core::localization::text("video-connect-a-media-server-da18b5"),
        [source] => enzui_core::localized!("video-browse-de410c", "value0" => source.kind.name()),
        _ => enzui_core::localization::text("video-browse-media-servers-d0c7f9"),
    }
}
