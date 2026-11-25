//! How long the call has lasted, under its status in the voice dock. Its
//! one timer runs only during a call, ticking once a second.

use std::time::{Duration, Instant};

use slint::{ComponentHandle, Timer, TimerMode};

use super::{Adapter, YapfastWindow};

/// The call it's timing and when it connected.
#[derive(Default)]
pub(super) struct CallTimer {
    since: Option<(String, Instant)>,
    attached: Option<slint::Weak<YapfastWindow>>,
    timer: Timer,
    clock: Clock,
}

#[derive(Clone, Default)]
struct Clock {
    #[cfg(test)]
    now: Option<std::rc::Rc<std::cell::Cell<Instant>>>,
}
impl Clock {
    fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(now) = &self.now {
            return now.get();
        }
        Instant::now()
    }
}

#[cfg(test)]
impl CallTimer {
    pub(super) fn use_clock(&mut self, now: std::rc::Rc<std::cell::Cell<Instant>>) {
        self.clock.now = Some(now);
    }
    pub(super) fn origin(&self) -> Option<(String, Instant)> {
        self.since.clone()
    }
    pub(super) fn running(&self) -> bool {
        self.timer.running()
    }
}

impl CallTimer {
    /// Apply EVERY actual voice transition, before any rendering/batching.
    pub(super) fn observe(&mut self, room: Option<&str>) {
        if self.since.as_ref().map(|(room, _)| room.as_str()) == room {
            return;
        }
        self.detach();
        self.since = room.map(|room| (room.to_owned(), self.clock.now()));
    }
    /// Detach only the renderer; the monotonic voice lifetime continues.
    pub(super) fn detach(&mut self) {
        self.timer.stop();
        self.attached = None;
    }
}

impl Adapter {
    /// Rendering never creates or changes a call lifetime.
    pub(super) fn time_call(&mut self, window: &YapfastWindow, room: Option<&str>) {
        let started = self
            .call_timer
            .since
            .as_ref()
            .filter(|(timed, _)| Some(timed.as_str()) == room)
            .map(|(_, started)| *started);
        let Some(started) = started else {
            self.call_timer.detach();
            window.set_voice_time(Default::default());
            return;
        };
        let attached = self
            .call_timer
            .attached
            .as_ref()
            .and_then(slint::Weak::upgrade);
        if self.call_timer.timer.running()
            && attached
                .as_ref()
                .is_some_and(|old| std::ptr::eq(old.window(), window.window()))
        {
            return;
        }
        self.call_timer.attached = Some(window.as_weak());
        let clock = self.call_timer.clock.clone();
        window.set_voice_time(elapsed(clock.now().saturating_duration_since(started)).into());
        let window = window.as_weak();
        self.call_timer
            .timer
            .start(TimerMode::Repeated, Duration::from_secs(1), move || {
                if let Some(window) = window.upgrade() {
                    window.set_voice_time(
                        elapsed(clock.now().saturating_duration_since(started)).into(),
                    );
                }
            });
    }
}

/// `0:42`, `12:05`, or `1:02:03`.
fn elapsed(time: Duration) -> String {
    let seconds = time.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_minutes_then_hours() {
        assert_eq!(elapsed(Duration::from_secs(42)), "0:42");
        assert_eq!(elapsed(Duration::from_secs(12 * 60 + 5)), "12:05");
        assert_eq!(elapsed(Duration::from_secs(3723)), "1:02:03");
    }
}
