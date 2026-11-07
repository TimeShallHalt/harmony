//! Cross-source regressions using the production Application, Task and Loader.
use super::*;

fn profile_command(
    effects: &[Effect],
) -> (u64, crate::profile_media::Asset, crate::original::Request) {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Backend(Command::Discord(DiscordCommand::ViewProfileMedia {
                request,
                asset,
                cancellation,
            })) => Some((*request, asset.clone(), cancellation.clone())),
            _ => None,
        })
        .expect("production Application requested profile media")
}

async fn next_event(incoming: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(2), incoming.recv())
        .await
        .unwrap()
        .unwrap()
}

fn profile_loaded(app: &Application) -> &crate::profile_media::Loaded {
    let Some(Viewer {
        source: ViewerSource::Profile(session),
        ..
    }) = &app.state.viewer
    else {
        panic!("profile is selected");
    };
    session.loaded.as_ref().expect("profile finished loading")
}

#[test]
fn profile_media_cross_source_dropped_loader_finishes_loading_with_an_error_and_reopen_recovers() {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut loader = crate::profile_media_io::Loader::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let (started, waiting) = oneshot::channel();
        let (cancelled, dropped) = oneshot::channel();
        start_profile(
            &mut loader,
            &profile_click(&mut app),
            async move {
                let _drop = Dropped(Some(cancelled));
                let _ = started.send(());
                std::future::pending().await
            },
            events.clone(),
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        drop(loader);
        tokio::time::timeout(Duration::from_secs(2), dropped)
            .await
            .unwrap()
            .unwrap();
        app.dispatch(Action::DismissNotice);
        assert!(
            profile_loaded(&app).picture.is_err(),
            "provider-owned loader drop must not strand Loading"
        );
        let mut loader = crate::profile_media_io::Loader::default();
        start_profile(
            &mut loader,
            &profile_click(&mut app),
            async { Ok(png()) },
            events,
        );
        app.apply(next_event(&mut incoming).await);
        assert!(profile_loaded(&app).picture.is_ok());
    });
}

#[test]
fn profile_media_cross_source_delayed_message_profile_message_returns_to_real_pixels() {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut task = crate::original::Task::default();
        let mut loader = crate::profile_media_io::Loader::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let effects = open(&mut app);
        let old_message = image_request(&effects);
        let (started, waiting) = oneshot::channel();
        let (cancelled, dropped) = oneshot::channel();
        start(
            &mut task,
            &effects,
            async move {
                let _drop = Dropped(Some(cancelled));
                let _ = started.send(());
                std::future::pending().await
            },
            |_| panic!("abandoned message must not decode"),
            events.clone(),
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        let effects = profile_click(&mut app);
        tokio::time::timeout(Duration::from_secs(2), dropped)
            .await
            .unwrap()
            .unwrap();
        let (started, waiting) = oneshot::channel();
        let (cancelled, dropped) = oneshot::channel();
        start_profile(
            &mut loader,
            &effects,
            async move {
                let _drop = Dropped(Some(cancelled));
                let _ = started.send(());
                std::future::pending().await
            },
            events.clone(),
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        let effects = open(&mut app);
        tokio::time::timeout(Duration::from_secs(2), dropped)
            .await
            .unwrap()
            .unwrap();
        assert!(old_message.is_cancelled());
        assert_ne!(old_message, image_request(&effects));
        start(
            &mut task,
            &effects,
            async { Ok(png()) },
            |data| chat::decode_view(&data),
            events,
        );
        app.apply(next_event(&mut incoming).await);
        let picture = app.state.viewer.as_ref().unwrap().pictures[&0]
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        assert_eq!((picture.width, picture.height), (2, 2));
        assert_eq!(&picture.rgba[..], &[1, 2, 3, 255].repeat(4));
        assert!(incoming.try_recv().is_err());
    });
}

