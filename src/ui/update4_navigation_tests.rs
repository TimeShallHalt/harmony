//! Real Adapter, fake backend, in-memory keyboard only.
use super::*;
fn fixture() -> (tempfile::TempDir, YapfastWindow, Rc<RefCell<Adapter>>) {
    let directory = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let adapter = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        directory.path().into(),
        &window,
    )
    .unwrap();
    adapter.borrow_mut().layout.sound_volume = Some(0); // sounds::play returns before any device/thread
    bind_callbacks(&window, &adapter);
    (directory, window, adapter)
}
#[cfg(feature = "discord")]
fn key(window: &YapfastWindow, key: slint::platform::Key) {
    window
        .window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.into() });
}
#[cfg(feature = "discord")]
fn timeline_rows(adapter: &Rc<RefCell<Adapter>>, count: usize) {
    let mut a = adapter.borrow_mut();
    let mut snapshot = yapfast_discord::service::Snapshot {
        status: yapfast_discord::service::Status::Online,
        user_id: Some("1".into()),
        ..Default::default()
    };
    snapshot
        .conversations
        .push(Arc::new(yapfast_discord::service::Conversation {
            id: "10".into(),
            name: "owned key fixture".into(),
            ..Default::default()
        }));
    a.app.apply(Event::Discord(Arc::new(snapshot)));
    a.app.state.selected = Some("discord:@me:10".into());
    a.app.state.room_ready = true;
    a.app.state.trusted = true;
    a.app.state.account = Some("1".into());
    a.app.state.dm_screen = None;
    a.app.state.messages = Arc::new(
        (0..count)
            .map(|index| {
                Arc::new(ChatMessage {
                    id: format!("{}", index + 100),
                    item: format!("d:{}", index + 100),
                    author: "Owned fixture".into(),
                    body: format!("Owned timeline row {index}"),
                    ..Default::default()
                })
            })
            .collect(),
    );
}

#[cfg(feature = "discord")]
fn settle_timeline(window: &YapfastWindow) {
    for _ in 0..8 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        slint::platform::update_timers_and_animations();
        let _ = window.invoke_timeline_extent();
    }
}
#[test]
fn intervening_navigation_and_detach_cancel_present_selection_intent() {
    i_slint_backend_testing::init_no_event_loop();
    let (_dir, window, adapter) = fixture();
    timeline_rows(&adapter, 80);
    let (backend, _commands) = Backend::anonymous_fixture(adapter.borrow().runtime.clone());
    adapter.borrow_mut().backend = Some(backend);
    adapter.borrow_mut().app.state.detached = true;
    adapter.borrow_mut().refresh(&window);
    window.invoke_jump_to_present();
    assert!(adapter.borrow().present_selection.is_some());
    adapter
        .borrow_mut()
        .action(&window, Action::Jump("d:9000".into()));
    assert!(
        adapter.borrow().present_selection.is_none(),
        "another navigation cancels deferred Present selection"
    );
    adapter.borrow_mut().app.state.detached = true;
    window.invoke_jump_to_present();
    assert!(adapter.borrow().present_selection.is_some());
    adapter.borrow_mut().detach();
    assert!(
        adapter.borrow().present_selection.is_none(),
        "detach cannot surprise-scroll a later window"
    );
}

#[test]
fn home_requests_discord_boundary_and_matrix_route_is_disabled() {
    i_slint_backend_testing::init_no_event_loop();
    let (_dir, window, adapter) = fixture();
    timeline_rows(&adapter, 80);
    let (backend, mut commands) = Backend::anonymous_fixture(adapter.borrow().runtime.clone());
    adapter.borrow_mut().backend = Some(backend);
    adapter.borrow_mut().refresh(&window);
    window.show().unwrap();
    settle_timeline(&window);
    window.invoke_browse_messages();
    while commands.try_recv().is_ok() {}
    key(&window, slint::platform::Key::Home);
    assert!(
        std::iter::from_fn(|| commands.try_recv().ok()).any(
            |c| matches!(c, Command::Around{room,item} if room == "discord:@me:10" && item == "d:1")
        ),
        "Home requests server-accessible start, not first loaded row"
    );
}

