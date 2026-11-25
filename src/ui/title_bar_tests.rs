//! Geometry regressions for both imported production title bars and resize edges.
#[path = "title_bar_test_window.rs"]
mod fixture;
use fixture::{ChromeTestWindow, ClientStyle};
use slint::{
    ComponentHandle,
    platform::{PointerEventButton, WindowEvent},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn sized(window: &ChromeTestWindow, width: f32, height: f32) {
    let size = slint::LogicalSize::new(width, height);
    window.window().set_size(size);
    window
        .window()
        .dispatch_event(WindowEvent::Resized { size });
    assert_eq!(window.get_observed_width(), width);
    assert_eq!(window.get_observed_height(), height);
}
fn press(window: &ChromeTestWindow, x: f32, y: f32) {
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: slint::LogicalPosition::new(x, y),
        button: PointerEventButton::Left,
    });
}
fn moved(window: &ChromeTestWindow, x: f32, y: f32) {
    window.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(x, y),
    });
}
fn release(window: &ChromeTestWindow, x: f32, y: f32) {
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(x, y),
            button: PointerEventButton::Left,
        });
}

#[test]
fn all_eight_resize_regions_follow_actual_size_and_stop_at_inner_boundaries() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ChromeTestWindow::new().unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let actions = seen.clone();
    window.on_resize_window(move |direction| actions.borrow_mut().push(direction));
    window.show().unwrap();
    for enabled in [true, false] {
        window.global::<ClientStyle>().set_enabled(enabled);
        for (width, height) in [(800., 560.), (1120., 760.), (1480., 900.)] {
            sized(&window, width, height);
            for (x, y, expected) in [
                (width - 1., height / 2., Some(0)),
                (width / 2., 1., Some(1)),
                (width - 1., 1., Some(2)),
                (1., 1., Some(3)),
                (width / 2., height - 1., Some(4)),
                (width - 1., height - 1., Some(5)),
                (1., height - 1., Some(6)),
                (1., height / 2., Some(7)),
                (4.9, height / 2., Some(7)),
                (5.1, height / 2., None),
                (width - 4.9, height / 2., Some(0)),
                (width - 5.1, height / 2., None),
                (width / 2., 4.9, Some(1)),
                (width / 2., 5.1, None),
                (width / 2., height - 4.9, Some(4)),
                (width / 2., height - 5.1, None),
                (7.9, 7.9, Some(3)),
                (8.1, 7.9, None),
                (width - 7.9, 7.9, Some(2)),
                (width - 8.1, 7.9, None),
                (7.9, height - 7.9, Some(6)),
                (8.1, height - 7.9, None),
                (width - 7.9, height - 7.9, Some(5)),
                (width - 8.1, height - 7.9, None),
                (width / 2., height / 2., None),
                (width - 18., 16., None),
            ] {
                seen.borrow_mut().clear();
                press(&window, x, y);
                release(&window, x, y);
                assert_eq!(
                    *seen.borrow(),
                    expected.into_iter().collect::<Vec<_>>(),
                    "enabled={enabled} size={width}x{height} point={x},{y}"
                );
            }
        }
    }
    window.set_test_maximized(true);
    seen.borrow_mut().clear();
    press(&window, 1., 300.);
    release(&window, 1., 300.);
    assert!(
        seen.borrow().is_empty(),
        "maximized windows have no resize overlay"
    );
    window.set_test_maximized(false);
    window.set_chrome(false);
    press(&window, 1., 300.);
    release(&window, 1., 300.);
    assert!(
        seen.borrow().is_empty(),
        "system chrome does not get a client resize overlay"
    );
    window.hide().unwrap();
}

#[test]
fn caption_buttons_remain_separate_from_dragging_at_minimum_width() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ChromeTestWindow::new().unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let actions = seen.clone();
    window.on_minimize_window(move || actions.borrow_mut().push("minimize"));
    let actions = seen.clone();
    window.on_maximize_window(move || actions.borrow_mut().push("maximize"));
    let actions = seen.clone();
    window.on_close_window(move || actions.borrow_mut().push("close"));
    let moves = Rc::new(Cell::new(0));
    let actions = moves.clone();
    window.on_move_window(move || actions.set(actions.get() + 1));
    window.show().unwrap();
    for enabled in [true, false] {
        let style = window.global::<ClientStyle>();
        style.set_enabled(enabled);
        sized(&window, 800., 560.);
        for title_height in [24., 32., 48.] {
            style.set_pack_title_height(title_height);
            let caption = style.get_caption_width();
            seen.borrow_mut().clear();
            for offset in [2.5, 1.5, 0.5] {
                let x = 800. - offset * caption;
                press(&window, x, title_height / 2.);
                release(&window, x, title_height / 2.);
            }
            assert_eq!(
                *seen.borrow(),
                ["minimize", "maximize", "close"],
                "enabled={enabled} title-height={title_height} caption={caption}"
            );
            moves.set(0);
            // Text/wordmark, location and empty background all share the drag target.
            for x in [30., 300., 600.] {
                let before = moves.get();
                press(&window, x, title_height / 2.);
                moved(&window, x + 3., title_height / 2.);
                assert_eq!(moves.get(), before, "threshold boundary is not a drag");
                moved(&window, x + 4., title_height / 2.);
                moved(&window, x + 12., title_height / 2.);
                assert_eq!(moves.get(), before + 1, "one native initiation per press");
                release(&window, x + 12., title_height / 2.);
            }
        }
    }
    window.hide().unwrap();
}
