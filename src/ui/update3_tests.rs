//! Update3: real Application/Adapter, in-memory Slint and owned empty storage only.
use super::*;

fn fixture() -> (tempfile::TempDir, YapfastWindow, Rc<RefCell<Adapter>>) {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let window = YapfastWindow::new().unwrap();
    let adapter = Adapter::new(
        Arc::new(BackgroundRuntime::new().unwrap()),
        dir.path().into(),
        &window,
    )
    .unwrap();
    // Suppress generated call effects before applying any events: no audio worker/device.
    adapter.borrow_mut().layout.sound_volume = Some(0);
    bind_callbacks(&window, &adapter);
    (dir, window, adapter)
}

fn voice(room: Option<&str>) -> voice::State {
    voice::State {
        room: room.map(Into::into),
        phase: if room.is_some() {
            voice::Phase::Connected
        } else {
            voice::Phase::Disconnected
        },
        ..Default::default()
    }
}

#[test]
fn call_origin_is_observed_before_first_render_and_survives_recreation() {
    let (_dir, window, adapter) = fixture();
    adapter
        .borrow_mut()
        .apply(Event::Voice(voice(Some("discord:10:11"))));
    std::thread::sleep(Duration::from_millis(1050));
    adapter.borrow_mut().refresh(&window);
    assert_ne!(
        window.get_voice_time(),
        "0:00",
        "rendering must not create the call origin"
    );
    assert!(!window.get_voice_time().is_empty());
    adapter.borrow_mut().detach();
    let replacement = YapfastWindow::new().unwrap();
    adapter.borrow_mut().window = replacement.as_weak();
    adapter.borrow_mut().refresh(&replacement);
    assert_ne!(replacement.get_voice_time(), "0:00");
    assert!(
        !replacement.get_voice_time().is_empty(),
        "new window must attach to the existing origin"
    );
    adapter.borrow_mut().apply(Event::Voice(voice(None)));
    adapter.borrow_mut().refresh(&replacement);
    assert_eq!(replacement.get_voice_time(), "");
}

#[test]
fn invalid_volume_callback_does_not_mutate_either_persisted_gain() {
    let (dir, window, _adapter) = fixture();
    let person = "update3-volume-invalid";
    window.invoke_volume_changed(person.into(), false, 137, true);
    window.invoke_volume_changed(person.into(), true, 176, true);
    let before = Layout::read(dir.path());
    for invalid in [-1, i32::MIN] {
        window.invoke_volume_changed(person.into(), false, invalid, true);
        window.invoke_volume_changed(person.into(), true, invalid, true);
    }
    window.invoke_volume_changed("".into(), false, 650, true);
    window.invoke_volume_changed("".into(), true, 650, true);
    let after = Layout::read(dir.path());
    assert_eq!(
        after.volumes, before.volumes,
        "invalid callback must not persist a clamped gain"
    );
    assert_eq!(after.stream_volumes, before.stream_volumes);
}