#[test]
fn profile_media_cross_source_running_profile_decoder_retains_slot_when_message_opens() {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut task = crate::original::Task::default();
        let mut loader = crate::profile_media_io::Loader::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let (request, asset, cancellation) = profile_command(&profile_click(&mut app));
        let (entered, waiting) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let completed = events.clone();
        loader.start_with_decoder(
            request,
            cancellation.clone(),
            async { Ok(png()) },
            move |data| {
                let _ = entered.send(());
                blocked.recv_timeout(Duration::from_secs(3)).unwrap();
                chat::decode_view(data)
            },
            move |loaded| {
                let _ = completed.send(Event::ProfileMedia {
                    request,
                    asset,
                    loaded,
                });
            },
            Duration::from_secs(2),
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        let next = open(&mut app);
        assert!(cancellation.is_cancelled());
        let (fetching, mut waiting) = oneshot::channel();
        start(
            &mut task,
            &next,
            async move {
                let _ = fetching.send(());
                Ok(png())
            },
            |data| chat::decode_view(&data),
            events,
        );
        let overlap = tokio::time::timeout(Duration::from_millis(80), &mut waiting).await;
        release.send(()).unwrap();
        assert!(
            overlap.is_err(),
            "cancelling an awaiter cannot release an already-running profile decoder's slot"
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        app.apply(next_event(&mut incoming).await);
        assert!(
            app.state.viewer.as_ref().unwrap().pictures[&0]
                .as_ref()
                .unwrap()
                .is_ok()
        );
        assert!(
            incoming.try_recv().is_err(),
            "cancelled profile decoder must not publish"
        );
    });
}

#[test]
fn profile_media_cross_source_cancelled_queued_blocking_decode_never_runs_or_releases_early() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (entered, waiting) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = entered.send(());
            blocked.recv_timeout(Duration::from_secs(3)).unwrap();
        });
        waiting.await.unwrap();
        let mut app = profile_fixture();
        let mut loader = crate::profile_media_io::Loader::default();
        let mut task = crate::original::Task::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let (request, asset, cancellation) = profile_command(&profile_click(&mut app));
        let (fetching, waiting) = oneshot::channel();
        let completed = events.clone();
        loader.start_with_decoder(
            request,
            cancellation,
            async move {
                let _ = fetching.send(());
                Ok(png())
            },
            |_| panic!("cancelled decoder queued behind a blocking barrier must never run"),
            move |loaded| {
                let _ = completed.send(Event::ProfileMedia {
                    request,
                    asset,
                    loaded,
                });
            },
            Duration::from_secs(2),
        );
        waiting.await.unwrap();
        tokio::task::yield_now().await;
        app.dispatch(Action::CloseImage);
        let next = open(&mut app);
        let (fetching, mut waiting) = oneshot::channel();
        start(
            &mut task,
            &next,
            async move {
                let _ = fetching.send(());
                Ok(png())
            },
            |data| chat::decode_view(&data),
            events,
        );
        let overlap = tokio::time::timeout(Duration::from_millis(80), &mut waiting).await;
        release.send(()).unwrap();
        assert!(
            overlap.is_err(),
            "queued blocking work retains admission until its closure is dropped/executed"
        );
        blocker.await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        app.apply(next_event(&mut incoming).await);
        assert!(
            app.state.viewer.as_ref().unwrap().pictures[&0]
                .as_ref()
                .unwrap()
                .is_ok()
        );
        assert!(incoming.try_recv().is_err());
    });
}

#[test]
fn profile_media_cross_source_close_reopen_rejects_queued_profile_then_loads_new_request() {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut loader = crate::profile_media_io::Loader::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let first = profile_click(&mut app);
        let (old, _, old_cancel) = profile_command(&first);
        start_profile(&mut loader, &first, async { Ok(png()) }, events.clone());
        let stale = next_event(&mut incoming).await;
        let (bytes, pixels) = match &stale {
            Event::ProfileMedia { loaded, .. } => (
                Arc::downgrade(loaded.value().bytes.as_ref().unwrap()),
                Arc::downgrade(loaded.value().picture.as_ref().unwrap()),
            ),
            _ => panic!("profile event"),
        };
        app.dispatch(Action::CloseImage);
        let next = profile_click(&mut app);
        assert!(old_cancel.is_cancelled());
        assert_ne!(old, profile_command(&next).0);
        let (fetching, mut waiting) = oneshot::channel();
        start_profile(
            &mut loader,
            &next,
            async move {
                let _ = fetching.send(());
                Ok(png())
            },
            events,
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut waiting)
                .await
                .is_err()
        );
        app.apply(stale);
        assert!(bytes.upgrade().is_none() && pixels.upgrade().is_none());
        let Some(Viewer {
            source: ViewerSource::Profile(session),
            ..
        }) = &app.state.viewer
        else {
            panic!("reopened profile")
        };
        assert!(
            session.loaded.is_none(),
            "old queued result cannot fill a reopened session"
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        app.apply(next_event(&mut incoming).await);
        let loaded = profile_loaded(&app);
        assert_eq!(
            (
                loaded.picture.as_ref().unwrap().width,
                loaded.picture.as_ref().unwrap().height
            ),
            (2, 2)
        );
        let bytes = Arc::downgrade(loaded.bytes.as_ref().unwrap());
        let pixels = Arc::downgrade(loaded.picture.as_ref().unwrap());
        app.dispatch(Action::CloseImage);
        assert!(bytes.upgrade().is_none() && pixels.upgrade().is_none());
    });
}

