//! Native accessibility, animation and overlay actions; testing backend only.
#![allow(deprecated)]
use i_slint_core::{
    accessibility::{AccessibilityAction, AccessibleStringProperty},
    item_tree::ItemRc,
};
use slint::platform::WindowEvent;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

slint::slint! {
    import { ProfileCard, ProfileModal, ProfileData } from "../../ui/people.slint";
    import { ImageViewer } from "../../ui/messages.slint";
    import { ProfileMediaReturnKeys } from "../../ui/profile-media.slint";
    export component CardHarness inherits Window {
        preferred-width: 800px; preferred-height: 600px;
        in-out property <ProfileData> data;
        in-out property <bool> preview;
        in-out property <bool> modal;
        out property <int> opened;
        out property <int> kind: -1;
        if !root.modal: ProfileCard {
            x: 0px; y: 0px; width: 320px; data: root.data; preview: root.preview;
            view-media(kind) => { root.opened += 1; root.kind = kind; }
        }
        if root.modal: ProfileModal {
            x: 0px; y: 0px; width: parent.width; height: parent.height; data: root.data;
            view-media(kind) => { root.opened += 1; root.kind = kind; }
        }
    }
    export component ViewerFlow inherits Window {
        preferred-width: 800px; preferred-height: 600px;
        in-out property <bool> viewer-open: true;
        in-out property <bool> save-ready;
        in-out property <string> note: "Loading fixture";
        in-out property <string> source-key: "profile:1";
        in-out property <image> picture;
        in-out property <[image]> frames;
        in-out property <[int]> delays;
        out property <int> saved;
        out property <int> message-saved;
        out property <int> stepped;
        out property <int> profile-closed;
        out property <string> opened;
        out property <float> zoom: viewer.zoom;
        out property <image> displayed: viewer.displayed-frame;
        out property <string> status: viewer.status;
        ProfileMediaReturnKeys {
            viewer-open: root.viewer-open;
            close-profile => { root.profile-closed += 1; }
        }
        viewer := ImageViewer {
            visible: root.viewer-open;
            x: 0px; y: 0px; width: parent.width; height: parent.height;
            profile-media: true; media-link: "https://cdn.discordapp.com/avatars/200/fixture.png?size=4096";
            source-key: root.source-key; save-ready: root.save-ready;
            viewer-image: root.picture; viewer-frames: root.frames; viewer-delays: root.delays;
            viewer-name: "avatar-200.png"; viewer-note: root.note;
            close-image => { root.viewer-open = false; }
            save-profile-media => { root.saved += 1; }
            save-attachment(item, index) => { root.message-saved += 1; }
            open-link(link) => { root.opened = link; }
            step-image(step) => { root.stepped += step; }
        }
    }
}
fn init() {
    i_slint_backend_testing::init_no_event_loop();
}
fn settle() {
    for _ in 0..5 {
        slint::platform::update_timers_and_animations();
        i_slint_core::properties::ChangeTracker::run_change_handlers();
    }
}
fn show(ui: &impl ComponentHandle) {
    ui.show().unwrap();
    let size = slint::LogicalSize::new(800., 600.);
    ui.window().set_size(size);
    ui.window().dispatch_event(WindowEvent::Resized { size });
    settle();
}
fn image() -> slint::Image {
    slint::Image::from_rgba8(slint::SharedPixelBuffer::new(8, 8))
}
fn data() -> ProfileData {
    ProfileData {
        id: "200".into(),
        name: "Fixture".into(),
        avatar_media: true,
        banner_media: true,
        picture: image(),
        banner: image(),
        ..Default::default()
    }
}
fn card() -> CardHarness {
    init();
    let ui = CardHarness::new().unwrap();
    ui.set_data(data());
    show(&ui);
    ui
}
fn target(ui: &impl ComponentHandle, label: &str) -> Option<ItemRc> {
    let root = ItemRc::new(
        i_slint_core::window::WindowInner::from_pub(ui.window()).component(),
        0,
    );
    let mut found = None;
    root.visit_descendants(|item| {
        if item.is_accessible()
            && item
                .accessible_string_property(AccessibleStringProperty::Label)
                .as_deref()
                == Some(label)
        {
            found = Some(item.clone());
            return std::ops::ControlFlow::Break(());
        }
        std::ops::ControlFlow::Continue(())
    });
    found
}
fn activate(ui: &impl ComponentHandle, label: &str) {
    let item = target(ui, label).unwrap_or_else(|| panic!("missing accessible target: {label}"));
    assert_eq!(
        item.accessible_role(),
        i_slint_core::items::AccessibleRole::Button
    );
    item.accessible_action(&AccessibilityAction::Default);
    settle();
}
fn key(ui: &impl ComponentHandle, text: impl Into<SharedString>) {
    let text = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
    settle();
}
#[test]
fn a_pointer_press_from_a_stale_profile_cannot_open_the_replacement_profiles_media() {
    let ui = card();
    let position = slint::LogicalPosition::new(55., 105.);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    let mut changed = data();
    changed.id = "201".into();
    changed.name = "Replacement".into();
    ui.set_data(changed);
    settle();
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    settle();
    assert_eq!(
        ui.get_opened(),
        0,
        "release must not retarget a press to another person"
    );
}

