//! Production dispatcher -> member/snapshot -> Application -> native card ordering.
use super::*;
use yapfast_core::{
    application::Navigation,
    chat::{Event, Presence},
};
use yapfast_discord::{
    event::Event as Gateway,
    service::{ContactSnapshotFixture, Live},
};
fn tracker() -> ContactSnapshotFixture {
    let mut t = ContactSnapshotFixture::new(&serde_json::json!({
        "user":{"id":"100"},"guilds":[{"id":"10","owner_id":"100","properties":{"name":"Fixture"},"roles":[{"id":"10","name":"@everyone","permissions":"1024","position":0}],"channels":[{"id":"11","type":0,"name":"general","position":0}]}],
        "users":[{"id":"200","username":"target"},{"id":"300","username":"other"}],"relationships":[{"id":"200","type":1},{"id":"300","type":1}]
    }));
    t.select_members("10", "11", "everyone");
    t.receive(Gateway::GuildMemberListUpdate,&serde_json::json!({"guild_id":"10","id":"everyone","ops":[{"op":"SYNC","range":[0,99],"items":[{"member":{"user":{"id":"200","username":"target"},"presence":{"status":"online"}}}]}]}));
    t.member_event();
    t
}
fn member(t: &mut ContactSnapshotFixture) -> Event {
    let Live::Members {
        members,
        observation,
        ..
    } = t.member_event().expect("actual member publication")
    else {
        panic!("not members")
    };
    yapfast_core::discord_text::received_members("discord:10:11".into(), members, observation)
}

#[test]
fn coalesced_presence_free_readd_clears_card_and_final_panel_until_new_receipt() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    owner.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&window, &owner);
    let mut adapter = owner.borrow_mut();
    let mut t = ContactSnapshotFixture::new(&serde_json::json!({
        "user":{"id":"100"},"users":[{"id":"200","username":"target"}],
        "relationships":[{"id":"200","type":1}],
        "private_channels":[{"id":"20","type":1,"recipient_ids":["200"]}]
    }));
    let web =
        serde_json::json!({"user":{"id":"200"},"status":"online","client_status":{"web":"online"}});
    assert!(t.receive(Gateway::PresenceUpdate, &web));
    let before = t.snapshot();
    let reach = adapter.apply(Event::Discord(Arc::new(before.clone())));
    adapter.show(&window, reach);
    let room = yapfast_core::discord::room_id(yapfast_discord::ready::PRIVATE, "20");
    adapter.app.state.navigation = Navigation::DirectMessages;
    adapter.app.state.selected = Some(room.clone());
    adapter.app.state.profile_panel = true;
    adapter.app.dispatch(Action::ShowDmProfile(room.clone()));
    adapter.refresh(&window);
    assert_eq!(window.get_profile().platforms, "Web (online)");
    assert_eq!(window.get_panel_profile().platforms, "Web (online)");
    let dot = window.get_panel_profile().presence;
    assert!(t.receive(
        Gateway::RelationshipRemove,
        &serde_json::json!({"id":"200"})
    ));
    assert!(t.receive(
        Gateway::RelationshipAdd,
        &serde_json::json!({"id":"200","type":1})
    ));
    let after = t.snapshot();
    assert_eq!(
        after.friend_list[0].presence_observation,
        before.friend_list[0].presence_observation
    );
    assert_eq!(
        after.conversations[0].presence_observation,
        before.conversations[0].presence_observation
    );
    assert!(
        after.presence_retirements["200"]
            .newer_than(before.friend_list[0].presence_observation.as_ref().unwrap())
    );
    let reach = adapter.apply(Event::Discord(Arc::new(after)));
    adapter.show(&window, reach);
    assert_eq!(
        window.get_profile().platforms,
        "",
        "presence-free re-add cannot erase retirement B"
    );
    assert_eq!(window.get_panel_profile().platforms, "");
    assert_eq!(window.get_panel_profile().presence, dot);
    adapter.app.dispatch(Action::CloseProfile);
    adapter.app.dispatch(Action::ShowDmProfile(room));
    adapter.refresh(&window);
    assert_eq!(window.get_profile().platforms, "");
    assert_eq!(window.get_panel_profile().platforms, "");
    assert!(t.receive(Gateway::PresenceUpdate, &web));
    let reach = adapter.apply(Event::Discord(Arc::new(t.snapshot())));
    adapter.show(&window, reach);
    assert_eq!(window.get_profile().platforms, "Web (online)");
    assert_eq!(window.get_panel_profile().platforms, "Web (online)");
}

