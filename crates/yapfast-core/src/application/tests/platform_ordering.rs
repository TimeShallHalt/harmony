//! Received ordering through the real dispatcher/tracker/member parser and adapter.
use super::*;
use yapfast_discord::{
    event::Event as Gateway,
    service::{ContactSnapshotFixture, Live},
};

fn tracker(dm: bool) -> ContactSnapshotFixture {
    let mut tracker = ContactSnapshotFixture::new(&serde_json::json!({
        "user":{"id":"100"},
        "guilds":[{"id":"10","owner_id":"100","properties":{"name":"Fixture"},
            "roles":[{"id":"10","name":"@everyone","permissions":"1024","position":0}],
            "channels":[{"id":"11","type":0,"name":"general","position":0}]}],
        "users":[{"id":"200","username":"target"},{"id":"300","username":"other"}],
        "relationships":[{"id":"200","type":1},{"id":"300","type":1}],
        "private_channels":if dm { serde_json::json!([{"id":"20","type":1,"recipient_ids":["200"]}]) } else { serde_json::json!([]) }
    }));
    tracker.select_members("10", "11", "everyone");
    tracker.receive(Gateway::GuildMemberListUpdate, &serde_json::json!({
        "guild_id":"10","id":"everyone","ops":[{"op":"SYNC","range":[0,99],"items":[
            {"member":{"user":{"id":"200","username":"target"},"presence":{"status":"online","client_status":{"web":"online"}}}}
        ]}]
    }));
    let _ = tracker.member_event();
    tracker
}
fn web() -> serde_json::Value {
    serde_json::json!({"user":{"id":"200"},"status":"online","client_status":{"web":"online"}})
}
fn members(tracker: &mut ContactSnapshotFixture) -> Event {
    let Live::Members {
        members,
        observation,
        ..
    } = tracker
        .member_event()
        .expect("actual dispatched member publication")
    else {
        panic!("not members");
    };
    crate::discord_text::received_members("discord:10:11".into(), members, observation)
}
fn app(tracker: &mut ContactSnapshotFixture) -> Application {
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    let mut app = Application::default();
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    app.state.navigation = Navigation::Server("discord:10".into());
    app.state.selected = Some("discord:10:11".into());
    app.dispatch(Action::ShowFriendProfile {
        user: "200".into(),
        large: false,
    });
    assert!(matches!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Clients(_))
    ));
    app
}
#[test]
fn pending_earlier_friend_observation_cannot_revive_newer_member_clear() {
    let mut tracker = tracker(false);
    let mut app = app(&mut tracker);
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    let clear = serde_json::json!({"user":{"id":"200"},"guild_id":"10","status":"online"});
    assert!(
        !tracker.receive(Gateway::PresenceUpdate, &clear),
        "no-DM friend guild presence is member-owned"
    );
    app.apply(members(&mut tracker));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Online)
    );
    assert!(tracker.receive(
        Gateway::PresenceUpdate,
        &serde_json::json!({"user":{"id":"300"},"status":"idle"})
    ));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Online),
        "coalesced earlier contact evidence must not revive platforms cleared by a newer member observation"
    );
    // A truly later SAME-VALUE receipt is not indefinitely masked.
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert!(matches!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Clients(_))
    ));
}
#[test]
fn delayed_older_member_and_metadata_cannot_replace_newer_contact_snapshot() {
    let mut tracker = tracker(false);
    let mut app = app(&mut tracker);
    let clear = serde_json::json!({"user":{"id":"200"},"guild_id":"10","status":"offline","client_status":{"web":"online"}});
    assert!(!tracker.receive(Gateway::PresenceUpdate, &clear));
    let older = members(&mut tracker);
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    app.apply(older);
    let expected = app.state.profile.as_ref().unwrap().platform_presence;
    assert!(matches!(expected, Some(chat::Presence::Clients(_))));
    // USER_UPDATE republishes member metadata without creating a receipt.
    tracker.receive(
        Gateway::UserUpdate,
        &serde_json::json!({"id":"200","username":"new name"}),
    );
    app.apply(members(&mut tracker));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        expected,
        "metadata republish retains the member's original receipt"
    );
    // A later member same-value offline is a real new observation and clears.
    assert!(!tracker.receive(Gateway::PresenceUpdate, &clear));
    app.apply(members(&mut tracker));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Offline)
    );
    app.dispatch(Action::CloseProfile);
    app.dispatch(Action::ShowFriendProfile {
        user: "200".into(),
        large: true,
    });
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Offline)
    );
}
#[test]
fn member_removal_receipt_and_retired_provider_events_are_fenced() {
    let mut tracker = tracker(false);
    let mut app = app(&mut tracker);
    // A current member publication establishes list membership.
    let clear = serde_json::json!({"user":{"id":"200"},"guild_id":"10","status":"online"});
    tracker.receive(Gateway::PresenceUpdate, &clear);
    app.apply(members(&mut tracker));
    tracker.receive(
        Gateway::GuildMemberListUpdate,
        &serde_json::json!({"guild_id":"10","id":"everyone","ops":[{"op":"DELETE","index":0}]}),
    );
    let removal = members(&mut tracker);
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    app.apply(removal);
    assert!(
        matches!(
            app.state.profile.as_ref().unwrap().platform_presence,
            Some(chat::Presence::Clients(_))
        ),
        "older removal must not clear a newer snapshot"
    );
    // Same account, fresh provider/session source. Old queued members retire.
    tracker.receive(Gateway::PresenceUpdate, &clear);
    // Member no longer listed: select and receive a fresh real SYNC instead.
    tracker.receive(Gateway::GuildMemberListUpdate,&serde_json::json!({"guild_id":"10","id":"everyone","ops":[{"op":"SYNC","range":[0,99],"items":[{"member":{"user":{"id":"200"},"presence":{"status":"offline"}}}]}]}));
    let retired = members(&mut tracker);
    let mut replacement = tracker_for_replacement();
    replacement.receive(Gateway::PresenceUpdate, &web());
    app.apply(Event::Discord(Arc::new(replacement.snapshot())));
    app.apply(retired);
    assert!(matches!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Clients(_))
    ));
    let mut offline = replacement.snapshot();
    offline.status = yapfast_discord::service::Status::Offline("fixture".into());
    app.apply(Event::Discord(Arc::new(offline)));
    assert_eq!(app.state.profile.as_ref().unwrap().platform_presence, None);
}
#[test]
fn cached_members_keep_legacy_rows_without_reviving_live_platforms() {
    let mut tracker = tracker(false);
    let mut app = app(&mut tracker);
    let mut offline = tracker.snapshot();
    offline.status = yapfast_discord::service::Status::Offline("fixture".into());
    offline.presence_observation = None;
    app.apply(Event::Discord(Arc::new(offline)));
    // The production cache serialization deliberately removes receipt authority.
    let mut list =
        yapfast_discord::member_list::MemberList::new("10".into(), "11".into(), "everyone".into());
    let mut names = yapfast_discord::message::Names::default();
    list.apply(&serde_json::json!({"guild_id":"10","id":"everyone","ops":[{"op":"SYNC","range":[0,99],"items":[{"member":{"user":{"id":"200"},"presence":{"status":"online","client_status":{"web":"online"}}}}]}]}), &mut names);
    let cached: yapfast_discord::member_list::MemberList =
        serde_json::from_value(serde_json::to_value(list).unwrap()).unwrap();
    app.apply(crate::discord_text::received_members(
        "discord:10:11".into(),
        cached.members(None, &names),
        None,
    ));
    let members = app.state.members.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(
        members.len(),
        1,
        "cached membership must still be presented offline"
    );
    assert_eq!(
        members[0].presence,
        Some(chat::Presence::Online),
        "cached platforms are unknown but the legacy dot remains"
    );
    assert_eq!(app.state.profile.as_ref().unwrap().platform_presence, None);
}

