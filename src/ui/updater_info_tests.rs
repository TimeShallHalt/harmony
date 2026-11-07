//! Production native callbacks, account-free. The parent owns full app builds.
use super::*;
fn fixture() -> (tempfile::TempDir, YapfastWindow, Rc<RefCell<Adapter>>) {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let state = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    bind_callbacks(&window, &state);
    (dir, window, state)
}
#[test]
fn harmony_about_uses_running_build_and_copies_only_allowlisted_diagnostics() {
    let (_dir, window, _state) = fixture();
    window.set_settings_section(9);
    window.set_settings_open(true);
    let expected = crate::harmony_update::BuildInfo::current().diagnostics();
    assert_eq!(
        window.global::<HarmonyUpdates>().get_running_info(),
        expected
    );
    window.global::<HarmonyUpdates>().invoke_copy_diagnostics();
    let copied = i_slint_core::window::WindowInner::from_pub(window.window())
        .context()
        .platform()
        .clipboard_text(i_slint_core::platform::Clipboard::DefaultClipboard);
    assert_eq!(copied.as_deref(), Some(expected.as_str()));
    assert!(!expected.contains("data-dir"));
    assert!(expected.contains("GPL-3.0-or-later"));
}
#[test]
fn harmony_update_preferences_commit_to_real_layout_and_restore_on_new_adapter() {
    let (dir, window, state) = fixture();
    window
        .global::<HarmonyUpdates>()
        .invoke_preferences_changed(false, true);
    let bytes = std::fs::read(dir.path().join(LAYOUT)).unwrap();
    let layout: Layout = serde_json::from_slice(&bytes).unwrap();
    assert!(!layout.harmony_updates.check_on_startup);
    assert!(layout.harmony_updates.download_automatically);
    assert!(!state.borrow().layout.harmony_updates.check_on_startup);
    let again = YapfastWindow::new().unwrap();
    let restored = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &again,
    )
    .unwrap();
    bind_callbacks(&again, &restored);
    assert!(!again.global::<HarmonyUpdates>().get_check_on_startup());
    assert!(
        again
            .global::<HarmonyUpdates>()
            .get_download_automatically()
    );
}
#[test]
fn harmony_preview_startup_never_checks_private_releases_or_loads_install_storage() {
    let (_dir, window, state) = fixture();
    window.set_preview(true);
    updater_info::startup(&window, &state, true);
    window.global::<HarmonyUpdates>().invoke_download();
    window.global::<HarmonyUpdates>().invoke_arm(true);
    window.global::<HarmonyUpdates>().invoke_discard();
    let c = state.borrow().harmony_updates.clone();
    assert!(!c.busy());
    assert!(c.snapshot().candidate.is_none());
    assert!(c.snapshot().staged.is_none());
    assert!(c.snapshot().status.contains("Preview"));
    assert!(
        state
            .borrow()
            .layout
            .harmony_updates
            .last_check_unix
            .is_none()
    );
}
