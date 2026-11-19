//! Responsive transport and timeline controls on a shared glass surface.

use crate::app::{App, Message};
use crate::model::time::{clock, rate_label};
use enzui_theme::{GlassInk, MaterialStyle};
use enzui_widgets::button::ButtonSize;
use enzui_widgets::clamp::{ClampSize, enz_clamp};
use enzui_widgets::glass::enz_glass;
use enzui_widgets::menu_button::{MenuButtonKind, enz_menu_button};
use enzui_widgets::slider::enz_slider;
use enzui_widgets::text::{TextSize, enz_text};
use enzui_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Space, column, container, mouse_area, row};
use iced::{Element, Length};
use std::time::Duration;

const COMPACT_BELOW: f32 = 850.0;
// The widest the pill grows, so the scrubber stays a comfortable reach.
const PILL_MAX_WIDTH: f32 = 880.0;
const CONTROL_HIT_TARGET: f32 = 32.0;
const CONTROL_GLYPH: f32 = 18.0;

fn button<'a>(
    id: &'static str,
    icon: &'a str,
    label: String,
    message: Message,
) -> enzui_widgets::toolbar_icon::EnzToolbarIcon<'a, Message> {
    enz_toolbar_icon(icon, message)
        .id(iced::advanced::widget::Id::new(id))
        .size(CONTROL_HIT_TARGET, CONTROL_GLYPH)
        .label(label)
}

pub(super) const MAX_HEIGHT: f32 = 100.0;