#[test]
fn profile_media_cross_source_queued_message_blocks_profile_then_is_dropped_without_filling_it() {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut task = crate::original::Task::default();
        let mut loader = crate::profile_media_io::Loader::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        start(
            &mut task,
            &open(&mut app),
            async { Ok(png()) },
            |data| chat::decode_view(&data),
            events.clone(),
        );
        let stale = next_event(&mut incoming).await;
        let pixels = match &stale {
            Event::Image {
                picture: Ok(picture),
                ..
            } => Arc::downgrade(picture.value()),
            _ => panic!("message event"),
        };
        let effects = profile_click(&mut app);
        let (fetching, mut waiting) = oneshot::channel();
        start_profile(
            &mut loader,
            &effects,
            async move {
                let _ = fetching.send(());
                Ok(png())
            },
            events,
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut waiting)
                .await
                .is_err()
        );
        app.apply(stale);
        assert!(
            pixels.upgrade().is_none(),
            "stale queued message pixels must not accumulate"
        );
        assert!(matches!(
            app.state.viewer.as_ref().map(|v| &v.source),
            Some(ViewerSource::Profile(_))
        ));
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        app.apply(next_event(&mut incoming).await);
        assert!(profile_loaded(&app).picture.is_ok());
    });
}

#[test]
fn profile_media_cross_source_real_decode_error_keeps_exact_save_bytes_until_close() {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut loader = crate::profile_media_io::Loader::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let effects = profile_click(&mut app);
        let request = profile_command(&effects).0;
        let raw = b"unsupported image fixture".to_vec();
        let expected = raw.clone();
        start_profile(&mut loader, &effects, async move { Ok(raw) }, events);
        app.apply(next_event(&mut incoming).await);
        let loaded = profile_loaded(&app);
        assert!(loaded.picture.as_ref().unwrap_err().contains("Save"));
        let bytes = Arc::downgrade(loaded.bytes.as_ref().unwrap());
        let effects = app.dispatch(Action::SaveProfileMedia { request, path: "fixture-not-written.png".into() });
        assert!(effects.iter().any(|effect| matches!(effect,
            Effect::Backend(Command::Discord(DiscordCommand::SaveProfileMedia { bytes, .. })) if &bytes[..] == expected.as_slice())));
        drop(effects);
        app.dispatch(Action::CloseImage);
        assert!(bytes.upgrade().is_none());
    });
}

#[test]
fn profile_media_cross_source_message_profile_message_preserves_new_selection() {
    let mut app = fixture();
    app.state.discord_user = Some("100".into());
    app.state.discord_online = true;
    app.state.profile = Some(ProfileView {
        profile: Profile {
            id: "200".into(),
            avatar: Some("https://cdn.discordapp.com/avatars/200/0123456789abcdef0123456789abcdef.png?size=128".into()),
            ..Default::default()
        },
        ..Default::default()
    });
    let message = image_request(&open(&mut app));
    let effects = app.dispatch(Action::ViewProfileMedia {
        user: "200".into(),
        slot: crate::profile_media::Slot::Profile,
        kind: chat::ProfilePicture::Avatar,
    });
    assert!(
        message.is_cancelled(),
        "message owner must cancel its old work"
    );
    assert!(
        matches!(
            app.state.viewer.as_ref().map(|v| &v.source),
            Some(ViewerSource::Profile(_))
        ),
        "message reconciliation must not erase the newly selected profile"
    );
    let profile = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Backend(Command::Discord(DiscordCommand::ViewProfileMedia {
                request, ..
            })) => Some(*request),
            _ => None,
        })
        .unwrap();
    let next = open(&mut app);
    assert_ne!(
        message,
        image_request(&next),
        "return needs a fresh original, not a stale Loading marker"
    );
    assert!(
        matches!(
            app.state.viewer.as_ref().map(|v| &v.source),
            Some(ViewerSource::Message)
        ),
        "profile reconciliation must preserve a newly opened message"
    );
    assert!(next.iter().any(|effect| matches!(effect,
        Effect::Backend(Command::Discord(DiscordCommand::CancelProfileMedia(request))) if *request == profile)));
}

