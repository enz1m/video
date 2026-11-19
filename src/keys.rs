//! Player shortcuts from uncaptured keys; focused widgets retain input priority.
//!
//! Seek, volume and frame-step bindings repeat while held; others fire once.

use crate::app::Message;
use enzui_core::shortcut::{Modifier, Shortcut};
use iced::keyboard::key::Named;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    TogglePlay,
    TogglePlayK,
    Back5,
    Forward5,
    Back10,
    Forward10,
    Back60,
    Forward60,
    VolumeUp,
    VolumeDown,
    Mute,
    Fullscreen,
    Escape,
    FrameBack,
    FrameForward,
    Slower,
    Faster,
    Subtitles,
    Audio,
    SubtitlesEarlier,
    SubtitlesLater,
    Next,
    Previous,
    Start,
    Open,
    OpenUrl,
    Playlist,
    SaveFrame,
    Info,
    Shortcuts,
}

impl Command {
    fn id(self) -> &'static str {
        match self {
            Self::TogglePlay => "video.key.toggleplay",
            Self::TogglePlayK => "video.key.toggleplayk",
            Self::Back5 => "video.key.back5",
            Self::Forward5 => "video.key.forward5",
            Self::Back10 => "video.key.back10",
            Self::Forward10 => "video.key.forward10",
            Self::Back60 => "video.key.back60",
            Self::Forward60 => "video.key.forward60",
            Self::VolumeUp => "video.key.volumeup",
            Self::VolumeDown => "video.key.volumedown",
            Self::Mute => "video.key.mute",
            Self::Fullscreen => "video.key.fullscreen",
            Self::Escape => "video.key.escape",
            Self::FrameBack => "video.key.frameback",
            Self::FrameForward => "video.key.frameforward",
            Self::Slower => "video.key.slower",
            Self::Faster => "video.key.faster",
            Self::Subtitles => "video.key.subtitles",
            Self::Audio => "video.key.audio",
            Self::SubtitlesEarlier => "video.key.subtitlesearlier",
            Self::SubtitlesLater => "video.key.subtitleslater",
            Self::Next => "video.key.next",
            Self::Previous => "video.key.previous",
            Self::Start => "video.key.start",
            Self::Open => "video.key.open",
            Self::OpenUrl => "video.key.openurl",
            Self::Playlist => "video.key.playlist",
            Self::SaveFrame => "video.key.saveframe",
            Self::Info => "video.key.info",
            Self::Shortcuts => "video.key.shortcuts",
        }
    }
}

pub(crate) struct Binding {
    pub(crate) shortcut: Shortcut,
    pub(crate) command: Command,
    pub(crate) repeats: bool,
}

const fn bind(shortcut: Shortcut, command: Command, repeats: bool) -> Binding {
    Binding {
        shortcut,
        command,
        repeats,
    }
}

pub(crate) fn bindings() -> Vec<Binding> {
    let plain = Modifier::NONE;
    vec![
        bind(
            Shortcut::new(plain, Named::Space),
            Command::TogglePlay,
            false,
        ),
        bind(Shortcut::new(plain, 'k'), Command::TogglePlayK, false),
        bind(Shortcut::new(plain, Named::ArrowLeft), Command::Back5, true),
        bind(
            Shortcut::new(plain, Named::ArrowRight),
            Command::Forward5,
            true,
        ),
        bind(Shortcut::new(plain, 'j'), Command::Back10, true),
        bind(Shortcut::new(plain, 'l'), Command::Forward10, true),
        bind(
            Shortcut::new(Modifier::SHIFT, Named::ArrowLeft),
            Command::Back60,
            true,
        ),
        bind(
            Shortcut::new(Modifier::SHIFT, Named::ArrowRight),
            Command::Forward60,
            true,
        ),
        bind(
            Shortcut::new(plain, Named::ArrowUp),
            Command::VolumeUp,
            true,
        ),
        bind(
            Shortcut::new(plain, Named::ArrowDown),
            Command::VolumeDown,
            true,
        ),
        bind(Shortcut::new(plain, 'm'), Command::Mute, false),
        bind(Shortcut::new(plain, 'f'), Command::Fullscreen, false),
        bind(Shortcut::new(plain, Named::Escape), Command::Escape, false),
        bind(Shortcut::new(plain, ','), Command::FrameBack, true),
        bind(Shortcut::new(plain, '.'), Command::FrameForward, true),
        bind(Shortcut::new(plain, '['), Command::Slower, false),
        bind(Shortcut::new(plain, ']'), Command::Faster, false),
        bind(Shortcut::new(plain, 'c'), Command::Subtitles, false),
        bind(Shortcut::new(plain, 'a'), Command::Audio, false),
        bind(Shortcut::new(plain, 'z'), Command::SubtitlesEarlier, true),
        bind(Shortcut::new(plain, 'x'), Command::SubtitlesLater, true),
        bind(Shortcut::new(plain, 'n'), Command::Next, false),
        bind(Shortcut::new(plain, 'p'), Command::Previous, false),
        bind(Shortcut::new(plain, Named::Home), Command::Start, false),
        bind(Shortcut::new(Modifier::CTRL, 'o'), Command::Open, false),
        bind(Shortcut::new(Modifier::CTRL, 'u'), Command::OpenUrl, false),
        bind(Shortcut::new(Modifier::CTRL, 'l'), Command::Playlist, false),
        bind(
            Shortcut::new(Modifier::CTRL, 's'),
            Command::SaveFrame,
            false,
        ),
        bind(Shortcut::new(Modifier::CTRL, 'i'), Command::Info, false),
        bind(Shortcut::new(plain, Named::F1), Command::Shortcuts, false),
    ]
}

