use super::*;
use crate::harmony_updates::{self, Release, Staged};
use slint::winit_030::winit::event::WindowEvent;
use std::{path::Path, time::Instant};
use yapfast_core::runtime::AbortHandle;

#[derive(Default)]
pub(super) struct Updates {
    active: bool,
    timer: slint::Timer,
    idle_timer: slint::Timer,
    last_input: Rc<Cell<Option<Instant>>>,
    last_message: Option<Instant>,
    restarting: bool,
    pending: Option<AbortHandle>,
    downloading: bool,
    offered: Option<Release>,
    staged: Option<Staged>,
    armed: bool,
    status: String,
}

impl Updates {
    fn watch(&mut self, worker: AbortHandle, observer: impl Future<Output = ()> + 'static) {
        // The UI observer runs once the worker finishes or is cancelled. Keep
        // the worker handle so Cancel can stop a download while it awaits I/O.
        match slint::spawn_local(observer) {
            Ok(_) => self.pending = Some(worker),
            Err(error) => {
                worker.abort();
                self.downloading = false;
                self.status = format!("Could not start the update task. {error}");
            }
        }
    }

    fn complete(&mut self, worker: &AbortHandle) -> bool {
        // A cancelled worker can report completion after a replacement starts.
        if self
            .pending
            .as_ref()
            .is_some_and(|current| current.id() == worker.id())
        {
            self.pending = None;
            true
        } else {
            false
        }
    }

    fn cancel(&mut self) {
        if let Some(worker) = self.pending.take() {
            worker.abort();
        }
        self.downloading = false;
        self.status = "Canceled.".into();
    }

    pub(super) fn input_clock(&self) -> Rc<Cell<Option<Instant>>> {
        self.last_input.clone()
    }

    pub(super) fn user_interacted(&self) {
        self.last_input.set(Some(Instant::now()));
    }

    pub(super) fn message_sent(&mut self) {
        self.last_message = Some(Instant::now());
    }

    pub(super) fn restarting(&self) -> bool {
        self.restarting
    }

    pub(super) fn can_restart(&self) -> bool {
        self.armed && self.staged.is_some() && harmony_updates::can_restart()
    }

    pub(super) fn finish(&mut self, reopen: Option<&Path>) -> anyhow::Result<Option<PathBuf>> {
        if self.armed
            && let Some(staged) = self.staged.take()
        {
            return staged.launch(reopen);
        }
        Ok(None)
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        if let Some(worker) = self.pending.take() {
            worker.abort();
        }
    }
}

pub(super) fn record_input(clock: &Cell<Option<Instant>>, event: &WindowEvent) {
    if matches!(
        event,
        WindowEvent::Focused(_)
            | WindowEvent::KeyboardInput { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::Touch(_)
            | WindowEvent::Ime(_)
            | WindowEvent::DroppedFile(_)
    ) {
        clock.set(Some(Instant::now()));
    }
}

pub(super) fn sends_message(command: &Command) -> bool {
    #[cfg(feature = "discord")]
    if matches!(command, Command::SendVoiceNote { .. }) {
        return true;
    }
    matches!(
        command,
        Command::Send { .. }
            | Command::SendPoll { .. }
            | Command::ScheduledSend(_)
            | Command::Discord(yapfast_core::chat::DiscordCommand::SendSticker { .. })
    )
}

fn checks(layout: &Layout) -> bool {
    layout.harmony_release_checks.unwrap_or(true)
}

fn downloads(layout: &Layout) -> bool {
    layout.harmony_release_downloads.unwrap_or(true)
}

fn has_work(a: &Adapter) -> bool {
    let state = &a.app.state;
    state.voice.active()
        || a.call_window.is_some()
        || state.has_unfinished_content()
        || state
            .video
            .as_ref()
            .is_some_and(|video| video.playing || video.loading)
        || a.dialog.is_some()
        || a.window.upgrade().is_some_and(|window| {
            window.get_timezone_editor_open()
                || window.global::<Scheduling>().get_open()
                || window.global::<VoiceNotes>().get_open()
        })
}

fn quiet_long_enough(
    now: Instant,
    since_input: Option<Instant>,
    last_message: Option<Instant>,
    delay: Duration,
) -> bool {
    !delay.is_zero()
        && since_input.is_some_and(|last| now.saturating_duration_since(last) >= delay)
        && last_message.is_none_or(|last| now.saturating_duration_since(last) >= delay * 2)
}

fn request_restart(a: &mut Adapter) {
    a.updates.restarting = true;
    if let Err(error) = slint::quit_event_loop() {
        a.updates.restarting = false;
        a.updates.status = format!("Could not restart Harmony. {error}");
        refresh(a);
    }
}

fn set_idle_timer(adapter: &Rc<RefCell<Adapter>>) {
    let a = adapter.borrow();
    a.updates.idle_timer.stop();
    if !a.layout.harmony_idle_restart
        || a.layout.harmony_idle_minutes == 0
        || !a.updates.can_restart()
    {
        return;
    }
    let weak = Rc::downgrade(adapter);
    a.updates.idle_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(5),
        move || {
            let Some(adapter) = weak.upgrade() else {
                return;
            };
            let mut a = adapter.borrow_mut();
            let focused = a.window.upgrade().is_some_and(|window| {
                window
                    .window()
                    .with_winit_window(|native| native.has_focus())
                    .unwrap_or(false)
            });
            let delay = Duration::from_secs(u64::from(a.layout.harmony_idle_minutes) * 60);
            if !focused
                && !a.updates.restarting
                && a.layout.harmony_idle_restart
                && quiet_long_enough(
                    Instant::now(),
                    a.updates.last_input.get(),
                    a.updates.last_message,
                    delay,
                )
                && !has_work(&a)
                && a.updates.can_restart()
            {
                request_restart(&mut a);
            }
        },
    );
}

