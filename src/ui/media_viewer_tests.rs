//! Native tests of the actual ImageViewer, without adapter/network fixtures.
//! Integration hook in src/ui.rs: #[cfg(test)] mod media_viewer_tests;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

slint::slint! {
    import { ImageViewer } from "../../ui/messages.slint";
    import { Motion } from "../../ui/controls.slint";
    export component MediaViewerHarness inherits Window {
        preferred-width: 1120px;
        preferred-height: 760px;
        in-out property <image> picture;
        in-out property <[image]> frames;
        in-out property <[int]> delays;
        in-out property <int> pixel-width: 1920;
        in-out property <int> pixel-height: 1080;
        in-out property <int> picture-index;
        in-out property <string> picture-item: "image:1";
        in-out property <string> position;
        out property <float> zoom: viewer.zoom;
        out property <float> fit-zoom: viewer.fit-zoom;
        out property <length> pan-x: viewer.pan-x;
        out property <length> pan-y: viewer.pan-y;
        out property <length> pan-limit-x: viewer.pan-limit-x;
        out property <length> pan-limit-y: viewer.pan-limit-y;
        out property <length> canvas-width: viewer.width;
        out property <length> canvas-height: viewer.height;
        out property <string> status: viewer.status;
        out property <image> displayed-frame: viewer.displayed-frame;
        out property <int> closed;
        out property <int> steps;
        callback saved(string, int);
        public function zoom-in() { viewer.zoom-in(); }
        public function zoom-out() { viewer.zoom-out(); }
        public function fit-image() { viewer.fit-image(); }
        public function actual-size() { viewer.actual-size(); }
        public function reset-view() { viewer.reset-view(); }
        public function pan-by(x: length, y: length) { viewer.pan-by(x, y); }
        public function set-playing(playing: bool) { Motion.playing = playing; }
        viewer := ImageViewer {
            width: parent.width;
            height: parent.height;
            viewer-image: root.picture;
            viewer-frames: root.frames;
            viewer-delays: root.delays;
            viewer-width: root.pixel-width;
            viewer-height: root.pixel-height;
            viewer-name: "Landscape.gif";
            viewer-index: root.picture-index;
            viewer-item: root.picture-item;
            viewer-position: root.position;
            close-image => { root.closed += 1; }
            step-image(direction) => { root.steps += direction; }
            save-attachment(item, index) => { root.saved(item, index); }
        }
    }
}

fn make_viewer() -> MediaViewerHarness {
    thread_local! {
        static INITIALIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    INITIALIZED.with(|initialized| {
        if !initialized.replace(true) {
            i_slint_backend_testing::init_no_event_loop();
        }
    });
    let viewer = MediaViewerHarness::new().unwrap();
    viewer.set_picture(frame(1920, 1080));
    viewer.show().unwrap();
    resize(&viewer, 1120., 760.);
    viewer
}

// The test adapter records a requested size, but unlike winit it does not
// dispatch the OS's subsequent resize notification to the Slint view.
fn resize(viewer: &MediaViewerHarness, width: f32, height: f32) {
    let size = slint::LogicalSize::new(width, height);
    viewer.window().set_size(size);
    viewer
        .window()
        .dispatch_event(WindowEvent::Resized { size });
    settle();
    near(viewer.get_canvas_width(), width);
    near(viewer.get_canvas_height(), height);
}

fn frame(width: u32, height: u32) -> slint::Image {
    slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(
        width, height,
    ))
}

fn settle() {
    for _ in 0..5 {
        slint::platform::update_timers_and_animations();
        i_slint_core::properties::ChangeTracker::run_change_handlers();
    }
}

fn tap(viewer: &MediaViewerHarness, text: impl Into<SharedString>) {
    let text = text.into();
    viewer
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    viewer
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text });
    settle();
}

