//! Update4 uses the production owner, window and in-memory backend only.
use super::*;

#[test]
fn reduced_motion_stops_actual_adapter_animation_policy() {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let adapter = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    adapter.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&window, &adapter);
    ANIMATE.set(Animate::Always);
    window.global::<Theme>().set_reduced_motion(false);
    show_motion(&window);
    assert!(window.global::<Motion>().get_playing());
    window.global::<Theme>().set_reduced_motion(true);
    show_motion(&window);
    assert!(
        !window.global::<Motion>().get_playing(),
        "reduced motion overrides Always in the real window"
    );
    window.global::<Theme>().set_reduced_motion(false);
    ANIMATE.set(Animate::Never);
    show_motion(&window);
    assert!(!window.global::<Motion>().get_playing());
    ANIMATE.set(Animate::Focused);
    FOCUSED.set(false);
    show_motion(&window);
    assert!(!window.global::<Motion>().get_playing());
    FOCUSED.set(true);
    show_motion(&window);
    assert!(window.global::<Motion>().get_playing());
    // Received theme preference must take effect without a refresh or polling.
    window.global::<Theme>().set_reduced_motion(true);
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    assert!(
        !window.global::<Motion>().get_playing(),
        "preference change is event driven"
    );
}