fn show(a: &Adapter, window: &YapfastWindow) {
    let view = window.global::<HarmonyAbout>();
    let state = &a.updates;
    view.set_check_automatically(checks(&a.layout));
    view.set_download_automatically(downloads(&a.layout));
    view.set_idle_restart(a.layout.harmony_idle_restart);
    view.set_idle_minutes(a.layout.harmony_idle_minutes.to_string().into());
    view.set_can_restart(harmony_updates::can_restart());
    view.set_update_restart_ready(state.staged.is_some() && harmony_updates::can_restart());
    view.set_interval_index(match a.layout.harmony_check_hours {
        Some(1) => 0,
        Some(24) => 2,
        _ => 1,
    });
    view.set_busy(state.pending.is_some());
    view.set_update_status(state.status.clone().into());
    view.set_release_page(
        state
            .offered
            .as_ref()
            .map(Release::page)
            .unwrap_or_default()
            .into(),
    );
    view.set_release_notes(
        state
            .offered
            .as_ref()
            .map(|r| r.notes.clone())
            .unwrap_or_default()
            .into(),
    );
    view.set_can_download(
        state.offered.as_ref().is_some_and(Release::downloadable)
            && state.pending.is_none()
            && state.staged.is_none(),
    );
    view.set_windows_installer(harmony_updates::WINDOWS_INSTALLER);
    view.set_can_install(harmony_updates::can_install());
    view.set_ready(state.staged.is_some());
    view.set_install_on_quit(state.armed);
}
fn refresh(a: &Adapter) {
    if let Some(window) = a.window.upgrade() {
        show(a, &window);
    }
}

pub(super) fn start(adapter: &Rc<RefCell<Adapter>>) {
    {
        let mut a = adapter.borrow_mut();
        a.updates.active = true;
        a.updates.user_interacted();
    }
    set_timer(adapter);
    if checks(&adapter.borrow().layout) {
        check(adapter);
    }
}

fn set_timer(adapter: &Rc<RefCell<Adapter>>) {
    let a = adapter.borrow();
    a.updates.timer.stop();
    if a.updates.active && checks(&a.layout) {
        let hours = match a.layout.harmony_check_hours {
            Some(1) => 1,
            Some(24) => 24,
            _ => 6,
        };
        let weak = Rc::downgrade(adapter);
        a.updates.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(hours * 3600),
            move || {
                if let Some(adapter) = weak.upgrade() {
                    check(&adapter);
                }
            },
        );
    }
}

