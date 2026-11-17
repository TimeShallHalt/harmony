//! Registered actual-main regressions. No services/devices; only owned data and in-memory UI.
use super::*;
use std::time::{Instant, UNIX_EPOCH};
use yapfast_discord::service::{Snapshot, Status};

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
    adapter.borrow_mut().layout.sound_volume = Some(0); // no audio worker or device
    bind_callbacks(&window, &adapter);
    (dir, window, adapter)
}
fn voice(room: &str) -> voice::State {
    voice::State {
        room: Some(room.into()),
        phase: voice::Phase::Connected,
        ..Default::default()
    }
}
fn snapshot() -> Snapshot {
    Snapshot {
        status: Status::Online,
        user_id: Some("100".into()),
        ..Default::default()
    }
}
fn receive(a: &Rc<RefCell<Adapter>>, w: &YapfastWindow, s: Snapshot) {
    let mut a = a.borrow_mut();
    let reach = a.apply(Event::Discord(Arc::new(s)));
    a.show(w, reach);
}

#[test]
fn coalesced_hidden_call_clock_and_account_replacement_are_event_owned() {
    let (_dir, window, adapter) = fixture();
    receive(&adapter, &window, snapshot());
    let start = Instant::now();
    let clock = Rc::new(Cell::new(start));
    adapter.borrow_mut().call_timer.use_clock(clock.clone());
    adapter
        .borrow_mut()
        .apply(Event::Voice(voice("discord:10:11")));
    adapter.borrow_mut().detach();
    clock.set(start + Duration::from_secs(60));
    adapter
        .borrow_mut()
        .apply(Event::Voice(voice("discord:10:12")));
    clock.set(start + Duration::from_secs(90));
    adapter
        .borrow_mut()
        .apply(Event::Voice(voice("discord:10:11")));
    let replacement = YapfastWindow::new().unwrap();
    adapter.borrow_mut().window = replacement.as_weak();
    clock.set(start + Duration::from_secs(3813));
    adapter.borrow_mut().refresh(&replacement);
    assert_eq!(replacement.get_voice_time(), "1:02:03");
    assert_eq!(
        adapter.borrow().call_timer.origin().unwrap().1,
        start + Duration::from_secs(90)
    );
    let mut s = snapshot();
    s.user_id = Some("101".into());
    receive(&adapter, &replacement, s);
    assert_eq!(
        replacement.get_voice_time(),
        "",
        "account replacement retires the previous call"
    );
    assert!(adapter.borrow().call_timer.origin().is_none());
    assert!(!adapter.borrow().call_timer.running());
    adapter.borrow_mut().refresh(&replacement);
    assert_eq!(
        replacement.get_voice_time(),
        "",
        "a retained room name cannot create a call"
    );
}

