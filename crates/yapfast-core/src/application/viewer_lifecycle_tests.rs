//! Real Application actions/events through the original request worker. Only
//! transport timing and decoder blocking are injected; fixtures are tiny PNGs.
use super::*;
use std::{future::Future, io::Cursor, time::Duration};
use tokio::sync::{mpsc, oneshot};

#[cfg(feature = "discord")]
#[path = "profile_media_lifecycle_tests.rs"]
mod profile_media_lifecycle;

fn fixture() -> Application {
    let mut app = connected();
    app.apply(Event::Timeline {
        room: "dm".into(),
        messages: vec![Arc::new(ChatMessage {
            item: "e:originals".into(),
            attachments: (0..3)
                .map(|_| chat::Attachment {
                    name: "fixture.png".into(),
                    size: None,
                    image: true,
                    dimensions: None,
                    preview: None,
                    preview_failed: false,
                    placement: chat::Placement::Message,
                    gif: None,
                    spoiler: false,
                    video: false,
                    url: None,
                })
                .collect(),
            ..Default::default()
        })],
    });
    app
}

fn open(app: &mut Application) -> Vec<Effect> {
    app.dispatch(Action::ViewImage {
        item: "e:originals".into(),
        index: 0,
    })
}

fn png() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn start<F, D>(
    task: &mut crate::original::Task,
    effects: &[Effect],
    fetch: F,
    decode: D,
    events: mpsc::UnboundedSender<Event>,
) where
    F: Future<Output = Result<Vec<u8>, &'static str>> + Send + 'static,
    D: FnOnce(Vec<u8>) -> Option<Picture> + Send + 'static,
{
    let command = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Backend(command @ Command::ViewImage { .. }) => Some(command.clone()),
            _ => None,
        })
        .expect("the production Application requested an original");
    let Command::ViewImage {
        room,
        item,
        index,
        request,
    } = command
    else {
        unreachable!()
    };
    task.start(request.clone(), fetch, decode, move |picture| {
        let _ = events.send(Event::Image {
            room,
            item,
            index,
            request,
            picture,
        });
    });
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn viewer_lifecycle_cancelled_blocking_decoder_keeps_its_slot_until_exit() {
    runtime().block_on(async {
        let mut app = fixture();
        let mut first = crate::original::Task::default();
        // A different channel/account owner must share the bound too.
        let mut second = crate::original::Task::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let (started, waiting) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        start(
            &mut first,
            &open(&mut app),
            async { Ok(png()) },
            move |data| {
                let _ = started.send(());
                blocked.recv_timeout(Duration::from_secs(3)).unwrap();
                chat::decode_view(&data)
            },
            events.clone(),
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        let next = app.dispatch(Action::StepImage(1));
        let (started, mut waiting) = oneshot::channel();
        start(
            &mut second,
            &next,
            async { Ok(png()) },
            move |data| {
                let _ = started.send(());
                chat::decode_view(&data)
            },
            events,
        );
        let overlap = tokio::time::timeout(Duration::from_millis(80), &mut waiting).await;
        release.send(()).unwrap();
        assert!(
            overlap.is_err(),
            "aborting spawn_blocking's awaiter must not free the decoder slot"
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), incoming.recv())
            .await
            .unwrap()
            .unwrap();
        app.apply(event);
        assert!(
            app.state.viewer.as_ref().unwrap().pictures[&1]
                .as_ref()
                .unwrap()
                .is_ok()
        );
        assert!(
            incoming.try_recv().is_err(),
            "the cancelled blocking decode never publishes"
        );
    });
}

struct Dropped(Option<oneshot::Sender<()>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        if let Some(signal) = self.0.take() {
            let _ = signal.send(());
        }
    }
}

#[test]
fn viewer_lifecycle_close_drops_delayed_fetch_without_waiting_for_another_command() {
    runtime().block_on(async {
        let mut app = fixture();
        let mut task = crate::original::Task::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let (started, waiting) = oneshot::channel();
        let (cancelled, dropped) = oneshot::channel();
        let (release, blocked) = oneshot::channel::<()>();
        start(
            &mut task,
            &open(&mut app),
            async move {
                let _drop = Dropped(Some(cancelled));
                let _ = started.send(());
                let _ = blocked.await;
                Ok(png())
            },
            |_| panic!("closed fetch must never reach the decoder"),
            events.clone(),
        );
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
        app.dispatch(Action::CloseImage);
        tokio::time::timeout(Duration::from_secs(2), dropped)
            .await
            .unwrap()
            .unwrap();
        assert!(
            release.send(()).is_err(),
            "the transport future was actually dropped"
        );
        assert!(incoming.try_recv().is_err());
        assert!(app.state.viewer.is_none());
    });
}

