//! Deterministic ownership races through Application's real event boundary.
use super::*;
use yapfast_discord::service::{Snapshot, Status};

fn snapshot(owner: &str) -> Arc<Snapshot> {
    Arc::new(Snapshot {
        status: Status::Online,
        user_id: Some(owner.into()),
        ..Default::default()
    })
}

fn app() -> Application {
    let mut app = Application::default();
    app.state.account = Some("discord:100".into());
    app.state.discord_user = Some("100".into());
    app.state.discord_online = true;
    app.discord = Some(snapshot("100"));
    for (id, direct) in [("discord:10:11", false), ("discord:@me:30", true)] {
        app.state.rooms.push(Arc::new(ChatRoom {
            network: Network::Discord,
            person: direct.then(|| "100".into()),
            ..preview::preview_room(id, "Cached name", direct, 2)
        }));
    }
    app.state.selected = Some("discord:10:11".into());
    app
}

fn bind(app: &Application, request: &Arc<chat::ProfileRequest>) -> Arc<chat::ProfilePublication> {
    let (publication, _publish) = chat::ProfilePublication::fixture(app.discord.clone().unwrap());
    *request.publication.lock().unwrap() = Some(publication.clone());
    publication
}

fn open(app: &mut Application, room: &str, panel: bool) -> Arc<chat::ProfileRequest> {
    if panel {
        app.state.selected = Some(room.into());
        app.state.profile_panel = true;
        app.state.panel_profile = Some(ProfileView {
            profile: profile("pending panel"),
            loading: true,
            ..Default::default()
        });
    } else {
        app.open_profile(room.into(), profile("pending card"), false);
    }
    let request = if panel {
        let Effect::Backend(Command::Discord(DiscordCommand::ScopedProfile(request))) =
            app.request_profile(room.into(), "100".into(), true)
        else {
            panic!("scoped panel")
        };
        request
    } else {
        app.profile_request.clone().unwrap()
    };
    bind(app, &request);
    request
}

fn account_request(app: &mut Application) -> Arc<chat::ProfileRequest> {
    let effects = app.load_own_profile();
    let [Effect::Backend(Command::Discord(DiscordCommand::AccountProfile(request)))] =
        effects.as_slice()
    else {
        panic!("dedicated account refresh")
    };
    let request = request.clone();
    bind(app, &request);
    request
}

fn profile(bio: &str) -> Profile {
    Profile {
        id: "100".into(),
        bio: bio.into(),
        ..Default::default()
    }
}

fn complete(app: &mut Application, request: Arc<chat::ProfileRequest>, bio: &str) {
    app.apply(Event::ProfilePresentation {
        request,
        kept: false,
        result: Ok(profile(bio)),
    });
}

fn account_complete(app: &mut Application, request: Arc<chat::ProfileRequest>, bio: &str) {
    app.apply(Event::AccountProfile {
        request,
        result: Ok(profile(bio)),
    });
}

fn frame() -> Arc<[chat::ProfileFrameLayer]> {
    Arc::from([chat::ProfileFrameLayer {
        src: "accepted-frame.png".into(),
        ..Default::default()
    }])
}

#[test]
fn account_main_completion_cannot_steal_scoped_server_card_or_dm_panel() {
    let mut app = app();
    let card = open(&mut app, "discord:10:11", false);
    let panel = open(&mut app, "discord:@me:30", true);
    let account = account_request(&mut app);
    account_complete(&mut app, account, "account main");
    assert_eq!(
        app.state.account_profile.as_ref().unwrap().bio,
        "account main"
    );
    assert!(app.state.profile.as_ref().unwrap().loading);
    assert!(app.state.panel_profile.as_ref().unwrap().loading);
    assert_eq!(
        app.state.profile.as_ref().unwrap().profile.bio,
        "pending card"
    );
    assert_eq!(
        app.state.panel_profile.as_ref().unwrap().profile.bio,
        "pending panel"
    );

    // Both the old account load and old editor cofetch used these user-only events.
    app.apply(Event::KeptProfile {
        user: "100".into(),
        profile: profile("legacy kept main"),
    });
    app.apply(Event::Profile {
        user: "100".into(),
        result: Ok(profile("legacy main")),
    });
    assert!(app.state.profile.as_ref().unwrap().loading);
    assert!(app.state.panel_profile.as_ref().unwrap().loading);
    complete(&mut app, card, "correct server");
    complete(&mut app, panel, "correct DM");
    assert_eq!(
        app.state.profile.as_ref().unwrap().profile.bio,
        "correct server"
    );
    assert_eq!(
        app.state.panel_profile.as_ref().unwrap().profile.bio,
        "correct DM"
    );
    assert_eq!(
        app.state.account_profile.as_ref().unwrap().bio,
        "account main"
    );
}

