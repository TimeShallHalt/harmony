//! Build identity and clipboard actions for About. Release controls are bound in release_updates.
use super::*;

pub(super) fn bind(window: &YapfastWindow) {
    window
        .global::<HarmonyAbout>()
        .set_running_info(crate::build_info::BuildInfo::current().diagnostics().into());
    let weak = window.as_weak();
    window
        .global::<HarmonyAbout>()
        .on_copy_diagnostics(move || {
            if let Some(window) = weak.upgrade() {
                window
                    .invoke_copy_text(crate::build_info::BuildInfo::current().diagnostics().into());
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_adapter_about_reports_running_build_and_copies_only_public_diagnostics() {
        i_slint_backend_testing::init_no_event_loop();
        let dir = tempfile::tempdir().unwrap();
        let window = YapfastWindow::new().unwrap();
        let adapter = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            dir.path().into(),
            &window,
        )
        .unwrap();
        bind_callbacks(&window, &adapter);
        let expected = crate::build_info::BuildInfo::current().diagnostics();
        assert_eq!(window.global::<HarmonyAbout>().get_running_info(), expected);
        assert!(expected.contains("Updates: Harmony public releases with publisher-signed Windows installers."));
        assert!(!expected.contains(&dir.path().display().to_string()));
        window.global::<HarmonyAbout>().invoke_copy_diagnostics();
        let copied = i_slint_core::window::WindowInner::from_pub(window.window())
            .context()
            .platform()
            .clipboard_text(i_slint_core::platform::Clipboard::DefaultClipboard);
        assert_eq!(copied.as_deref(), Some(expected.as_str()));
        drop(window);
        let restored = YapfastWindow::new().unwrap();
        bind_callbacks(&restored, &adapter);
        assert_eq!(
            restored.global::<HarmonyAbout>().get_running_info(),
            expected
        );
    }

    #[test]
    fn obsolete_update_consent_is_ignored_not_reenabled_or_preserved() {
        let layout: Layout = serde_json::from_str(r#"{"harmony_updates":{"consent_version":1,"check_on_startup":true,"download_automatically":true,"check_interval_hours":1}}"#).unwrap();
        let saved = serde_json::to_value(layout).unwrap();
        assert!(saved.get("harmony_updates").is_none());
        let upstream: Layout = serde_json::from_str(
            r#"{"check_for_updates":true,"update_nightlies":true,"skipped_update":"v99.0.0","announced_update":"v99.0.0"}"#,
        ).unwrap();
        let saved = serde_json::to_value(upstream).unwrap();
        for key in [
            "check_for_updates",
            "update_nightlies",
            "skipped_update",
            "announced_update",
        ] {
            assert!(
                saved.get(key).is_none(),
                "{key} must not revive stock updating"
            );
        }
    }
}
