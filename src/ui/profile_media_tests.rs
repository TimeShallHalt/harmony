//! No OS windows/input: the testing backend exercises the actual profile card.
#![allow(deprecated)]
#[path = "profile_media_flow_tests.rs"]
mod flow;
use slint::{ComponentHandle, SharedString};

slint::slint! {
    import { ProfileCard, ProfileData } from "../../ui/people.slint";
    export component ProfileMediaHarness inherits Window {
        preferred-width: 320px;
        preferred-height: 600px;
        in-out property <ProfileData> data;
        out property <int> enlarged;
        out property <int> media-opened;
        out property <int> media-kind: -1;
        out property <length> card-y: card.absolute-position.y;
        out property <length> card-height: card.height;
        card := ProfileCard {
            x: 0px; y: 0px;
            width: 320px;
            data: root.data;
            enlarge => { root.enlarged += 1; }
            view-media(kind) => { root.media-opened += 1; root.media-kind = kind; }
        }
    }
}

fn card() -> ProfileMediaHarness {
    i_slint_backend_testing::init_no_event_loop();
    let ui = ProfileMediaHarness::new().unwrap();
    ui.set_data(ProfileData {
        id: SharedString::from("200"),
        name: SharedString::from("Fixture"),
        avatar_media: true,
        picture: slint::Image::from_rgba8(slint::SharedPixelBuffer::new(8, 8)),
        ..Default::default()
    });
    ui.show().unwrap();
    let size = slint::LogicalSize::new(320., 600.);
    ui.window().set_size(size);
    ui.window()
        .dispatch_event(slint::platform::WindowEvent::Resized { size });
    for _ in 0..5 {
        slint::platform::update_timers_and_animations();
        i_slint_core::properties::ChangeTracker::run_change_handlers();
    }
    ui
}

#[test]
fn the_profile_avatar_is_media_not_a_second_open_profile_action() {
    let ui = card();
    let position = slint::LogicalPosition::new(55., 105.);
    for event in [
        slint::platform::WindowEvent::PointerMoved { position },
        slint::platform::WindowEvent::PointerPressed {
            position,
            button: slint::platform::PointerEventButton::Left,
        },
        slint::platform::WindowEvent::PointerReleased {
            position,
            button: slint::platform::PointerEventButton::Left,
        },
    ] {
        ui.window().dispatch_event(event);
    }
    assert_eq!(
        ui.get_enlarged(),
        0,
        "the avatar inside an already-open profile must open media"
    );
    assert_eq!(ui.get_media_opened(), 1);
    assert_eq!(ui.get_media_kind(), 0);
    for text in [" ", "\n"] {
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: text.into() });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: text.into() });
    }
    assert_eq!(
        ui.get_media_opened(),
        3,
        "the native target handles Space and Enter"
    );
}