#[test]
fn viewer_lifecycle_fast_navigation_reloads_the_earlier_attachment() {
    runtime().block_on(async {
        let mut app = fixture();
        let mut task = crate::original::Task::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        let mut effects = open(&mut app);
        let earliest = image_request(&effects);
        for _ in 0..32 {
            let request = image_request(&effects);
            let (started, waiting) = oneshot::channel();
            let (cancelled, dropped) = oneshot::channel();
            start(
                &mut task,
                &effects,
                async move {
                    let _drop = Dropped(Some(cancelled));
                    let _ = started.send(());
                    std::future::pending::<()>().await;
                    Ok(png())
                },
                |_| panic!("superseded fetch must never decode"),
                events.clone(),
            );
            tokio::time::timeout(Duration::from_secs(2), waiting)
                .await
                .unwrap()
                .unwrap();
            effects = app.dispatch(Action::StepImage(1));
            assert!(request.is_cancelled());
            assert_eq!(app.state.viewer.as_ref().unwrap().pictures.len(), 1);
            tokio::time::timeout(Duration::from_secs(2), dropped)
                .await
                .unwrap()
                .unwrap();
        }
        // Explicitly return to index zero, including if it was pending before.
        effects = open(&mut app);
        let current = image_request(&effects);
        assert_ne!(earliest, current);
        app.apply(Event::Image {
            room: "dm".into(),
            item: "e:originals".into(),
            index: 0,
            request: earliest,
            picture: Err("late first attempt"),
        });
        assert_eq!(app.state.viewer.as_ref().unwrap().pictures[&0], None);
        start(
            &mut task,
            &effects,
            async { Ok(png()) },
            |data| chat::decode_view(&data),
            events,
        );
        app.apply(
            tokio::time::timeout(Duration::from_secs(2), incoming.recv())
                .await
                .unwrap()
                .unwrap(),
        );
        let viewer = app.state.viewer.as_ref().unwrap();
        let picture = viewer.pictures[&0].as_ref().unwrap().as_ref().unwrap();
        assert_eq!((picture.width, picture.height), (2, 2));
        assert_eq!(&*picture.rgba, &[1, 2, 3, 255].repeat(4));
    });
}

#[test]
fn viewer_lifecycle_failed_backend_cancels_pending_and_reopen_reloads() {
    let mut app = fixture();
    let effects = open(&mut app);
    let request = image_request(&effects);
    let command = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Backend(command @ Command::ViewImage { .. }) => Some(command),
            _ => None,
        })
        .unwrap();
    app.backend_failed(command);
    assert!(
        request.is_cancelled(),
        "no backend will ever complete this request"
    );
    assert!(
        app.state.viewer.as_ref().unwrap().pictures[&0]
            .as_ref()
            .unwrap()
            .is_err(),
        "a rejected command must not leave Loading forever"
    );
    assert_ne!(request, image_request(&open(&mut app)));
}

#[test]
fn viewer_lifecycle_closing_the_source_thread_cancels_its_request() {
    let mut app = fixture();
    let messages = app.state.messages.clone();
    app.state.messages = Arc::default();
    app.state.aside = Some(aside::Aside {
        room: "thread".into(),
        messages,
        ..Default::default()
    });
    let request = image_request(&open(&mut app));
    app.dispatch(Action::CloseAside);
    assert!(request.is_cancelled());
    assert!(app.state.viewer.is_none());
}

#[test]
fn viewer_lifecycle_unlink_cancels_before_the_account_answers() {
    let mut app = fixture();
    app.state.discord_linked = true;
    app.state.discord_user = Some("10".into());
    let request = image_request(&open(&mut app));
    app.dispatch(Action::UnlinkDiscord);
    assert!(request.is_cancelled());
    assert!(app.state.viewer.is_none());
}

#[test]
fn viewer_lifecycle_dropping_application_cancels_its_request() {
    let mut app = fixture();
    let request = image_request(&open(&mut app));
    drop(app);
    assert!(request.is_cancelled());
}

#[test]
fn viewer_lifecycle_queued_completion_holds_the_slot_until_consumed() {
    runtime().block_on(async {
        let mut app = fixture();
        let mut task = crate::original::Task::default();
        let (events, mut incoming) = mpsc::unbounded_channel();
        start(
            &mut task,
            &open(&mut app),
            async { Ok(png()) },
            |data| chat::decode_view(&data),
            events.clone(),
        );
        let stale = tokio::time::timeout(Duration::from_secs(2), incoming.recv())
            .await
            .unwrap()
            .unwrap();
        let next = app.dispatch(Action::StepImage(1));
        let (started, mut waiting) = oneshot::channel();
        start(
            &mut task,
            &next,
            async move {
                let _ = started.send(());
                Ok(png())
            },
            |data| chat::decode_view(&data),
            events,
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut waiting)
                .await
                .is_err(),
            "even a completed original queued for the UI must retain its admission slot"
        );
        app.apply(stale);
        assert_eq!(app.state.viewer.as_ref().unwrap().pictures.len(), 1);
        assert_eq!(app.state.viewer.as_ref().unwrap().pictures[&1], None);
        tokio::time::timeout(Duration::from_secs(2), waiting)
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
            app.state.viewer.as_ref().unwrap().pictures[&1]
                .as_ref()
                .unwrap()
                .is_ok()
        );
    });
}

#[test]
fn viewer_lifecycle_source_transitions_cancel_and_reject_late_events() {
    let transitions: &[(&str, fn(&mut Application))] = &[
        ("close", |app| {
            app.dispatch(Action::CloseImage);
        }),
        ("navigation", |app| {
            app.dispatch(Action::StepImage(1));
        }),
        ("room", |app| {
            app.dispatch(Action::OpenChat("general".into()));
        }),
        ("server", |app| {
            app.dispatch(Action::Navigate(Navigation::Server("friends".into())));
        }),
        ("account", |app| {
            app.apply(Event::Connected {
                user: "@other:test".into(),
                trusted: true,
                storage: "test",
            });
        }),
        ("logout", |app| {
            app.dispatch(Action::Logout);
        }),
        ("logged out", |app| {
            app.apply(Event::LoggedOut);
        }),
        ("stopped", |app| app.stopped()),
    ];
    for (name, transition) in transitions {
        let mut app = fixture();
        let request = image_request(&open(&mut app));
        transition(&mut app);
        assert!(
            request.is_cancelled(),
            "{name} must cancel the original request immediately"
        );
        app.apply(Event::Image {
            room: "dm".into(),
            item: "e:originals".into(),
            index: 0,
            request,
            picture: Err("stale after source change"),
        });
        if let Some(viewer) = &app.state.viewer {
            assert!(
                !viewer.pictures.contains_key(&0),
                "{name} retained a stale result"
            );
        }
    }
}
