//! Mounted player dialogs and the compact queue sheet.

use crate::app::{App, Dialog, Message};
use crate::keys;
use crate::model::time::{clock, rate_label};
use enzui_widgets::form_dialog::{FormPresenter, enz_form_dialog};
use enzui_widgets::modal_host::enz_modal_host;
use enzui_widgets::modal_surface::enz_modal_surface;
use enzui_widgets::shortcut_list::{ShortcutEntry, enz_shortcut_list};
use enzui_widgets::text::{TextSize, TextTone, enz_text};
use iced::widget::{column, container, row};
use iced::{Element, Length};

pub(crate) fn over<'a>(app: &'a App, shell: Element<'a, Message>) -> Element<'a, Message> {
    let queue = (app.show_playlist && !app.playlist.is_empty() && app.window_size.width < 800.0)
        .then(|| {
            enzui_widgets::sheet::enz_sheet(super::playlist::panel(app))
                .height((app.window_size.height * 0.7).max(200.0))
                .into_element()
        });
    let page = enz_modal_host(shell, queue, |_| Message::TogglePlaylist)
        .placement(enzui_widgets::modal_host::SurfacePlacement::BottomEdge)
        .label(enzui_core::localization::text("video-queue-3b2fe0"))
        .into_element();
    app.dialogs.view_with(
        page,
        |key| match key {
            Dialog::OpenUrl => match &app.url_presenter {
                Some(presenter) => url_dialog(presenter),
                None => iced::widget::Space::new().into(),
            },
            Dialog::Info => modal(
                app,
                Dialog::Info,
                enzui_core::localization::text("video-media-information-c01b2b"),
                info(app),
            ),
            Dialog::Shortcuts => modal(
                app,
                Dialog::Shortcuts,
                enzui_core::localization::text("video-keyboard-shortcuts-e9bef0"),
                enz_shortcut_list(shortcut_entries()).into_element(),
            ),
        },
        Message::DialogEvent,
        |key, host| {
            let label = enzui_core::localization::text(match key {
                Dialog::OpenUrl => "video-open-network-stream-99aeb3",
                Dialog::Info => "video-media-information-c01b2b",
                Dialog::Shortcuts => "video-keyboard-shortcuts-e9bef0",
            });
            let host = host.label(label);
            if app.window_size.width < 500.0 {
                host.placement(enzui_widgets::modal_host::SurfacePlacement::BottomEdge)
            } else {
                host
            }
        },
    )
}

fn url_dialog(presenter: &FormPresenter) -> Element<'_, Message> {
    enz_form_dialog(
        enzui_core::localization::text("video-open-network-stream-99aeb3"),
        presenter.session(),
        Message::UrlForm,
    )
    .presenter(presenter)
    .description(enzui_core::localization::text(
        "video-play-a-video-from-the-web-using-an-https-link-to-a-vide-d5c24c",
    ))
    .submit_label(enzui_core::localization::text("video-play-436e61"))
    .cancel_label(enzui_core::localization::text("video-cancel-19766e"))
    .into_surface()
}

fn modal<'a>(
    app: &'a App,
    key: Dialog,
    title: String,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let sp = app.themes.theme().spacing;
    let bounded_body = enzui_widgets::scroll_view::enz_scroll_view(body)
        // The window minus the title, the surface padding and a 48 px margin
        // above and below: at `- 120` the whole surface overran the window.
        .height(Length::Shrink.max((app.window_size.height - 220.0).max(100.0)))
        .into_element();
    let content = column![
        enz_text(title).size(TextSize::Title3).into_element(),
        bounded_body
    ]
    .spacing(sp.md());
    enz_modal_surface(Element::from(content))
        .width(480.0)
        .on_dismiss(|| Message::CloseDialog)
        .close_button_id(key.close_button_id())
        .into_element()
}

