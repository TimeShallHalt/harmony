//! The title bar Yapfast draws in place of the desktop's on Linux: whether it
//! does, and its native move, resize, minimize, maximize and close actions.
//! Linux uses winit; Windows uses Win32 move/size commands for its custom bar.
//!
//! A desktop that gives its windows title bars, as KDE and GNOME do, draws
//! one on Yapfast's too unless Yapfast draws its own. A tiling compositor such
//! as Hyprland or sway draws none, and there Yapfast's would be in the way.
//! The desktop doesn't say which it does: KWin and Hyprland both answer
//! xdg-decoration with server-side. So `XDG_CURRENT_DESKTOP` decides, until
//! the person chooses in Appearance.
//!
//! Each window is made with the frame or without, rather than given it or
//! losing it once made. Where the desktop draws no frame, as GNOME and
//! Weston, winit draws one, and putting it on a maximized window or taking it
//! off makes the window larger or smaller than maximized, for which Weston
//! ends the connection.

#[cfg(not(windows))]
use slint::winit_030::winit::window::ResizeDirection;
use slint::winit_030::winit::window::WindowAttributes;

#[cfg(windows)]
mod windows;

thread_local! {
    /// Windows are made without the desktop's frame, for Yapfast's title bar.
    static OWN: Cell<bool> = const { Cell::new(false) };
}

use super::*;

/// Desktops whose windows have title bars, as `XDG_CURRENT_DESKTOP` names them.
const FRAMED: [&str; 14] = [
    "KDE",
    "GNOME",
    "Unity",
    "Cinnamon",
    "X-Cinnamon",
    "XFCE",
    "MATE",
    "LXQt",
    "LXDE",
    "Budgie",
    "Pantheon",
    "Deepin",
    "COSMIC",
    "Enlightenment",
];

/// The edges and corners ResizeEdges numbers, in its order.
#[cfg(not(windows))]
const DIRECTIONS: [ResizeDirection; 8] = [
    ResizeDirection::East,
    ResizeDirection::North,
    ResizeDirection::NorthEast,
    ResizeDirection::NorthWest,
    ResizeDirection::South,
    ResizeDirection::SouthEast,
    ResizeDirection::SouthWest,
    ResizeDirection::West,
];

/// Yapfast also offers the existing window actions on Windows. Upstream
/// presentation and other platforms keep their inherited frame policy.
fn offered() -> bool {
    cfg!(target_os = "linux") || (cfg!(windows) && yapfast::enabled())
}

/// Whether the desktop draws the title bar unless the person chose: on a
/// desktop not known to give windows one, it's the desktop's, which may be
/// none at all.
pub(super) fn system_by_default() -> bool {
    if cfg!(windows) && yapfast::enabled() && !cfg!(test) {
        false
    } else {
        cfg!(test) || !std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktops| framed(&desktops))
    }
}

/// Whether any of the desktops in a colon-separated `XDG_CURRENT_DESKTOP`,
/// such as "ubuntu:GNOME", gives its windows title bars.
fn framed(desktops: &str) -> bool {
    desktops.split(':').any(|desktop| {
        FRAMED
            .iter()
            .any(|framed| desktop.eq_ignore_ascii_case(framed))
    })
}

/// Makes the windows from now on with Yapfast's title bar or the desktop's,
/// as the layout has it, and returns whether Yapfast's.
pub(super) fn choose(system: Option<bool>) -> bool {
    let own = offered() && !system.unwrap_or_else(system_by_default);
    OWN.set(own);
    own
}

/// Each window as Slint makes it: with the desktop's frame unless Yapfast
/// draws its own title bar.
pub(super) fn attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes.with_decorations(!OWN.get())
}

/// Shows the title bar as the layout has it: the desktop's, or Yapfast's. A
/// window made since the choice changed shows it from before it's shown.
pub(super) fn show(window: &YapfastWindow, system: Option<bool>) {
    let own = choose(system);
    window.set_title_bar_choice(offered());
    window.set_system_title_bar(!own);
    window.set_own_title_bar(own);
}

/// What the title bar and the window's edges do, and the setting.
pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let weak = window.as_weak();
    window.on_move_window(move || {
        tracing::debug!("client title bar requested window move");
        winit(&weak, |winit| {
            #[cfg(windows)]
            let result = windows::begin(winit, None);
            #[cfg(not(windows))]
            let result = winit.drag_window();
            if let Err(error) = result {
                tracing::warn!(%error, "could not move the window");
            }
        });
    });
    let weak = window.as_weak();
    window.on_resize_window(move |direction| {
        tracing::debug!(direction, "client resize edge requested window resize");
        winit(&weak, |winit| {
            #[cfg(windows)]
            let result = windows::begin(winit, Some(direction));
            #[cfg(not(windows))]
            let result = {
                let Some(&direction) = usize::try_from(direction)
                    .ok()
                    .and_then(|index| DIRECTIONS.get(index))
                else {
                    return;
                };
                winit.drag_resize_window(direction)
            };
            if let Err(error) = result {
                tracing::warn!(%error, "could not resize the window");
            }
        });
    });
    let weak = window.as_weak();
    // Straight to winit: Slint keeps whether the window is minimized from
    // winit, which on Wayland never says, so Slint's own may be stale.
    window.on_minimize_window(move || winit(&weak, |winit| winit.set_minimized(true)));
    let weak = window.as_weak();
    window.on_maximize_window(move || {
        if let Some(window) = weak.upgrade() {
            let native = window.window();
            native.set_maximized(!native.is_maximized());
        }
    });
    let weak = window.as_weak();
    window.on_close_window(move || {
        // As the desktop's close button: to the tray, or quit; see Shell::close.
        if let Some(window) = weak.upgrade() {
            window
                .window()
                .dispatch_event(slint::platform::WindowEvent::CloseRequested);
        }
    });
    let adapter = Rc::downgrade(adapter);
    let weak = window.as_weak();
    window.on_title_bar_toggled(move || {
        let (Some(adapter), Some(window)) = (adapter.upgrade(), weak.upgrade()) else {
            return;
        };
        let mut adapter = adapter.borrow_mut();
        let system = window.get_system_title_bar();
        adapter.layout.system_title_bar = Some(system);
        adapter.layout.write(&adapter.root);
        drop(adapter);
        // A maximized window is made again with the frame or without; see above.
        if window.window().is_maximized() {
            choose(Some(system));
            // Not from inside the window's own callback, which dropping it ends.
            slint::Timer::single_shot(Duration::ZERO, shell::remake_window);
        } else {
            show(&window, Some(system));
        }
    });
}

/// Acts on the window's winit window, once it has one.
fn winit(
    weak: &slint::Weak<YapfastWindow>,
    act: impl FnOnce(&slint::winit_030::winit::window::Window),
) {
    if let Some(window) = weak.upgrade() {
        window.window().with_winit_window(act);
    }
}

#[cfg(test)]
#[path = "title_bar_tests.rs"]
mod hit_region_tests;

#[cfg(all(test, windows))]
#[path = "title_bar_native_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_desktops_by_name() {
        assert!(framed("KDE"));
        assert!(framed("ubuntu:GNOME"));
        assert!(framed("X-Cinnamon"));
        assert!(!framed("Hyprland"));
        assert!(!framed("sway"));
        assert!(!framed("niri"));
        assert!(!framed(""));
    }
}