#[test]
fn avatar_and_banner_expose_native_accessible_actions() {
    let ui = card();
    activate(&ui, "View Fixture's avatar");
    assert_eq!(ui.get_kind(), 0);
    activate(&ui, "View Fixture's banner");
    assert_eq!(ui.get_kind(), 1);
    assert_eq!(ui.get_opened(), 2);
}
#[test]
fn banner_only_and_missing_assets_have_no_phantom_avatar_target() {
    let ui = card();
    let mut data = data();
    data.avatar_media = false;
    data.picture = Default::default();
    ui.set_data(data.clone());
    settle();
    assert!(target(&ui, "View Fixture's avatar").is_none());
    activate(&ui, "View Fixture's banner");
    assert_eq!(ui.get_kind(), 1);
    data.banner_media = false;
    data.banner = Default::default();
    ui.set_data(data);
    settle();
    assert!(target(&ui, "View Fixture's banner").is_none());
}
#[test]
fn editor_previews_never_offer_a_media_fetch() {
    let ui = card();
    ui.set_preview(true);
    settle();
    assert!(target(&ui, "View Fixture's avatar").is_none());
    assert!(target(&ui, "View Fixture's banner").is_none());
    assert_eq!(ui.get_opened(), 0);
}
#[test]
fn full_profile_propagates_both_media_callbacks() {
    let ui = card();
    ui.set_modal(true);
    settle();
    activate(&ui, "View Fixture's avatar");
    assert_eq!(ui.get_kind(), 0);
    activate(&ui, "View Fixture's banner");
    assert_eq!(ui.get_kind(), 1);
    assert_eq!(ui.get_opened(), 2);
}
#[test]
fn loading_or_error_opens_original_but_never_saves_a_message() {
    init();
    let ui = ViewerFlow::new().unwrap();
    show(&ui);
    let save = target(&ui, "Save avatar-200.png").unwrap();
    assert_eq!(
        save.accessible_string_property(AccessibleStringProperty::Enabled)
            .as_deref(),
        Some("false")
    );
    save.accessible_action(&AccessibilityAction::Default);
    assert_eq!(ui.get_saved(), 0);
    activate(&ui, "Open original");
    assert!(ui.get_opened().ends_with("size=4096"));
    ui.set_note("Decoder error fixture".into());
    ui.set_save_ready(true);
    settle();
    activate(&ui, "Save avatar-200.png");
    assert_eq!(ui.get_saved(), 1);
    assert_eq!(ui.get_message_saved(), 0);
    key(&ui, slint::platform::Key::RightArrow);
    assert_eq!(ui.get_stepped(), 0);
}
#[test]
fn escape_returns_to_profile_before_closing_the_profile() {
    init();
    let ui = ViewerFlow::new().unwrap();
    show(&ui);
    key(&ui, slint::platform::Key::Escape);
    assert!(!ui.get_viewer_open());
    assert_eq!(ui.get_profile_closed(), 0);
    key(&ui, slint::platform::Key::Escape);
    assert_eq!(ui.get_profile_closed(), 1);
}
#[test]
fn native_animation_and_source_switch_share_the_existing_viewer() {
    init();
    let ui = ViewerFlow::new().unwrap();
    let first = image();
    let second = image();
    ui.set_picture(first.clone());
    ui.set_frames(ModelRc::new(VecModel::from(vec![
        first.clone(),
        second.clone(),
    ])));
    ui.set_delays(ModelRc::new(VecModel::from(vec![40, 80])));
    show(&ui);
    assert_eq!(ui.get_displayed(), first);
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
    settle();
    assert_eq!(ui.get_displayed(), second);
    key(&ui, "+");
    assert!(ui.get_zoom() > 1.);
    ui.set_source_key("profile:2".into());
    settle();
    assert_eq!(ui.get_zoom(), 1.);
    assert!(ui.get_status().contains("Decoded 8 × 8 px"));
}
