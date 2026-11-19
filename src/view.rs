//! Full-bleed playback, adaptive navigation and fading media chrome.

mod controls;
mod dialogs;

mod playlist;
mod plex;
pub(crate) mod plex_onboarding;
mod surface;
mod titlebar;

use crate::app::{App, Message};
use enzui_widgets::activity_pulse::{DEFAULT_IDLE, enz_activity_pulse};
use enzui_widgets::app_shell::{ShellGeometry, ShellLayout, enz_app_shell_with_geometry};
use enzui_widgets::motion::{MotionEffect, motion};
use enzui_widgets::toast_host::{ToastOverlayPlacement, enz_toast_overlay};
use iced::widget::{container, mouse_area, stack};
use iced::{Element, Length};

pub(crate) const PLAYBACK_CONTROLS_HEIGHT: f32 = controls::MAX_HEIGHT;

pub(crate) mod osd {
    use iced::Color;

    pub(crate) const TEXT: Color = Color::from_rgb(0.96, 0.96, 0.97);
    pub(crate) const MUTED: Color = Color::from_rgb(0.78, 0.78, 0.80);
    pub(crate) const PILL: Color = Color::from_rgba(0.08, 0.08, 0.09, 0.82);
    pub(crate) const BACKDROP: Color = Color::BLACK;
}

