//! Video playback and media-server browsing through enzUI desktop and media APIs.

#![deny(unconditional_recursion)]

mod app;
mod desktop;
mod jellyfin;
mod keys;
mod l10n;
mod model;
mod persistence;
mod plex;
mod view;

use app::App;
use enzui_widgets::app_shell::{DesktopApplicationIdentity, ShellLayout, enz_window_settings};

fn main() -> iced::Result {
    enzui_core::crash_report::CrashReporter::for_app("video-app", env!("CARGO_PKG_VERSION"));
    let opening: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let instance = match desktop::connect(&opening) {
        Ok(None) => return Ok(()),
        Ok(Some(instance)) => instance,
        Err(error) => {
            eprintln!("Desktop activation unavailable: {error}");
            return Ok(());
        }
    };
    let instance = std::sync::Mutex::new(Some(instance));
    let window = DesktopApplicationIdentity::new(app::DESKTOP_APP_ID)
        .map_err(|error| iced::Error::WindowCreationFailed(Box::new(error)))?
        .apply_to_window_settings(iced::window::Settings {
            // Closing waits for the final settings save attempt.
            exit_on_close_request: false,
            size: iced::Size::new(1100.0, 680.0),
            min_size: Some(iced::Size::new(375.0, 320.0)),
            ..enz_window_settings(ShellLayout::FloatingTitlebar)
        });
    enzui_widgets::app_setup::run_with_services(
        iced::application(
            move || {
                let (mut app, task) = App::new(opening.clone());
                app.instance = instance
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                    .unwrap_or_default();
                (app, task)
            },
            App::update,
            view::root,
        )
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window(window),
        enzui_widgets::app_setup::AppServices::default().localizer(l10n::localizer()),
    )
}
