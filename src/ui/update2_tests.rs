//! Update2: actual native Adapter + received snapshots, no service constructors.
use super::*;
use i_slint_core::{accessibility::AccessibleStringProperty, item_tree::ItemRc};
use std::ops::ControlFlow;
use yapfast_discord::{
    ready,
    service::{Snapshot, Status, VoiceMember},
};

fn fixture() -> (tempfile::TempDir, YapfastWindow, Rc<RefCell<Adapter>>) {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let adapter = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    bind_callbacks(&window, &adapter);
    adapter.borrow_mut().app.state.account = Some("anonymous-fixture".into());
    adapter.borrow_mut().app.state.trusted = true;
    window.set_workspace(true);
    window.set_trusted(true);
    window.show().unwrap();
    (dir, window, adapter)
}

fn snapshot() -> Snapshot {
    let server = |id: &str, name: &str, channel: &str| {
        Arc::new(ready::Server {
            id: id.into(),
            name: name.into(),
            text_channels: vec![ready::Channel {
                id: format!("{id}2"),
                name: "General".into(),
                ..Default::default()
            }],
            voice_channels: vec![ready::Channel {
                id: channel.into(),
                name: "Lounge".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .into()
    };
    Snapshot {
        status: Status::Online,
        user_id: Some("100".into()),
        servers: vec![server("10", "Alpha", "11"), server("20", "Beta", "21")],
        ..Default::default()
    }
}

fn member(streaming: bool) -> VoiceMember {
    VoiceMember {
        user_id: "200".into(),
        name: "Anonymous fixture".into(),
        avatar: String::new(),
        muted: false,
        deafened: false,
        server_muted: false,
        server_deafened: false,
        streaming,
        video: false,
        elsewhere: false,
    }
}

fn receive(adapter: &Rc<RefCell<Adapter>>, window: &YapfastWindow, state: Snapshot) {
    let mut a = adapter.borrow_mut();
    let reach = a.apply(Event::Discord(Arc::new(state)));
    a.show(window, reach);
    drop(a);
    i_slint_core::window::WindowInner::from_pub(window.window()).ensure_tree_instantiated();
    slint::platform::update_timers_and_animations();
}

fn label(window: &YapfastWindow, name: &str, voice: usize, streams: usize) -> bool {
    let text = format!("{name}, received voice: {voice}, received streams: {streams}");
    native_item(window, &text).is_some()
}

fn native_item(window: &YapfastWindow, label: &str) -> Option<ItemRc> {
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    ItemRc::new_root(inner.component()).visit_descendants(|item| {
        if item.is_visible()
            && item
                .accessible_string_property(AccessibleStringProperty::Label)
                .as_deref()
                == Some(label)
        {
            ControlFlow::Break(item.clone())
        } else {
            ControlFlow::Continue(())
        }
    })
}

#[test]
fn received_server_badges_follow_snapshot_changes_and_clear_at_lifetime_boundaries() {
    let (_dir, window, adapter) = fixture();
    let mut state = snapshot();
    state.voice.insert("11".into(), vec![member(true)]);
    receive(&adapter, &window, state.clone());
    assert!(window.get_workspace());
    assert!(window.get_servers().iter().any(|row| row.name == "Alpha"));
    assert!(
        label(&window, "Alpha", 1, 1),
        "received Alpha voice + stream must reach the native server icon"
    );
    assert!(
        !label(&window, "Beta", 1, 1),
        "activity must not leak to another server"
    );
    assert!(native_item(&window, "Received voice activity").is_some());
    assert!(native_item(&window, "Received streaming activity").is_some());
    state.voice.insert("11".into(), vec![member(false)]);
    receive(&adapter, &window, state.clone());
    assert!(
        label(&window, "Alpha", 1, 0),
        "ordinary voice is not a stream"
    );
    assert!(native_item(&window, "Received voice activity").is_some());
    assert!(native_item(&window, "Received streaming activity").is_none());
    state.voice.clear();
    state.voice.insert("21".into(), vec![member(true)]);
    receive(&adapter, &window, state.clone());
    assert!(!label(&window, "Alpha", 1, 0));
    assert!(label(&window, "Beta", 1, 1));
    Arc::make_mut(&mut state.servers[1].server).unavailable = true;
    receive(&adapter, &window, state.clone());
    assert!(
        !label(&window, "Beta", 1, 1),
        "unavailable must clear stale activity"
    );
    Arc::make_mut(&mut state.servers[1].server).unavailable = false;
    state.status = Status::Offline("anonymous fixture disconnect".into());
    receive(&adapter, &window, state.clone());
    assert!(
        !label(&window, "Beta", 1, 1),
        "retained cached voice must not survive disconnect"
    );
    assert!(native_item(&window, "Received voice activity").is_none());
    assert!(native_item(&window, "Received streaming activity").is_none());
    state.status = Status::Online;
    receive(&adapter, &window, state);
    assert!(label(&window, "Beta", 1, 1));
    let replacement = Snapshot {
        status: Status::Online,
        user_id: Some("101".into()),
        ..Default::default()
    };
    receive(&adapter, &window, replacement);
    assert!(
        !label(&window, "Beta", 1, 1),
        "replacement must remove the former account's icons"
    );
    assert!(
        adapter.borrow().backend.is_none(),
        "received badges never start a backend/call/probe"
    );
}

#[test]
fn silent_typing_uses_the_real_toggle_composer_dispatch_and_anonymous_transport_across_recreation()
{
    let (dir, window, adapter) = fixture();
    receive(&adapter, &window, snapshot());
    let room = adapter
        .borrow()
        .app
        .state
        .rooms
        .iter()
        .find(|room| !room.voice)
        .unwrap()
        .id
        .clone();
    {
        let mut a = adapter.borrow_mut();
        a.app.state.selected = Some(room.clone());
        a.app.state.room_ready = true;
    }
    let (backend, mut commands) =
        yapfast_core::backend::Backend::anonymous_fixture(adapter.borrow().runtime.clone());
    adapter.borrow_mut().backend = Some(backend);
    assert!(
        window.get_share_typing(),
        "ordinary typing remains the default"
    );
    window.invoke_draft_edited("ordinary".into());
    assert!(
        matches!(commands.try_recv(), Ok(Command::Typing { typing: true, room: ref id }) if id == &room)
    );
    assert!(matches!(commands.try_recv(), Ok(Command::SaveDraft { .. })));
    window.invoke_draft_edited("".into());
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::Typing { typing: false, .. })
    ));
    assert!(matches!(commands.try_recv(), Ok(Command::SaveDraft { .. })));
    window.set_settings_section(2);
    window.set_settings_open(true);
    for _ in 0..3 {
        i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        i_slint_core::window::WindowInner::from_pub(window.window()).ensure_tree_instantiated();
    }
    // Materialize the conditional page and scroll its real production viewport.
    window
        .window()
        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(700., 480.),
            delta_x: 0.,
            delta_y: -320.,
        });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(200));
    i_slint_core::window::WindowInner::from_pub(window.window()).ensure_tree_instantiated();
    // The real production settings control, driven through native accessibility,
    // not a duplicate checkbox or an OS accessibility/input API.
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    ItemRc::new_root(inner.component())
        .visit_descendants(|item| {
            if item.is_visible()
                && item
                    .accessible_string_property(AccessibleStringProperty::Label)
                    .as_deref()
                    == Some("Let others see when you're typing")
                && item
                    .supported_accessibility_actions()
                    .contains(i_slint_core::accessibility::SupportedAccessibilityAction::Default)
            {
                ControlFlow::Break(item.clone())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("actionable production typing switch (not its descriptive Text)")
        .accessible_action(&i_slint_core::accessibility::AccessibilityAction::Default);
    assert!(!window.get_share_typing());
    assert_eq!(Layout::read(dir.path()).share_typing, Some(false));
    window.set_settings_open(false);
    for text in ["silent", ""] {
        window.invoke_draft_edited(text.into());
        assert!(
            matches!(commands.try_recv(), Ok(Command::SaveDraft { .. })),
            "only draft saving, never start/stop typing"
        );
        assert!(commands.try_recv().is_err());
    }
    {
        let mut a = adapter.borrow_mut();
        let reach = a.apply(Event::Typing {
            room: room.clone(),
            names: vec!["Received fixture".into()],
        });
        a.show(&window, reach);
    }
    assert_eq!(window.get_typing(), "Received fixture is typing...");
    // Device-local intent remains off when the provider account is replaced.
    let mut replacement = snapshot();
    replacement.user_id = Some("101".into());
    receive(&adapter, &window, replacement);
    while commands.try_recv().is_ok() {}
    adapter.borrow_mut().app.state.selected = Some(room.clone());
    window.invoke_draft_edited("new context".into());
    assert!(matches!(commands.try_recv(), Ok(Command::SaveDraft { .. })));
    assert!(commands.try_recv().is_err());
    let recreated = YapfastWindow::new().unwrap();
    Layout::read(dir.path()).apply(&recreated);
    adapter.borrow_mut().window = recreated.as_weak();
    bind_callbacks(&recreated, &adapter);
    assert!(!recreated.get_share_typing());
    recreated.invoke_draft_edited("recreated".into());
    assert!(matches!(commands.try_recv(), Ok(Command::SaveDraft { .. })));
    assert!(commands.try_recv().is_err());
    recreated.set_share_typing(true);
    recreated.invoke_preferences_changed();
    recreated.invoke_draft_edited("allowed again".into());
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::Typing { typing: true, .. })
    ));
    assert!(matches!(commands.try_recv(), Ok(Command::SaveDraft { .. })));
    assert_eq!(Layout::read(dir.path()).share_typing, Some(true));
}