pub(crate) fn root(app: &App) -> Element<'_, Message> {
    let theme = app.themes.theme();
    let visible = app.chrome_visible();
    let shell_tokens = enzui_widgets::app_shell::ShellTokens::from_theme(theme)
        .edge_inset(0.0)
        .sidebar_width(playlist::WIDTH);
    let chrome_height = titlebar::CHROME_HEIGHT;
    let chrome_inset = enzui_widgets::window_controls::trailing_inset_for_square_corner(
        shell_tokens.titlebar_height,
        chrome_height,
    );
    let mut shell = enz_app_shell_with_geometry(move |geometry| content(app, geometry))
        .layout(if app.plex.visible {
            if plex_onboarding::active(app) {
                ShellLayout::TitlebarContent
            } else {
                ShellLayout::FloatingTitlebar
            }
        } else {
            ShellLayout::FloatingTitlebarSidebar
        })
        .tokens(shell_tokens)
        .breakpoints(theme.breakpoints)
        .state(app.queue_navigation)
        .chrome_visible(visible)
        .fullscreen(app.fullscreen)
        .on_window_gesture(Message::Window)
        .titlebar(move || titlebar::titlebar(app))
        .titlebar_island(enzui_theme::MaterialStyle::Regular, || {
            titlebar::action_island(app)
        })
        .titlebar_island_ink(enzui_theme::GlassInk::Theme)
        .titlebar_scrim(app.has_source() && !app.plex.visible)
        .sidebar(|| playlist::panel(app))
        .sidebar_titlebar(|| playlist::header(app))
        .sidebar_label(enzui_core::localized!("video-queue-e75185", "value0" => app.playlist.len()))
        .sidebar_available(app.queue_available() && app.window_size.width >= 800.0)
        .forced_by(|state| {
            if state.preference == enzui_widgets::nav_split::SidebarPreference::Hidden {
                enzui_widgets::nav_split::SidebarPresentation::Hidden
            } else {
                enzui_widgets::nav_split::SidebarPresentation::Inline
            }
        })
        .on_sidebar_state(Message::QueueNavigation);
    let geometry = shell.geometry_for(app.window_size.width);
    if geometry.sidebar == enzui_widgets::nav_split::SidebarPresentation::Hidden
        && geometry.shows_toggle
    {
        let icon = enzui_widgets::toolbar_icon::ToolbarIconTokens::from_palette(&theme.palette)
            .button_size;
        shell = shell.sidebar_toggle_side_inset(chrome_inset + (chrome_height - icon) / 2.0);
    }
    shell = shell.titlebar_background_with_geometry(move |geometry| {
        if geometry.sidebar != enzui_widgets::nav_split::SidebarPresentation::Hidden
            || !geometry.shows_toggle
        {
            return iced::widget::Space::new().into();
        }
        let rtl = crate::l10n::rtl();
        container(
            enzui_widgets::glass::enz_glass(
                iced::widget::Space::new()
                    .width(chrome_height)
                    .height(chrome_height),
            )
            .style(enzui_theme::MaterialStyle::Regular)
            .ink(enzui_theme::GlassInk::Theme)
            .radius(chrome_height / 2.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(iced::Padding {
            left: if rtl { 0.0 } else { chrome_inset },
            right: if rtl { chrome_inset } else { 0.0 },
            ..Default::default()
        })
        .align_y(iced::Alignment::Center)
        .align_x(if rtl {
            iced::alignment::Horizontal::Right
        } else {
            iced::alignment::Horizontal::Left
        })
        .into()
    });
    let shell: Element<'_, Message> = shell.into();
    let menu = app
        .queue_context
        .host(shell, Message::QueueMenu)
        .into_element();
    let with_dialog = dialogs::over(app, menu);
    let window = stack([
        with_dialog,
        enz_toast_overlay(
            app.toasts.view().map(Message::Toast),
            ToastOverlayPlacement::default().bottom_inset(
                if app.has_source() && app.status.error.is_none() {
                    controls::MAX_HEIGHT + theme.spacing.lg()
                } else {
                    0.0
                },
            ),
        ),
    ]);
    let page = app.chooser.view(
        enz_activity_pulse(window, Message::Activity)
            .on_idle(DEFAULT_IDLE, Message::HideChrome)
            .into_element(),
        Message::ChooserFallback,
    );
    // Playback is always presented dark, as QuickTime, GNOME's video player and
    // every streaming app do: the scrim, the titlebar island and the control
    // bar sit on the picture, and a light theme would put a white wash and
    // white glass over the video.
    let page: Element<'_, Message> =
        if app.has_source() && !app.plex.visible && !theme.palette.is_dark() {
            iced::widget::themer(Some(playback_theme(theme).iced_theme()), page).into()
        } else {
            page
        };
    let key = app.drop_target_key();
    enzui_widgets::file_drop::enz_file_drop_target_with_receipt(page, move |drop| {
        Message::ReceiveFiles(crate::app::FileOffer::new(key.0, drop))
    })
    .copy_only(true)
    .key(key)
    .into()
}

/// The app theme in its dark palette, for the playback surface.
pub(crate) fn playback_theme(theme: &enzui_theme::EnzTheme) -> enzui_theme::EnzTheme {
    let mut dark = theme.clone();
    if !theme.palette.is_dark() {
        dark.palette = enzui_theme::Palette::dark();
    }
    dark
}

fn content<'a>(app: &'a App, geometry: &ShellGeometry) -> Element<'a, Message> {
    if app.plex.visible {
        let tint = iced::Color {
            a: 0.12,
            ..app.themes.theme().palette.background
        };
        let fade = container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(geometry.content_inset[0] * 1.5)
            .style(move |_| container::Style {
                background: Some(iced::Background::Gradient(
                    iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                        .add_stop(0.0, tint)
                        .add_stop(
                            0.5,
                            iced::Color {
                                a: tint.a / 2.0,
                                ..tint
                            },
                        )
                        .add_stop(1.0, iced::Color { a: 0.0, ..tint })
                        .into(),
                )),
                ..Default::default()
            });
        return stack![plex::page(app, geometry), fade].into();
    }
    if !app.has_source() {
        return surface::start_page(app, geometry);
    }
    let visible = app.chrome_visible();
    let mut layers: Vec<Element<'_, Message>> = vec![surface::picture(app)];
    if let Some(osd) = &app.osd {
        layers.push(surface::notice(&osd.text));
    }
    if app.status.error.is_none() {
        let [_, right, _, left] = geometry.content_inset;
        let available_width = (geometry.content_width - left - right).max(0.0);
        layers.push(
            container(motion(
                controls::controls(app, available_width),
                visible,
                MotionEffect::Fade,
            ))
            .padding(iced::Padding {
                left,
                right,
                ..Default::default()
            })
            .align_bottom(Length::Fill)
            .width(Length::Fill)
            .into(),
        );
    }
    let picture_area: Element<'_, Message> = stack(layers).into();
    let interaction = if visible {
        iced::mouse::Interaction::default()
    } else {
        iced::mouse::Interaction::Hidden
    };
    let picture_area = mouse_area(picture_area)
        .on_press(Message::SurfacePress)
        .on_double_click(Message::ToggleFullscreen)
        .on_scroll(|delta| {
            let lines = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
            };
            Message::VolumeBy(f64::from(lines.clamp(-3.0, 3.0)) * 0.05)
        })
        .interaction(interaction);
    picture_area.into()
}

pub(crate) fn file_stem(title: &str) -> String {
    let stem: String = title
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    let stem = stem.trim().trim_start_matches('.');
    if stem.is_empty() {
        "Video".to_owned()
    } else {
        stem.to_owned()
    }
}