fn facts<'a>(rows: Vec<(String, String)>) -> Element<'a, Message> {
    column(rows.into_iter().map(|(name, value)| {
        row![
            container(enz_text(name).tone(TextTone::Muted).into_element()).width(140.0),
            container(enz_text(value).into_element()).width(Length::Fill),
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(12)
        .into()
    }))
    .spacing(6)
    .into()
}

fn info(app: &App) -> Element<'_, Message> {
    let status = &app.status;
    let mut rows = vec![(
        enzui_core::localization::text("video-title-7e8cd2"),
        app.current_title().unwrap_or_default(),
    )];
    if let Some(entry) = app.playlist.current() {
        rows.push((
            enzui_core::localization::text("video-location-15b619"),
            match entry {
                crate::model::Entry::File(path) => path.display().to_string(),
                crate::model::Entry::Url(url) => url.split('?').next().unwrap_or(url).to_owned(),
            },
        ));
    }
    if let Some(duration) = status.duration {
        rows.push((
            enzui_core::localization::text("video-length-adc956"),
            clock(duration, false),
        ));
    }
    if let Some((width, height)) = status.natural_size {
        rows.push((
            enzui_core::localization::text("video-picture-9a1c54"),
            format!(
                "{} × {}",
                crate::l10n::number(f64::from(width), 0),
                crate::l10n::number(f64::from(height), 0)
            ),
        ));
    }
    if let Some(rate) = status.frame_rate {
        rows.push((
            enzui_core::localization::text("video-frame-rate-68e19e"),
            enzui_core::localized!("video-frame-rate-value", "rate" => rate),
        ));
    }
    if let Some(codec) = &status.video_codec {
        rows.push((
            enzui_core::localization::text("video-app-name"),
            codec.clone(),
        ));
    }
    if let Some(track) = status.audio_track() {
        let codec = track
            .codec
            .as_deref()
            .map(|c| format!(" ({c})"))
            .unwrap_or_default();
        rows.push((
            enzui_core::localization::text("video-audio-bc1b88"),
            format!("{}{codec}", crate::l10n::track(track)),
        ));
    }
    rows.push((
        enzui_core::localization::text("video-audio-tracks-59d03b"),
        match status.audio_tracks.len() {
            0 => enzui_core::localization::text("video-none-dc937b"),
            n => crate::l10n::decimal(&n.to_string()),
        },
    ));
    rows.push((
        enzui_core::localization::text("video-subtitle-tracks-fff273"),
        match status.subtitle_tracks.len() {
            0 => enzui_core::localization::text("video-none-dc937b"),
            n => crate::l10n::decimal(&n.to_string()),
        },
    ));
    if (status.rate - 1.0).abs() > 1e-6 {
        rows.push((
            enzui_core::localization::text("video-speed-c372fe"),
            rate_label(status.rate),
        ));
    }
    if let Some((width, height)) = app.presenter.size() {
        rows.push((
            enzui_core::localization::text("video-frame-size-cb90c0"),
            format!(
                "{} × {}",
                crate::l10n::number(f64::from(width), 0),
                crate::l10n::number(f64::from(height), 0)
            ),
        ));
    }
    for decoder in &status.missing_decoders {
        rows.push((
            enzui_core::localization::text("video-missing-decoder-262741"),
            decoder.clone(),
        ));
    }
    facts(rows)
}

pub(crate) fn shortcut_entries() -> Vec<ShortcutEntry> {
    let catalog = keys::catalog();
    let mut entries = enzui_widgets::shortcut_list::shortcut_catalog_entries(catalog.iter());
    entries.push(ShortcutEntry::new(
        enzui_core::localization::text("video-moving-around-ab611a"),
        "0 – 9",
        enzui_core::localization::text("video-jump-to-that-tenth-of-the-video-bb4f69"),
    ));
    let order = [
        enzui_core::localization::text("video-playback-2db1db"),
        enzui_core::localization::text("video-moving-around-ab611a"),
        enzui_core::localization::text("video-sound-46124e"),
        enzui_core::localization::text("video-subtitles-0ee695"),
        enzui_core::localization::text("video-queue-3b2fe0"),
        enzui_core::localization::text("video-window-19734a"),
    ];
    entries.sort_by_key(|entry| order.iter().position(|group| *group == entry.section));
    entries
}
