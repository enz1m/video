//! Window navigation and player commands in the shell-owned titlebar.

use crate::app::{App, Message, Pick};
use crate::keys::Command;
use enzui_core::action_overflow::{Descriptor, OverflowPolicy};
use enzui_core::menu_model::CheckState;
use enzui_core::{Action, Icon};
use enzui_widgets::action_bar::{ActionItem, Collector, CollectorPlacement, enz_action_bar};
use enzui_widgets::text::{TextSize, enz_text};
use enzui_widgets::title_bar::enz_title_bar;
use enzui_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{container, row};
use iced::{Element, Length};

pub(crate) fn titlebar(app: &App) -> Element<'_, Message> {
    let mut bar = enz_title_bar().title(if app.plex.visible {
        String::new()
    } else {
        app.current_title()
            .filter(|_| app.has_source() && !app.plex.visible)
            .unwrap_or_else(|| enzui_core::localization::text("video-app-name"))
    });
    let library = if app.plex.visible && app.plex.selected.is_none() && app.plex.parents.is_empty()
    {
        app.plex.connected.as_ref().and_then(|connected| {
            app.plex
                .library
                .and_then(|index| connected.libraries.get(index))
        })
    } else {
        None
    };
    let service_home = app.plex.visible
        && app.plex.connected.is_some()
        && !super::plex_onboarding::active(app)
        && app.plex.library.is_none()
        && app.plex.selected.is_none()
        && app.plex.parents.is_empty();
    let navigation = if service_home {
        None
    } else if app.plex.visible {
        let event = if !super::plex_onboarding::active(app)
            && (app.plex.library.is_some()
                || app.plex.selected.is_some()
                || !app.plex.parents.is_empty())
        {
            crate::plex::Event::Back
        } else {
            crate::plex::Event::Close
        };
        let label = enzui_core::localization::text(if matches!(event, crate::plex::Event::Back) {
            "video-back-76900f"
        } else if app.has_source() {
            "video-return-to-video-0422ac"
        } else {
            "video-local-videos-51e353"
        });
        Some((event, label))
    } else if app.plex.playing.is_some() && app.has_source() {
        let kind = app
            .plex
            .playing_server
            .as_ref()
            .map_or(app.plex.kind, |server| server.kind);
        Some((
            crate::plex::Event::LeavePlayer,
            enzui_core::localized!("video-back-to-a63817", "value0" => kind.name()),
        ))
    } else {
        None
    };
    if let Some((event, label)) = navigation {
        let back = enz_toolbar_icon("navigation.back", Message::Plex(event))
            .id(iced::advanced::widget::Id::new("video-titlebar-navigation"))
            .label(label)
            .into_element();
        let navigation = if let Some(library) = library {
            row![
                back,
                container(
                    enz_text(library.title.clone())
                        .size(TextSize::Headline)
                        .single_line_elided()
                        .into_element()
                )
                .padding(iced::Padding {
                    left: if crate::l10n::rtl() {
                        app.themes.theme().spacing.sm()
                    } else {
                        0.0
                    },
                    right: if crate::l10n::rtl() {
                        0.0
                    } else {
                        app.themes.theme().spacing.sm()
                    },
                    ..Default::default()
                })
            ]
            .mirrored(crate::l10n::rtl())
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            back
        };
        bar = bar.leading(glass_chrome(navigation));
    }
    if let Some(library) = library {
        bar = bar.center(super::plex::search(app, library));
    }
    bar.into_element()
}

pub(super) const CHROME_HEIGHT: f32 = enzui_widgets::window_controls::DEFAULT_CONTROL_SIZE + 8.0;

pub(super) fn glass_chrome(content: Element<'_, Message>) -> Element<'_, Message> {
    let height = CHROME_HEIGHT;
    enzui_widgets::glass::enz_glass(
        iced::widget::container(content)
            .height(height)
            .align_y(iced::Alignment::Center)
            .padding([0.0, 4.0]),
    )
    .style(enzui_theme::MaterialStyle::Regular)
    .ink(enzui_theme::GlassInk::Theme)
    .radius(height / 2.0)
    .into_element()
}

pub(crate) fn action_island(app: &App) -> Element<'_, Message> {
    let actions = actions(app);
    let slots = actions
        .iter()
        .filter(|item| {
            item.descriptor().visible && item.descriptor().policy != OverflowPolicy::AlwaysOverflow
        })
        .count()
        + 1;
    let control =
        enzui_widgets::toolbar_icon::TitlebarAffordanceTokens::default().effective_control_size();
    let width = slots as f32 * control + slots.saturating_sub(1) as f32 * 4.0;
    iced::widget::container(
        enz_action_bar(actions, Message::ToggleTitlebarMenu)
            .collector(Collector::Kebab {
                icon: "ellipsis-vertical".into(),
                tokens: None,
            })
            .collector_placement(CollectorPlacement::TrailingCluster)
            .gap(4.0)
            .overflow_open(app.titlebar_menu)
            .on_dismiss(Message::TitlebarMenu(false))
            .menu_id_prefix("video-titlebar-actions")
            .overflow_trigger_id("video-titlebar-actions-overflow-trigger")
            .into_element(),
    )
    .width(Length::Fill.max(width))
    .into()
}

