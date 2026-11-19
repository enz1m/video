//! Playback and asynchronous application workflows through the real reducer.

use super::*;
use enzui_core::menu_model::{CheckState, EntryId};
use enzui_services::media::PlaybackState;
use enzui_test_support::test_scratch::ScratchDir;
use enzui_widgets::menu_button::MenuButtonEvent;
use iced::advanced::renderer::Headless;

// The clip includes English and French audio and an English subtitle track.
const CLIP: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/clip.mkv");

struct Fixture {
    app: App,
    scratch: ScratchDir,
}

fn fixture(tag: &str, player: bool) -> Fixture {
    let scratch = ScratchDir::new(&format!("video-app-{tag}"));
    let themes = enzui_core::theme_file::integration::ThemeIntegration::unmanaged_compiled_theme(
        enzui_theme::EnzTheme::dark(),
    )
    .0;
    let video = if player {
        create_video_service()
    } else {
        Err("no player in this test".to_owned())
    };
    let settings = Settings {
        muted: true,
        ..Settings::default()
    };
    let app = App::with(themes, video, settings, Some(scratch.path().join("state")));
    Fixture { app, scratch }
}

// Paused timers complete immediately; OSD expiry is outside these media workflows.
fn settle(app: &mut App, task: iced::Task<Message>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .expect("test runtime");
    let mut pending = vec![task];
    let mut image_renderer = None;
    while let Some(task) = pending.pop() {
        let Some(stream) = iced_test::runtime::task::into_stream(task) else {
            continue;
        };
        let messages: Vec<Message> = runtime.block_on(async {
            use iced_test::futures::futures::StreamExt;
            stream
                .filter_map(|action| {
                    let message = match action {
                        iced_test::runtime::Action::Output(message) => Some(message),
                        iced_test::runtime::Action::Image(
                            iced_test::runtime::image::Action::Allocate(handle, sender),
                        ) => {
                            let renderer = image_renderer.get_or_insert_with(|| {
                                iced_test::futures::futures::executor::block_on(
                                    <iced_test::renderer::Renderer as Headless>::new(
                                        Default::default(),
                                        Some("tiny-skia"),
                                    ),
                                )
                                .expect("image renderer")
                            });
                            let _ = sender.send(iced::advanced::image::Renderer::load_image(
                                renderer, &handle,
                            ));
                            None
                        }
                        _ => None,
                    };
                    async move { message }
                })
                .collect()
                .await
        });
        for message in messages {
            if matches!(message, Message::OsdExpired(_)) {
                continue;
            }
            pending.push(app.update(message));
        }
    }
}

fn send(app: &mut App, message: Message) {
    let task = app.update(message);
    settle(app, task);
}

fn until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let started = std::time::Instant::now();
    while !done(app) {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}: {:#?}",
            app.status
        );
        std::thread::sleep(Duration::from_millis(15));
        send(app, Message::VideoWake);
    }
}

fn open(app: &mut App, paths: &[std::path::PathBuf]) {
    send(app, Message::Dropped(paths.to_vec()));
}

#[test]
fn queue_completion_and_session_end_preserve_resume_history() {
    {
        let mut fixture = fixture("queue", true);
        let first = fixture.scratch.path().join("first.mkv");
        let second = fixture.scratch.path().join("second.mkv");
        std::fs::copy(CLIP, &first).expect("copy");
        std::fs::copy(CLIP, &second).expect("copy");
        let app = &mut fixture.app;
        open(app, &[first.clone(), second]);
        assert_eq!(app.playlist.len(), 2);
        app.plex.visible = true;
        send(app, Message::TogglePlaylist);
        assert!(!app.show_playlist && app.plex.visible);
        send(app, Message::Plex(crate::plex::Event::Close));
        send(app, Message::TogglePlaylist);
        assert!(app.show_playlist && !app.plex.visible);
        until(app, "the second one", |app| {
            app.playlist.current_index() == Some(1) && app.status.is_playing()
        });
        let recent = app.settings.history.items();
        assert_eq!(
            recent.first().map(|item| &item.entry),
            Some(&crate::model::Entry::File(first))
        );
        assert_eq!(
            recent[0].position_ms, 0,
            "watched to the end: starts over next time"
        );
        let window = iced::window::Id::unique();
        send(app, Message::CloseRequested(window));
        assert_eq!(app.settings.history.items().len(), 2);
        let saved = crate::persistence::load(&fixture.scratch.path().join("state"));
        assert_eq!(saved.history.items().len(), 2, "on disk too");
    }
    {
        let mut fixture = fixture("session-end", true);
        let app = &mut fixture.app;
        open(app, &[CLIP.into()]);
        until(app, "playing", |app| {
            app.status.is_playing() && app.status.seekable
        });
        let resumable_position = Duration::from_secs(20);
        app.status.position = resumable_position;
        app.status.duration = Some(Duration::from_secs(120));
        send(
            app,
            Message::Session(enzui_core::portal::session_monitor::SessionEvent {
                state: enzui_core::portal::session_monitor::SessionState::Ending,
                screensaver_active: false,
                ack: None,
            }),
        );
        let saved = crate::persistence::load(&fixture.scratch.path().join("state"));
        let recent = saved.history.items().first().expect("saved playback");
        assert_eq!(recent.entry, Entry::File(CLIP.into()));
        assert_eq!(recent.position_ms, resumable_position.as_millis() as u64);
        assert!(
            !app.saving,
            "session end persists current playback progress"
        );
    }
}