#[track_caller]
fn near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn viewer_zoom_keys_use_the_same_bounded_scale_as_the_controls() {
    let viewer = make_viewer();
    let fit = viewer.get_zoom();
    assert!(fit > 0. && fit < 1.);
    near(fit, viewer.get_fit_zoom());
    tap(&viewer, "+");
    near(viewer.get_zoom(), fit * 1.25);
    tap(&viewer, "-");
    near(viewer.get_zoom(), fit);
    viewer.invoke_actual_size();
    near(viewer.get_zoom(), 1.);
    for _ in 0..40 {
        viewer.invoke_zoom_in();
    }
    near(viewer.get_zoom(), 8.);
    for _ in 0..80 {
        viewer.invoke_zoom_out();
    }
    near(viewer.get_zoom(), 0.1);
    tap(&viewer, "0");
    near(viewer.get_zoom(), fit);
    viewer.invoke_actual_size();
    viewer.invoke_fit_image();
    near(viewer.get_zoom(), fit);
    viewer.invoke_actual_size();
    viewer.invoke_reset_view();
    near(viewer.get_zoom(), fit);
    assert_eq!(viewer.get_closed(), 0);
}

#[test]
fn viewer_pan_clamps_each_axis_and_recenters_when_it_fits() {
    let viewer = make_viewer();
    viewer.invoke_pan_by(5000., -5000.);
    near(viewer.get_pan_x(), 0.);
    near(viewer.get_pan_y(), 0.);
    viewer.invoke_actual_size();
    viewer.invoke_pan_by(5000., -5000.);
    near(viewer.get_pan_x(), viewer.get_pan_limit_x());
    near(viewer.get_pan_y(), -viewer.get_pan_limit_y());
    assert!(viewer.get_pan_limit_x() > 0. && viewer.get_pan_limit_y() > 0.);
    viewer.invoke_pan_by(-10000., 10000.);
    near(viewer.get_pan_x(), -viewer.get_pan_limit_x());
    near(viewer.get_pan_y(), viewer.get_pan_limit_y());
    resize(&viewer, 4000., 3000.);
    near(viewer.get_pan_limit_x(), 0.);
    near(viewer.get_pan_limit_y(), 0.);
    near(viewer.get_pan_x(), 0.);
    near(viewer.get_pan_y(), 0.);
    resize(&viewer, 1120., 760.);
    settle();
    viewer.invoke_pan_by(200., -100.);
    viewer.invoke_reset_view();
    near(viewer.get_pan_x(), 0.);
    near(viewer.get_pan_y(), 0.);
    near(viewer.get_zoom(), viewer.get_fit_zoom());
}

#[test]
fn viewer_pointer_drags_pan_without_dismissing_the_image() {
    use slint::platform::PointerEventButton;
    let viewer = make_viewer();
    viewer.invoke_actual_size();
    let at = |x, y| slint::LogicalPosition::new(x, y);
    let start = at(560., 360.);
    let end = at(700., 450.);
    let button = PointerEventButton::Left;
    viewer
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position: start });
    viewer.window().dispatch_event(WindowEvent::PointerPressed {
        position: start,
        button,
    });
    viewer
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position: end });
    viewer
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: end,
            button,
        });
    settle();
    near(viewer.get_pan_x(), 140.);
    near(viewer.get_pan_y(), 90.);
    assert_eq!(viewer.get_closed(), 0);
}

#[test]
fn viewer_actual_size_reports_and_uses_the_decoded_preview_dimensions() {
    let viewer = make_viewer();
    viewer.set_pixel_width(4000);
    viewer.set_pixel_height(3000);
    viewer.set_picture(frame(1200, 800));
    settle();
    viewer.invoke_actual_size();
    near(viewer.get_zoom(), 1.);
    near(viewer.get_pan_limit_x(), (1200. - 960.) / 2.);
    near(viewer.get_pan_limit_y(), (800. - 592.) / 2.);
    let status = viewer.get_status();
    assert!(status.contains("Decoded 1200 × 800 px"), "{status}");
    assert!(status.contains(" · Preview"), "{status}");
    assert!(
        !status.contains("4000 × 3000"),
        "metadata is not decoded resolution: {status}"
    );
    assert!(status.contains("100%"), "{status}");
    // A small decoded thumbnail is never silently enlarged to source metadata.
    viewer.set_picture(frame(120, 80));
    viewer.invoke_fit_image();
    settle();
    near(viewer.get_fit_zoom(), 1.);
    near(viewer.get_pan_limit_x(), 0.);
    near(viewer.get_pan_limit_y(), 0.);
}