#[test]
fn nonfriend_dm_panels_and_wrong_room_events_keep_received_authority() {
    let mut tracker = tracker(true);
    assert!(tracker.receive(
        Gateway::RelationshipRemove,
        &serde_json::json!({"id":"200"})
    ));
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    let mut app = Application::default();
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    let room = crate::discord::room_id(yapfast_discord::ready::PRIVATE, "20");
    app.state.navigation = Navigation::DirectMessages;
    app.state.selected = Some(room.clone());
    app.state.profile_panel = true;
    app.dispatch(Action::ShowDmProfile(room));
    let initial = app.state.profile.as_ref().unwrap().platform_presence;
    assert!(matches!(initial, Some(chat::Presence::Clients(_))));
    assert_eq!(
        app.state.panel_profile.as_ref().unwrap().platform_presence,
        initial
    );
    let clear = serde_json::json!({"user":{"id":"200"},"guild_id":"10","status":"online"});
    assert!(
        tracker.receive(Gateway::PresenceUpdate, &clear),
        "nonfriend DM routing stays intact"
    );
    app.apply(members(&mut tracker)); // real guild event for a different selected room
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        initial
    );
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Online)
    );
    assert_eq!(
        app.state.panel_profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Online)
    );
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert!(matches!(
        app.state.profile.as_ref().unwrap().platform_presence,
        Some(chat::Presence::Clients(_))
    ));
    assert_eq!(
        app.state.panel_profile.as_ref().unwrap().platform_presence,
        initial
    );
}

fn tracker_for_replacement() -> ContactSnapshotFixture {
    tracker(false)
}