fn profile_fixture() -> Application {
    let mut app = fixture();
    app.state.discord_user = Some("100".into());
    app.state.discord_online = true;
    app.state.profile = Some(ProfileView {
        profile: Profile {
            id: "200".into(),
            avatar: Some("https://cdn.discordapp.com/avatars/200/0123456789abcdef0123456789abcdef.png?size=128".into()),
            ..Default::default()
        },
        ..Default::default()
    });
    app
}

fn profile_click(app: &mut Application) -> Vec<Effect> {
    app.dispatch(Action::ViewProfileMedia {
        user: "200".into(),
        slot: crate::profile_media::Slot::Profile,
        kind: chat::ProfilePicture::Avatar,
    })
}

fn start_profile<F>(
    loader: &mut crate::profile_media_io::Loader,
    effects: &[Effect],
    fetch: F,
    events: mpsc::UnboundedSender<Event>,
) where
    F: Future<Output = Result<Vec<u8>, &'static str>> + Send + 'static,
{
    let (request, asset, cancellation) = profile_command(effects);
    loader.start(request, cancellation, fetch, move |loaded| {
        let _ = events.send(Event::ProfileMedia {
            request,
            asset,
            loaded,
        });
    });
}

#[test]
fn profile_media_cross_source_profile_fetch_waits_for_cancelled_message_decoder() {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut task = crate::original::Task::default();
        let mut loader = crate::profile_media_io::Loader::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let (entered, wait_entered) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        start(
            &mut task,
            &open(&mut app),
            async { Ok(png()) },
            move |data| {
                let _ = entered.send(());
                blocked.recv_timeout(Duration::from_secs(3)).unwrap();
                chat::decode_view(&data)
            },
            events.clone(),
        );
        tokio::time::timeout(Duration::from_secs(2), wait_entered)
            .await
            .unwrap()
            .unwrap();
        let effects = profile_click(&mut app);
        let (fetching, mut wait_fetching) = oneshot::channel();
        start_profile(
            &mut loader,
            &effects,
            async move {
                let _ = fetching.send(());
                Ok(png())
            },
            events,
        );
        let overlap = tokio::time::timeout(Duration::from_millis(80), &mut wait_fetching).await;
        release.send(()).unwrap();
        assert!(
            overlap.is_err(),
            "profile fetch must share the message decoder's admission slot"
        );
        tokio::time::timeout(Duration::from_secs(2), wait_fetching)
            .await
            .unwrap()
            .unwrap();
        app.apply(
            tokio::time::timeout(Duration::from_secs(2), incoming.recv())
                .await
                .unwrap()
                .unwrap(),
        );
        let Some(Viewer {
            source: ViewerSource::Profile(session),
            ..
        }) = &app.state.viewer
        else {
            panic!("profile remains selected")
        };
        assert!(session.loaded.as_ref().unwrap().picture.is_ok());
        assert!(
            incoming.try_recv().is_err(),
            "cancelled message must never publish"
        );
    });
}

