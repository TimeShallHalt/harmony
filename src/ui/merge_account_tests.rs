//! Native account-card ownership regressions; testing backend, no OS window/input.
use super::*;

#[test]
fn account_card_never_projects_a_profile_from_another_or_unlinked_account() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let adapter = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        directory.path().into(),
        &window,
    )
    .unwrap();
    let profile = |id: &str| {
        Arc::new(yapfast_core::chat::Profile {
            id: id.into(),
            name: "Owned profile".into(),
            theme: Some([0x123456, 0x654321]),
            ..Default::default()
        })
    };
    let mut adapter = adapter.borrow_mut();
    adapter.app.state.discord_user = Some("current".into());
    adapter.app.state.account_profile = Some(profile("previous"));
    adapter.refresh(&window);
    assert!(window.get_account_profile().id.is_empty());
    assert!(!window.get_account_profile().has_theme);

    adapter.app.state.account_profile = Some(profile("current"));
    adapter.refresh(&window);
    assert_eq!(window.get_account_profile().id, "current");
    assert!(window.get_account_profile().has_theme);

    // Even an unchanged Arc cached by the adapter must retire on owner change.
    adapter.app.state.discord_user = Some("replacement".into());
    adapter.refresh(&window);
    assert!(window.get_account_profile().id.is_empty());
    assert!(!window.get_account_profile().has_theme);
    adapter.app.state.discord_user = None;
    adapter.refresh(&window);
    assert!(window.get_account_profile().id.is_empty());
    assert!(!window.get_account_profile().has_theme);
}