#[test]
fn local_launches_preserve_order_literal_paths_and_safe_uri_routing() {
    {
        let mut fixture = fixture("mixed-inputs", false);
        let file = fixture.scratch.path().join("first.mkv");
        std::fs::write(&file, "fixture").unwrap();
        let folder = fixture.scratch.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        let inside = folder.join("second.mp4");
        std::fs::write(&inside, "fixture").unwrap();
        let first_url = "https://example.com/first.mp4";
        let last_url = "https://example.com/last.mp4";
        let app = &mut fixture.app;
        app.plex.visible = true;
        let task = app.open_arguments(vec![
            first_url.into(),
            file.clone().into_os_string(),
            folder.into_os_string(),
            last_url.into(),
        ]);
        settle(app, task);
        assert!(!app.plex.visible);
        assert_eq!(
            app.playlist.entries(),
            &[
                Entry::Url(first_url.into()),
                Entry::File(file),
                Entry::File(inside),
                Entry::Url(last_url.into()),
            ],
            "mixed files, folders and URLs preserve launch order"
        );
    }
    {
        let mut fixture = fixture("desktop-uris", false);
        let path = fixture.scratch.path().join("spaces # café.mp4");
        std::fs::write(&path, "fixture").unwrap();
        let uri = format!("file://{}", path.display())
            .replace(' ', "%20")
            .replace('#', "%23");
        send(&mut fixture.app, Message::Launched(vec![uri.into()]));
        assert_eq!(
            fixture.app.playlist.current(),
            Some(&Entry::File(path.clone()))
        );
        for rejected in [
            "file://other-host/tmp/video.mp4",
            "file:///tmp/%ZZ",
            "javascript://alert",
            "http://user:secret@example.com/video.mp4",
        ] {
            send(&mut fixture.app, Message::Launched(vec![rejected.into()]));
            assert_eq!(
                fixture.app.playlist.current(),
                Some(&Entry::File(path.clone())),
                "{rejected}"
            );
        }
        assert!(
            !fixture.app.toasts.is_empty(),
            "desktop file uris decode and unsafe addresses do not replace playback"
        );
    }
    {
        use std::os::unix::ffi::OsStringExt;
        let mut fixture = fixture("native-name", false);
        let path = fixture
            .scratch
            .path()
            .join(std::ffi::OsString::from_vec(b"--movie-\xff.mp4".to_vec()));
        std::fs::write(&path, "fixture").unwrap();
        send(
            &mut fixture.app,
            Message::Launched(vec![path.clone().into_os_string()]),
        );
        let entry = Entry::File(path);
        assert_eq!(fixture.app.playlist.current(), Some(&entry));
        fixture.app.settings.history.record(
            &entry,
            "Native movie",
            Duration::from_secs(30),
            Some(Duration::from_secs(120)),
            1,
        );
        let state = fixture.scratch.path().join("saved");
        persistence::save(&state, &fixture.app.settings).unwrap();
        assert_eq!(persistence::load(&state).history.items()[0].entry, entry);
    }
}

#[test]
fn a_late_folder_result_cannot_replace_a_newer_open_request() {
    let mut fixture = fixture("open-order", false);
    let app = &mut fixture.app;
    let late = app.open_arguments(vec!["https://example.com/older.mp4".into()]);
    send(
        app,
        Message::Launched(vec!["https://example.com/newer.mp4".into()]),
    );
    settle(app, late);
    assert_eq!(
        app.playlist.current(),
        Some(&Entry::Url("https://example.com/newer.mp4".into()))
    );
}

#[test]
fn a_drop_batch_keeps_event_order_when_disk_jobs_finish_in_reverse() {
    let mut fixture = fixture("drop-order", false);
    let first = fixture.scratch.path().join("first.mp4");
    let second = fixture.scratch.path().join("second.mp4");
    std::fs::write(&first, "fixture").unwrap();
    std::fs::write(&second, "fixture").unwrap();
    let late = fixture.app.dropped(vec![first.clone()]);
    send(&mut fixture.app, Message::Dropped(vec![second.clone()]));
    assert!(
        fixture.app.playlist.is_empty(),
        "a drop batch keeps event order when disk jobs finish in reverse"
    );
    settle(&mut fixture.app, late);
    assert_eq!(
        fixture.app.playlist.entries(),
        &[Entry::File(first), Entry::File(second)]
    );
}