#[test]
fn received_folder_counts_follow_open_and_closed_rail_without_fabricating_a_call() {
    let mut app = Application::default();
    let mut received = snapshot();
    received.voice.insert("11".into(), vec![member(false)]);
    received.voice.insert("21".into(), vec![member(true)]);
    app.apply(Event::Discord(Arc::new(received)));
    let folder = yapfast_core::chat::ServerFolder {
        id: "folder".into(),
        name: Some("Fixture".into()),
        color: None,
    };
    for server in &mut app.state.servers {
        Arc::make_mut(server).folder = Some(folder.clone());
    }
    let rows = |state: &AppState, open| {
        let rooms = state.rooms.iter().map(|r| (r.id.as_str(), &**r)).collect();
        let open = if open {
            ["folder".to_string()].into_iter().collect()
        } else {
            Default::default()
        };
        refresh::rail_rows(state, &rooms, &open)
            .iter()
            .map(|r| (r.id.to_string(), r.voice_members, r.live_streams, r.call))
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&app.state, false), [("folder".into(), 2, 1, false)]);
    assert_eq!(
        rows(&app.state, true),
        [
            ("folder".into(), 2, 1, false),
            ("discord:10".into(), 1, 0, false),
            ("discord:20".into(), 1, 1, false)
        ]
    );
    app.state.discord_online = false;
    assert!(
        rows(&app.state, true)
            .iter()
            .all(|r| r.1 == 0 && r.2 == 0 && !r.3)
    );
}
