use super::*;

#[cfg(feature = "discord")]
#[test]
fn a_new_account_cannot_click_media_from_the_previous_accounts_received_profile() {
    let mut app = app();
    app.apply(Event::Discord(Arc::new(
        yapfast_discord::service::Snapshot {
            user_id: Some("101".into()),
            status: yapfast_discord::service::Status::Online,
            ..Default::default()
        },
    )));
    assert!(
        app.state.profile.is_none(),
        "a profile received under another account is not a current media authority"
    );
    app.dispatch(click(chat::ProfilePicture::Avatar));
    assert!(app.state.viewer.is_none());
}
fn app() -> Application {
    let mut app = Application::default();
    app.state.discord_user = Some("100".into());
    app.state.discord_online = true;
    app.state.profile = Some(ProfileView { profile: Profile {
        id: "200".into(), name: "Fixture".into(),
        avatar: Some("https://cdn.discordapp.com/avatars/200/0123456789abcdef0123456789abcdef.png?size=128".into()),
        banner: Some("https://cdn.discordapp.com/banners/200/a_0123456789abcdef0123456789abcdef.png?size=600".into()),
        ..Default::default()
    }, ..Default::default() });
    app
}
fn click(kind: chat::ProfilePicture) -> Action {
    Action::ViewProfileMedia {
        user: "200".into(),
        slot: Slot::Profile,
        kind,
    }
}
fn source(app: &Application) -> &Session {
    match &app.state.viewer.as_ref().unwrap().source {
        ViewerSource::Profile(session) => session,
        _ => panic!("not a profile source"),
    }
}
#[test]
fn profile_media_fetch_is_explicit_and_does_not_create_messages() {
    let mut app = app();
    let profile = app.state.profile.clone();
    assert!(
        !app.dispatch(Action::DismissNotice)
            .iter()
            .any(|effect| matches!(
                effect,
                Effect::Backend(Command::Discord(DiscordCommand::ViewProfileMedia { .. }))
            ))
    );
    let effects = app.dispatch(click(chat::ProfilePicture::Avatar));
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Backend(Command::Discord(DiscordCommand::ViewProfileMedia { asset, .. })) if asset.url().ends_with("size=4096"))));
    assert!(app.state.messages.is_empty());
    assert_eq!(app.state.profile, profile);
    assert!(app.state.viewer.as_ref().unwrap().item.is_empty());
    assert!(app.dispatch(Action::StepImage(1)).is_empty());
}
#[test]
fn closing_cancels_the_request_releases_bytes_and_keeps_the_profile() {
    let mut app = app();
    app.dispatch(click(chat::ProfilePicture::Banner));
    let (request, asset) = (source(&app).request, source(&app).asset.clone());
    let bytes: Arc<[u8]> = vec![1, 2, 3].into();
    let weak = Arc::downgrade(&bytes);
    app.apply(Event::ProfileMedia {
        request,
        asset,
        loaded: crate::original::Completion::fixture(crate::profile_media::Loaded {
            bytes: Some(bytes),
            picture: Err("decode error fixture"),
        }),
    });
    assert!(weak.upgrade().is_some());
    let profile = app.state.profile.clone();
    let effects = app.dispatch(Action::CloseImage);
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Backend(Command::Discord(DiscordCommand::CancelProfileMedia(id))) if *id == request)));
    assert!(app.state.viewer.is_none());
    assert!(weak.upgrade().is_none());
    assert_eq!(app.state.profile, profile);
}
#[test]
fn stale_completion_and_dialog_cannot_target_a_new_source() {
    let mut app = app();
    app.dispatch(click(chat::ProfilePicture::Avatar));
    let (request, asset) = (source(&app).request, source(&app).asset.clone());
    app.dispatch(click(chat::ProfilePicture::Banner));
    app.apply(Event::ProfileMedia {
        request,
        asset,
        loaded: crate::original::Completion::fixture(crate::profile_media::Loaded {
            bytes: Some(vec![1].into()),
            picture: Err("stale"),
        }),
    });
    assert!(source(&app).loaded.is_none());
    assert!(
        app.dispatch(Action::SaveProfileMedia {
            request,
            path: "must-not-write.png".into()
        })
        .is_empty()
    );
}
#[test]
fn profile_or_account_changes_cancel_even_without_a_media_completion() {
    for account_change in [false, true] {
        let mut app = app();
        app.dispatch(click(chat::ProfilePicture::Avatar));
        let request = source(&app).request;
        if account_change {
            app.state.discord_user = Some("101".into());
        } else {
            app.state.profile.as_mut().unwrap().profile.avatar = None;
        }
        let effects = app.dispatch(Action::DismissNotice);
        assert!(app.state.viewer.is_none());
        assert!(effects.iter().any(|effect| matches!(effect, Effect::Backend(Command::Discord(DiscordCommand::CancelProfileMedia(id))) if *id == request)));
    }
}
#[test]
fn missing_banner_and_stale_click_are_not_fetch_capabilities() {
    let mut app = app();
    app.state.profile.as_mut().unwrap().profile.banner = None;
    app.dispatch(click(chat::ProfilePicture::Banner));
    assert!(app.state.viewer.is_none());
    app.state.profile.as_mut().unwrap().profile.id = "201".into();
    app.dispatch(click(chat::ProfilePicture::Avatar));
    assert!(app.state.viewer.is_none());
}
#[test]
fn a_decode_error_still_saves_only_the_exact_downloaded_original_bytes() {
    let mut app = app();
    app.dispatch(click(chat::ProfilePicture::Avatar));
    let (request, asset) = (source(&app).request, source(&app).asset.clone());
    let bytes: Arc<[u8]> = vec![5, 4, 3].into();
    app.apply(Event::ProfileMedia {
        request,
        asset,
        loaded: crate::original::Completion::fixture(crate::profile_media::Loaded {
            bytes: Some(bytes.clone()),
            picture: Err("decode error fixture"),
        }),
    });
    let effects = app.dispatch(Action::SaveProfileMedia {
        request,
        path: "chosen.png".into(),
    });
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Backend(Command::Discord(DiscordCommand::SaveProfileMedia { bytes: saved, account, .. })) if Arc::ptr_eq(saved, &bytes) && account == "100")));
}