#[test]
fn playback_pauses_seeks_selects_tracks_and_resumes() {
    let mut fixture = fixture("transport", true);
    let app = &mut fixture.app;
    open(app, &[CLIP.into()]);
    until(app, "decoded playback and tracks", |app| {
        app.status.is_playing()
            && app.presenter.handle().is_some()
            && app.status.audio_tracks.len() == 2
            && !app.status.subtitle_tracks.is_empty()
            && app.status.seekable
    });
    assert!(app.awake.is_wanted(), "playing video inhibits idle");
    let anchored = app.position_received;
    let first = app.shown_position_at(anchored + Duration::from_millis(16));
    let second = app.shown_position_at(anchored + Duration::from_millis(32));
    assert!(
        second > first,
        "the presentation clock advances between backend samples"
    );
    assert!(second - first <= Duration::from_millis(17));
    send(app, Message::ScrubStarted);
    send(app, Message::PointerOverControls(false));
    send(app, Message::HideChrome);
    assert!(
        app.chrome_visible(),
        "a held scrub keeps its controls mounted beyond idle"
    );
    send(app, Message::ScrubReleased);
    send(app, Message::Activity);
    send(app, Message::TogglePlay);
    until(app, "paused", |app| {
        app.status.playback == PlaybackState::Paused
    });
    assert!(
        !app.awake.is_wanted(),
        "paused video releases idle inhibition"
    );
    let activate = |id: &str| MenuButtonEvent::Activated {
        id: EntryId::new(id),
        check: CheckState::Checked,
    };
    send(
        app,
        Message::AudioMenu(MenuButtonEvent::Open(
            enzui_core::menu_model::OpenCause::Pointer,
        )),
    );
    send(app, Message::AudioMenu(activate("track:1")));
    send(
        app,
        Message::SubtitlesMenu(MenuButtonEvent::Open(
            enzui_core::menu_model::OpenCause::Pointer,
        )),
    );
    send(app, Message::SubtitlesMenu(activate("track:0")));
    until(app, "selected tracks", |app| {
        app.status.current_audio == Some(1) && app.status.current_subtitle == Some(0)
    });
    assert_eq!(
        app.shown_position_at(app.position_received + Duration::from_secs(1)),
        app.status.position,
        "paused playback does not extrapolate"
    );
    send(app, Message::SeekTo(Duration::from_secs(2)));
    until(app, "forward seek before scrubbing backward", |app| {
        app.pending_seek.is_none()
            && app.status.position.abs_diff(Duration::from_secs(2)) < Duration::from_millis(150)
    });
    send(app, Message::ScrubStarted);
    send(app, Message::Scrub(2.0));
    std::thread::sleep(Duration::from_millis(250));
    send(app, Message::VideoWake);
    let mut stale = app.status.clone();
    stale.position = Duration::from_millis(500);
    let changed = app.status_changed(stale);
    settle(app, changed);
    assert_eq!(
        app.shown_position(),
        Duration::from_secs(2),
        "backend samples cannot displace a held thumb"
    );
    send(app, Message::Scrub(1.0));
    std::thread::sleep(Duration::from_millis(250));
    send(app, Message::VideoWake);
    let mut stale = app.status.clone();
    stale.position = Duration::from_millis(250);
    let changed = app.status_changed(stale);
    settle(app, changed);
    assert_eq!(
        app.shown_position(),
        Duration::from_secs(1),
        "backend samples cannot displace a held thumb"
    );
    send(app, Message::ScrubReleased);
    assert_eq!(
        app.shown_position(),
        Duration::from_secs(1),
        "preview stays owned until the accurate seek receipt"
    );
    until(app, "settled seek", |app| {
        app.pending_seek.is_none()
            && app.scrub.is_none()
            && app.status.position.abs_diff(Duration::from_secs(1)) < Duration::from_millis(150)
    });
    send(app, Message::VideoWake);
    assert!(
        app.shown_position().abs_diff(Duration::from_secs(1)) < Duration::from_millis(150),
        "a queued forward sample cannot displace the completed backward seek"
    );
    send(app, Message::ScrubStarted);
    send(app, Message::Scrub(1.8));
    let mut unseekable = app.status.clone();
    unseekable.seekable = false;
    let changed = app.status_changed(unseekable);
    settle(app, changed);
    assert_eq!(
        app.shown_position(),
        app.status.position,
        "loss of seekability releases the held preview"
    );
    send(app, Message::ScrubReleased);
    let mut recovered = app.status.clone();
    recovered.seekable = true;
    let changed = app.status_changed(recovered);
    settle(app, changed);
    send(app, Message::SeekTo(Duration::from_secs(1)));
    until(app, "seek after capability recovery", |app| {
        app.pending_seek.is_none()
            && app.status.position.abs_diff(Duration::from_secs(1)) < Duration::from_millis(150)
    });
    send(app, Message::TogglePlay);
    until(app, "resumed", |app| app.status.is_playing());
}