fn check(adapter: &Rc<RefCell<Adapter>>) {
    let mut a = adapter.borrow_mut();
    if !a.updates.active || a.updates.pending.is_some() || a.updates.staged.is_some() {
        return;
    }
    let task = a.runtime.spawn(harmony_updates::check());
    let worker = task.abort_handle();
    a.updates.status = "Checking for a Harmony release…".into();
    let weak = Rc::downgrade(adapter);
    a.updates.watch(worker.clone(), async move {
        let result = task.await;
        let Some(adapter) = weak.upgrade() else {
            return;
        };
        let mut a = adapter.borrow_mut();
        if !a.updates.complete(&worker) {
            return;
        }
        a.updates.offered = None;
        match result {
            Ok(Ok(Some(release))) => {
                a.updates.status = format!("{} is available.", release.tag);
                if !release.downloadable() && harmony_updates::can_install() {
                    a.updates
                        .status
                        .push_str(" This release has no signed package for this installation.");
                }
                if checks(&a.layout) && !downloads(&a.layout) {
                    let message = format!(
                        "A Harmony update is available in Settings → About ({})",
                        release.tag
                    );
                    a.app.state.notice = Some(message.clone());
                    if let Some(window) = a.window.upgrade() {
                        window.set_notice(message.into());
                    }
                }
                a.updates.offered = Some(release);
            }
            Ok(Ok(None)) => a.updates.status = "You have the latest Harmony release.".into(),
            Ok(Err(error)) => a.updates.status = format!("Update check failed. {error}"),
            Err(_) => a.updates.status = "Update check interrupted.".into(),
        }
        let automatic = downloads(&a.layout)
            && a.updates
                .offered
                .as_ref()
                .is_some_and(Release::downloadable);
        refresh(&a);
        drop(a);
        if automatic {
            download(&adapter);
        }
    });
    refresh(&a);
}

