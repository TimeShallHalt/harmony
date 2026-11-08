//! Integrated adapter regressions for the parent Cargo test run.
//! Temporary local preferences only; no backend, account, audio device or input.
use super::*;

#[test]
fn local_timezone_adapter_persists_scopes_rejects_stale_edits_and_rolls_back_storage_errors() {
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let adapter = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        directory.path().into(),
        &window,
    )
    .unwrap();
    bind_callbacks(&window, &adapter);
    {
        let mut a = adapter.borrow_mut();
        a.app.state.discord_user = Some("100".into());
        a.app.state.profile = Some(ProfileView {
            profile: yapfast_core::chat::Profile {
                id: "200".into(),
                ..Default::default()
            },
            large: false,
            loading: false,
            kept: false,
            note: None,
            platform_presence: None,
        });
        window.set_profile(ProfileData {
            id: "200".into(),
            ..Default::default()
        });
        a.tick_timezones(&window);
    }
    assert!(window.get_profile().local_time.contains("not set"));
    assert_eq!(
        window.global::<LocalTimezones>().invoke_save(
            "discord:100".into(),
            "200".into(),
            "America/New_York".into()
        ),
        ""
    );
    assert_eq!(
        Layout::read(directory.path())
            .local_timezones
            .get("discord:100", "200"),
        Some("America/New_York")
    );
    assert!(window.get_profile().local_time.contains("America/New_York"));
    assert!(adapter.borrow().timezone_clock.running());
    adapter.borrow_mut().app.state.discord_user = Some("101".into());
    adapter.borrow_mut().tick_timezones(&window);
    assert!(window.get_profile().local_time.contains("not set"));
    assert!(
        !window
            .global::<LocalTimezones>()
            .invoke_save("discord:100".into(), "200".into(), "UTC".into())
            .is_empty()
    );
    adapter.borrow_mut().app.state.discord_user = Some("100".into());
    let blocked = directory.path().join("not-a-directory");
    std::fs::write(&blocked, b"fixture").unwrap();
    adapter.borrow_mut().root = blocked;
    let before = adapter.borrow().layout.clone();
    assert!(
        !window
            .global::<LocalTimezones>()
            .invoke_save("discord:100".into(), "200".into(), "UTC".into())
            .is_empty()
    );
    assert!(adapter.borrow().layout == before);
    adapter.borrow_mut().root = directory.path().into();
    assert_eq!(
        window
            .global::<LocalTimezones>()
            .invoke_reset("discord:100".into(), "200".into()),
        ""
    );
    assert_eq!(
        Layout::read(directory.path())
            .local_timezones
            .get("discord:100", "200"),
        None
    );
    assert!(!adapter.borrow().timezone_clock.running());
}
