//! Isolated, account-free native probe for test_window_chrome_native.py.
//! Imports the real Slint hit targets and the real Windows bridge.
#![cfg(windows)]
#[path = "../src/ui/title_bar_test_window.rs"]
mod fixture;
#[path = "../src/ui/title_bar/windows.rs"]
mod windows;
use fixture::ChromeTestWindow;
use slint::{
    ComponentHandle,
    winit_030::{
        WinitWindowAccessor,
        winit::{
            raw_window_handle::{HasWindowHandle, RawWindowHandle},
            window::ResizeDirection,
        },
    },
};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let legacy = std::env::args().any(|arg| arg == "--legacy");
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("skia-software".into())
        .with_winit_window_attributes_hook(|attributes| attributes.with_decorations(false))
        .select()?;
    let window = ChromeTestWindow::new()?;
    let weak = window.as_weak();
    window.on_move_window(move || {
        if let Some(window) = weak.upgrade() {
            window.window().with_winit_window(|native| {
                if legacy {
                    native.drag_window().unwrap();
                } else {
                    windows::begin(native, None).unwrap();
                }
            });
        }
    });
    let weak = window.as_weak();
    window.on_resize_window(move |direction| {
        if let Some(window) = weak.upgrade() {
            window.window().with_winit_window(|native| {
                if legacy {
                    let directions = [
                        ResizeDirection::East,
                        ResizeDirection::North,
                        ResizeDirection::NorthEast,
                        ResizeDirection::NorthWest,
                        ResizeDirection::South,
                        ResizeDirection::SouthEast,
                        ResizeDirection::SouthWest,
                        ResizeDirection::West,
                    ];
                    native
                        .drag_resize_window(directions[direction as usize])
                        .unwrap();
                } else {
                    windows::begin(native, Some(direction)).unwrap();
                }
            });
        }
    });
    let weak = window.as_weak();
    window.on_minimize_window(move || {
        if let Some(window) = weak.upgrade() {
            window
                .window()
                .with_winit_window(|native| native.set_minimized(true));
        }
    });
    let weak = window.as_weak();
    window.on_maximize_window(move || {
        if let Some(window) = weak.upgrade() {
            let native = window.window();
            native.set_maximized(!native.is_maximized());
            window.set_test_maximized(native.is_maximized());
        }
    });
    let weak = window.as_weak();
    window.on_close_window(move || {
        if let Some(window) = weak.upgrade() {
            window
                .window()
                .dispatch_event(slint::platform::WindowEvent::CloseRequested);
        }
    });
    window.window().on_close_requested(|| {
        slint::quit_event_loop().unwrap();
        slint::CloseRequestResponse::HideWindow
    });
    // Only this test process has a watchdog. Production has no timer/polling.
    slint::Timer::single_shot(std::time::Duration::from_secs(45), || {
        let _ = slint::quit_event_loop();
    });
    window.show()?;
    let weak = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        let window = weak.upgrade().unwrap();
        window
            .window()
            .with_winit_window(|native| {
                let RawWindowHandle::Win32(handle) = native.window_handle().unwrap().as_raw()
                else {
                    unreachable!()
                };
                println!(
                    "{{\"hwnd\":{},\"pid\":{},\"legacy\":{legacy}}}",
                    handle.hwnd.get(),
                    std::process::id()
                );
                io::stdout().flush().unwrap();
            })
            .expect("the probe must own a native window after event-loop startup");
    });
    slint::run_event_loop()?;
    Ok(())
}