#[test]
fn viewer_wheel_zoom_is_anchored_at_the_pointer_and_stays_bounded() {
    let viewer = make_viewer();
    let position = slint::LogicalPosition::new(660., 380.);
    viewer
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    viewer
        .window()
        .dispatch_event(WindowEvent::PointerScrolled {
            position,
            delta_x: 0.,
            delta_y: 120.,
        });
    settle();
    near(viewer.get_zoom(), 0.625);
    near(viewer.get_pan_x(), -25.);
    near(viewer.get_pan_y(), -5.);
    viewer
        .window()
        .dispatch_event(WindowEvent::PointerScrolled {
            position,
            delta_x: 0.,
            delta_y: -120.,
        });
    settle();
    near(viewer.get_zoom(), 0.5);
    near(viewer.get_pan_x(), 0.);
    near(viewer.get_pan_y(), 0.);
    viewer
        .window()
        .dispatch_event(WindowEvent::PointerScrolled {
            position,
            delta_x: 120.,
            delta_y: 0.,
        });
    near(viewer.get_zoom(), 0.5);
    for _ in 0..60 {
        viewer
            .window()
            .dispatch_event(WindowEvent::PointerScrolled {
                position,
                delta_x: 0.,
                delta_y: 120.,
            });
    }
    near(viewer.get_zoom(), 8.);
    assert!(viewer.get_pan_x().abs() <= viewer.get_pan_limit_x());
    assert!(viewer.get_pan_y().abs() <= viewer.get_pan_limit_y());
    assert_eq!(viewer.get_closed(), 0);
}

#[test]
fn viewer_shift_arrows_pan_but_plain_arrows_still_navigate_and_escape_closes() {
    use slint::platform::Key;
    let viewer = make_viewer();
    viewer.set_position("2 / 3".into());
    viewer.invoke_actual_size();
    viewer.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Shift.into(),
    });
    tap(&viewer, Key::RightArrow);
    viewer.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Shift.into(),
    });
    near(viewer.get_pan_x(), -48.);
    assert_eq!(viewer.get_steps(), 0);
    tap(&viewer, Key::RightArrow);
    assert_eq!(viewer.get_steps(), 1);
    tap(&viewer, Key::LeftArrow);
    assert_eq!(viewer.get_steps(), 0);
    tap(&viewer, Key::Escape);
    assert_eq!(viewer.get_closed(), 1);
}

#[test]
fn viewer_keeps_original_animation_frames_and_their_delays_while_zooming() {
    use std::time::Duration;
    let viewer = make_viewer();
    let colored_frame = |red| {
        let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(32, 24);
        for pixel in pixels.make_mut_bytes().chunks_exact_mut(4) {
            pixel.copy_from_slice(&[red, 0, 0, 255]);
        }
        slint::Image::from_rgba8(pixels)
    };
    let first = colored_frame(40);
    let second = colored_frame(200);
    viewer.set_frames(ModelRc::new(VecModel::from(vec![
        first.clone(),
        second.clone(),
    ])));
    viewer.set_delays(ModelRc::new(VecModel::from(vec![30, 70])));
    viewer.invoke_set_playing(true);
    settle();
    assert_eq!(viewer.get_displayed_frame(), first);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(29));
    settle();
    assert_eq!(viewer.get_displayed_frame(), first);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    settle();
    assert_eq!(viewer.get_displayed_frame(), second);
    viewer.invoke_zoom_in();
    viewer.invoke_pan_by(100., 100.);
    assert_eq!(
        viewer.get_displayed_frame(),
        second,
        "zoom must not replace or restart the animation"
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(69));
    settle();
    assert_eq!(viewer.get_displayed_frame(), second);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    settle();
    assert_eq!(viewer.get_displayed_frame(), first);
    viewer.invoke_set_playing(false);
    settle();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(2));
    settle();
    assert_eq!(
        viewer.get_displayed_frame(),
        first,
        "the existing idle/focus motion gate still applies"
    );
    viewer.invoke_set_playing(true);
}