pub(crate) fn catalog() -> Vec<enzui_core::ShortcutBinding<Command>> {
    bindings()
        .into_iter()
        .filter(|binding| binding.command != Command::TogglePlayK)
        .map(|binding| {
            let mut item = enzui_core::ShortcutBinding::new(
                binding.command.id(),
                binding.command,
                binding.shortcut,
            )
            .with_section(group(binding.command))
            .with_description(describe(binding.command));
            if binding.command == Command::TogglePlay {
                item = item.with_alternate(Shortcut::new(Modifier::NONE, 'k'));
            }
            item
        })
        .collect()
}

pub(crate) fn label(command: Command) -> String {
    bindings()
        .into_iter()
        .find_map(|binding| {
            (binding.command == command).then(|| binding.shortcut.localized_label())
        })
        .unwrap_or_default()
}

pub(crate) fn group(command: Command) -> String {
    match command {
        Command::TogglePlay
        | Command::TogglePlayK
        | Command::FrameBack
        | Command::FrameForward
        | Command::Slower
        | Command::Faster => enzui_core::localization::text("video-playback-2db1db"),
        Command::Back5
        | Command::Forward5
        | Command::Back10
        | Command::Forward10
        | Command::Back60
        | Command::Forward60
        | Command::Start => enzui_core::localization::text("video-moving-around-ab611a"),
        Command::VolumeUp | Command::VolumeDown | Command::Mute | Command::Audio => {
            enzui_core::localization::text("video-sound-46124e")
        }
        Command::Subtitles | Command::SubtitlesEarlier | Command::SubtitlesLater => {
            enzui_core::localization::text("video-subtitles-0ee695")
        }
        Command::Next | Command::Previous | Command::Playlist => {
            enzui_core::localization::text("video-queue-3b2fe0")
        }
        Command::Fullscreen
        | Command::Escape
        | Command::Open
        | Command::OpenUrl
        | Command::SaveFrame
        | Command::Info
        | Command::Shortcuts => enzui_core::localization::text("video-window-19734a"),
    }
}

pub(crate) fn describe(command: Command) -> String {
    match command {
        Command::TogglePlay | Command::TogglePlayK => {
            enzui_core::localization::text("video-play-or-pause-a78afa")
        }
        Command::Back5 => enzui_core::localization::text("video-back-5-seconds-2477b4"),
        Command::Forward5 => enzui_core::localization::text("video-forward-5-seconds-5fa764"),
        Command::Back10 => enzui_core::localization::text("video-back-10-seconds-b07313"),
        Command::Forward10 => enzui_core::localization::text("video-forward-10-seconds-26551d"),
        Command::Back60 => enzui_core::localization::text("video-back-1-minute-a7943b"),
        Command::Forward60 => enzui_core::localization::text("video-forward-1-minute-c83e21"),
        Command::VolumeUp => enzui_core::localization::text("video-volume-up-81233d"),
        Command::VolumeDown => enzui_core::localization::text("video-volume-down-6ad507"),
        Command::Mute => enzui_core::localization::text("video-mute-or-unmute-45d4f7"),
        Command::Fullscreen => enzui_core::localization::text("video-fullscreen-c461db"),
        Command::Escape => enzui_core::localization::text("video-leave-fullscreen-d58f23"),
        Command::FrameBack => enzui_core::localization::text("video-previous-frame-27060b"),
        Command::FrameForward => enzui_core::localization::text("video-next-frame-92489d"),
        Command::Slower => enzui_core::localization::text("video-slower-4eda52"),
        Command::Faster => enzui_core::localization::text("video-faster-8d0160"),
        Command::Subtitles => enzui_core::localization::text("video-next-subtitle-track-2095eb"),
        Command::Audio => enzui_core::localization::text("video-next-audio-track-198f31"),
        Command::SubtitlesEarlier => {
            enzui_core::localization::text("video-subtitles-earlier-4e1684")
        }
        Command::SubtitlesLater => enzui_core::localization::text("video-subtitles-later-16f9e9"),
        Command::Next => enzui_core::localization::text("video-next-in-queue-06483f"),
        Command::Previous => enzui_core::localization::text("video-previous-in-queue-56016a"),
        Command::Start => enzui_core::localization::text("video-back-to-the-start-7c0daa"),
        Command::Open => enzui_core::localization::text("video-open-video-6ac7b3"),
        Command::OpenUrl => enzui_core::localization::text("video-open-network-stream-99aeb3"),
        Command::Playlist => enzui_core::localization::text("video-show-or-hide-the-queue-6f67fc"),
        Command::SaveFrame => enzui_core::localization::text("video-save-frame-as-picture-a001d0"),
        Command::Info => enzui_core::localization::text("video-media-information-c01b2b"),
        Command::Shortcuts => enzui_core::localization::text("video-keyboard-shortcuts-e9bef0"),
    }
}

