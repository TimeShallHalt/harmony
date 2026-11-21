//! Merge boundaries: upstream cosmetics/tabs use Harmony's scoped requests,
//! and bounded Update 4 artwork still obeys the upstream cosmetic switch.
use super::*;

#[test]
fn scoped_profiles_carry_cosmetic_opt_ins_but_panels_never_ask_for_a_frame() {
    let mut app = Application::default();
    let flags = |effect| match effect {
        Effect::Backend(Command::Discord(DiscordCommand::ScopedProfile(request))) => {
            (request.effect, request.frame)
        }
        _ => panic!("expected an identity-bound profile request"),
    };
    assert_eq!(
        flags(app.request_profile("discord:@me:30".into(), "200".into(), false)),
        (true, true)
    );
    assert_eq!(
        flags(app.request_profile("discord:@me:30".into(), "200".into(), true)),
        (true, false)
    );
    app.state.cosmetics.effects = false;
    app.state.cosmetics.frames = false;
    assert_eq!(
        flags(app.request_profile("discord:@me:30".into(), "200".into(), false)),
        (false, false)
    );
}

#[test]
fn received_decoration_fetches_require_both_visibility_and_the_cosmetic_switch() {
    let pictures = |enabled, expires_at| {
        let mut state = AppState::default();
        state.cosmetics.decorations = enabled;
        state.profile = Some(ProfileView {
            profile: Profile {
                decoration: Some(crate::appearance::Decoration {
                    url: "received-decoration.png".into(),
                    expires_at,
                }),
                ..Default::default()
            },
            ..Default::default()
        });
        pictures_on_screen(&state, &[], false, false).any(|url| url == "received-decoration.png")
    };
    assert!(pictures(true, None));
    assert!(!pictures(false, None));
    assert!(!pictures(true, Some(1)));
}

#[test]
fn upstream_shop_artwork_cannot_revive_expired_received_decoration() {
    let mut profile = Profile {
        avatar_decoration: Some("shop-artwork/static".into()),
        decoration: Some(crate::appearance::Decoration {
            url: "received-decoration.png".into(),
            expires_at: Some(1),
        }),
        ..Default::default()
    };
    assert_eq!(profile.avatar_decoration_url(), None);
    profile.decoration.as_mut().unwrap().expires_at = None;
    assert_eq!(profile.avatar_decoration_url(), Some("shop-artwork/static"));
}

#[cfg(all(feature = "discord", feature = "test-fixtures"))]
#[test]
fn scoped_profile_completion_continues_an_open_upstream_activity_tab() {
    use yapfast_discord::service::{Snapshot, Status};
    let mut app = Application::default();
    app.state.account = Some("discord:100".into());
    app.state.discord_user = Some("100".into());
    app.state.discord_online = true;
    app.state.rooms.push(Arc::new(ChatRoom {
        network: Network::Discord,
        ..preview::preview_room("discord:@me:30", "Ada", true, 0)
    }));
    let snapshot = Arc::new(Snapshot {
        status: Status::Online,
        user_id: Some("100".into()),
        ..Default::default()
    });
    app.discord = Some(snapshot.clone());
    let effect = app.request_profile("discord:@me:30".into(), "200".into(), false);
    let Effect::Backend(Command::Discord(DiscordCommand::ScopedProfile(request))) = effect else {
        panic!("scoped profile request");
    };
    let (publication, _publish) = chat::ProfilePublication::fixture(snapshot);
    *request.publication.lock().unwrap() = Some(publication);
    app.state.profile = Some(ProfileView {
        profile: Profile {
            id: "200".into(),
            ..Default::default()
        },
        loading: true,
        large: true,
        activity_open: true,
        ..Default::default()
    });
    let effects = app.apply(Event::ProfilePresentation {
        request,
        kept: false,
        result: Ok(Profile {
            id: "200".into(),
            ..Default::default()
        }),
    });
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Backend(Command::Discord(DiscordCommand::ProfileRecent { user, .. })) if user == "200"
    )));
    assert!(matches!(
        app.state.profile.as_ref().unwrap().profile.recent,
        crate::chat::ActivityHistory::Loading(_)
    ));
}