pub(crate) fn controls(app: &App, available_width: f32) -> Element<'_, Message> {
    let theme = app.themes.theme();
    let sp = theme.spacing;
    let compact = available_width < COMPACT_BELOW;
    let status = &app.status;
    use enzui_widgets::media_controls::{
        MEDIA_THUMB, MEDIA_TRACK_HEIGHT, MediaControlsMessage, MediaControlsTokens, TimeDisplay,
        enz_media_controls,
    };
    let mut snapshot = status.snapshot(None);
    snapshot.position = app.shown_position();
    snapshot.error = None;
    // The app timeline localises digits and keeps hour width stable across the duration.
    snapshot.capabilities.seekable = false;
    snapshot.capabilities.has_audio = false;
    // The composite bar has one shared glass backing, so transport must remain transparent.
    // Controls only exist during playback, which is presented dark (see `view::root`).
    let mut transport_tokens =
        MediaControlsTokens::from_palette(&super::playback_theme(app.themes.theme()).palette);
    transport_tokens.background = iced::Color::TRANSPARENT;
    let next_episode = app.plex.next_episode.is_some();
    let transport_width = ButtonSize::Xs.tokens().height
        + 36.0
        + if next_episode {
            CONTROL_HIT_TARGET + sp.xs()
        } else {
            0.0
        };
    let transport = enz_media_controls("video-transport", snapshot)
        .time_display(TimeDisplay::Hidden)
        .transport_button_size(ButtonSize::Xs)
        .tokens(transport_tokens)
        .into_element()
        .map(|message| match message {
            MediaControlsMessage::PlayPause => Message::TogglePlay,
            MediaControlsMessage::Seek(at) => Message::Scrub(at.as_secs_f32()),
            MediaControlsMessage::SetVolume(volume) => Message::SetVolume(volume),
            MediaControlsMessage::ToggleMute => Message::ToggleMute,
            MediaControlsMessage::PreviousTrack => Message::Previous,
            MediaControlsMessage::NextTrack => Message::Next,
            MediaControlsMessage::CycleRepeat => Message::CycleRepeat,
            MediaControlsMessage::ToggleShuffle => Message::Activity,
        });
    let transport: Element<'_, Message> = if next_episode {
        row![
            transport,
            button(
                "video-next-episode",
                "skip-forward.fill",
                enzui_core::localization::text("video-next-episode"),
                Message::Next
            )
            .size(CONTROL_HIT_TARGET, 16.0)
            .enabled(!app.plex.busy)
            .into_element()
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(sp.xs())
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        transport
    };
    let position = app.shown_position();
    let long = status
        .duration
        .is_some_and(|duration| duration >= Duration::from_secs(3600));
    let readout = |value: Duration| {
        enz_text(clock(value, long))
            .size(TextSize::Footnote)
            .no_wrap()
            .into_element()
    };
    let timeline: Element<'_, Message> = match status.duration {
        Some(duration) if status.seekable => {
            let maximum = duration.as_secs_f32().max(0.001);
            row![
                readout(position.min(duration)),
                enz_slider(
                    0.0..=maximum,
                    position.as_secs_f32().min(maximum),
                    Message::Scrub
                )
                .aria_label(&enzui_core::localization::text("enzui-media-controls-seek"))
                .value_format(move |seconds| clock(Duration::from_secs_f32(seconds.max(0.0)), long))
                .track_height(MEDIA_TRACK_HEIGHT)
                .thumb_width(MEDIA_THUMB)
                .thumb_height(MEDIA_THUMB)
                .hover_preview(true)
                .step_marks(false)
                .on_drag_start(Message::ScrubStarted)
                .on_release(Message::ScrubReleased)
                .id_prefix("video-seek")
                .into_element(),
                readout(duration),
            ]
            .mirrored(crate::l10n::rtl())
            .spacing(sp.xs())
            .align_y(iced::Alignment::Center)
            .into()
        }
        Some(duration) => row![
            readout(position.min(duration)),
            enz_text("/").into_element(),
            readout(duration)
        ]
        .mirrored(crate::l10n::rtl())
        .spacing(sp.xs())
        .into(),
        None => readout(position),
    };
    let mut queue = row![]
        .mirrored(crate::l10n::rtl())
        .spacing(sp.xs())
        .align_y(iced::Alignment::Center);
    if app.queue_available() && app.playlist.len() > 1 {
        queue = queue
            .push(
                button(
                    "video-previous",
                    "skip-back.fill",
                    enzui_core::localization::text("video-previous-in-queue"),
                    Message::Previous,
                )
                .enabled(app.playlist.has_previous() || status.seekable)
                .into_element(),
            )
            .push(
                button(
                    "video-next",
                    "skip-forward.fill",
                    enzui_core::localization::text("video-next-in-queue"),
                    Message::Next,
                )
                .enabled(app.playlist.has_next())
                .into_element(),
            );
    }
    let mut options = row![]
        .mirrored(crate::l10n::rtl())
        .spacing(0)
        .align_y(iced::Alignment::Center);
    options = options.push(
        enz_menu_button(
            "",
            &app.menus.subtitles.0,
            &app.menus.subtitles.1,
            Message::SubtitlesMenu,
        )
        .kind(MenuButtonKind::Toolbar)
        .trigger_icon(
            if app.transcoded_tracks().map_or_else(
                || status.current_subtitle.is_some(),
                |server| server.subtitle_choice.is_some(),
            ) || status.subtitle_file.is_some()
            {
                "captions"
            } else {
                "captions-off"
            },
        )
        .menu_id(iced::advanced::widget::Id::new("video-subtitle-menu"))
        .aria_label(enzui_core::localization::text("video-subtitles-0ee695"))
        .into_element(),
    );
    let audio_tracks = app
        .transcoded_tracks()
        .map_or(status.audio_tracks.len(), |server| server.audio.len());
    if audio_tracks > 1 {
        options = options.push(
            enz_menu_button(
                "",
                &app.menus.audio.0,
                &app.menus.audio.1,
                Message::AudioMenu,
            )
            .kind(MenuButtonKind::Toolbar)
            .trigger_icon("languages")
            .menu_id(iced::advanced::widget::Id::new("video-audio-menu"))
            .aria_label(enzui_core::localization::text("video-audio-track-e4a847"))
            .into_element(),
        );
    }
    // A media server's stream offers its original file or a transcode.
    if app.plex.playing.is_some() {
        let quality = app.settings.stream_quality;
        options = options.push(
            enz_menu_button(
                quality.short_label(),
                &app.menus.quality.0,
                &app.menus.quality.1,
                Message::QualityMenu,
            )
            .menu_id(iced::advanced::widget::Id::new("video-quality-menu"))
            .kind(MenuButtonKind::Quiet(ButtonSize::Sm))
            .aria_label(
                enzui_core::localized!("video-streaming-quality", "quality" => quality.label()),
            )
            .into_element(),
        );
    }
    if status.seekable {
        options = options.push(
            enz_menu_button(
                rate_label(status.rate),
                &app.menus.speed.0,
                &app.menus.speed.1,
                Message::SpeedMenu,
            )
            .menu_id(iced::advanced::widget::Id::new("video-speed-menu"))
            .kind(MenuButtonKind::Quiet(ButtonSize::Sm))
            .aria_label(
                enzui_core::localized!("video-playback-speed-93675f", "value0" => rate_label(status.rate)),
            )
            .into_element(),
        );
    }
    let fullscreen = button(
        "video-fullscreen",
        if app.fullscreen {
            "minimize"
        } else {
            "maximize"
        },
        if app.fullscreen {
            enzui_core::localization::text("video-leave-fullscreen-d58f23")
        } else {
            enzui_core::localization::text("video-fullscreen-c461db")
        },
        Message::ToggleFullscreen,
    )
    .into_element();

    let transport = container(transport).width(transport_width);
    let mut volume = row![].align_y(iced::Alignment::Center);
    if status.has_audio {
        volume = volume
            .push(
                button(
                    "video-mute",
                    if status.muted {
                        "volume-x"
                    } else {
                        enzui_widgets::media_controls::volume_icon_name(status.volume)
                    },
                    enzui_core::localization::text(if status.muted {
                        "enzui-media-controls-unmute"
                    } else {
                        "enzui-media-controls-mute"
                    }),
                    Message::ToggleMute,
                )
                .into_element(),
            )
            .push(
                container(
                    enz_slider(0.0..=1.0, status.volume as f32, |value| {
                        Message::SetVolume(f64::from(value))
                    })
                    .aria_label(&enzui_core::localization::text(
                        "enzui-media-controls-volume",
                    ))
                    .step(0.05)
                    .track_height(MEDIA_TRACK_HEIGHT)
                    .thumb_width(MEDIA_THUMB)
                    .thumb_height(MEDIA_THUMB)
                    .step_marks(false)
                    .id_prefix("video-volume")
                    .into_element(),
                )
                .width(90.0),
            );
    }
    let content: Element<'_, Message> = if compact {
        column![
            row![
                transport,
                queue,
                Space::new().width(Length::Fill),
                options,
                fullscreen,
            ]
            .spacing(sp.xs())
            .align_y(iced::Alignment::Center),
            row![container(timeline).width(Length::Fill), volume]
                .spacing(sp.sm())
                .align_y(iced::Alignment::Center),
        ]
        .spacing(sp.xs())
        .into()
    } else {
        row![
            queue,
            transport,
            container(timeline).width(Length::Fill),
            volume,
            options,
            fullscreen
        ]
        .spacing(sp.xs())
        .align_y(iced::Alignment::Center)
        .into()
    };
    let pill = enz_glass(
        container(content)
            .align_y(iced::Alignment::Center)
            .padding([if compact { 8.0 } else { 2.0 }, 16.0]),
    )
    .style(MaterialStyle::Regular)
    .ink(GlassInk::Theme)
    .radius(if compact {
        theme.window_radius
    } else {
        theme.radius.full
    });
    let panel = container(
        enz_clamp(pill, ClampSize::Wide)
            .max_width(PILL_MAX_WIDTH)
            .into_element(),
    )
    .center_x(Length::Fill)
    .padding(iced::Padding {
        top: 0.0,
        right: sp.lg(),
        bottom: sp.lg(),
        left: sp.lg(),
    });
    // Pressing between the controls must not toggle playback, and the chrome
    // stays while the pointer is over them.
    mouse_area(panel)
        .on_press(Message::Activity)
        .on_enter(Message::PointerOverControls(true))
        .on_exit(Message::PointerOverControls(false))
        .into()
}