pub(crate) fn message(command: Command) -> Message {
    match command {
        Command::TogglePlay | Command::TogglePlayK => Message::TogglePlay,
        Command::Back5 => Message::SkipBy(-5_000),
        Command::Forward5 => Message::SkipBy(5_000),
        Command::Back10 => Message::SkipBy(-10_000),
        Command::Forward10 => Message::SkipBy(10_000),
        Command::Back60 => Message::SkipBy(-60_000),
        Command::Forward60 => Message::SkipBy(60_000),
        Command::VolumeUp => Message::VolumeBy(0.05),
        Command::VolumeDown => Message::VolumeBy(-0.05),
        Command::Mute => Message::ToggleMute,
        Command::Fullscreen => Message::ToggleFullscreen,
        Command::Escape => Message::ExitFullscreenOrClose,
        Command::FrameBack => Message::StepFrame(false),
        Command::FrameForward => Message::StepFrame(true),
        Command::Slower => Message::RateStep(false),
        Command::Faster => Message::RateStep(true),
        Command::Subtitles => Message::CycleSubtitles,
        Command::Audio => Message::CycleAudio,
        Command::SubtitlesEarlier => Message::SubtitleDelay(-100),
        Command::SubtitlesLater => Message::SubtitleDelay(100),
        Command::Next => Message::Next,
        Command::Previous => Message::Previous,
        Command::Start => Message::SeekTo(std::time::Duration::ZERO),
        Command::Open => Message::Choose(crate::app::Pick::Videos),
        Command::OpenUrl => Message::OpenUrlDialog,
        Command::Playlist => Message::TogglePlaylist,
        Command::SaveFrame => Message::SaveFrame,
        Command::Info => Message::ShowInfo,
        Command::Shortcuts => Message::ShowShortcuts,
    }
}

fn decode(
    event: iced::Event,
    status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    let iced::Event::Keyboard(key @ iced::keyboard::Event::KeyPressed { .. }) = event else {
        return None;
    };
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::keyboard::Event::KeyPressed {
        key: pressed,
        repeat,
        modifiers,
        ..
    } = &key
    else {
        return None;
    };
    if matches!(pressed, iced::keyboard::Key::Named(Named::Backspace)) && modifiers.is_empty() {
        return Some(Message::Backspace);
    }
    if let iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character(text),
        ..
    } = &key
        && !repeat
        && modifiers.is_empty()
        && let Some(digit) = text.chars().next().and_then(|c| c.to_digit(10))
        && text.chars().count() == 1
    {
        return Some(Message::SeekFraction(digit as f32 / 10.0));
    }
    if *repeat {
        return bindings().into_iter().find_map(|binding| {
            (binding.repeats && binding.shortcut.matches(pressed, *modifiers))
                .then(|| message(binding.command))
        });
    }
    enzui_core::dispatch_shortcut_command(catalog().iter(), &key, &()).map(message)
}

pub(crate) fn subscription() -> iced::Subscription<Message> {
    iced::event::listen_with(decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Key, Location, Modifiers, key::Physical};

    fn press(key: Key, modifiers: Modifiers, repeat: bool) -> iced::Event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat,
        })
    }

    fn decoded(key: Key, modifiers: Modifiers, repeat: bool) -> Option<Message> {
        decode(
            press(key, modifiers, repeat),
            iced::event::Status::Ignored,
            iced::window::Id::unique(),
        )
    }

    #[test]
    fn seek_keys_repeat_while_held_and_toggles_do_not() {
        let seek = press(Key::Named(Named::ArrowRight), Modifiers::empty(), true);
        assert!(
            decode(
                seek.clone(),
                iced::event::Status::Captured,
                iced::window::Id::unique()
            )
            .is_none(),
            "focused widgets retain input priority"
        );
        assert!(
            matches!(
                decode(
                    seek,
                    iced::event::Status::Ignored,
                    iced::window::Id::unique()
                ),
                Some(Message::SkipBy(_))
            ),
            "held seeking repeats"
        );
        let space = || Key::Named(Named::Space);
        assert!(
            matches!(
                decoded(space(), Modifiers::empty(), false),
                Some(Message::TogglePlay)
            ),
            "seek keys repeat while held and toggles do not"
        );
        assert!(
            decoded(space(), Modifiers::empty(), true).is_none(),
            "a held Space toggles once"
        );
    }
}