#[test]
fn repair1_navigation_failure_retires_only_matching_direction_intent() {
    i_slint_backend_testing::init_no_event_loop();
    for home in [false, true] {
        let (_dir, window, adapter) = fixture();
        timeline_rows(&adapter, 80);
        let (backend, _commands) = Backend::anonymous_fixture(adapter.borrow().runtime.clone());
        adapter.borrow_mut().backend = Some(backend);
        adapter.borrow_mut().app.state.detached = !home;
        adapter.borrow_mut().refresh(&window);
        if home {
            window.invoke_jump_to_start();
        } else {
            window.invoke_jump_to_present();
        }
        {
            let mut a = adapter.borrow_mut();
            let intended = if home {
                &a.start_selection
            } else {
                &a.present_selection
            };
            assert!(intended.is_some());
            a.apply(Event::HistoryLoaded {
                room: "discord:@me:10".into(),
                direction: if home {
                    yapfast_core::chat::Direction::Latest
                } else {
                    yapfast_core::chat::Direction::Around
                },
                history: yapfast_core::chat::History::Failed,
            });
            assert!(
                if home {
                    a.start_selection.is_some()
                } else {
                    a.present_selection.is_some()
                },
                "unrelated direction cannot retire current request"
            );
            a.apply(Event::HistoryLoaded {
                room: "discord:@me:10".into(),
                direction: if home {
                    yapfast_core::chat::Direction::Around
                } else {
                    yapfast_core::chat::Direction::Latest
                },
                history: yapfast_core::chat::History::Failed,
            });
            assert!(
                if home {
                    a.start_selection.is_none()
                } else {
                    a.present_selection.is_none()
                },
                "matching failure must retire deferred navigation"
            );
        }
    }
}

#[test]
fn repair1_home_disjoint_earliest_page_preserves_boundary_and_selects_first_visible() {
    i_slint_backend_testing::init_no_event_loop();
    for (blocked_first, empty) in [(false, false), (true, false), (false, true)] {
        let (_dir, window, adapter) = fixture();
        timeline_rows(&adapter, 80);
        let (backend, _commands) = Backend::anonymous_fixture(adapter.borrow().runtime.clone());
        adapter.borrow_mut().backend = Some(backend);
        adapter.borrow_mut().app.state.hide_blocked = true;
        adapter.borrow_mut().refresh(&window);
        window.show().unwrap();
        settle_timeline(&window);
        window.invoke_jump_to_start();
        {
            let mut a = adapter.borrow_mut();
            a.apply(Event::HistoryLoaded {
                room: "discord:@me:10".into(),
                direction: yapfast_core::chat::Direction::Around,
                history: yapfast_core::chat::History::Loaded {
                    start: true,
                    present: false,
                },
            });
            a.apply(Event::Timeline {
                room: "discord:@me:10".into(),
                messages: if empty {
                    vec![]
                } else {
                    (0..80)
                        .map(|i| {
                            Arc::new(ChatMessage {
                                id: format!("{}", i + 2),
                                item: format!("d:{}", i + 2),
                                body: format!("earliest accessible {i}"),
                                blocked: blocked_first && i == 0,
                                ..Default::default()
                            })
                        })
                        .collect()
                },
            });
            assert!(
                a.app.state.history_start,
                "correlated earliest boundary must survive a disjoint replacement"
            );
            assert!(
                a.app
                    .state
                    .notice
                    .as_ref()
                    .is_none_or(|n| !n.contains("did not establish"))
            );
            a.refresh(&window);
        }
        settle_timeline(&window);
        if !empty {
            assert_eq!(
                window.get_active_item(),
                if blocked_first { "d:3" } else { "d:2" }
            );
            assert_eq!(window.get_active_index(), 0);
            // A fresh boundary is consumed once: later oldest-row eviction is
            // still allowed to invalidate it.
            let mut a = adapter.borrow_mut();
            a.apply(Event::Timeline {
                room: "discord:@me:10".into(),
                messages: vec![],
            });
            assert!(!a.app.state.history_start);
        } else {
            assert!(adapter.borrow().app.state.messages.is_empty());
        }
    }
}