#[test]
fn coalesced_unfriend_then_later_dm_receipt_reaches_card_and_final_panel() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    owner.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&window, &owner);
    let mut adapter = owner.borrow_mut();
    let mut t = ContactSnapshotFixture::new(&serde_json::json!({
        "user":{"id":"100"},"users":[{"id":"200","username":"target"}],
        "relationships":[{"id":"200","type":1}],
        "private_channels":[{"id":"20","type":1,"recipient_ids":["200"]}]
    }));
    let web =
        serde_json::json!({"user":{"id":"200"},"status":"online","client_status":{"web":"online"}});
    assert!(t.receive(Gateway::PresenceUpdate, &web));
    let reach = adapter.apply(Event::Discord(Arc::new(t.snapshot())));
    adapter.show(&window, reach);
    let room = yapfast_core::discord::room_id(yapfast_discord::ready::PRIVATE, "20");
    adapter.app.state.navigation = Navigation::DirectMessages;
    adapter.app.state.selected = Some(room.clone());
    adapter.app.state.profile_panel = true;
    adapter.app.dispatch(Action::ShowDmProfile(room.clone()));
    adapter.refresh(&window);
    assert_eq!(window.get_profile().platforms, "Web (online)");
    assert_eq!(window.get_panel_profile().platforms, "Web (online)");
    let dot = window.get_panel_profile().presence;
    assert!(t.receive(
        Gateway::RelationshipRemove,
        &serde_json::json!({"id":"200"})
    ));
    assert!(t.receive(Gateway::PresenceUpdate, &web));
    let s = t.snapshot();
    assert!(
        s.conversations[0]
            .presence_observation
            .as_ref()
            .unwrap()
            .newer_than(&s.presence_retirements["200"])
    );
    let reach = adapter.apply(Event::Discord(Arc::new(s)));
    adapter.show(&window, reach);
    assert_eq!(
        window.get_profile().platforms,
        "Web (online)",
        "later DM C must beat retirement B"
    );
    assert_eq!(window.get_panel_profile().platforms, "Web (online)");
    assert_eq!(
        window.get_panel_profile().presence,
        dot,
        "legacy dot is unchanged"
    );
    adapter.app.dispatch(Action::CloseProfile);
    adapter.app.dispatch(Action::ShowDmProfile(room));
    adapter.refresh(&window);
    assert_eq!(window.get_profile().platforms, "Web (online)");
    assert_eq!(window.get_panel_profile().platforms, "Web (online)");
}
#[test]
fn gateway_receipt_order_reaches_native_card_and_survives_reopen() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let owner = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    owner.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&window, &owner);
    let mut adapter = owner.borrow_mut();
    let mut t = tracker();
    let web =
        serde_json::json!({"user":{"id":"200"},"status":"online","client_status":{"web":"online"}});
    assert!(t.receive(Gateway::PresenceUpdate, &web));
    let reach = adapter.apply(Event::Discord(Arc::new(t.snapshot())));
    adapter.show(&window, reach);
    adapter.app.state.navigation = Navigation::Server("discord:10".into());
    adapter.app.state.selected = Some("discord:10:11".into());
    adapter.app.dispatch(Action::ShowFriendProfile {
        user: "200".into(),
        large: false,
    });
    adapter.refresh(&window);
    assert!(window.get_profile_open());
    assert_eq!(window.get_profile().platforms, "Web (online)");
    let dot = window.get_profile().presence;
    assert!(t.receive(Gateway::PresenceUpdate, &web)); // accepted but unpublished
    let clear = serde_json::json!({"user":{"id":"200"},"guild_id":"10","status":"online"});
    assert!(!t.receive(Gateway::PresenceUpdate, &clear));
    let reach = adapter.apply(member(&mut t));
    adapter.show(&window, reach);
    assert_eq!(
        adapter
            .app
            .state
            .profile
            .as_ref()
            .unwrap()
            .platform_presence,
        Some(Presence::Online)
    );
    assert_eq!(window.get_profile().platforms, "");
    assert_eq!(window.get_profile().presence, dot, "legacy dot unchanged");
    assert!(t.receive(
        Gateway::PresenceUpdate,
        &serde_json::json!({"user":{"id":"300"},"status":"idle"})
    ));
    let reach = adapter.apply(Event::Discord(Arc::new(t.snapshot())));
    adapter.show(&window, reach);
    assert_eq!(
        window.get_profile().platforms,
        "",
        "coalesced older Web cannot revive"
    );
    assert!(t.receive(Gateway::PresenceUpdate, &web));
    let reach = adapter.apply(Event::Discord(Arc::new(t.snapshot())));
    adapter.show(&window, reach);
    assert_eq!(
        window.get_profile().platforms,
        "Web (online)",
        "later same-value receipt restores"
    );
    assert!(!t.receive(Gateway::PresenceUpdate, &clear));
    let older = member(&mut t);
    assert!(t.receive(Gateway::PresenceUpdate, &web));
    let reach = adapter.apply(Event::Discord(Arc::new(t.snapshot())));
    adapter.show(&window, reach);
    let reach = adapter.apply(older);
    adapter.show(&window, reach);
    assert_eq!(
        window.get_profile().platforms,
        "Web (online)",
        "delayed older member cannot clear newer snapshot"
    );
    t.receive(
        Gateway::UserUpdate,
        &serde_json::json!({"id":"200","username":"metadata"}),
    );
    let reach = adapter.apply(member(&mut t));
    adapter.show(&window, reach);
    assert_eq!(
        window.get_profile().platforms,
        "Web (online)",
        "metadata republish must not restamp member presence"
    );
    adapter.app.dispatch(Action::CloseProfile);
    adapter.app.dispatch(Action::ShowFriendProfile {
        user: "200".into(),
        large: true,
    });
    adapter.refresh(&window);
    assert!(window.get_profile_open());
    assert_eq!(window.get_profile().platforms, "Web (online)");
    let mut replacement = tracker();
    replacement.receive(Gateway::PresenceUpdate, &web);
    assert!(!t.receive(Gateway::PresenceUpdate, &clear));
    let retired = member(&mut t);
    let reach = adapter.apply(Event::Discord(Arc::new(replacement.snapshot())));
    adapter.show(&window, reach);
    let reach = adapter.apply(retired);
    adapter.show(&window, reach);
    assert_eq!(
        window.get_profile().platforms,
        "Web (online)",
        "retired same-account provider cannot clear"
    );
}