fn download(adapter: &Rc<RefCell<Adapter>>) {
    let mut a = adapter.borrow_mut();
    if !a.updates.active || a.updates.pending.is_some() || a.updates.staged.is_some() {
        return;
    }
    let Some(release) = a.updates.offered.clone().filter(Release::downloadable) else {
        return;
    };
    let profile = a.root.clone();
    let task = a.runtime.spawn(async move {
        let cache = std::path::absolute(profile.join("update-cache"))?;
        harmony_updates::download(&release, &cache).await
    });
    let worker = task.abort_handle();
    a.updates.downloading = true;
    a.updates.status = "Downloading and verifying the Harmony update…".into();
    let weak = Rc::downgrade(adapter);
    a.updates.watch(worker.clone(), async move {
        let result = task.await;
        let Some(adapter) = weak.upgrade() else {
            return;
        };
        let mut a = adapter.borrow_mut();
        if !a.updates.complete(&worker) {
            return;
        }
        a.updates.downloading = false;
        match result {
            Ok(Ok(staged)) => {
                a.updates.staged = Some(staged);
                a.updates.armed = downloads(&a.layout);
                a.updates.status = if a.updates.armed {
                    "Update ready. It will install on your next full quit.".into()
                } else {
                    "Update ready to install.".into()
                };
            }
            Ok(Err(error)) => a.updates.status = format!("Could not prepare this update. {error}"),
            Err(_) => a.updates.status = "Download interrupted.".into(),
        }
        refresh(&a);
        drop(a);
        set_idle_timer(&adapter);
    });
    refresh(&a);
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    show(&adapter.borrow(), window);
    let view = window.global::<HarmonyAbout>();
    let weak = Rc::downgrade(adapter);
    view.on_check_release(move || {
        if let Some(a) = weak.upgrade() {
            check(&a);
        }
    });
    let weak = Rc::downgrade(adapter);
    view.on_download_release(move || {
        if let Some(a) = weak.upgrade() {
            download(&a);
        }
    });
    let weak = Rc::downgrade(adapter);
    view.on_cancel_update(move || {
        if let Some(a) = weak.upgrade() {
            let mut a = a.borrow_mut();
            a.updates.cancel();
            refresh(&a);
        }
    });
    let weak = Rc::downgrade(adapter);
    view.on_arm_install(move |arm| {
        if let Some(a) = weak.upgrade() {
            {
                let mut adapter = a.borrow_mut();
                adapter.updates.armed =
                    adapter.updates.active && arm && adapter.updates.staged.is_some();
                refresh(&adapter);
            }
            set_idle_timer(&a);
        }
    });
    let weak = Rc::downgrade(adapter);
    view.on_save_idle_options(move |enabled, minutes| {
        let Some(adapter) = weak.upgrade() else {
            return "".into();
        };
        let Ok(minutes) = minutes.trim().parse::<u32>() else {
            return "Enter a whole number of minutes, or 0 to wait for a manual restart.".into();
        };
        let mut a = adapter.borrow_mut();
        if !a.updates.active {
            return "".into();
        }
        a.layout.harmony_idle_restart = enabled;
        a.layout.harmony_idle_minutes = minutes;
        a.layout.write(&a.root);
        refresh(&a);
        drop(a);
        set_idle_timer(&adapter);
        "".into()
    });
    let weak = Rc::downgrade(adapter);
    view.on_restart_update(move || {
        if let Some(adapter) = weak.upgrade() {
            let mut a = adapter.borrow_mut();
            if a.updates.staged.is_none() || !harmony_updates::can_restart() {
                return;
            }
            if has_work(&a) {
                a.updates.status =
                    "Finish any drafts, calls and open tasks before restarting.".into();
                refresh(&a);
                return;
            }
            a.updates.armed = true;
            request_restart(&mut a);
        }
    });
    let weak = Rc::downgrade(adapter);
    view.on_save_update_options(move |check_auto, download_auto, interval| {
        let Some(adapter) = weak.upgrade() else {
            return;
        };
        let mut a = adapter.borrow_mut();
        if !a.updates.active {
            return;
        }
        let start_now = check_auto && !checks(&a.layout);
        let was_auto = downloads(&a.layout);
        let download_now = download_auto && !was_auto;
        a.layout.harmony_release_checks = Some(check_auto);
        a.layout.harmony_release_downloads = Some(download_auto);
        a.layout.harmony_check_hours = Some(match interval {
            0 => 1,
            2 => 24,
            _ => 6,
        });
        if was_auto && !download_auto && a.updates.downloading {
            a.updates.cancel();
        }
        if a.updates.staged.is_some() && was_auto != download_auto {
            a.updates.armed = download_auto;
        }
        a.layout.write(&a.root);
        refresh(&a);
        drop(a);
        set_timer(&adapter);
        set_idle_timer(&adapter);
        if start_now {
            check(&adapter);
        } else if download_now {
            download(&adapter);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_restarts_wait_twice_as_long_after_sending_and_zero_waits_forever() {
        let now = Instant::now();
        let delay = Duration::from_secs(300);
        let ago = |seconds| now.checked_sub(Duration::from_secs(seconds));
        assert!(!quiet_long_enough(now, ago(900), ago(900), Duration::ZERO));
        assert!(!quiet_long_enough(now, ago(299), None, delay));
        assert!(!quiet_long_enough(now, ago(300), ago(599), delay));
        assert!(quiet_long_enough(now, ago(300), ago(600), delay));
        assert!(quiet_long_enough(now, ago(300), None, delay));
    }

    #[test]
    fn cancellation_stops_the_observed_worker_and_ignores_its_late_result() {
        use std::sync::mpsc;

        let runtime = BackgroundRuntime::new().unwrap();
        let (started, start) = mpsc::channel();
        let (observing, observed) = mpsc::channel();
        let (done, result) = mpsc::channel();
        let worker = runtime.spawn(async move {
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let old = worker.abort_handle();
        let observer = runtime.spawn(async move {
            observing.send(()).unwrap();
            done.send(worker.await.unwrap_err().is_cancelled()).unwrap();
        });
        let timeout = Duration::from_secs(3);
        start.recv_timeout(timeout).unwrap();
        observed.recv_timeout(timeout).unwrap();

        let mut updates = Updates::default();
        updates.pending = Some(old.clone());
        updates.cancel();
        let next = runtime.spawn(std::future::pending::<()>());
        let current = next.abort_handle();
        updates.pending = Some(current.clone());
        assert!(result.recv_timeout(timeout).unwrap());
        assert!(!updates.complete(&old));
        assert!(updates.complete(&current));
        runtime.block_on(observer).unwrap();
    }

    #[test]
    fn updating_preferences_require_a_real_window_and_survive_reopening_settings() {
        i_slint_backend_testing::init_no_event_loop();
        let root = tempfile::tempdir().unwrap();
        let window = YapfastWindow::new().unwrap();
        let adapter = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            root.path().into(),
            &window,
        )
        .unwrap();
        bind(&window, &adapter);
        let about = window.global::<HarmonyAbout>();
        about.invoke_save_update_options(true, true, 0);
        assert!(checks(&adapter.borrow().layout));
        assert!(downloads(&adapter.borrow().layout));
        assert!(adapter.borrow().updates.pending.is_none());
        {
            let mut a = adapter.borrow_mut();
            a.updates.active = true;
            a.updates.last_input.set(Some(Instant::now()));
        }
        about.invoke_save_update_options(false, true, 2);
        about.invoke_arm_install(true);
        assert!(!adapter.borrow().updates.armed);
        let saved = Layout::read(root.path());
        assert_eq!(saved.harmony_check_hours, Some(24));
        assert!(downloads(&saved));
        assert!(!checks(&saved));
        let restored = YapfastWindow::new().unwrap();
        bind(&restored, &adapter);
        assert_eq!(restored.global::<HarmonyAbout>().get_interval_index(), 2);
        assert!(
            restored
                .global::<HarmonyAbout>()
                .get_download_automatically()
        );
    }
}
