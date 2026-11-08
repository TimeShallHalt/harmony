//! Narrow native About/Updates integration. No idle timer, account API or quit.
use super::*;
use crate::harmony_update::{
    controller::{Controller, startup_due},
    gh::Gh,
    transaction::Phase,
};

fn present(window: &YapfastWindow, controller: &Controller) {
    let state = controller.snapshot();
    let ui = window.global::<HarmonyUpdates>();
    ui.set_running_info(controller.current.diagnostics().into());
    ui.set_status(state.status.into());
    ui.set_busy(controller.busy());
    ui.set_available(state.candidate.is_some());
    ui.set_offered_build(
        state
            .candidate
            .as_ref()
            .map(|c| {
                format!(
                    "Available: Harmony {} / build {} / commit {}",
                    c.manifest.version, c.manifest.build_id, c.manifest.commit
                )
            })
            .unwrap_or_default()
            .into(),
    );
    ui.set_staged(state.staged.is_some());
    ui.set_may_arm(
        state
            .staged
            .as_ref()
            .is_some_and(|(_, p)| matches!(p.phase, Phase::Staged | Phase::Applying)),
    );
    ui.set_may_discard(
        state
            .staged
            .as_ref()
            .is_some_and(|(_, p)| p.phase != Phase::Applying),
    );
    ui.set_armed(state.staged.as_ref().is_some_and(|(_, p)| p.armed));
    ui.set_staged_build(
        state
            .staged
            .as_ref()
            .map(|(_, p)| {
                format!(
                    "Staged: Harmony {} / build {} / commit {}",
                    p.manifest.version, p.manifest.build_id, p.manifest.commit
                )
            })
            .unwrap_or_default()
            .into(),
    );
}
fn spawn(controller: Arc<Controller>, work: impl FnOnce(&Controller) + Send + 'static) {
    let error_to = controller.clone();
    if std::thread::Builder::new()
        .name("harmony-update".into())
        .spawn(move || work(&controller))
        .is_err()
    {
        error_to.status("Could not start update worker. Nothing installed.");
    }
}
fn preferences(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let adapter = adapter.borrow();
    let prefs = &adapter.layout.harmony_updates;
    let ui = window.global::<HarmonyUpdates>();
    ui.set_check_on_startup(prefs.automatic_checks_enabled());
    ui.set_download_automatically(prefs.automatic_downloads_enabled());
}
fn request_check(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>, automatic: bool) {
    let mut state = adapter.borrow_mut();
    if state.harmony_updates.busy()
        || (automatic && !state.layout.harmony_updates.automatic_checks_enabled())
    {
        return;
    }
    let mut next = state.layout.clone();
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        state
            .harmony_updates
            .status("System clock unavailable; check deferred.");
        return;
    };
    next.harmony_updates.last_check_unix = Some(now.as_secs());
    if let Err(error) = next.try_write(&state.root) {
        state.harmony_updates.status(format!(
            "Check deferred: could not persist startup backoff ({error})."
        ));
        return;
    }
    let download = automatic && next.harmony_updates.automatic_downloads_enabled();
    state.layout = next;
    let controller = state.harmony_updates.clone();
    drop(state);
    preferences(window, adapter);
    spawn(controller, move |controller| {
        let _ = controller.check(&Gh, download);
    });
}
pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    let controller = adapter.borrow().harmony_updates.clone();
    // Weak Slint handles cross threads only to enqueue a UI event, never to
    // access a component from a worker. Rebinding replaces the old observer.
    let weak = window.as_weak();
    let updates = Arc::downgrade(&controller);
    controller.observe(Box::new(move || {
        let weak = weak.clone();
        let updates = updates.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let (Some(window), Some(controller)) = (weak.upgrade(), updates.upgrade()) {
                present(&window, &controller);
            }
        });
    }));
    present(window, &controller);
    preferences(window, adapter);
    let weak = window.as_weak();
    window
        .global::<HarmonyUpdates>()
        .on_copy_diagnostics(move || {
            if let Some(window) = weak.upgrade() {
                window.invoke_copy_text(
                    crate::harmony_update::BuildInfo::current()
                        .diagnostics()
                        .into(),
                );
            }
        });
    let weak = window.as_weak();
    let state = Rc::downgrade(adapter);
    window.global::<HarmonyUpdates>().on_check(move || {
        if let (Some(window), Some(state)) = (weak.upgrade(), state.upgrade()) {
            request_check(&window, &state, false);
        }
    });
    let updates = controller.clone();
    let weak = window.as_weak();
    window.global::<HarmonyUpdates>().on_download(move || {
        if weak.upgrade().is_some_and(|w| !w.get_preview()) {
            spawn(updates.clone(), |c| {
                let _ = c.download(&Gh);
            });
        } else {
            updates.status("Preview does not download or modify installation staging.");
        }
    });
    let updates = controller.clone();
    window.global::<HarmonyUpdates>().on_cancel(move || {
        updates.cancel();
    });
    // Authorization/discard are tiny atomic local transactions on the UI
    // thread: a subsequent full-quit event cannot overtake a queued disarm.
    let updates = controller.clone();
    let weak = window.as_weak();
    window.global::<HarmonyUpdates>().on_arm(move |enabled| {
        if weak.upgrade().is_some_and(|w| !w.get_preview()) {
            let _ = updates.arm(enabled);
        } else {
            updates.status("Preview cannot authorize installation; use normal Harmony.");
        }
    });
    let updates = controller.clone();
    let weak = window.as_weak();
    window.global::<HarmonyUpdates>().on_discard(move || {
        if weak.upgrade().is_some_and(|w| !w.get_preview()) {
            let _ = updates.discard();
        } else {
            updates.status("Preview does not modify installation staging.");
        }
    });
    let weak = window.as_weak();
    let state = Rc::downgrade(adapter);
    window.global::<HarmonyUpdates>().on_preferences_changed(move |check, download| {
        let (Some(window), Some(state)) = (weak.upgrade(), state.upgrade()) else { return; };
        let mut adapter = state.borrow_mut(); let mut next = adapter.layout.clone();
        next.harmony_updates.consent_version = 1;
        next.harmony_updates.check_on_startup = check; next.harmony_updates.download_automatically = download;
        match next.try_write(&adapter.root) {
            Ok(()) => { adapter.layout = next; adapter.harmony_updates.status("Update preferences saved. Checks occur only on app startup or manual request."); }
            Err(error) => adapter.harmony_updates.status(format!("Preferences unchanged: {error}")),
        }
        drop(adapter); preferences(&window, &state);
    });
}
pub(super) fn startup(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>, preview: bool) {
    if preview {
        adapter.borrow().harmony_updates.status("Preview: no automatic checks, staging or installation. Running build information is available.");
        return;
    }
    let updates = adapter.borrow().harmony_updates.clone();
    // Disk state is tiny, but still read off the UI thread. On completion,
    // enqueue exactly one startup-policy event; no recurring timer is created.
    let weak = window.as_weak();
    let state = Rc::downgrade(adapter);
    // The existing runtime is an event-driven bridge, not a UI polling loop.
    let waiting = adapter
        .borrow()
        .runtime
        .spawn_blocking(move || updates.load_pending());
    let _ = slint::spawn_local(async move {
        let _ = waiting.await;
        if let (Some(window), Some(state)) = (weak.upgrade(), state.upgrade()) {
            let due = {
                let state = state.borrow();
                let p = &state.layout.harmony_updates;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |v| v.as_secs());
                !state.harmony_updates.snapshot().staged.is_some()
                    && startup_due(p.automatic_checks_enabled(), now, p.last_check_unix)
            };
            if due {
                request_check(&window, &state, true);
            }
        }
    });
}
