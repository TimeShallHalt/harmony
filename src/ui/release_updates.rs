use super::*;
use crate::harmony_updates::{self, Release, Staged};
use yapfast_core::runtime::AbortHandle;

#[derive(Default)]
pub(super) struct Updates {
    active: bool,
    timer: slint::Timer,
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

    pub(super) fn finish(&mut self) -> anyhow::Result<()> {
        if self.armed
            && let Some(staged) = self.staged.take()
        {
            staged.launch()?;
        }
        Ok(())
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        if let Some(worker) = self.pending.take() {
            worker.abort();
        }
    }
}

fn show(a: &Adapter, window: &YapfastWindow) {
    let view = window.global::<HarmonyAbout>();
    let state = &a.updates;
    view.set_check_automatically(a.layout.harmony_release_checks);
    view.set_download_automatically(a.layout.harmony_release_downloads);
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
    adapter.borrow_mut().updates.active = true;
    set_timer(adapter);
    if adapter.borrow().layout.harmony_release_checks {
        check(adapter);
    }
}

fn set_timer(adapter: &Rc<RefCell<Adapter>>) {
    let a = adapter.borrow();
    a.updates.timer.stop();
    if a.updates.active && a.layout.harmony_release_checks {
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
                if a.layout.harmony_release_checks {
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
        let automatic = a.layout.harmony_release_downloads
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
                a.updates.status = "Signature verified. The update is ready to install.".into();
            }
            Ok(Err(error)) => a.updates.status = format!("Could not prepare this update. {error}"),
            Err(_) => a.updates.status = "Download interrupted.".into(),
        }
        refresh(&a);
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
            let mut a = a.borrow_mut();
            a.updates.armed = a.updates.active && arm && a.updates.staged.is_some();
            refresh(&a);
        }
    });
    let weak = Rc::downgrade(adapter);
    view.on_save_update_options(move |checks, downloads, interval| {
        let Some(adapter) = weak.upgrade() else {
            return;
        };
        let mut a = adapter.borrow_mut();
        if !a.updates.active {
            return;
        }
        let start_now = checks && !a.layout.harmony_release_checks;
        let was_auto = a.layout.harmony_release_downloads;
        let download_now = downloads && !was_auto;
        a.layout.harmony_release_checks = checks;
        a.layout.harmony_release_downloads = downloads;
        a.layout.harmony_check_hours = Some(match interval {
            0 => 1,
            2 => 24,
            _ => 6,
        });
        if was_auto && !downloads && a.updates.downloading {
            a.updates.cancel();
        }
        a.layout.write(&a.root);
        refresh(&a);
        drop(a);
        set_timer(&adapter);
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
        assert!(!adapter.borrow().layout.harmony_release_checks);
        assert!(!adapter.borrow().layout.harmony_release_downloads);
        assert!(adapter.borrow().updates.pending.is_none());
        adapter.borrow_mut().updates.active = true;
        about.invoke_save_update_options(false, true, 2);
        about.invoke_arm_install(true);
        assert!(!adapter.borrow().updates.armed);
        let saved = Layout::read(root.path());
        assert_eq!(saved.harmony_check_hours, Some(24));
        assert!(saved.harmony_release_downloads);
        assert!(!saved.harmony_release_checks);
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