fn overflow<'a>(
    id: &'static str,
    icon: &'static str,
    label: String,
    message: Message,
    enabled: bool,
    shortcut: Option<Command>,
) -> ActionItem<'a, Message> {
    let mut action = Action::new(id, message)
        .with_label(label)
        .with_icon(Icon::named(icon))
        .with_enabled(enabled);
    if let Some(command) = shortcut
        && let Some(binding) = crate::keys::bindings()
            .into_iter()
            .find(|binding| binding.command == command)
    {
        action = action.with_shortcut(binding.shortcut);
    }
    ActionItem::command(Descriptor::new(id).always_overflow(), action)
}

fn actions(app: &App) -> Vec<ActionItem<'_, Message>> {
    let mut actions = vec![
        ActionItem::icon(
            "video.library",
            "library",
            enzui_core::localization::text("video-media-library-f1ae34"),
            Message::Plex(crate::plex::Event::Open),
        ),
        ActionItem::icon(
            "video.open",
            "folder-open",
            enzui_core::localization::text("video-open-video-6ac7b3"),
            Message::Choose(Pick::Videos),
        ),
    ];
    if app.queue_available() && app.window_size.width < 800.0 {
        actions.push(ActionItem::icon(
            "video.playlist",
            "list-video",
            enzui_core::localization::text(if app.show_playlist {
                "video-hide-queue-224ad4"
            } else {
                "video-show-queue-05726c"
            }),
            Message::TogglePlaylist,
        ));
    }
    if app.plex.visible && app.plex.connected.is_some() {
        let kind = app.plex.kind.name();
        actions.extend([
            overflow(
                "video.refresh",
                "refresh-cw",
                enzui_core::localization::text("video-refresh"),
                Message::Plex(crate::plex::Event::Refresh),
                !app.plex.busy,
                None,
            )
            .group("library"),
            overflow(
                "video.scan",
                "folder-sync",
                if app.plex.library.is_some() {
                    enzui_core::localization::text("video-scan-library")
                } else {
                    enzui_core::localized!("video-scan-libraries", "server" => kind)
                },
                Message::Plex(crate::plex::Event::Scan),
                true,
                None,
            )
            .group("library"),
        ]);
    }
    actions.extend([
        overflow(
            "video.queue",
            "list-video",
            enzui_core::localization::text("video-add-to-queue-53ae23"),
            Message::Choose(Pick::Queue),
            !app.plex.visible && app.plex.playing.is_none(),
            None,
        ),
        overflow(
            "video.url",
            "link",
            enzui_core::localization::text("video-open-network-stream-b1cc49"),
            Message::OpenUrlDialog,
            true,
            Some(Command::OpenUrl),
        ),
        overflow(
            "video.repeat",
            "repeat",
            app.playlist.repeat.label(),
            Message::CycleRepeat,
            app.queue_available(),
            None,
        )
        .check(if app.playlist.repeat == crate::model::Repeat::Off {
            CheckState::Unchecked
        } else {
            CheckState::Checked
        }),
        overflow(
            "video.frame",
            "camera",
            enzui_core::localization::text("video-save-frame-as-picture-a001d0"),
            Message::SaveFrame,
            app.presenter.handle().is_some(),
            Some(Command::SaveFrame),
        ),
        overflow(
            "video.info",
            "info",
            enzui_core::localization::text("video-media-information-c01b2b"),
            Message::ShowInfo,
            app.has_source(),
            Some(Command::Info),
        ),
        overflow(
            "video.shortcuts",
            "keyboard",
            enzui_core::localization::text("video-keyboard-shortcuts-e9bef0"),
            Message::ShowShortcuts,
            true,
            Some(Command::Shortcuts),
        ),
    ]);
    actions
}