#[test]
fn coalesced_remove_readd_without_presence_preserves_the_retirement() {
    let mut tracker = tracker(false);
    let mut app = app(&mut tracker);
    let before = tracker.snapshot();
    let a = before
        .friend_list
        .iter()
        .find(|f| f.user_id == "200")
        .unwrap();
    assert!(tracker.receive(
        Gateway::RelationshipRemove,
        &serde_json::json!({"id":"200"})
    ));
    assert!(tracker.receive(
        Gateway::RelationshipAdd,
        &serde_json::json!({"id":"200","type":1})
    ));
    let after = tracker.snapshot();
    let readded = after
        .friend_list
        .iter()
        .find(|f| f.user_id == "200")
        .unwrap();
    assert_eq!(
        (readded.presence, &readded.presence_observation),
        (a.presence, &a.presence_observation),
        "a relationship-only re-add must not manufacture a presence receipt"
    );
    assert!(after.presence_retirements["200"].newer_than(a.presence_observation.as_ref().unwrap()));
    app.apply(Event::Discord(Arc::new(after)));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        None,
        "retirement B must be consumed even when the final friend tuple is unchanged"
    );
    app.dispatch(Action::CloseProfile);
    app.dispatch(Action::ShowFriendProfile {
        user: "200".into(),
        large: true,
    });
    assert_eq!(app.state.profile.as_ref().unwrap().platform_presence, None);
    tracker.receive(
        Gateway::UserUpdate,
        &serde_json::json!({"id":"200","username":"metadata"}),
    );
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert_eq!(app.state.profile.as_ref().unwrap().platform_presence, None);
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert!(
        matches!(
            app.state.profile.as_ref().unwrap().platform_presence,
            Some(chat::Presence::Clients(_))
        ),
        "a genuinely later same-value presence restores, not permanent suppression"
    );
}

#[test]
fn coalesced_friend_retirement_cannot_hide_a_later_same_value_dm_receipt() {
    let mut tracker = tracker(true);
    let mut app = app(&mut tracker);
    let room = crate::discord::room_id(yapfast_discord::ready::PRIVATE, "20");
    app.state.navigation = Navigation::DirectMessages;
    app.state.selected = Some(room.clone());
    app.state.profile_panel = true;
    app.dispatch(Action::ShowDmProfile(room.clone()));
    let initial = app.state.profile.as_ref().unwrap().platform_presence;
    assert!(matches!(initial, Some(chat::Presence::Clients(_))));
    assert_eq!(
        app.state.panel_profile.as_ref().unwrap().platform_presence,
        initial
    );
    let a = tracker.snapshot().conversations[0]
        .presence_observation
        .clone()
        .unwrap();
    assert!(tracker.receive(
        Gateway::RelationshipRemove,
        &serde_json::json!({"id":"200"})
    ));
    let b = tracker.snapshot().presence_retirements["200"].clone();
    assert!(b.newer_than(&a));
    assert!(tracker.receive(Gateway::PresenceUpdate, &web()));
    let coalesced = tracker.snapshot();
    let c = coalesced.conversations[0]
        .presence_observation
        .as_ref()
        .unwrap();
    assert!(c.newer_than(&b));
    assert_eq!(
        coalesced.conversations[0].presence.map(discord::presence),
        initial
    );
    app.apply(Event::Discord(Arc::new(coalesced)));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        initial,
        "later same-value DM receipt C wins over friend retirement B"
    );
    assert_eq!(
        app.state.panel_profile.as_ref().unwrap().platform_presence,
        initial
    );
    assert!(app.platform_observations["200"].newer_than(&b));
    app.dispatch(Action::CloseProfile);
    app.dispatch(Action::ShowDmProfile(room));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        initial
    );
    assert_eq!(
        app.state.panel_profile.as_ref().unwrap().platform_presence,
        initial
    );
}

#[test]
fn coalesced_contact_removal_uses_its_own_receipt_not_snapshot_watermark() {
    let mut tracker = tracker(false);
    let mut app = app(&mut tracker);
    assert!(tracker.receive(
        Gateway::RelationshipRemove,
        &serde_json::json!({"id":"200"})
    ));
    let mobile = serde_json::json!({"user":{"id":"200"},"guild_id":"10","status":"online","client_status":{"mobile":"online"}});
    assert!(!tracker.receive(Gateway::PresenceUpdate, &mobile));
    app.apply(members(&mut tracker));
    let newer = app.state.profile.as_ref().unwrap().platform_presence;
    assert!(matches!(newer, Some(chat::Presence::Clients(_))));
    assert!(tracker.receive(
        Gateway::PresenceUpdate,
        &serde_json::json!({"user":{"id":"300"},"status":"idle"})
    ));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert_eq!(
        app.state.profile.as_ref().unwrap().platform_presence,
        newer,
        "unrelated publication must not restamp an older contact removal"
    );
    // A genuinely later loss of contact authority can clear instead.
    assert!(tracker.receive(
        Gateway::RelationshipAdd,
        &serde_json::json!({"id":"200","type":1})
    ));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert!(tracker.receive(
        Gateway::RelationshipRemove,
        &serde_json::json!({"id":"200"})
    ));
    app.apply(Event::Discord(Arc::new(tracker.snapshot())));
    assert_eq!(app.state.profile.as_ref().unwrap().platform_presence, None);
}
