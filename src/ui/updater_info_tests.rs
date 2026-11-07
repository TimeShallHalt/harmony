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
#[ignore = "separate testing event loop with owned saved-journal fixtures; use runner"]
fn harmony_normal_startup_exposes_safe_applying_recovery_in_native_ui() {
    use crate::harmony_update::{
        BuildInfo,
        controller::Controller,
        release::{Artifact, Manifest},
        transaction::{Phase, Plan},
    };
    use sha2::{Digest, Sha256};
    i_slint_backend_testing::init_integration_test_with_system_time();
    let pump = || {
        slint::Timer::single_shot(std::time::Duration::from_millis(300), || {
            let _ = slint::quit_event_loop();
        });
        slint::run_event_loop().unwrap();
    };
    for published in [false, true] {
        // Saved-journal fixture, not fabricated service data. Native module
        // tests separately produce these states by actual owned-process cuts.
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("harmony.exe");
        let txn = dir.path().join(".harmony-updates/txn-ui-cut");
        std::fs::create_dir_all(&txn).unwrap();
        let old = b"old owned UI fixture";
        let mut payload = vec![0u8; 128];
        payload[..2].copy_from_slice(b"MZ");
        payload[60..64].copy_from_slice(&64u32.to_le_bytes());
        payload[64..68].copy_from_slice(b"PE\0\0");
        payload[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        let artifact = Artifact {
            platform: BuildInfo::current().platform_key(),
            name: "Harmony-1.0.1-windows-x86_64.exe".into(),
            size: payload.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&payload)),
            format: "exe".into(),
        };
        let plan = Plan {
            schema: 1,
            target: "harmony.exe".into(),
            previous_sha256: format!("{:x}", Sha256::digest(old)),
            manifest: Manifest {
                schema: 1,
                repository: "TimeShallHalt/harmony".into(),
                tag: "harmony-v1.0.1-b1".into(),
                version: "1.0.1".into(),
                build_id: 1,
                commit: "0123456789abcdef0123456789abcdef01234567".into(),
                artifacts: vec![artifact],
            },
            armed: true,
            phase: Phase::Applying,
        };
        std::fs::write(&install, if published { &payload } else { old.as_slice() }).unwrap();
        std::fs::write(txn.join("rollback"), old).unwrap();
        if !published {
            std::fs::write(txn.join("payload"), &payload).unwrap();
        }
        std::fs::write(txn.join("plan.json"), serde_json::to_vec(&plan).unwrap()).unwrap();
        std::fs::write(
            dir.path().join(".harmony-updates/pending.json"),
            br#""txn-ui-cut""#,
        )
        .unwrap();
        let window = YapfastWindow::new().unwrap();
        let state = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            dir.path().into(),
            &window,
        )
        .unwrap();
        state.borrow_mut().harmony_updates = Arc::new(Controller::new(Ok(install.clone())));
        bind_callbacks(&window, &state);
        updater_info::startup(&window, &state, false);
        pump();
        let ui = window.global::<HarmonyUpdates>();
        if published {
            assert!(!ui.get_staged());
            assert_eq!(
                crate::harmony_update::transaction::pending(&install)
                    .unwrap()
                    .unwrap()
                    .1
                    .phase,
                Phase::Installed
            );
        } else {
            assert!(ui.get_staged());
            assert!(ui.get_may_arm());
            assert!(!ui.get_armed());
            assert!(!ui.get_may_discard());
            ui.invoke_arm(true);
            pump();
            assert!(ui.get_armed());
            assert_eq!(
                crate::harmony_update::transaction::pending(&install)
                    .unwrap()
                    .unwrap()
                    .1
                    .phase,
                Phase::Applying,
                "no blind journal reset"
            );
            assert_eq!(
                std::fs::read(&install).unwrap(),
                old,
                "UI authorization cannot replace a running image"
            );
            ui.invoke_arm(false);
            pump();
            assert!(!ui.get_armed());
        }
    }
}
#[test]
#[ignore = "requires runner-owned gh PATH and a separate testing event loop"]
fn harmony_normal_startup_requires_fresh_and_migrated_consent_before_gh() {
    i_slint_backend_testing::init_integration_test_with_system_time();
    let log = PathBuf::from(
        std::env::var_os("HARMONY_FIXTURE_GH_LOG").expect("runner-owned log and gh PATH required"),
    );
    let count = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .count()
    };
    let pump = || {
        slint::Timer::single_shot(std::time::Duration::from_millis(400), || {
            let _ = slint::quit_event_loop();
        });
        slint::run_event_loop().unwrap();
    };
    for saved in [
        "{}",
        r#"{"check_on_startup":true,"download_automatically":true}"#,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let window = YapfastWindow::new().unwrap();
        let state = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            dir.path().into(),
            &window,
        )
        .unwrap();
        state.borrow_mut().layout.harmony_updates = serde_json::from_str(saved).unwrap();
        bind_callbacks(&window, &state);
        let before = count();
        updater_info::startup(&window, &state, false);
        pump();
        assert_eq!(
            count(),
            before,
            "normal fresh/migrated startup invoked gh without consent"
        );
        assert!(!window.global::<HarmonyUpdates>().get_check_on_startup());
        assert!(
            !window
                .global::<HarmonyUpdates>()
                .get_download_automatically()
        );
        window.global::<HarmonyUpdates>().invoke_check();
        pump();
        assert_eq!(
            count(),
            before + 2,
            "manual check still runs fixture gh auth/api without auto consent"
        );
        window
            .global::<HarmonyUpdates>()
            .invoke_preferences_changed(true, false);
        state.borrow_mut().layout.harmony_updates.last_check_unix = None;
        updater_info::startup(&window, &state, false);
        pump();
        assert_eq!(count(), before + 4, "explicit opt-in enables startup check");
    }
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