#[test]
fn account_refresh_requires_current_generation_account_and_exact_profile_id() {
    let mut app = app();
    let old = account_request(&mut app);
    let current = account_request(&mut app);
    account_complete(&mut app, old, "stale generation");
    assert!(app.state.account_profile.is_none());
    app.state.account = Some("replacement".into());
    account_complete(&mut app, current.clone(), "wrong account");
    assert!(app.state.account_profile.is_none());
    app.state.account = Some("discord:100".into());
    app.apply(Event::AccountProfile {
        request: current.clone(),
        result: Ok(Profile {
            id: "200".into(),
            ..profile("wrong user")
        }),
    });
    assert!(app.state.account_profile.is_none());
    app.apply(Event::AccountProfile {
        request: current.clone(),
        result: Ok(Profile {
            joined: Some("server join".into()),
            ..profile("not main")
        }),
    });
    assert!(app.state.account_profile.is_none());
    account_complete(&mut app, current, "accepted main");
    assert_eq!(
        app.state.account_profile.as_ref().unwrap().bio,
        "accepted main"
    );
}

#[test]
fn main_editor_cofetch_keeps_the_dedicated_account_generation() {
    let mut app = app();
    let effects = app.edit_profile();
    let [
        Effect::Backend(Command::Discord(DiscordCommand::OwnProfile {
            server: None,
            card: Some(request),
        })),
    ] = effects.as_slice()
    else {
        panic!("same-answer account publication")
    };
    let request = request.clone();
    assert!(Arc::ptr_eq(
        app.account_profile_request.as_ref().unwrap(),
        &request
    ));
    bind(&app, &request);
    let server = open(&mut app, "discord:10:11", false);
    account_complete(&mut app, request, "editor main");
    assert!(app.state.profile.as_ref().unwrap().loading);
    complete(&mut app, server, "server profile");
    assert_eq!(
        app.state.account_profile.as_ref().unwrap().bio,
        "editor main"
    );
    assert_eq!(
        app.state.profile.as_ref().unwrap().profile.bio,
        "server profile"
    );
}

#[test]
fn unlink_and_account_replacement_retire_cached_appearance_and_all_requests() {
    for unlink in [true, false] {
        let mut app = app();
        let card = open(&mut app, "discord:10:11", false);
        let panel = open(&mut app, "discord:@me:30", true);
        let account = account_request(&mut app);
        account_complete(&mut app, account.clone(), "old account");
        app.state.account_decoration = Some("old-decoration.png".into());
        app.state.account_nameplate = Some(chat::Nameplate {
            picture: "old-plate.png".into(),
            colors: None,
        });
        if unlink {
            app.apply(Event::DiscordUnlinked);
        } else {
            app.apply(Event::Discord(snapshot("200")));
        }
        assert!(app.state.account_profile.is_none());
        assert!(app.state.account_decoration.is_none());
        assert!(app.state.account_nameplate.is_none());
        assert!(app.profile_request.is_none() && app.panel_profile_request.is_none());
        assert!(app.account_profile_request.is_none());

        // Returning to the very same user is a new generation, not permission
        // for callbacks that retained the earlier provider or Application state.
        app.apply(Event::Discord(snapshot("100")));
        let fresh = account_request(&mut app);
        account_complete(&mut app, account, "late old account");
        complete(&mut app, card, "late card");
        complete(&mut app, panel, "late panel");
        assert!(app.state.account_profile.is_none());
        account_complete(&mut app, fresh, "fresh account");
        assert_eq!(
            app.state.account_profile.as_ref().unwrap().bio,
            "fresh account"
        );
    }
}