#[test]
fn viewer_fit_handles_tall_images_small_windows_and_missing_pixels() {
    let viewer = make_viewer();
    viewer.set_picture(frame(1, 20000));
    viewer.set_pixel_width(0);
    viewer.set_pixel_height(0);
    viewer.invoke_fit_image();
    settle();
    let fit = viewer.get_fit_zoom();
    assert!(fit > 0. && fit < 0.1 && fit.is_finite());
    for _ in 0..20 {
        viewer.invoke_zoom_out();
    }
    near(viewer.get_zoom(), fit);
    viewer
        .window()
        .set_size(slint::LogicalSize::new(320., 240.));
    viewer.invoke_fit_image();
    settle();
    assert!(viewer.get_zoom().is_finite() && viewer.get_zoom() > 0.);
    assert!(viewer.get_pan_x().abs() < 0.001 && viewer.get_pan_y().abs() < 0.001);
    viewer.set_picture(slint::Image::default());
    settle();
    let empty_zoom = viewer.get_zoom();
    viewer.invoke_zoom_in();
    viewer.invoke_actual_size();
    near(viewer.get_zoom(), empty_zoom);
    assert_eq!(viewer.get_status(), "Image not loaded");
}

#[test]
fn viewer_attachment_changes_reset_pan_and_zoom_but_frame_refreshes_do_not() {
    let viewer = make_viewer();
    viewer.invoke_actual_size();
    viewer.invoke_pan_by(100., 100.);
    viewer.set_picture(frame(1920, 1080));
    settle();
    near(viewer.get_zoom(), 1.);
    near(viewer.get_pan_x(), 100.);
    viewer.set_picture_index(1);
    settle();
    near(viewer.get_zoom(), viewer.get_fit_zoom());
    near(viewer.get_pan_x(), 0.);
    near(viewer.get_pan_y(), 0.);
    viewer.invoke_actual_size();
    viewer.invoke_pan_by(100., 100.);
    viewer.set_picture_item("image:2".into());
    settle();
    near(viewer.get_zoom(), viewer.get_fit_zoom());
    near(viewer.get_pan_x(), 0.);
    near(viewer.get_pan_y(), 0.);
}

#[test]
fn viewer_background_click_still_closes_but_clicking_the_picture_does_not() {
    use slint::platform::PointerEventButton;
    let viewer = make_viewer();
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        viewer
            .window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        viewer
            .window()
            .dispatch_event(WindowEvent::PointerPressed { position, button });
        viewer
            .window()
            .dispatch_event(WindowEvent::PointerReleased { position, button });
        settle();
    };
    click(560., 360.);
    assert_eq!(viewer.get_closed(), 0);
    click(500., 70.); // Letterbox area inside the clipped viewport, not the header.
    assert_eq!(viewer.get_closed(), 1);
    click(8., 400.); // Original backdrop close target outside the viewport.
    assert_eq!(viewer.get_closed(), 2);
}

#[test]
fn viewer_navigation_buttons_keep_their_visible_hit_bounds() {
    use slint::platform::PointerEventButton;
    let viewer = make_viewer();
    viewer.set_position("2 / 3".into());
    settle();
    let click = |x, y| {
        let position = slint::LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        viewer
            .window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        viewer
            .window()
            .dispatch_event(WindowEvent::PointerPressed { position, button });
        viewer
            .window()
            .dispatch_event(WindowEvent::PointerReleased { position, button });
        settle();
    };
    click(40., 360.);
    assert_eq!(viewer.get_steps(), -1);
    click(1080., 360.);
    assert_eq!(viewer.get_steps(), 0);
    assert_eq!(viewer.get_closed(), 0);
    click(40., 100.); // Beside, not inside, the visible previous-image button.
    assert_eq!(viewer.get_steps(), 0);
    assert_eq!(viewer.get_closed(), 1);
}
