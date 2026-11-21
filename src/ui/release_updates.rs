use super::*;
use crate::harmony_updates::{self, Release, Staged};

#[derive(Default)]
pub(super) struct Updates {
    active: bool,
    timer: slint::Timer,
    pending: Option<slint::JoinHandle<()>>,
    downloading: bool,
    offered: Option<Release>,
    staged: Option<Staged>,
    armed: bool,
    status: String,
}

impl Updates {
    pub(super) fn finish(&mut self) -> anyhow::Result<()> {
        if self.armed
            && let Some(staged) = self.staged.take()
        {
            staged.launch()?;
        }
        Ok(())
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
        harmony_updates::WINDOWS_INSTALLER
            && state.offered.as_ref().is_some_and(|r| r.has_installer)
            && state.pending.is_none()
            && state.staged.is_none(),
    );
    view.set_windows_installer(harmony_updates::WINDOWS_INSTALLER);
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
    a.updates.status = "Checking for a Harmony release…".into();
    let weak = Rc::downgrade(adapter);
    a.updates.pending = slint::spawn_local(async move {
        let result = task.await;
        let Some(adapter) = weak.upgrade() else {
            return;
        };
        let mut a = adapter.borrow_mut();
        a.updates.pending = None;
        a.updates.offered = None;
        match result {
            Ok(Ok(Some(release))) => {
                a.updates.status = format!("{} is available.", release.tag);
                if !release.has_installer && harmony_updates::WINDOWS_INSTALLER {
                    a.updates
                        .status
                        .push_str(" This release has no signed installer.");
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
            && a.updates.offered.as_ref().is_some_and(|r| r.has_installer);
        refresh(&a);
        drop(a);
        if automatic {
            download(&adapter);
        }
    })
    .ok();
    refresh(&a);
}

fn download(adapter: &Rc<RefCell<Adapter>>) {
    let mut a = adapter.borrow_mut();
    if !harmony_updates::WINDOWS_INSTALLER
        || !a.updates.active
        || a.updates.pending.is_some()
        || a.updates.staged.is_some()
    {
        return;
    }
    let Some(release) = a.updates.offered.clone().filter(|r| r.has_installer) else {
        return;
    };
    let task = a.runtime.spawn(async move {
        let cache = yapfast_core::default_directory()?.join("update-cache");
        harmony_updates::download(&release, &cache).await
    });
    a.updates.downloading = true;
    a.updates.status = "Downloading and verifying Harmony Setup…".into();
    let weak = Rc::downgrade(adapter);
    a.updates.pending = slint::spawn_local(async move {
        let result = task.await;
        let Some(adapter) = weak.upgrade() else {
            return;
        };
        let mut a = adapter.borrow_mut();
        a.updates.pending = None;
        a.updates.downloading = false;
        match result {
            Ok(Ok(staged)) => {
                a.updates.staged = Some(staged);
                a.updates.status =
                    "Signature verified. You can run Setup the next time you quit Harmony.".into();
            }
            Ok(Err(error)) => a.updates.status = format!("Could not prepare this update. {error}"),
            Err(_) => a.updates.status = "Download interrupted.".into(),
        }
        refresh(&a);
    })
    .ok();
    refresh(&a);
}

fn cancel(a: &mut Adapter) {
    if let Some(job) = a.updates.pending.take() {
        job.abort();
    }
    a.updates.downloading = false;
    a.updates.status = "Canceled.".into();
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
            cancel(&mut a);
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
            cancel(&mut a);
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