#[test]
fn retired_publication_rejects_account_and_frame_before_ui_snapshot_catches_up() {
    let mut app = app();
    let card = open(&mut app, "discord:10:11", false);
    complete(&mut app, card.clone(), "base accepted");
    let account = account_request(&mut app);
    let card_publication = card.publication.lock().unwrap().clone().unwrap();
    let account_publication = account.publication.lock().unwrap().clone().unwrap();
    card_publication.retire();
    account_publication.retire();
    // State still says online: authority lives in the actor's publication.
    account_complete(&mut app, account, "retired main");
    app.apply(Event::ProfileFrame {
        request: card,
        frame: frame(),
    });
    assert!(app.state.account_profile.is_none());
    assert!(app.state.profile.as_ref().unwrap().profile.frame.is_empty());
}

#[test]
fn frame_completion_enriches_only_its_nonloading_slot_not_another_same_user_view() {
    let mut app = app();
    let card = open(&mut app, "discord:10:11", false);
    let panel = open(&mut app, "discord:@me:30", true);
    app.apply(Event::ProfileFrame {
        request: card.clone(),
        frame: frame(),
    });
    assert!(
        app.state.profile.as_ref().unwrap().profile.frame.is_empty(),
        "base is still loading"
    );
    complete(&mut app, card.clone(), "server");
    complete(&mut app, panel, "DM");
    app.apply(Event::ProfileFrame {
        request: card,
        frame: frame(),
    });
    assert_eq!(app.state.profile.as_ref().unwrap().profile.frame, frame());
    assert!(
        app.state
            .panel_profile
            .as_ref()
            .unwrap()
            .profile
            .frame
            .is_empty()
    );
}

#[test]
fn stale_frame_after_close_reopen_same_user_or_different_room_is_rejected() {
    for room in ["discord:10:11", "discord:@me:30"] {
        let mut app = app();
        let old = open(&mut app, "discord:10:11", false);
        complete(&mut app, old.clone(), "old base");
        app.dispatch(Action::CloseProfile);
        let current = open(&mut app, room, false);
        complete(&mut app, current.clone(), "new base");
        app.apply(Event::ProfileFrame {
            request: old,
            frame: frame(),
        });
        assert!(app.state.profile.as_ref().unwrap().profile.frame.is_empty());
        app.apply(Event::ProfileFrame {
            request: current,
            frame: frame(),
        });
        assert_eq!(app.state.profile.as_ref().unwrap().profile.frame, frame());
    }
}

#[test]
fn stale_frame_from_another_account_cannot_enrich_an_accepted_open() {
    let mut app = app();
    let request = open(&mut app, "discord:10:11", false);
    complete(&mut app, request.clone(), "base");
    app.state.account = Some("different account".into());
    app.apply(Event::ProfileFrame {
        request,
        frame: frame(),
    });
    assert!(app.state.profile.as_ref().unwrap().profile.frame.is_empty());
}

#[test]
fn offline_snapshot_cannot_restore_account_cosmetics_or_requeue_retired_panels() {
    use yapfast_discord::service::{Account, Conversation};
    let mut app = app();
    open(&mut app, "discord:@me:30", true);
    let account = account_request(&mut app);
    account_complete(&mut app, account.clone(), "cached main");
    let effects = app.apply(Event::Discord(Arc::new(Snapshot {
        status: Status::Offline("retired".into()),
        user_id: Some("100".into()),
        conversations: vec![Arc::new(Conversation {
            id: "30".into(),
            name: "Cached DM".into(),
            person: Some("100".into()),
            people: 2,
            ..Default::default()
        })],
        account: Some(Account {
            username: "me".into(),
            name: "Me".into(),
            avatar: "avatar.png".into(),
            decoration: Some("cached-decoration.png".into()),
            nameplate: Some(yapfast_discord::collectibles::Nameplate {
                picture: "cached-plate.png".into(),
                colors: None,
            }),
            premium_type: 0,
            mfa_enabled: false,
            has_email: true,
            nsfw_allowed: false,
            nsfw_commands_in_dms: false,
        }),
        ..Default::default()
    })));
    assert!(app.state.account_profile.is_none());
    assert!(app.state.account_decoration.is_none() && app.state.account_nameplate.is_none());
    assert!(app.account_profile_request.is_none() && app.panel_profile_request.is_none());
    assert!(
        app.state
            .panel_profile
            .as_ref()
            .is_none_or(|view| !view.loading)
    );
    assert!(!effects.iter().any(|effect| matches!(
        effect,
        Effect::Backend(Command::Discord(
            DiscordCommand::ScopedProfile(_) | DiscordCommand::AccountProfile(_)
        ))
    )));
    account_complete(&mut app, account, "late after offline");
    assert!(app.state.account_profile.is_none());
}

