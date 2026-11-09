//! repair1 gap completion: real Application/Adapter/window, anonymous received fixtures only.
use super::*;
use i_slint_core::{
    accessibility::{AccessibilityAction, AccessibleStringProperty},
    item_tree::ItemRc,
};
use yapfast_discord::{event::Event as Received, service::ContactSnapshotFixture};
fn fixture() -> (tempfile::TempDir, YapfastWindow, Rc<RefCell<Adapter>>) {
    let d = tempfile::tempdir().unwrap();
    let w = YapfastWindow::new().unwrap();
    let a = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        d.path().into(),
        &w,
    )
    .unwrap();
    a.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&w, &a);
    (d, w, a)
}
fn find(w: &YapfastWindow, label: &str) -> Option<ItemRc> {
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    inner.ensure_tree_instantiated();
    std::iter::once(inner.component())
        .chain(inner.active_popups().iter().map(|p| p.component.clone()))
        .collect::<Vec<_>>()
        .into_iter()
        .find_map(|tree| {
            ItemRc::new_root(tree).visit_descendants(|i| {
                if i.is_visible()
                    && i.accessible_string_property(AccessibleStringProperty::Label)
                        .as_deref()
                        .is_some_and(|s| s.contains(label))
                {
                    std::ops::ControlFlow::Break(i.clone())
                } else {
                    std::ops::ControlFlow::Continue(())
                }
            })
        })
}
fn click(w: &YapfastWindow, label: &str) {
    find(w, label)
        .unwrap_or_else(|| panic!("missing {label}"))
        .accessible_action(&AccessibilityAction::Default);
    settle(w);
}
fn settle(w: &YapfastWindow) {
    for _ in 0..8 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        slint::platform::update_timers_and_animations();
        let _ = w.invoke_timeline_extent();
    }
}
fn received() -> ContactSnapshotFixture {
    ContactSnapshotFixture::new(
        &serde_json::json!({"user":{"id":"1"},"guilds":[{"id":"10","properties":{"name":"owned fixture","owner_id":"9"},"roles":[{"id":"10","permissions":"68608"}],"channels":[{"id":"11","name":"received general","type":0,"last_message_id":"200"}]}],"merged_members":[[{"user_id":"1","roles":[]}]],"read_state":{"entries":[{"id":"11","last_message_id":"100","mention_count":1}]}}),
    )
}
fn deliver(a: &Rc<RefCell<Adapter>>, w: &YapfastWindow, f: &mut ContactSnapshotFixture) {
    let mut a = a.borrow_mut();
    a.apply(Event::Discord(Arc::new(f.snapshot())));
    a.app.state.trusted = true;
    a.app.state.account = Some("1".into());
    a.app.state.discord_linked = true;
    a.app.state.sync_discord_reads = false;
    a.refresh(w);
}
#[test]
fn acceptance_read_all_received_permission_and_membership_change_at_confirm() {
    i_slint_backend_testing::init_no_event_loop();
    for remove in [false, true] {
        let (_d, w, a) = fixture();
        let mut f = received();
        let (b, mut sink) = Backend::anonymous_fixture(a.borrow().runtime.clone());
        a.borrow_mut().backend = Some(b);
        deliver(&a, &w, &mut f);
        assert!(
            a.borrow()
                .app
                .state
                .rooms
                .iter()
                .any(|r| r.id == "discord:10:11" && !r.locked && r.mentions > 0),
            "real received eligible room"
        );
        a.borrow_mut().action(&w, Action::AskReadAll);
        assert_eq!(
            a.borrow().app.state.read_all.as_ref().unwrap().rooms,
            ["discord:10:11"]
        );
        if remove {
            f.receive(Received::GuildDelete, &serde_json::json!({"id":"10"}));
        } else {
            f.receive(Received::ChannelUpdate,&serde_json::json!({"guild_id":"10","id":"11","type":0,"name":"received general","permission_overwrites":[{"id":"10","type":0,"allow":"0","deny":"1024"}]}));
        }
        deliver(&a, &w, &mut f);
        assert!(
            !a.borrow()
                .app
                .state
                .rooms
                .iter()
                .any(|r| r.id == "discord:10:11" && !r.locked)
        );
        while sink.try_recv().is_ok() {}
        a.borrow_mut().action(&w, Action::ConfirmReadAll);
        assert!(
            sink.try_recv().is_err(),
            "current received authority excludes revoked/removed room"
        );
    }
}
#[test]
fn acceptance_read_all_account_roundtrip_and_coalesced_new_lifetime_retire_review() {
    i_slint_backend_testing::init_no_event_loop();
    for coalesced in [false, true] {
        let (_d, w, a) = fixture();
        let mut first = received();
        let (b, mut sink) = Backend::anonymous_fixture(a.borrow().runtime.clone());
        a.borrow_mut().backend = Some(b);
        deliver(&a, &w, &mut first);
        a.borrow_mut().action(&w, Action::AskReadAll);
        assert!(a.borrow().app.state.read_all.is_some());
        if !coalesced {
            let mut other = ContactSnapshotFixture::new(&serde_json::json!({"user":{"id":"2"}}));
            deliver(&a, &w, &mut other);
        }
        // A new owner/session returning to the same account must have a new tracker source,
        // even when the intermediate B snapshot was coalesced away.
        let mut returned = received();
        deliver(&a, &w, &mut returned);
        assert!(a.borrow().app.state.read_all.is_none());
        while sink.try_recv().is_ok() {}
        a.borrow_mut().action(&w, Action::ConfirmReadAll);
        assert!(sink.try_recv().is_err());
        a.borrow_mut().action(&w, Action::AskReadAll);
        a.borrow_mut().action(&w, Action::ConfirmReadAll);
        assert!(
            std::iter::from_fn(|| sink.try_recv().ok())
                .any(|c| matches!(c,Command::MarkRead{room,sync:false}if room=="discord:10:11")),
            "fresh explicit review recovers"
        );
        a.borrow_mut().action(&w, Action::AskReadAll);
        assert_eq!(
            a.borrow().app.state.read_all.as_ref().unwrap().eligible,
            0,
            "accepted pending work is suppressed before lifetime retirement"
        );
        if !coalesced {
            let mut other = ContactSnapshotFixture::new(&serde_json::json!({"user":{"id":"2"}}));
            deliver(&a, &w, &mut other);
        }
        let mut final_owner = received();
        deliver(&a, &w, &mut final_owner);
        assert!(a.borrow().app.state.read_all.is_none());
        a.borrow_mut().action(&w, Action::AskReadAll);
        assert_eq!(
            a.borrow().app.state.read_all.as_ref().unwrap().rooms,
            ["discord:10:11"],
            "old provider pending suppression retired even when B was coalesced"
        );
        while sink.try_recv().is_ok() {}
        a.borrow_mut().action(&w, Action::ConfirmReadAll);
        assert!(
            matches!(sink.try_recv().unwrap(),Command::MarkRead{room,sync:false}if room=="discord:10:11")
        );
    }
}
#[test]
fn acceptance_home_populated_disjoint_page_is_exactly_top_not_highlight_offset() {
    i_slint_backend_testing::init_no_event_loop();
    let (_d, w, a) = fixture();
    let mut f = received();
    let (b, _sink) = Backend::anonymous_fixture(a.borrow().runtime.clone());
    a.borrow_mut().backend = Some(b);
    deliver(&a, &w, &mut f);
    {
        let mut a = a.borrow_mut();
        a.app.state.selected = Some("discord:10:11".into());
        a.app.state.room_ready = true;
        a.app.state.messages = Arc::new(rows(1000, 80));
        a.refresh(&w);
    }
    w.show().unwrap();
    settle(&w);
    w.invoke_jump_to_start();
    {
        let mut a = a.borrow_mut();
        let events = yapfast_core::discord_text::navread_fixture::earliest_completion();
        let history = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Event::HistoryLoaded {
                        direction: yapfast_core::chat::Direction::Around,
                        history: yapfast_core::chat::History::Loaded { start: true, .. },
                        ..
                    }
                )
            })
            .expect("actual provider boundary");
        let timeline = events
            .iter()
            .position(|e| matches!(e, Event::Timeline { .. }))
            .expect("actual published timeline");
        assert!(history < timeline);
        for event in events {
            a.apply(event);
        }
        a.refresh(&w);
    }
    settle(&w);
    assert_eq!(w.get_active_item(), "d:2");
    assert_eq!(w.get_active_index(), 0);
    assert_eq!(w.get_top_index(), 0);
    assert_eq!(w.get_top_item(), "d:2");
    // Exact native Flickable viewport, independent of selection/highlight assertion.
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    let tree = inner.component();
    let mut offsets = vec![];
    ItemRc::new_root(tree).visit_descendants(|item| {
        if let Some(f) = item.downcast::<i_slint_core::items::Flickable>() {
            let f = f.as_pin_ref();
            let h = i_slint_core::items::Flickable::FIELD_OFFSETS
                .content_height()
                .apply_pin(f)
                .get()
                .get();
            let y = i_slint_core::items::Flickable::FIELD_OFFSETS
                .content_y()
                .apply_pin(f)
                .get()
                .get();
            if (h - w.invoke_timeline_extent()).abs() <= 0.5 && h > 1000. {
                offsets.push(y);
            }
        }
        std::ops::ControlFlow::<()>::Continue(())
    });
    assert_eq!(offsets.len(), 1, "exact overflowing timeline viewport");
    assert!(offsets[0].abs() <= 0.5, "top viewport {offsets:?}");
    println!(
        "actual provider ordered disjoint replacement: active={} index={} top={} exact_y={:?}",
        w.get_active_item(),
        w.get_active_index(),
        w.get_top_item(),
        offsets
    );
    assert!(w.invoke_timeline_below() > 100.0, "not bottom");
}
fn rows(start: usize, count: usize) -> Vec<Arc<ChatMessage>> {
    (start..start + count)
        .map(|i| {
            Arc::new(ChatMessage {
                id: i.to_string(),
                item: format!("d:{i}"),
                body: format!("owned received row {i}"),
                ..Default::default()
            })
        })
        .collect()
}
#[test]
fn acceptance_navigation_locked_offline_cancel_and_new_provider_lifetime() {
    i_slint_backend_testing::init_no_event_loop();
    for mode in [0, 1, 2, 3] {
        let (_d, w, a) = fixture();
        let mut f = received();
        let (b, mut sink) = Backend::anonymous_fixture(a.borrow().runtime.clone());
        a.borrow_mut().backend = Some(b);
        deliver(&a, &w, &mut f);
        {
            let mut a = a.borrow_mut();
            a.app.state.selected = Some("discord:10:11".into());
            a.app.state.room_ready = true;
            a.app.state.messages = Arc::new(rows(1000, 80));
            a.refresh(&w);
        }
        while sink.try_recv().is_ok() {}
        if mode == 0 {
            f.receive(Received::ChannelUpdate,&serde_json::json!({"guild_id":"10","id":"11","type":0,"name":"general","permission_overwrites":[{"id":"10","type":0,"allow":"0","deny":"1024"}]}));
            deliver(&a, &w, &mut f);
            while sink.try_recv().is_ok() {}
            w.invoke_jump_to_start();
            assert!(
                !std::iter::from_fn(|| sink.try_recv().ok())
                    .any(|c| matches!(c, Command::Around { .. }))
            );
            assert!(a.borrow().start_selection.is_none());
            continue;
        }
        if mode == 1 {
            let mut s = f.snapshot();
            s.status = yapfast_discord::service::Status::Offline("owned offline fixture".into());
            a.borrow_mut().apply(Event::Discord(Arc::new(s)));
            while sink.try_recv().is_ok() {}
            w.invoke_jump_to_start();
            assert!(
                !std::iter::from_fn(|| sink.try_recv().ok())
                    .any(|c| matches!(c, Command::Around { .. }))
            );
            continue;
        }
        w.invoke_jump_to_start();
        assert!(a.borrow().start_selection.is_some());
        if mode == 2 {
            a.borrow_mut().action(&w, Action::Jump("d:500".into()));
        } else {
            let mut next = received();
            deliver(&a, &w, &mut next);
        }
        assert!(
            a.borrow().start_selection.is_none(),
            "cancel/provider replacement retires pending Home (mode {mode})"
        );
    }
}

