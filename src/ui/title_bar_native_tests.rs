//! Regressions at the actual Windows message boundary, not replaced callbacks.
#![cfg(windows)]

use super::windows as native;

use slint::winit_030::winit::{
    application::ApplicationHandler,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::windows::EventLoopBuilderExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowId},
};
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    UI::{
        Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture},
        WindowsAndMessaging::{
            GWL_STYLE, GetCursorPos, GetWindowLongPtrW, MSG, PM_REMOVE, PeekMessageW, SC_MOVE,
            SC_SIZE, WM_NCLBUTTONDOWN, WM_SYSCOMMAND, WS_THICKFRAME,
        },
    },
};

/// Read back the real queue for a real undecorated, resizable winit HWND.
/// This consumes commands without injecting physical input, but SetCapture can
/// still disturb a desktop user. Opt in only while the desktop is idle.
#[test]
#[ignore = "manipulates OS mouse capture; opt in with testing/run_window_chrome_tests.py --native while the desktop is idle"]
fn native_queue_preserves_coordinates_all_directions_and_invalid_no_ops() {
    struct Probe;
    impl ApplicationHandler for Probe {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(
                    Window::default_attributes()
                        .with_visible(false)
                        .with_decorations(false),
                )
                .unwrap();
            let hwnd = hwnd(&window);
            let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
            assert_ne!(
                style & WS_THICKFRAME as isize,
                0,
                "the actual HWND must remain resizable"
            );
            for (direction, command) in [
                (None, SC_MOVE | 2),
                (Some(0), SC_SIZE | 2), // east
                (Some(1), SC_SIZE | 3), // north
                (Some(2), SC_SIZE | 5), // north-east
                (Some(3), SC_SIZE | 4), // north-west
                (Some(4), SC_SIZE | 6), // south
                (Some(5), SC_SIZE | 8), // south-east
                (Some(6), SC_SIZE | 7), // south-west
                (Some(7), SC_SIZE | 1), // west
            ] {
                let mut cursor = POINT { x: 0, y: 0 };
                assert_ne!(unsafe { GetCursorPos(&mut cursor) }, 0);
                let packed =
                    (u32::from(cursor.x as u16) | (u32::from(cursor.y as u16) << 16)) as isize;
                unsafe { SetCapture(hwnd) };
                native::begin(&window, direction).unwrap();
                assert!(
                    unsafe { GetCapture() }.is_null(),
                    "native loop takes over capture"
                );
                let message =
                    queued(hwnd, WM_SYSCOMMAND).expect("native initiation must reach the OS queue");
                assert_eq!(message.wParam, command as usize, "direction {direction:?}");
                assert_eq!(
                    message.lParam, packed,
                    "packed screen coordinates, not a POINTS address"
                );
                assert!(
                    queued(hwnd, WM_SYSCOMMAND).is_none(),
                    "exactly one initiation"
                );
                assert!(
                    queued(hwnd, WM_NCLBUTTONDOWN).is_none(),
                    "do not invoke winit's dummy mouse move handler"
                );
            }
            for invalid in [i32::MIN, -1, 8, 9, i32::MAX] {
                unsafe { SetCapture(hwnd) };
                native::begin(&window, Some(invalid)).unwrap();
                assert_eq!(
                    unsafe { GetCapture() },
                    hwnd,
                    "invalid directions must not release capture"
                );
                assert!(
                    queued(hwnd, WM_SYSCOMMAND).is_none(),
                    "invalid direction must not dispatch"
                );
                unsafe { ReleaseCapture() };
            }
            event_loop.exit();
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: WindowId,
            _: slint::winit_030::winit::event::WindowEvent,
        ) {
        }
    }
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    builder.build().unwrap().run_app(&mut Probe).unwrap();
}

fn queued(hwnd: HWND, message: u32) -> Option<MSG> {
    let mut result = unsafe { std::mem::zeroed() };
    (unsafe { PeekMessageW(&mut result, hwnd, message, message, PM_REMOVE) } != 0).then_some(result)
}

fn hwnd(window: &Window) -> HWND {
    match window.window_handle().unwrap().as_raw() {
        RawWindowHandle::Win32(handle) => handle.hwnd.get() as HWND,
        _ => panic!("Windows test needs a Win32 window"),
    }
}