#[test]
fn ignored_legacy_completion_cannot_trigger_tabs_on_a_scoped_open() {
    let mut app = app();
    let request = open(&mut app, "discord:10:11", false);
    complete(&mut app, request, "accepted");
    let view = app.state.profile.as_mut().unwrap();
    view.large = true;
    view.activity_open = true;
    app.apply(Event::Profile {
        user: "100".into(),
        result: Ok(profile("legacy")),
    });
    let view = app.state.profile.as_ref().unwrap();
    assert_eq!(view.profile.bio, "accepted");
    assert!(
        matches!(view.profile.recent, crate::chat::ActivityHistory::Unasked),
        "an ignored user-only completion has no follow-up authority"
    );
}

#[test]
fn provider_retirement_clears_both_styles_and_cached_member_dm_shells_cannot_restore_them() {
    let mut app = app();
    let request = open(&mut app, "discord:10:11", false);
    let style = Arc::new(chat::NameStyle::new(8, 2, &[0xff0000]));
    let styled = Profile {
        style: Some(style.clone()),
        name_style: Some(crate::appearance::NameStyle {
            font_id: 8,
            effect_id: 2,
            colors: vec![0xff0000],
        }),
        avatar_decoration: Some("cached-decoration.png".into()),
        ..profile("accepted appearance")
    };
    app.apply(Event::ProfilePresentation {
        request: request.clone(),
        kept: false,
        result: Ok(styled.clone()),
    });
    assert!(app.state.profile.as_ref().unwrap().profile.style.is_some());
    app.apply(Event::Discord(Arc::new(Snapshot {
        status: Status::Offline("retired".into()),
        ..(*snapshot("100")).clone()
    })));
    let retired = &app.state.profile.as_ref().unwrap().profile;
    assert!(retired.name_style.is_none() && retired.style.is_none());
    assert!(retired.avatar_decoration_url().is_none());

    let mut members = preview::preview_members("discord:10:11");
    members[0].id = "100".into();
    members[0].style = Some(style.clone());
    members[0].decoration = Some("cached-decoration.png".into());
    app.state.members = Some(Ok(Arc::new(members)));
    let mut room = preview::preview_room("discord:10:11", "Server", false, 2);
    room.network = Network::Discord;
    app.state.rooms = vec![Arc::new(room)];
    app.state.selected = Some("discord:10:11".into());
    app.show_profile("100".into(), false);
    assert!(
        app.state.profile.as_ref().unwrap().profile.style.is_none(),
        "member shell"
    );

    let dm = ChatRoom {
        network: Network::Discord,
        person: Some("100".into()),
        style: Some(style),
        decoration: Some("cached-decoration.png".into()),
        ..preview::preview_room("discord:@me:30", "Cached DM", true, 2)
    };
    app.state.rooms = vec![Arc::new(dm)];
    app.state.selected = Some("discord:@me:30".into());
    app.state.profile_panel = true;
    app.show_dm_profile("discord:@me:30".into());
    app.show_panel_profile();
    for view in app.state.profile.iter().chain(&app.state.panel_profile) {
        assert!(view.profile.style.is_none() && view.profile.name_style.is_none());
        assert!(
            view.profile.avatar_decoration_url().is_none(),
            "no cached shop revival"
        );
    }
    app.state.discord_online = true;
    app.discord = Some(snapshot("100"));
    app.show_dm_profile("discord:@me:30".into());
    let fresh = app.profile_request.clone().unwrap();
    bind(&app, &fresh);
    app.apply(Event::ProfilePresentation {
        request,
        kept: false,
        result: Ok(styled.clone()),
    });
    assert!(
        app.state.profile.as_ref().unwrap().profile.style.is_none(),
        "old open cannot restore appearance"
    );
    app.apply(Event::ProfilePresentation {
        request: fresh,
        kept: false,
        result: Ok(styled),
    });
    assert!(
        app.state.profile.as_ref().unwrap().profile.style.is_some(),
        "fresh scoped appearance still works"
    );
}