#[test]
fn acceptance_tall_account_popover_normal_minimum_scroll_review_cancel_escape() {
    i_slint_backend_testing::init_no_event_loop();
    for (width, height) in [(1120., 760.), (800., 560.)] {
        let (_d, w, a) = fixture();
        let mut f = received();
        let (b, mut sink) = Backend::anonymous_fixture(a.borrow().runtime.clone());
        a.borrow_mut().backend = Some(b);
        deliver(&a, &w, &mut f);
        w.show().unwrap();
        w.window().set_size(slint::LogicalSize::new(width, height));
        w.set_has_custom_status(true);
        w.set_account_status("long owned received status ".repeat(100).into());
        w.set_game_activity_name("owned fixture game".into());
        settle(&w);
        click(&w, "Set your status");
        click(&w, "Change status");
        settle(&w);
        assert!(
            find(&w, "Account").is_some(),
            "actual production AccountPopover"
        );
        let pop_scroll = || {
            let inner = i_slint_core::window::WindowInner::from_pub(w.window());
            let mut v = vec![];
            for pop in inner.active_popups().iter() {
                ItemRc::new_root(pop.component.clone()).visit_descendants(|item| {
                    if let Some(f) = item.downcast::<i_slint_core::items::Flickable>() {
                        let f = f.as_pin_ref();
                        v.push((
                            i_slint_core::items::Flickable::FIELD_OFFSETS
                                .content_y()
                                .apply_pin(f)
                                .get()
                                .get(),
                            i_slint_core::items::Flickable::FIELD_OFFSETS
                                .content_height()
                                .apply_pin(f)
                                .get()
                                .get(),
                            item.geometry().size.height,
                        ));
                    }
                    std::ops::ControlFlow::<()>::Continue(())
                });
            }
            v
        };
        let before = pop_scroll();
        assert_eq!(before.len(), 1);
        assert!(
            before[0].1 > before[0].2 + 50.,
            "genuinely tall scrollable body {before:?}"
        );
        // Actual in-memory wheel input targets the account ScrollView, never OS input.
        w.window()
            .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(130., height - 180.),
                delta_x: 0.,
                delta_y: -2400.,
            });
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(500));
        settle(&w);
        let after = pop_scroll();
        println!("actual account ScrollView {width}x{height}: before={before:?} after={after:?}");
        assert!(
            after[0].0 < before[0].0 - 10.,
            "actual account body scrolled: {before:?} -> {after:?}"
        );
        assert!(
            find(&w, "User Settings").is_some(),
            "bottom control reachable after scroll"
        );
        while sink.try_recv().is_ok() {}
        click(&w, "Read all notifications");
        assert!(a.borrow().app.state.read_all.is_some());
        assert!(sink.try_recv().is_err());
        click(&w, "Cancel");
        assert!(a.borrow().app.state.read_all.is_none());
        assert!(sink.try_recv().is_err());
        click(&w, "Read all notifications");
        w.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Escape.into(),
            });
        settle(&w);
        assert!(
            a.borrow().app.state.read_all.is_none(),
            "Escape closes popup and cancels review"
        );
        assert!(sink.try_recv().is_err());
        click(&w, "Set your status");
        click(&w, "Read all notifications");
        let reviewed = a
            .borrow()
            .app
            .state
            .read_all
            .as_ref()
            .unwrap()
            .rooms
            .clone();
        assert_eq!(reviewed, ["discord:10:11"]);
        click(&w, "Mark read");
        let marked: Vec<_> = std::iter::from_fn(|| sink.try_recv().ok())
            .filter_map(|c| {
                if let Command::MarkRead { room, sync } = c {
                    Some((room, sync))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(marked, vec![(reviewed[0].clone(), false)]);
    }
}

#[test]
fn repair2_repeated_home_keeps_pending_disjoint_earliest_selection() {
    i_slint_backend_testing::init_no_event_loop();
    let (_d, w, a) = fixture();
    let mut f = received();
    let (b, mut sink) = Backend::anonymous_fixture(a.borrow().runtime.clone());
    a.borrow_mut().backend = Some(b);
    deliver(&a, &w, &mut f);
    {
        let mut a = a.borrow_mut();
        a.app.state.selected = Some("discord:10:11".into());
        a.app.state.room_ready = true;
        a.app.state.messages = Arc::new(rows(1000, 80));
        a.refresh(&w);
    }
    w.show().unwrap();
    settle(&w);
    w.invoke_browse_messages();
    w.set_active_index(40);
    w.set_active_item("d:1040".into());
    assert_eq!(w.get_active_item(), "d:1040", "populated old selection");
    while sink.try_recv().is_ok() {}
    w.invoke_jump_to_start();
    w.invoke_jump_to_start();
    let around: Vec<_> = std::iter::from_fn(|| sink.try_recv().ok())
        .filter_map(|c| match c {
            Command::Around { room, item } => Some((room, item)),
            _ => None,
        })
        .collect();
    assert_eq!(
        around,
        vec![("discord:10:11".into(), "d:1".into())],
        "actual callback deduplicates one pending boundary request"
    );
    {
        let mut a = a.borrow_mut();
        let events = yapfast_core::discord_text::navread_fixture::earliest_completion();
        let history = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Event::HistoryLoaded {
                        direction: yapfast_core::chat::Direction::Around,
                        history: yapfast_core::chat::History::Loaded { start: true, .. },
                        ..
                    }
                )
            })
            .expect("actual provider boundary");
        let timeline = events
            .iter()
            .position(|e| matches!(e, Event::Timeline { .. }))
            .expect("actual published timeline");
        assert!(history < timeline);
        for event in events {
            a.apply(event);
        }
        a.refresh(&w);
    }
    settle(&w);
    assert_eq!(w.get_active_item(), "d:2");
    assert_eq!(w.get_active_index(), 0);
    assert_eq!(w.get_top_index(), 0);
    assert_eq!(w.get_top_item(), "d:2");
    // Exact native Flickable viewport, independent of selection/highlight assertion.
    let inner = i_slint_core::window::WindowInner::from_pub(w.window());
    let tree = inner.component();
    let mut offsets = vec![];
    ItemRc::new_root(tree).visit_descendants(|item| {
        if let Some(f) = item.downcast::<i_slint_core::items::Flickable>() {
            let f = f.as_pin_ref();
            let h = i_slint_core::items::Flickable::FIELD_OFFSETS
                .content_height()
                .apply_pin(f)
                .get()
                .get();
            let y = i_slint_core::items::Flickable::FIELD_OFFSETS
                .content_y()
                .apply_pin(f)
                .get()
                .get();
            if (h - w.invoke_timeline_extent()).abs() <= 0.5 && h > 1000. {
                offsets.push(y);
            }
        }
        std::ops::ControlFlow::<()>::Continue(())
    });
    assert_eq!(offsets.len(), 1, "exact overflowing timeline viewport");
    assert!(offsets[0].abs() <= 0.5, "top viewport {offsets:?}");
    println!(
        "actual provider ordered disjoint replacement: active={} index={} top={} exact_y={:?}",
        w.get_active_item(),
        w.get_active_index(),
        w.get_top_item(),
        offsets
    );
    assert!(w.invoke_timeline_below() > 100.0, "not bottom");
    w.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::DownArrow.into(),
        });
    settle(&w);
    assert_eq!(
        w.get_active_index(),
        1,
        "next harmless Down starts at earliest row"
    );
    assert_eq!(w.get_active_item(), "d:3");
}