#[test]
fn profile_media_cross_source_queued_profile_completion_bounds_message_fetch_and_drops_stale_buffers()
 {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut loader = crate::profile_media_io::Loader::default();
        let mut task = crate::original::Task::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        start_profile(
            &mut loader,
            &profile_click(&mut app),
            async { Ok(png()) },
            events.clone(),
        );
        let stale = tokio::time::timeout(Duration::from_secs(2), incoming.recv())
            .await
            .unwrap()
            .unwrap();
        let (bytes, pixels) = match &stale {
            Event::ProfileMedia { loaded, .. } => (
                Arc::downgrade(loaded.value().bytes.as_ref().unwrap()),
                Arc::downgrade(loaded.value().picture.as_ref().unwrap()),
            ),
            _ => panic!("profile event"),
        };
        let next = open(&mut app);
        let (fetching, mut wait_fetching) = oneshot::channel();
        start(
            &mut task,
            &next,
            async move {
                let _ = fetching.send(());
                Ok(png())
            },
            |data| chat::decode_view(&data),
            events,
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut wait_fetching)
                .await
                .is_err(),
            "a queued profile completion must own the shared slot until consumed or dropped"
        );
        app.apply(stale);
        assert!(
            bytes.upgrade().is_none(),
            "stale original bytes must not accumulate"
        );
        assert!(
            pixels.upgrade().is_none(),
            "stale decoded pixels must be dropped, not accepted"
        );
        assert!(matches!(
            app.state.viewer.as_ref().map(|v| &v.source),
            Some(ViewerSource::Message)
        ));
        tokio::time::timeout(Duration::from_secs(2), wait_fetching)
            .await
            .unwrap()
            .unwrap();
        app.apply(
            tokio::time::timeout(Duration::from_secs(2), incoming.recv())
                .await
                .unwrap()
                .unwrap(),
        );
        assert!(
            app.state.viewer.as_ref().unwrap().pictures[&0]
                .as_ref()
                .unwrap()
                .is_ok(),
            "returning to the message must not stay Loading"
        );
    });
}

#[test]
fn profile_media_cross_source_application_owns_cancellation_not_cloned_presentations() {
    runtime().block_on(async {
        let mut app = profile_fixture();
        let mut loader = crate::profile_media_io::Loader::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let (started, waiting) = oneshot::channel();
        let (cancelled, dropped) = oneshot::channel();
        start_profile(
            &mut loader,
            &profile_click(&mut app),
            async move {
                let _drop = Dropped(Some(cancelled));
                let _ = started.send(());
                std::future::pending().await
            },
            events,
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        let presentation = app.state.viewer.clone();
        drop(app);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), dropped)
                .await
                .is_ok(),
            "Application drop must cancel the actual profile fetch even if UI state is cloned"
        );
        assert!(
            presentation.is_some(),
            "snapshot is deliberately still alive"
        );
        assert!(
            incoming.try_recv().is_err(),
            "cancelled fetch must not publish"
        );
    });
}

#[test]
fn profile_media_cross_source_transitions_drop_pending_fetch_without_cleanup_delivery() {
    let transitions: &[(&str, fn(&mut Application))] = &[
        ("close", |app| {
            app.dispatch(Action::CloseImage);
        }),
        ("message", |app| {
            open(app);
        }),
        ("replacement profile", |app| {
            profile_click(app);
        }),
        ("profile identity", |app| {
            app.state.profile.as_mut().unwrap().profile.avatar = None;
            app.dispatch(Action::DismissNotice);
        }),
        ("account", |app| {
            app.apply(Event::Discord(Arc::new(
                yapfast_discord::service::Snapshot {
                    user_id: Some("101".into()),
                    status: yapfast_discord::service::Status::Online,
                    ..Default::default()
                },
            )));
        }),
        ("unlink", |app| {
            app.state.discord_linked = true;
            app.dispatch(Action::UnlinkDiscord);
        }),
        ("logout", |app| {
            app.dispatch(Action::Logout);
        }),
        ("logged out", |app| {
            app.apply(Event::LoggedOut);
        }),
        ("stopped", |app| {
            app.stopped();
        }),
    ];
    runtime().block_on(async {
        for (name, transition) in transitions {
            let mut app = profile_fixture();
            let mut loader = crate::profile_media_io::Loader::default();
            let (events, mut incoming) = mpsc::unbounded_channel();
            let (started, waiting) = oneshot::channel();
            let (cancelled, dropped) = oneshot::channel();
            start_profile(
                &mut loader,
                &profile_click(&mut app),
                async move {
                    let _drop = Dropped(Some(cancelled));
                    let _ = started.send(());
                    std::future::pending().await
                },
                events,
            );
            tokio::time::timeout(Duration::from_secs(2), waiting)
                .await
                .unwrap()
                .unwrap();
            let presentation = app.state.viewer.clone();
            transition(&mut app);
            assert!(
                tokio::time::timeout(Duration::from_millis(100), dropped)
                    .await
                    .is_ok(),
                "{name} must drop the actual transport without relying on a Cancel command"
            );
            assert!(presentation.is_some());
            assert!(
                incoming.try_recv().is_err(),
                "{name} must not publish after cancellation"
            );
        }
    });
}