#[test]
fn repair1_repeated_end_and_pending_home_end_select_delivered_newest() {
    i_slint_backend_testing::init_no_event_loop();
    for home_pending in [false, true] {
        let (_dir, window, adapter) = fixture();
        timeline_rows(&adapter, 80);
        let (backend, mut commands) = Backend::anonymous_fixture(adapter.borrow().runtime.clone());
        adapter.borrow_mut().backend = Some(backend);
        adapter.borrow_mut().app.state.detached = !home_pending;
        adapter.borrow_mut().refresh(&window);
        window.show().unwrap();
        settle_timeline(&window);
        window.invoke_browse_messages();
        key(&window, slint::platform::Key::PageUp);
        while commands.try_recv().is_ok() {}
        if home_pending {
            window.invoke_jump_to_start();
        } else {
            window.invoke_jump_to_present();
        }
        window.invoke_jump_to_present();
        assert!(
            std::iter::from_fn(|| commands.try_recv().ok())
                .any(|c| matches!(c, Command::Present(_)))
        );
        {
            let mut a = adapter.borrow_mut();
            a.apply(Event::HistoryLoaded {
                room: "discord:@me:10".into(),
                direction: yapfast_core::chat::Direction::Latest,
                history: yapfast_core::chat::History::Loaded {
                    start: false,
                    present: true,
                },
            });
            a.apply(Event::Timeline {
                room: "discord:@me:10".into(),
                messages: (0..100)
                    .map(|i| {
                        Arc::new(ChatMessage {
                            id: format!("{}", i + 2000),
                            item: format!("d:{}", i + 2000),
                            body: format!("delivered {i}"),
                            ..Default::default()
                        })
                    })
                    .collect(),
            });
            a.refresh(&window);
        }
        settle_timeline(&window);
        assert_eq!(
            window.get_active_item(),
            "d:2099",
            "both End sequences must retain actual Present completion intent"
        );
        assert_eq!(window.get_active_index(), 99);
        assert!(window.invoke_timeline_below() <= 0.5);
        window.invoke_browse_messages();
        key(&window, slint::platform::Key::UpArrow);
        assert_eq!(window.get_active_item(), "d:2098");
    }
}

#[test]
fn present_replacement_selects_newest_item_and_next_key_uses_it() {
    i_slint_backend_testing::init_no_event_loop();
    for empty in [false, true] {
        let (_dir, window, adapter) = fixture();
        timeline_rows(&adapter, if empty { 0 } else { 80 });
        let (backend, mut commands) = Backend::anonymous_fixture(adapter.borrow().runtime.clone());
        adapter.borrow_mut().backend = Some(backend);
        adapter.borrow_mut().app.state.detached = true;
        adapter.borrow_mut().refresh(&window);
        window.show().unwrap();
        settle_timeline(&window);
        if !empty {
            window.invoke_browse_messages();
            key(&window, slint::platform::Key::PageUp);
        }
        while commands.try_recv().is_ok() {}
        window.invoke_jump_to_present();
        assert!(
            std::iter::from_fn(|| commands.try_recv().ok())
                .any(|c| matches!(c, Command::Present(_)))
        );
        {
            let mut a = adapter.borrow_mut();
            let messages = (0..100)
                .map(|i| {
                    Arc::new(ChatMessage {
                        id: format!("{}", i + 1000),
                        item: format!("d:{}", i + 1000),
                        body: format!("New received row {i}"),
                        ..Default::default()
                    })
                })
                .collect();
            a.apply(Event::HistoryLoaded {
                room: "discord:@me:10".into(),
                direction: yapfast_core::chat::Direction::Latest,
                history: yapfast_core::chat::History::Loaded {
                    start: false,
                    present: true,
                },
            });
            a.apply(Event::Timeline {
                room: "discord:@me:10".into(),
                messages,
            });
            a.refresh(&window);
        }
        settle_timeline(&window);
        assert_eq!(
            window.get_active_index(),
            99,
            "Present replacement must select newest, not just scroll"
        );
        assert_eq!(window.get_active_item(), "d:1099");
        assert!(window.invoke_timeline_below() <= 0.5);
        window.invoke_browse_messages();
        key(&window, slint::platform::Key::UpArrow);
        assert_eq!(
            window.get_active_item(),
            "d:1098",
            "next harmless key starts at newest"
        );
    }
}