#[test]
fn received_platforms_reach_native_friends_and_clear_stale_lifetimes() {
    let (_dir, window, adapter) = fixture();
    adapter.borrow_mut().app.state.account = Some("anonymous-fixture".into());
    adapter.borrow_mut().app.state.trusted = true;
    adapter.borrow_mut().app.state.dm_screen = Some(DmScreen::Friends);
    window.set_workspace(true);
    window.set_trusted(true);
    window.show().unwrap();
    let mut friend: yapfast_discord::service::Friend = serde_json::from_value(
        serde_json::json!({"user_id":"200","name":"Received fixture","avatar":""}),
    )
    .unwrap();
    let received = |value| yapfast_discord::member_list::Presence::parse(&value);
    friend.presence = Some(received(
        serde_json::json!({"status":"online","client_status":{"desktop":"idle","web":"online","mobile":"dnd"}}),
    ));
    let mut s = snapshot();
    s.friend_list = vec![friend.clone()].into();
    receive(&adapter, &window, s.clone());
    let expected = "Desktop (idle) · Web (online) · Mobile (dnd)";
    assert_eq!(
        window.get_friends().row_data(0).unwrap().platforms,
        expected
    );
    assert!(matches!(
        adapter.borrow().app.state.friends[0].presence,
        Some(Presence::Clients(_))
    ));
    i_slint_core::window::WindowInner::from_pub(window.window()).ensure_tree_instantiated();
    let label = format!("Received platforms: {expected}");
    use i_slint_core::{accessibility::AccessibleStringProperty, item_tree::ItemRc};
    let inner = i_slint_core::window::WindowInner::from_pub(window.window());
    let found = ItemRc::new_root(inner.component()).visit_descendants(|item| {
        if item.is_visible()
            && item
                .accessible_string_property(AccessibleStringProperty::Label)
                .as_deref()
                == Some(label.as_str())
        {
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    });
    assert!(found.is_some(), "accessible native platform label");
    friend.presence = Some(received(serde_json::json!({"status":"online"})));
    s.friend_list = vec![friend.clone()].into();
    receive(&adapter, &window, s.clone());
    assert_eq!(
        window.get_friends().row_data(0).unwrap().platforms,
        "",
        "unknown is not desktop"
    );
    friend.presence = Some(received(
        serde_json::json!({"status":"online","client_status":{"web":"online"}}),
    ));
    s.friend_list = vec![friend.clone()].into();
    receive(&adapter, &window, s.clone());
    assert_eq!(
        window.get_friends().row_data(0).unwrap().platforms,
        "Web (online)"
    );
    s.status = Status::Offline("fixture disconnect".into());
    receive(&adapter, &window, s.clone());
    assert_eq!(
        window.get_friends().row_data(0).unwrap().platforms,
        "",
        "cached offline evidence clears"
    );
    s.status = Status::Online;
    s.user_id = Some("101".into());
    receive(&adapter, &window, s.clone());
    assert_eq!(
        window.get_friends().row_data(0).unwrap().platforms,
        "",
        "old shared list cannot revive in a new account"
    );
    s.friend_list = vec![friend].into();
    receive(&adapter, &window, s);
    assert_eq!(
        window.get_friends().row_data(0).unwrap().platforms,
        "Web (online)",
        "fresh new-account evidence allowed"
    );
}

#[test]
fn open_profile_platforms_follow_exact_current_friend_and_member_presence() {
    let (_dir, window, adapter) = fixture();
    let mut s = snapshot();
    let mut friend: yapfast_discord::service::Friend = serde_json::from_value(
        serde_json::json!({"user_id":"200","name":"Open fixture","avatar":""}),
    )
    .unwrap();
    let web = yapfast_discord::member_list::Presence::parse(
        &serde_json::json!({"status":"online","client_status":{"web":"online"}}),
    );
    friend.presence = Some(web);
    s.friend_list = vec![friend.clone()].into();
    receive(&adapter, &window, s.clone());
    {
        let mut a = adapter.borrow_mut();
        a.app.state.selected = Some("discord:10:11".into());
        a.app.dispatch(Action::ShowFriendProfile {
            user: "200".into(),
            large: false,
        });
        a.refresh(&window);
    }
    assert_eq!(window.get_profile().platforms, "Web (online)");
    for (presence, expected) in [
        (Some(yapfast_discord::member_list::Presence::Offline), ""),
        (None, ""),
        (
            Some(yapfast_discord::member_list::Presence::parse(
                &serde_json::json!({"status":"online","client_status":{"desktop":"idle"}}),
            )),
            "Desktop (idle)",
        ),
        (Some(web), "Web (online)"),
    ] {
        friend.presence = presence;
        s.friend_list = vec![friend.clone()].into();
        receive(&adapter, &window, s.clone());
        assert!(adapter.borrow().app.state.discord_online);
        assert_eq!(
            adapter
                .borrow()
                .app
                .state
                .profile
                .as_ref()
                .unwrap()
                .profile
                .id,
            "200"
        );
        assert_eq!(
            window.get_profile().platforms,
            expected,
            "open profile uses current friend evidence, including explicit unknown"
        );
    }
    // Keep the same card open while current member snapshots replace that evidence.
    s.friend_list = Vec::new().into();
    receive(&adapter, &window, s.clone());
    for (presence, expected) in [
        (Some(Presence::Online), ""),
        (Some(Presence::Offline), ""),
        (None, ""),
        (Some(yapfast_core::discord::presence(web)), "Web (online)"),
    ] {
        let mut a = adapter.borrow_mut();
        a.app.state.selected = Some("discord:10:11".into());
        let reach = a.apply(Event::Members {
            room: "discord:10:11".into(),
            members: Ok(vec![yapfast_core::chat::ChatMember {
                id: "200".into(),
                bot: false,
                name: "Member fixture".into(),
                username: None,
                role: Role::Member,
                avatar: None,
                decoration: None,
                nameplate: None,
                style: None,
                group: None,
                presence,
                color: None,
                activity: None,
                activities: Default::default(),
                roles: Default::default(),
                timed_out_until: None,
                group_icon: None,
                role_icon: None,
                tag: None,
                boosting_since: None,
            }]),
        });
        a.show(&window, reach);
        assert_eq!(
            window.get_profile().platforms,
            expected,
            "member replacement must not fall back to the retained web profile"
        );
    }
    // When the same contact is in both lists, the latest received source wins.
    let mut member = adapter
        .borrow()
        .app
        .state
        .members
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()[0]
        .clone();
    friend.presence = Some(web);
    s.friend_list = vec![friend.clone()].into();
    receive(&adapter, &window, s.clone());
    member.presence = None;
    {
        let mut a = adapter.borrow_mut();
        a.app.state.selected = Some("discord:10:11".into());
        let reach = a.apply(Event::Members {
            room: "discord:10:11".into(),
            members: Ok(vec![member]),
        });
        a.show(&window, reach);
    }
    assert_eq!(
        window.get_profile().platforms,
        "",
        "fresh member unknown must replace even a richer retained friend set"
    );
    assert_eq!(
        window.get_profile().presence,
        "online",
        "new platform reconciliation leaves the legacy status dot untouched"
    );
    receive(&adapter, &window, s.clone());
    assert_eq!(
        window.get_profile().platforms,
        "",
        "an unchanged friend list in an unrelated snapshot cannot revive its older richer set"
    );
    // A genuinely received same-contact observation may publish evidence again.
    // Reallocating a collection of unchanged old values is NOT an observation.
    let mut producer = contact_tracker(false);
    assert!(producer.receive(
        yapfast_discord::event::Event::PresenceUpdate,
        &serde_json::json!({"user":{"id":"200"},"status":"online","client_status":{"web":"online"}})
    ));
    s.friend_list = producer.snapshot().friend_list;
    receive(&adapter, &window, s.clone());
    assert_eq!(window.get_profile().platforms, "Web (online)");
    s.status = Status::Offline("fixture".into());
    receive(&adapter, &window, s.clone());
    assert_eq!(window.get_profile().platforms, "");
    s.status = Status::Online;
    s.user_id = Some("101".into());
    receive(&adapter, &window, s);
    assert_eq!(window.get_profile().platforms, "");
}

#[test]
fn nonfriend_dm_open_profiles_follow_received_conversation_platforms() {
    use yapfast_discord::{member_list::Presence as Received, service::Conversation};
    let (_dir, window, adapter) = fixture();
    let web =
        Received::parse(&serde_json::json!({"status":"online","client_status":{"web":"online"}}));
    let mut dm = Conversation {
        id: "20".into(),
        name: "Nonfriend".into(),
        person: Some("200".into()),
        people: 2,
        presence: Some(web),
        ..Default::default()
    };
    let room = yapfast_core::discord::room_id(yapfast_discord::ready::PRIVATE, "20");
    let mut s = snapshot();
    s.conversations = vec![Arc::new(dm.clone())];
    receive(&adapter, &window, s.clone());
    assert!(adapter.borrow().app.state.friends.is_empty());
    for large in [false, true] {
        {
            let mut a = adapter.borrow_mut();
            a.app.dispatch(Action::ShowDmProfile(room.clone()));
            if !large {
                a.app.state.profile.as_mut().unwrap().large = false;
            }
            a.refresh(&window);
        }
        assert_eq!(window.get_profile().platforms, "Web (online)");
        for (value, expected) in [
            (Some(Received::Offline), ""),
            (None, ""),
            (Some(Received::Online), ""),
            (
                Some(Received::parse(
                    &serde_json::json!({"status":"online","client_status":{"desktop":"idle"}}),
                )),
                "Desktop (idle)",
            ),
            (
                Some(Received::parse(
                    &serde_json::json!({"status":"online","client_status":{"mobile":"online","web":"idle"}}),
                )),
                "Web (idle) · Mobile (online)",
            ),
            (Some(web), "Web (online)"),
        ] {
            dm.presence = value;
            s.conversations = vec![Arc::new(dm.clone())];
            receive(&adapter, &window, s.clone());
            assert_eq!(
                window.get_profile().platforms,
                expected,
                "already-open nonfriend DM card follows the received conversation"
            );
            assert_eq!(
                window.get_profile().presence,
                "online",
                "legacy cached card dot is independent"
            );
        }
        adapter.borrow_mut().app.dispatch(Action::CloseProfile);
    }
}

fn platform_member(presence: Option<Presence>) -> yapfast_core::chat::ChatMember {
    yapfast_core::chat::ChatMember {
        id: "200".into(),
        bot: false,
        name: "Member fixture".into(),
        username: None,
        role: Role::Member,
        avatar: None,
        decoration: None,
        nameplate: None,
        style: None,
        group: None,
        presence,
        color: None,
        activity: None,
        activities: Default::default(),
        roles: Default::default(),
        timed_out_until: None,
        group_icon: None,
        role_icon: None,
        tag: None,
        boosting_since: None,
    }
}

#[test]
fn dm_panel_renders_authoritative_platforms_without_richer_fallback() {
    use yapfast_discord::{member_list::Presence as Received, service::Conversation};
    let (_dir, window, adapter) = fixture();
    let web =
        Received::parse(&serde_json::json!({"status":"online","client_status":{"web":"online"}}));
    let room = yapfast_core::discord::room_id(yapfast_discord::ready::PRIVATE, "20");
    let mut dm = Conversation {
        id: "20".into(),
        name: "Nonfriend".into(),
        person: Some("200".into()),
        people: 2,
        presence: Some(web),
        ..Default::default()
    };
    let mut s = snapshot();
    s.conversations = vec![Arc::new(dm.clone())];
    receive(&adapter, &window, s.clone());
    {
        let mut a = adapter.borrow_mut();
        a.app.state.selected = Some(room.clone());
        a.app.state.profile_panel = true;
        a.app.dispatch(Action::ShowDmProfile(room.clone()));
        a.refresh(&window);
    }
    assert_eq!(window.get_panel_profile().platforms, "Web (online)");
    for (presence, expected) in [
        (None, ""),
        (Some(Presence::Offline), ""),
        (
            Some(yapfast_core::discord::presence(Received::parse(
                &serde_json::json!({"status":"online","client_status":{"desktop":"online","mobile":"idle"}}),
            ))),
            "Desktop (online) · Mobile (idle)",
        ),
    ] {
        let mut a = adapter.borrow_mut();
        let reach = a.apply(Event::Members {
            room: room.clone(),
            members: Ok(vec![platform_member(presence)]),
        });
        a.show(&window, reach);
        assert_eq!(
            a.app
                .state
                .panel_profile
                .as_ref()
                .unwrap()
                .platform_presence,
            presence
        );
        assert_eq!(
            window.get_panel_profile().platforms,
            expected,
            "final panel must use the same authority as the open card, not older room evidence"
        );
        assert_eq!(window.get_profile().platforms, expected);
        assert_eq!(
            window.get_panel_profile().presence,
            "online",
            "legacy panel status dot remains room based"
        );
    }
    // Explicit DM unknown cannot fall back to the richer profile captured at open.
    dm.presence = None;
    s.conversations = vec![Arc::new(dm)];
    receive(&adapter, &window, s);
    assert_eq!(window.get_panel_profile().platforms, "");
    assert_eq!(window.get_profile().platforms, "");
    assert_eq!(window.get_panel_profile().presence, "online");
}

fn contact_tracker(dm: bool) -> yapfast_discord::service::ContactSnapshotFixture {
    yapfast_discord::service::ContactSnapshotFixture::new(&serde_json::json!({
        "user":{"id":"100"},
        "guilds":[{"id":"10","owner_id":"100","properties":{"name":"Fixture server"},
            "roles":[{"id":"10","name":"@everyone","permissions":"1024","position":0}],
            "channels":[{"id":"11","type":0,"name":"general","position":0}]}],
        "users":[{"id":"200","username":"target"},{"id":"300","username":"other"}],
        "relationships":[{"id":"200","type":1},{"id":"300","type":1}],
        "private_channels": if dm { serde_json::json!([{"id":"20","type":1,"recipient_ids":["200"]}]) } else { serde_json::json!([]) }
    }))
}

#[test]
fn exact_contact_observations_survive_metadata_republication_and_reopen() {
    use yapfast_discord::event::Event as Gateway;
    let (_dir, window, adapter) = fixture();
    let mut tracker = contact_tracker(false);
    let web =
        serde_json::json!({"user":{"id":"200"},"status":"online","client_status":{"web":"online"}});
    assert!(tracker.receive(Gateway::PresenceUpdate, &web));
    let initial = tracker.snapshot();
    assert!(initial.servers.iter().any(|server| server.id == "10"));
    receive(&adapter, &window, initial.clone());
    {
        let mut a = adapter.borrow_mut();
        assert!(
            a.app
                .state
                .rooms
                .iter()
                .any(|room| room.id == "discord:10:11")
        );
        a.app.state.navigation = Navigation::Server("discord:10".into());
        a.app.state.selected = Some("discord:10:11".into());
        a.app.dispatch(Action::ShowFriendProfile {
            user: "200".into(),
            large: false,
        });
        a.refresh(&window);
    }
    assert_eq!(window.get_profile().platforms, "Web (online)");
    for clear in [
        serde_json::json!({"status":"online"}),
        serde_json::json!({"status":"offline"}),
    ] {
        // The real provider intentionally doesn't import this no-DM friend's guild
        // observation into its global friend cache. The actual member event does.
        let mut guild = clear.clone();
        guild["user"] = serde_json::json!({"id":"200"});
        guild["guild_id"] = "10".into();
        assert!(!tracker.receive(Gateway::PresenceUpdate, &guild));
        {
            let mut a = adapter.borrow_mut();
            let presence = yapfast_core::discord::presence(
                yapfast_discord::member_list::Presence::parse(&clear),
            );
            let reach = a.apply(Event::Members {
                room: "discord:10:11".into(),
                members: Ok(vec![platform_member(Some(presence))]),
            });
            a.show(&window, reach);
        }
        assert_eq!(window.get_profile().platforms, "");
        let mut previous = tracker.snapshot();
        for (event, data) in [
            (
                Gateway::PresenceUpdate,
                serde_json::json!({"user":{"id":"300"},"status":"idle"}),
            ),
            (
                Gateway::RelationshipUpdate,
                serde_json::json!({"id":"200","nickname":format!("nick-{clear}")}),
            ),
            (
                Gateway::RelationshipUpdate,
                serde_json::json!({"id":"200","user":{"id":"200","username":"target","avatar":format!("art-{clear}")}}),
            ),
        ] {
            assert!(tracker.receive(event, &data));
            let next = tracker.snapshot();
            assert!(
                !Arc::ptr_eq(&previous.friend_list, &next.friend_list),
                "actual collection replacement is required for this regression"
            );
            receive(&adapter, &window, next.clone());
            assert_eq!(
                window.get_profile().platforms,
                "",
                "another contact or target metadata must not republish the old target web evidence"
            );
            previous = next;
        }
        {
            let mut a = adapter.borrow_mut();
            a.app.dispatch(Action::CloseProfile);
            a.app.dispatch(Action::ShowFriendProfile {
                user: "200".into(),
                large: true,
            });
            a.refresh(&window);
        }
        assert_eq!(
            window.get_profile().platforms,
            "",
            "reopening cannot re-seed older friend evidence"
        );
        // A genuine same-target, SAME-VALUE new observation must be published.
        assert!(
            tracker.receive(Gateway::PresenceUpdate, &web),
            "same-value observations must reach the coalesced publisher"
        );
        let fresh = tracker.snapshot();
        let old_contact = previous
            .friend_list
            .iter()
            .find(|friend| friend.user_id == "200")
            .unwrap();
        let new_contact = fresh
            .friend_list
            .iter()
            .find(|friend| friend.user_id == "200")
            .unwrap();
        assert_ne!(
            old_contact.presence_observation, new_contact.presence_observation,
            "real tracker same-value provenance changes"
        );
        assert_eq!(new_contact.presence, old_contact.presence);
        let expected_presence = new_contact.presence.map(yapfast_core::discord::presence);
        receive(&adapter, &window, fresh);
        assert_eq!(
            adapter
                .borrow()
                .app
                .state
                .profile
                .as_ref()
                .unwrap()
                .platform_presence,
            expected_presence,
            "core consumes new exact-contact provenance"
        );
        assert_eq!(window.get_profile().platforms, "Web (online)");
        // Coalescing may discard intermediate values, but not the fact the final
        // same value is newly observed for this exact contact.
        assert!(tracker.receive(
            Gateway::PresenceUpdate,
            &serde_json::json!({"user":{"id":"200"},"status":"offline"})
        ));
        assert!(tracker.receive(Gateway::PresenceUpdate, &web));
        receive(&adapter, &window, tracker.snapshot());
        assert_eq!(window.get_profile().platforms, "Web (online)");
    }
    let mut s = tracker.snapshot();
    s.status = Status::Offline("fixture".into());
    receive(&adapter, &window, s.clone());
    assert_eq!(window.get_profile().platforms, "");
    s.status = Status::Online;
    s.user_id = Some("101".into());
    receive(&adapter, &window, s);
    assert_eq!(window.get_profile().platforms, "");
}

#[test]
fn nonfriend_gateway_observations_reach_both_profiles_and_keep_provenance() {
    use yapfast_discord::{event::Event as Gateway, member_list::Presence as Received};
    let (_dir, window, adapter) = fixture();
    let mut tracker = contact_tracker(true);
    assert!(tracker.receive(
        Gateway::RelationshipRemove,
        &serde_json::json!({"id":"200"})
    ));
    let mobile = serde_json::json!({"user":{"id":"200"},"guild_id":"10","status":"online","client_status":{"mobile":"online"}});
    assert!(tracker.receive(Gateway::PresenceUpdate, &mobile));
    let initial = tracker.snapshot();
    assert!(
        !initial
            .friend_list
            .iter()
            .any(|friend| friend.user_id == "200")
    );
    let dm = initial.conversations[0].clone();
    let encoded = serde_json::to_value(dm.as_ref()).unwrap();
    assert!(encoded.get("presence").is_none());
    assert!(
        encoded.get("presence_observation").is_none(),
        "observation provenance is never persisted or uploaded"
    );
    receive(&adapter, &window, initial);
    let room = yapfast_core::discord::room_id(yapfast_discord::ready::PRIVATE, "20");
    {
        let mut a = adapter.borrow_mut();
        a.app.state.selected = Some(room.clone());
        a.app.state.profile_panel = true;
        a.app.dispatch(Action::ShowDmProfile(room.clone()));
        a.refresh(&window);
    }
    assert_eq!(window.get_profile().platforms, "Mobile (online)");
    assert_eq!(window.get_panel_profile().platforms, "Mobile (online)");
    {
        let mut a = adapter.borrow_mut();
        let reach = a.apply(Event::Members {
            room: room.clone(),
            members: Ok(vec![platform_member(Some(Presence::Online))]),
        });
        a.show(&window, reach);
    }
    assert!(tracker.receive(
        Gateway::RelationshipUpdate,
        &serde_json::json!({"id":"200","nickname":"DM metadata only"})
    ));
    let metadata = tracker.snapshot();
    assert!(!Arc::ptr_eq(&dm, &metadata.conversations[0]));
    assert_eq!(
        dm.presence_observation,
        metadata.conversations[0].presence_observation
    );
    receive(&adapter, &window, metadata);
    assert_eq!(window.get_profile().platforms, "");
    assert_eq!(window.get_panel_profile().platforms, "");
    assert!(
        tracker.receive(Gateway::PresenceUpdate, &mobile),
        "same-value DM observation is fresh"
    );
    receive(&adapter, &window, tracker.snapshot());
    assert_eq!(window.get_profile().platforms, "Mobile (online)");
    assert_eq!(window.get_panel_profile().platforms, "Mobile (online)");
    for (value, expected) in [
        (serde_json::json!({"status":"online"}), ""),
        (
            serde_json::json!({"status":"offline","client_status":{"mobile":"online"}}),
            "",
        ),
        (
            serde_json::json!({"status":"online","client_status":{"desktop":"idle"}}),
            "Desktop (idle)",
        ),
        (
            serde_json::json!({"status":"online","client_status":{"desktop":"online","web":"idle","mobile":"dnd"}}),
            "Desktop (online) · Web (idle) · Mobile (dnd)",
        ),
    ] {
        let mut value = value;
        value["user"] = serde_json::json!({"id":"200"});
        value["guild_id"] = "10".into();
        assert!(tracker.receive(Gateway::PresenceUpdate, &value));
        // Publisher's coalesced snapshot is the actual tracker's final state.
        let next = tracker.snapshot();
        assert_eq!(
            next.conversations[0].presence,
            Some(Received::parse(&value))
        );
        receive(&adapter, &window, next);
        assert_eq!(window.get_profile().platforms, expected);
        assert_eq!(window.get_panel_profile().platforms, expected);
    }
    let mut s = tracker.snapshot();
    s.status = Status::Offline("fixture".into());
    receive(&adapter, &window, s.clone());
    assert_eq!(window.get_profile().platforms, "");
    assert_eq!(window.get_panel_profile().platforms, "");
    s.status = Status::Online;
    receive(&adapter, &window, s);
    assert_eq!(
        window.get_profile().platforms,
        "",
        "old reconnect snapshot cannot revive evidence"
    );
    assert_eq!(window.get_panel_profile().platforms, "");
    assert!(tracker.receive(Gateway::PresenceUpdate, &mobile));
    receive(&adapter, &window, tracker.snapshot());
    assert_eq!(window.get_profile().platforms, "Mobile (online)");
    let mut s = tracker.snapshot();
    s.user_id = Some("101".into());
    receive(&adapter, &window, s);
    let a = adapter.borrow();
    assert!(
        a.app
            .state
            .profile
            .iter()
            .chain(&a.app.state.panel_profile)
            .all(|view| view.platform_presence.is_none()),
        "account replacement retires every retained profile authority"
    );
    assert_eq!(window.get_profile_open(), a.app.state.profile.is_some());
    assert_eq!(
        window.get_panel_profile_shown(),
        a.app.state.panel_profile.is_some()
    );
    assert!(!window.get_profile_open() || window.get_profile().platforms.is_empty());
    assert!(!window.get_panel_profile_shown() || window.get_panel_profile().platforms.is_empty());
}

#[test]
fn exact_voice_stream_controls_have_independent_local_persistence() {
    let (dir, window, adapter) = fixture();
    let id = "update3-exact-gains";
    for bad in ["NaN", "inf", "13.7", "201", "-1", "1e2", ""] {
        assert_eq!(window.invoke_parse_volume(bad.into()), -1);
    }
    assert_eq!(window.invoke_parse_volume("137".into()), 137);
    window.invoke_volume_changed(id.into(), false, 137, true);
    window.invoke_volume_changed(id.into(), true, 176, true);
    let saved = Layout::read(dir.path());
    let replacement = YapfastWindow::new().unwrap();
    saved.apply(&replacement);
    bind_callbacks(&replacement, &adapter);
    assert_eq!(replacement.invoke_volume_of(id.into(), false), 137);
    assert_eq!(replacement.invoke_volume_of(id.into(), true), 176);
    replacement.invoke_volume_changed(id.into(), false, 100, true);
    let read = Layout::read(dir.path());
    assert!(!read.volumes.contains_key(id));
    assert_eq!(read.stream_volumes[id], saved.stream_volumes[id]);
    replacement.invoke_volume_changed(id.into(), true, 0, true);
    assert_eq!(replacement.invoke_volume_of(id.into(), true), 0);
    replacement.invoke_volume_changed(id.into(), true, 200, true);
    assert_eq!(replacement.invoke_volume_of(id.into(), true), 200);
}

#[test]
fn exact_stream_entry_55_reaches_adapter_persists_and_reopens_with_default_boundaries() {
    use super::update3_volume_controls_tests::VolumeHarness;
    use yapfast_core::volume::{self, Kind};
    let (dir, window, adapter) = fixture();
    let control = VolumeHarness::new().unwrap();
    let weak = window.as_weak();
    control.on_parse(move |text| weak.upgrade().unwrap().invoke_parse_volume(text));
    let weak = window.as_weak();
    control.on_commit(move |stream, value| {
        weak.upgrade().unwrap().invoke_volume_changed(
            "repair-stream-55".into(),
            stream,
            value,
            true,
        );
    });
    control.show().unwrap();
    let enter = |value: i32| {
        control.set_stream_entry(value.to_string().into());
        control.invoke_focus_stream();
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        control
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "\n".into() });
    };
    window.invoke_volume_changed("repair-stream-55".into(), false, 137, true);
    enter(55);
    let expected = volume::from_slider(55.);
    assert_eq!(
        volume::get(Kind::Stream, "repair-stream-55"),
        expected,
        "actual control-to-Adapter dispatch must not snap exact 55 to 0.18"
    );
    assert_eq!(
        Layout::read(dir.path()).stream_volumes["repair-stream-55"],
        expected
    );
    assert_eq!(window.invoke_volume_of("repair-stream-55".into(), true), 55);
    assert_eq!(
        window.invoke_volume_of("repair-stream-55".into(), false),
        137
    );
    let replacement = YapfastWindow::new().unwrap();
    let saved = Layout::read(dir.path());
    // Retire the in-memory override first: readback must genuinely reload persistence.
    volume::set(
        Kind::Stream,
        "repair-stream-55",
        Kind::Stream.default_gain(),
    );
    saved.apply(&replacement);
    bind_callbacks(&replacement, &adapter);
    let reopened = VolumeHarness::new().unwrap();
    reopened.set_stream(replacement.invoke_volume_of("repair-stream-55".into(), true));
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    assert_eq!(reopened.get_stream(), 55);
    assert_eq!(reopened.get_stream_entry(), "55");
    assert_eq!(volume::get(Kind::Stream, "repair-stream-55"), expected);
    for value in [0, 200, 54] {
        enter(value);
        assert_eq!(
            window.invoke_volume_of("repair-stream-55".into(), true),
            value
        );
        assert_eq!(
            window.invoke_volume_of("repair-stream-55".into(), false),
            137
        );
        let gain = if value == 54 {
            Kind::Stream.default_gain()
        } else {
            volume::from_slider(value as f32).min(volume::MAX)
        };
        assert_eq!(volume::get(Kind::Stream, "repair-stream-55"), gain);
        let saved = Layout::read(dir.path());
        if value == 54 {
            assert!(!saved.stream_volumes.contains_key("repair-stream-55"));
        } else {
            assert_eq!(saved.stream_volumes["repair-stream-55"], gain);
        }
    }
    // Every accepted whole stream percentage survives the same native entry path.
    for value in 0..=200 {
        enter(value);
        assert_eq!(
            window.invoke_volume_of("repair-stream-55".into(), true),
            value
        );
        assert_eq!(control.get_stream(), value);
        let saved = Layout::read(dir.path());
        let expected = if value == 54 {
            Kind::Stream.default_gain()
        } else {
            volume::from_slider(value as f32).min(volume::MAX)
        };
        assert_eq!(
            saved
                .stream_volumes
                .get("repair-stream-55")
                .copied()
                .unwrap_or(Kind::Stream.default_gain()),
            expected
        );
        assert_eq!(
            window.invoke_volume_of("repair-stream-55".into(), false),
            137
        );
    }
    window.invoke_volume_changed("repair-stream-55".into(), false, 100, true);
    assert!(
        !Layout::read(dir.path())
            .volumes
            .contains_key("repair-stream-55")
    );
}

