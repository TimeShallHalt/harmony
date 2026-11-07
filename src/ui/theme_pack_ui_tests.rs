//! Native callback regressions. Included from ui.rs; no credentials/network required.
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
    bind_callbacks(&window, &adapter);
    (dir, window, adapter)
}

#[test]
fn pack_cancel_restores_bonfire_when_no_theme_was_explicitly_selected() {
    let (_dir, window, _adapter) = fixture();
    let before = theme::Theme::shown(&window);
    assert_eq!(window.get_theme_id(), "bonfire");
    window.invoke_new_theme();
    window.invoke_theme_colour_edited(0, "#123456".into());
    window.invoke_cancel_theme();
    assert_eq!(theme::Theme::shown(&window), before);
    assert_eq!(window.get_theme_id(), "bonfire");
}

#[test]
fn pack_import_restore_edit_export_reset_is_one_native_vertical_slice() {
    let (dir, window, adapter) = fixture();
    let file = dir.path().join("test.harmony.json");
    std::fs::write(&file, br##"{"version":1,"name":"Orchid","base":"nord","colors":{"accent":"#cc88ff","mentioned":"#cc88ff20"},"layout":{"title-height":42,"sidebar-width":270,"profile-width":350},"typography":{"font-family":"DM Sans","text-size":17},"motion":{"transition-ms":75,"reduced-motion":true}}"##).unwrap();
    adapter
        .borrow_mut()
        .import_theme_pack(&window, &file)
        .unwrap();
    assert_eq!(window.get_theme_id(), theme::PACK_ID);
    assert_eq!(window.global::<ClientStyle>().get_title_height(), 42.0);
    assert_eq!(window.get_sidebar_width(), 270.0);
    assert_eq!(window.global::<ClientStyle>().get_text_size(), 17.0);
    assert_eq!(window.global::<yapfast_ui::Theme>().get_quick(), 0);
    let imported = theme::Theme::shown(&window);
    assert_eq!(theme::hex(imported.accent), "#cc88ff");
    window.invoke_new_theme();
    assert_eq!(
        window.get_theme_start(),
        4,
        "a copied pack names its Nord base"
    );
    assert_eq!(window.get_theme_name(), "Orchid copy");
    assert_eq!(theme::Theme::shown(&window), imported);
    window.invoke_cancel_theme();
    // Cancelling a live palette edit keeps the pack and every style token.
    window.invoke_edit_theme_pack();
    window.invoke_theme_colour_edited(0, "#012345".into());
    window.invoke_cancel_theme();
    assert_eq!(theme::Theme::shown(&window), imported);
    assert_eq!(window.global::<ClientStyle>().get_title_height(), 42.0);
    window.invoke_edit_theme_pack();
    window.invoke_theme_colour_edited(0, "#012345".into());
    window.invoke_save_theme();
    assert_eq!(theme::hex(theme::Theme::shown(&window).rail), "#012345");
    assert_eq!(window.get_theme_id(), theme::PACK_ID);
    let out = dir.path().join("export.harmony.json");
    adapter.borrow().export_theme_pack(&window, &out).unwrap();
    let exported = theme_pack::Applied::load(&out).unwrap();
    assert_eq!(exported.pack.colors["rail"], "#012345");
    assert_eq!(exported.pack.layout.title_height, Some(42.0));
    // Startup uses the persisted snapshot, even after deleting the chosen pack.
    std::fs::remove_file(&file).unwrap();
    let saved = Layout::read(dir.path());
    let again = YapfastWindow::new().unwrap();
    saved.apply_appearance(&again);
    assert_eq!(theme::Theme::shown(&again), theme::Theme::shown(&window));
    assert_eq!(again.global::<ClientStyle>().get_title_height(), 42.0);
    // A failed explicit reload keeps exactly what is on screen and on disk.
    let before = adapter.borrow().layout.clone();
    window.invoke_reload_theme_pack();
    assert!(adapter.borrow().layout == before);
    assert_eq!(theme::Theme::shown(&window), theme::Theme::shown(&again));
    assert!(!window.get_theme_pack_status().is_empty());
    // Choosing a built-in clears every pack input, not enabled/native defaults.
    window.invoke_choose_theme("sakura".into());
    assert!(adapter.borrow().layout.theme_pack.is_none());
    assert_eq!(window.global::<ClientStyle>().get_title_height(), 32.0);
    assert_eq!(window.global::<ClientStyle>().get_profile_width(), 320.0);
    assert_eq!(window.global::<ClientStyle>().get_pack_text_size(), -1.0);
    assert!(!window.global::<yapfast_ui::Theme>().get_reduced_motion());
    assert_eq!(
        window.global::<yapfast_ui::Theme>().get_transition_ms(),
        120
    );
    assert_eq!(window.get_sidebar_width(), 304.0);
}

#[test]
fn pack_logo_reset_reload_and_pack_reset_keep_native_defaults() {
    let (dir, window, adapter) = fixture();
    let file = dir.path().join("logo.harmony.json");
    std::fs::write(
        dir.path().join("logo.png"),
        include_bytes!("../../examples/themes/logos/orchid.png"),
    )
    .unwrap();
    std::fs::write(&file, br#"{"version":1,"name":"Logo","base":"nord","logo":"logo.png","layout":{"title-height":40},"typography":{"font-family":"Local Missing Font"}}"#).unwrap();
    adapter
        .borrow_mut()
        .import_theme_pack(&window, &file)
        .unwrap();
    assert!(window.get_theme_pack_has_logo());
    assert_eq!(
        window.global::<ClientStyle>().get_title_logo().size().width,
        128
    );
    window.invoke_reset_theme_logo();
    assert!(!window.get_theme_pack_has_logo());
    assert_eq!(
        window.global::<ClientStyle>().get_title_logo().size().width,
        0
    );
    assert_eq!(window.global::<ClientStyle>().get_title_height(), 40.0);
    assert!(
        Layout::read(dir.path())
            .theme_pack
            .unwrap()
            .pack
            .logo
            .is_none()
    );
    window.invoke_reload_theme_pack();
    assert!(window.get_theme_pack_has_logo());
    assert_eq!(
        window.global::<ClientStyle>().get_title_logo().size().width,
        128
    );
    window.invoke_reset_theme_pack();
    assert_eq!(window.get_theme_id(), "nord");
    assert_eq!(window.global::<ClientStyle>().get_title_height(), 32.0);
    assert_eq!(window.global::<ClientStyle>().get_font_family(), "DM Sans");
    assert_eq!(
        window.global::<ClientStyle>().get_title_logo().size().width,
        0
    );
    assert!(Layout::read(dir.path()).theme_pack.is_none());
}

#[test]
fn pack_keeps_explicit_sidebar_and_message_text_size_choices() {
    let (dir, window, adapter) = fixture();
    adapter.borrow_mut().layout.sidebar_width = Some(330.0);
    adapter.borrow_mut().layout.text_size = Some(2);
    window.set_text_size(2);
    let file = dir.path().join("test.harmony.json");
    std::fs::write(&file, br#"{"version":1,"name":"Precedence","base":"bonfire","layout":{"sidebar-width":200},"typography":{"text-size":12}}"#).unwrap();
    adapter
        .borrow_mut()
        .import_theme_pack(&window, &file)
        .unwrap();
    assert_eq!(window.get_sidebar_width(), 330.0);
    assert_eq!(window.get_message_font_size(), 18.0);
    window.invoke_reset_theme_pack();
    assert_eq!(window.get_sidebar_width(), 330.0);
    assert_eq!(window.get_message_font_size(), 18.0);
}

#[test]
fn pack_validation_and_persistence_failures_never_change_visible_appearance() {
    let (dir, window, adapter) = fixture();
    let file = dir.path().join("bad.harmony.json");
    std::fs::write(&file, br#"{"version":99,"name":"Bad","base":"bonfire"}"#).unwrap();
    let before = theme::Theme::shown(&window);
    let layout = adapter.borrow().layout.clone();
    assert!(
        adapter
            .borrow_mut()
            .import_theme_pack(&window, &file)
            .is_err()
    );
    assert!(adapter.borrow().layout == layout);
    assert_eq!(theme::Theme::shown(&window), before);
    std::fs::write(&file, br#"{"version":1,"name":"Valid","base":"sakura"}"#).unwrap();
    // A directory at the exact destination is a portable persistence failure.
    let layout_file = dir.path().join(LAYOUT);
    if layout_file.exists() {
        std::fs::remove_file(&layout_file).unwrap();
    }
    std::fs::create_dir(&layout_file).unwrap();
    assert!(
        adapter
            .borrow_mut()
            .import_theme_pack(&window, &file)
            .is_err()
    );
    assert!(adapter.borrow().layout == layout);
    assert_eq!(theme::Theme::shown(&window), before);
}
