//! The queue, beside the picture: what plays, in order, and what is next.

use crate::app::{App, Message, Pick};
use crate::model::Entry;
use enzui_widgets::button::{ButtonKind, enz_button};
use enzui_widgets::scroll_view::enz_scroll_view;
use enzui_widgets::sidebar_content::enz_sidebar_content;
use enzui_widgets::sidebar_row::enz_sidebar_row;
use enzui_widgets::text::{TextSize, TextTone, enz_text};
use enzui_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{column, row};
use iced::{Element, Length};

pub(crate) const WIDTH: f32 = 300.0;

pub(crate) fn panel(app: &App) -> Element<'_, Message> {
    let theme = app.themes.theme();
    let sp = theme.spacing;
    let current = app.playlist.current_index();
    let rows = app
        .playlist
        .entries()
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let name = entry.name();
            let row_id = iced::advanced::widget::Id::from(format!("video-queue-{}",
                crate::model::history::thumbnail_key(entry)));
            let target = entry.clone();
            let remove = enz_toolbar_icon("x", Message::RemoveFromQueue(index))
                .id(iced::advanced::widget::Id::from(format!("video-queue-remove-{}", crate::model::history::thumbnail_key(entry))))
                .label(enzui_core::localized!("video-remove-name-from-the-queue-8f56ba", "name" => name))
                .into_element();
            let open = enz_sidebar_row(name, Message::PlayIndex(index))
                .id(row_id)
                .icon(match entry {
                    Entry::File(_) => "film",
                    Entry::Url(_) => "link",
                })
                .selected(current == Some(index))
                .trailing(remove)
                .into_element();
            let metadata = app.queue_drag_data(entry);
            let key = (app.queue_drag_owner.clone(), app.playlist.revision(), entry.clone());
            let source: Element<'_, Message> = match entry {
                Entry::File(path) => enzui_widgets::file_drag::enz_file_drag_source(open, [path.clone()])
                    .temporary_files(app.is_temporary_media(path)).key(key.clone()).with_data(crate::app::queue_drop::MIME, metadata)
                    .allows_captured_press(true).into(),
                Entry::Url(_) => enzui_widgets::file_drag::enz_data_drag_source(open, key.clone(),
                    enzui_widgets::file_drag::Payload::new().with(crate::app::queue_drop::MIME, metadata))
                    .allows_captured_press(true).into(),
            };
            let destination = entry.clone();
            let source: Element<'_, Message> = enzui_widgets::file_drop::enz_data_drop_target(source,
                [enzui_widgets::file_drag::MimeType::new(crate::app::queue_drop::MIME)],
                move |drop| Message::QueueDropped(Some(destination.clone()), drop))
                .copy_only(true).key(key).into();
            enzui_widgets::context_target::enz_context_target(source,
                move |at| Message::QueueContext(target.clone(), at))
                .height(Length::Shrink).into_element()

        });
    let heading =
        enz_text(enzui_core::localized!("video-queue-e75185", "value0" => app.playlist.len()))
            .size(TextSize::Headline)
            .into_element();
    let mut body = enz_sidebar_content().spacing_scale(sp);
    if app.window_size.width < 800.0 {
        body = body.push(
            row![
                heading,
                iced::widget::Space::new().width(Length::Fill),
                enz_toolbar_icon("x", Message::TogglePlaylist)
                    .id(iced::advanced::widget::Id::new("video-queue-close"))
                    .label(enzui_core::localization::text("video-close-queue-5433ac"))
                    .into_element()
            ]
            .mirrored(crate::l10n::rtl())
            .align_y(iced::Alignment::Center),
        );
    }
    body.push(
        enz_text(app.playlist.repeat.label())
            .size(TextSize::Footnote)
            .tone(TextTone::Muted)
            .into_element(),
    )
    .push(
        enz_scroll_view(column![
            column(rows).spacing(2),
            enzui_widgets::file_drop::enz_data_drop_target(
                iced::widget::Space::new().width(Length::Fill).height(48),
                [enzui_widgets::file_drag::MimeType::new(
                    crate::app::queue_drop::MIME
                )],
                |drop| Message::QueueDropped(None, drop)
            )
            .copy_only(true)
            .key((app.queue_drag_owner.clone(), app.playlist.revision(), "end"))
        ])
        .height(Length::Fill)
        .into_element(),
    )
    .push(
        enz_button(
            enzui_core::localization::text("video-add-videos-04dc04"),
            Message::Choose(Pick::Queue),
        )
        .id(iced::advanced::widget::Id::new("video-add-videos-04dc04"))
        .kind(ButtonKind::Secondary)
        .icon("plus")
        .into_element(),
    )
    .into_element()
}

pub(super) fn header(app: &App) -> Element<'_, Message> {
    enzui_widgets::title_bar::enz_title_bar()
        .title(enzui_core::localized!("video-queue-e75185", "value0" => app.playlist.len()))
        .into_element()
}
