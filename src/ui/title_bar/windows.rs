//! Windows' native modal move/size loop for the client-drawn title bar.
//!
//! winit 0.30.13 posts the address of a temporary POINTS as WM_NCLBUTTONDOWN's
//! lParam, not its packed value. Its caption handler also injects a dummy
//! WM_MOUSEMOVE. Post the corresponding system command directly, after the
//! Slint pointer callback returns. DefWindowProc still owns snapping, sizing,
//! minimum/maximum constraints, DPI changes and the modal message loop.

use slint::winit_030::winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};
use std::io;
use windows_sys::Win32::{
    Foundation::POINT,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, ReleaseCapture, VK_LBUTTON},
        WindowsAndMessaging::{
            GetCursorPos, HTCAPTION, PostMessageW, SC_MOVE, SC_SIZE, WM_SYSCOMMAND, WMSZ_BOTTOM,
            WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT, WMSZ_LEFT, WMSZ_RIGHT, WMSZ_TOP, WMSZ_TOPLEFT,
            WMSZ_TOPRIGHT,
        },
    },
};

/// `None` moves; `Some` uses ResizeEdges' eight directions. Invalid directions
/// do nothing, including leaving capture untouched.
pub(super) fn begin(window: &Window, direction: Option<i32>) -> io::Result<()> {
    let Some(command) = command(direction) else {
        return Ok(());
    };
    let handle = window.window_handle().map_err(io::Error::other)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(io::Error::other("the window has no Win32 handle"));
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    let mut cursor = POINT { x: 0, y: 0 };
    // Called on the window's UI thread, with the borrowed winit window alive.
    // lParam contains values, not a pointer whose lifetime ends on return.
    unsafe {
        if GetCursorPos(&mut cursor) == 0 {
            return Err(io::Error::last_os_error());
        }
        ReleaseCapture();
        if PostMessageW(
            hwnd,
            WM_SYSCOMMAND,
            command,
            packed_position(cursor.x, cursor.y),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        tracing::debug!(
            hwnd = hwnd as usize,
            ?direction,
            command,
            cursor_x = cursor.x,
            cursor_y = cursor.y,
            left_down = GetAsyncKeyState(i32::from(VK_LBUTTON)) < 0,
            "queued Windows native window move/size"
        );
    }
    Ok(())
}

fn command(direction: Option<i32>) -> Option<usize> {
    let Some(direction) = direction else {
        return Some((SC_MOVE | HTCAPTION) as usize);
    };
    let edges = [
        WMSZ_RIGHT,
        WMSZ_TOP,
        WMSZ_TOPRIGHT,
        WMSZ_TOPLEFT,
        WMSZ_BOTTOM,
        WMSZ_BOTTOMRIGHT,
        WMSZ_BOTTOMLEFT,
        WMSZ_LEFT,
    ];
    let edge = *edges.get(usize::try_from(direction).ok()?)?;
    Some((SC_SIZE | edge) as usize)
}

/// Equivalent to MAKELPARAM(x, y), retaining signed coordinates on monitors
/// left of or above the primary display. The upper 32 bits are not an address.
fn packed_position(x: i32, y: i32) -> isize {
    (u32::from(x as u16) | (u32::from(y as u16) << 16)) as isize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_coordinate_words_preserve_both_signs_and_boundaries() {
        for x in [i16::MIN, -1920, -1, 0, 1, 1920, i16::MAX] {
            for y in [i16::MIN, -1080, -1, 0, 1, 1080, i16::MAX] {
                let packed = packed_position(i32::from(x), i32::from(y));
                assert_eq!(packed as u16 as i16, x);
                assert_eq!((packed >> 16) as u16 as i16, y);
                assert_eq!((packed as usize as u64) >> 32, 0);
            }
        }
    }
}