#[test]
fn contact_clock_uses_fake_wall_time_dst_recreation_and_no_remote_dispatch() {
    let (dir, window, adapter) = fixture();
    let (backend, mut commands) = yapfast_core::backend::Backend::anonymous_fixture(Arc::new(
        BackgroundRuntime::new().unwrap(),
    ));
    adapter.borrow_mut().backend = Some(backend);
    let mut a = adapter.borrow_mut();
    a.app.state.discord_user = Some("100".into());
    a.app.state.profile = Some(ProfileView {
        profile: yapfast_core::chat::Profile {
            id: "200".into(),
            ..Default::default()
        },
        ..Default::default()
    });
    a.layout
        .local_timezones
        .set("discord:100", "200", "America/New_York")
        .unwrap();
    a.layout.try_write(dir.path()).unwrap();
    a.timezone_now = Some(UNIX_EPOCH + Duration::from_secs(1_772_953_140));
    a.refresh(&window);
    assert!(
        window
            .get_profile()
            .local_time
            .contains("01:59 EST (UTC-05:00)")
    );
    a.timezone_now = Some(UNIX_EPOCH + Duration::from_secs(1_772_953_260));
    a.tick_timezones(&window);
    assert!(
        window
            .get_profile()
            .local_time
            .contains("03:01 EDT (UTC-04:00)")
    );
    a.detach();
    assert!(!a.timezone_clock.running());
    let replacement = YapfastWindow::new().unwrap();
    a.window = replacement.as_weak();
    a.refresh(&replacement);
    assert!(
        replacement
            .get_profile()
            .local_time
            .contains("03:01 EDT (UTC-04:00)")
    );
    assert!(a.timezone_clock.running());
    assert!(
        commands.try_recv().is_err(),
        "no contact-ID uploads or remote settings"
    );
    a.app.state.profile = None;
    a.refresh(&replacement);
    assert!(!a.timezone_clock.running());
}
