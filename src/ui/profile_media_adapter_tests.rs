//! Integrated Adapter regression for the parent's desktop test build.
use super::*;

#[test]
fn profile_media_adapter_uses_loaded_pixels_and_releases_them_on_close() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let window = classic_window();
    let adapter = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        directory.path().into(),
        &window,
    )
    .unwrap();
    let mut adapter = adapter.borrow_mut();
    adapter.app.state.discord_user = Some("100".into());
    adapter.app.state.discord_online = true;
    adapter.app.state.profile = Some(ProfileView { profile: yapfast_core::chat::Profile {
        id: "200".into(), name: "Fixture".into(),
        avatar: Some("https://cdn.discordapp.com/avatars/200/0123456789abcdef0123456789abcdef.png?size=128".into()),
        ..Default::default()
    }, ..Default::default() });
    // Dispatch pure model work without sending effects to a network backend.
    adapter.app.dispatch(Action::ViewProfileMedia {
        user: "200".into(),
        slot: yapfast_core::profile_media::Slot::Profile,
        kind: ProfilePicture::Avatar,
    });
    let session = match &adapter.app.state.viewer.as_ref().unwrap().source {
        yapfast_core::application::ViewerSource::Profile(session) => session.clone(),
        _ => panic!("profile source"),
    };
    let picture = Arc::new(Picture {
        width: 9,
        height: 7,
        rgba: yapfast_core::chat::Pixels::new(9, 7, [30, 70, 110, 255].repeat(9 * 7)),
        ..Default::default()
    });
    let weak = Arc::downgrade(&picture);
    adapter.app.apply(Event::ProfileMedia {
        request: session.request,
        asset: session.asset,
        loaded: yapfast_core::original::Completion::fixture(yapfast_core::profile_media::Loaded {
            bytes: Some(vec![1, 2].into()),
            picture: Ok(picture),
        }),
    });
    adapter.refresh(&window);
    assert!(window.get_viewer_open());
    assert!(window.get_viewer_profile_media());
    assert_eq!(
        (
            window.get_viewer_image().size().width,
            window.get_viewer_image().size().height
        ),
        (9, 7)
    );
    assert_eq!(
        &window.get_viewer_image().to_rgba8().unwrap().as_bytes()[..4],
        &[30, 70, 110, 255]
    );
    adapter.app.dispatch(Action::CloseImage);
    adapter.refresh(&window);
    assert!(!window.get_viewer_open());
    assert_eq!(window.get_viewer_image().size().width, 0);
    assert_eq!(window.get_viewer_frames().row_count(), 0);
    assert!(weak.upgrade().is_none());
    assert!(adapter.app.state.profile.is_some());
}
