use super::*;
#[test]
fn discord_start_uses_received_boundary_deduplicates_and_end_cancels_late_start() {
    let mut app = app();
    app.state.rooms.truncate(1);
    let id = app.state.rooms[0].id.clone();
    app.state.selected = Some(id.clone());
    app.state.room_ready = true;
    let first = app.dispatch(Action::JumpToStart);
    assert!(
        matches!(first.as_slice(),[Effect::Backend(Command::Around{room,item})] if room==&id && item=="d:1")
    );
    assert!(app.dispatch(Action::JumpToStart).is_empty());
    app.apply(Event::HistoryLoaded {
        room: id.clone(),
        direction: chat::Direction::Around,
        history: chat::History::Loaded {
            start: true,
            present: false,
        },
    });
    app.apply(Event::Timeline {
        room: id.clone(),
        messages: vec![Arc::new(ChatMessage {
            item: "d:101".into(),
            ..Default::default()
        })],
    });
    assert_eq!(app.state.jump.as_ref().unwrap().0, "d:101");
    app.state.jump = None;
    app.dispatch(Action::JumpToStart);
    app.dispatch(Action::JumpToPresent);
    app.apply(Event::HistoryLoaded {
        room: id.clone(),
        direction: chat::Direction::Around,
        history: chat::History::Loaded {
            start: true,
            present: false,
        },
    });
    app.apply(Event::Timeline {
        room: id,
        messages: vec![Arc::new(ChatMessage {
            item: "d:102".into(),
            ..Default::default()
        })],
    });
    assert!(app.state.jump.is_none());
}
#[test]
fn read_all_expired_pending_and_changed_marker_are_retryable() {
    let mut app = app();
    app.state.rooms.truncate(1);
    app.dispatch(Action::AskReadAll);
    app.dispatch(Action::ConfirmReadAll);
    app.dispatch(Action::AskReadAll);
    assert_eq!(app.state.read_all.as_ref().unwrap().eligible, 0);
    app.read_all_submitted.values_mut().for_each(|(_, time)| {
        *time = std::time::Instant::now() - std::time::Duration::from_secs(31)
    });
    app.dispatch(Action::AskReadAll);
    assert_eq!(app.state.read_all.as_ref().unwrap().eligible, 1);
    app.dispatch(Action::ConfirmReadAll);
    Arc::make_mut(&mut app.state.rooms[0]).last_read = Some("d:2000".into());
    app.dispatch(Action::AskReadAll);
    assert_eq!(app.state.read_all.as_ref().unwrap().eligible, 1);
}
#[test]
fn matrix_only_read_all_and_start_are_disabled_even_via_direct_actions() {
    let mut app = app();
    app.state.rooms = vec![Arc::new(ChatRoom {
        network: Network::Matrix,
        ..(*room("!held:example.org")).clone()
    })];
    app.state.selected = Some("!held:example.org".into());
    app.state.room_ready = true;
    app.dispatch(Action::AskReadAll);
    assert_eq!(app.state.read_all.as_ref().unwrap().eligible, 0);
    assert!(app.dispatch(Action::ConfirmReadAll).is_empty());
    assert!(app.dispatch(Action::JumpToStart).is_empty());
    assert!(
        app.state
            .notice
            .as_deref()
            .unwrap()
            .contains("Matrix is disabled")
    );
}
#[cfg(all(feature = "discord", feature = "test-fixtures"))]
#[test]
fn same_account_new_provider_source_cancels_review_and_pending() {
    use yapfast_discord::service::ContactSnapshotFixture;
    let mut a = ContactSnapshotFixture::new(&serde_json::json!({"user":{"id":"100"}}));
    let mut b = ContactSnapshotFixture::new(&serde_json::json!({"user":{"id":"100"}}));
    let mut app = app();
    app.apply(Event::Discord(Arc::new(a.snapshot())));
    app.state.rooms = vec![room("discord:@me:10")];
    app.dispatch(Action::AskReadAll);
    assert!(app.state.read_all.is_some());
    app.apply(Event::Discord(Arc::new(b.snapshot())));
    assert!(app.state.read_all.is_none());
    assert!(app.dispatch(Action::ConfirmReadAll).is_empty());
}
fn room(id: &str) -> Arc<ChatRoom> {
    Arc::new(ChatRoom {
        id: id.into(),
        network: Network::Discord,
        unread: 1,
        ..super::preview::preview_room(id, id, true, 2)
    })
}
fn app() -> Application {
    let mut app = Application::default();
    app.state.discord_online = true;
    app.state.discord_user = Some("100".into());
    app.state.rooms = (0..130)
        .map(|i| room(&format!("discord:@me:{i}")))
        .collect();
    app
}
fn marked(effects: Vec<Effect>) -> Vec<(String, bool)> {
    effects
        .into_iter()
        .filter_map(|e| match e {
            Effect::Backend(Command::MarkRead { room, sync }) => Some((room, sync)),
            _ => None,
        })
        .collect()
}
#[test]
fn read_all_is_explicit_bounded_frozen_discord_only_and_retryable() {
    let mut app = app();
    app.state.rooms.push(room("discord:@me:1"));
    app.state.rooms.push(Arc::new(ChatRoom {
        network: Network::Matrix,
        ..(*room("!held:example.org")).clone()
    }));
    app.state.rooms.push(Arc::new(ChatRoom {
        locked: true,
        ..(*room("locked")).clone()
    }));
    assert!(app.dispatch(Action::AskReadAll).is_empty());
    let plan = app.state.read_all.as_ref().unwrap();
    assert_eq!(plan.eligible, 130);
    assert_eq!(plan.rooms.len(), 100);
    let reviewed = plan.rooms.clone();
    app.state.sync_discord_reads = true;
    app.state.rooms.push(room("new-room"));
    let effects = marked(app.dispatch(Action::ConfirmReadAll));
    assert_eq!(effects.len(), 100);
    assert!(
        effects
            .iter()
            .all(|(id, sync)| reviewed.contains(id) && !sync)
    );
    assert!(app.dispatch(Action::ConfirmReadAll).is_empty());
    app.dispatch(Action::AskReadAll);
    assert_eq!(app.state.read_all.as_ref().unwrap().eligible, 31);
    app.dispatch(Action::CancelReadAll);
    assert!(app.dispatch(Action::ConfirmReadAll).is_empty());
    app.backend_failed(&Command::MarkRead {
        room: reviewed[0].clone(),
        sync: false,
    });
    app.dispatch(Action::AskReadAll);
    assert!(
        app.state
            .read_all
            .as_ref()
            .unwrap()
            .rooms
            .contains(&reviewed[0])
    );
    app.state.discord_user = Some("101".into());
    assert!(app.dispatch(Action::ConfirmReadAll).is_empty());
}