#[test]
fn surface_double_click_changes_fullscreen_without_toggling_paused_playback() {
    let mut fixture = fixture("surface-clicks", true);
    let app = &mut fixture.app;
    open(app, &[CLIP.into()]);
    until(app, "playing", |app| app.status.is_playing());
    send(app, Message::TogglePlay);
    until(app, "paused", |app| {
        app.status.playback == PlaybackState::Paused
    });
    for fullscreen in [true, false] {
        let first = app.update(Message::SurfacePress);
        let second = app.update(Message::SurfacePress);
        let _window = app.update(Message::ToggleFullscreen);
        assert_eq!(app.fullscreen, fullscreen);
        settle(app, first);
        settle(app, second);
        std::thread::sleep(Duration::from_millis(80));
        send(app, Message::VideoWake);
        assert_eq!(app.status.playback, PlaybackState::Paused);
    }
    send(app, Message::SurfacePress);
    until(app, "single click plays", |app| app.status.is_playing());
}

#[test]
fn timeline_interpolation_is_bounded_and_reanchors_after_stalls_and_seeks() {
    let mut fixture = fixture("stalled-clock", false);
    let app = &mut fixture.app;
    app.status.playback = PlaybackState::Playing;
    app.status.position = Duration::from_secs(10);
    app.position_anchor = app.status.position;
    let now = app.position_received;
    assert_eq!(
        app.shown_position_at(now + Duration::from_secs(20)),
        Duration::from_millis(10_250)
    );
    let stale = app.status.clone();
    let _ = app.status_changed(stale);
    assert_eq!(app.position_anchor, Duration::from_secs(10));
    for (buffering, seeking) in [(Some(30), false), (None, true)] {
        app.status.buffering = buffering;
        app.status.seeking = seeking;
        assert_eq!(
            app.shown_position_at(now + Duration::from_secs(60)),
            Duration::from_secs(10)
        );
    }
    app.status.seeking = false;
    let mut after_seek = app.status.clone();
    after_seek.position = Duration::from_secs(2);
    let _ = app.status_changed(after_seek);
    assert_eq!(app.position_anchor, Duration::from_secs(2));
}

#[test]
fn subsecond_service_resume_is_silent_and_later_resume_announces_its_position() {
    for (offset, announce) in [(0, false), (500, false), (1000, true), (60_000, true)] {
        let mut fixture = fixture("resume-osd", true);
        let app = &mut fixture.app;
        let url = "https://example.invalid/clip.mkv".to_owned();
        app.plex.playing_url = Some(url.clone());
        app.plex.playing = Some(crate::plex::Item {
            view_offset: offset,
            ..Default::default()
        });
        app.playlist.replace(vec![Entry::Url(url)]);
        let _ = app.load_with(app.load_token, None);
        assert_eq!(app.osd.is_some(), announce);
        app.video.as_mut().expect("video service").unload();
    }
}

#[test]
fn episode_end_requests_the_next_episode_once_and_rejects_old_lookup_results() {
    let mut fixture = fixture("episode-end", false);
    let app = &mut fixture.app;
    let episode = crate::plex::Item {
        rating_key: "current".into(),
        r#type: "episode".into(),
        ..Default::default()
    };
    app.plex.playing = Some(episode);
    app.plex.visible = false;
    app.status.playback = PlaybackState::Playing;
    let mut ended = app.status.clone();
    ended.playback = PlaybackState::EndOfStream;
    let _ = app.status_changed(ended);
    assert!(app.plex.auto_next);
    let next = crate::plex::Item {
        rating_key: "next".into(),
        r#type: "episode".into(),
        ..Default::default()
    };
    let _ = app.plex_event(crate::plex::Event::NextEpisode(
        app.load_token.wrapping_sub(1),
        "current".into(),
        Box::new(Ok(Some(next.clone()))),
    ));
    assert!(app.plex.selected.is_none());
    let _ = app.plex_event(crate::plex::Event::NextEpisode(
        app.load_token,
        "current".into(),
        Box::new(Ok(Some(next))),
    ));
    assert_eq!(
        app.plex
            .selected
            .as_ref()
            .expect("next selected")
            .rating_key,
        "next"
    );
    assert!(!app.plex.auto_next);
    assert!(app.plex.next_episode.is_none());
}
