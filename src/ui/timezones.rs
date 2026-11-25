//! Device-local contact clocks. No provider commands, HTTP, discovery or uploads.
use super::*;
use yapfast_core::local_timezones;

fn owner(state: &AppState, contact: &str) -> Option<String> {
    if state.preview || contact.is_empty() {
        return None;
    }
    if contact.starts_with('@') {
        state
            .account
            .as_ref()
            .filter(|id| !id.is_empty())
            .map(|id| format!("matrix:{id}"))
    } else if contact.bytes().all(|b| b.is_ascii_digit()) {
        state
            .discord_user
            .as_ref()
            .filter(|id| !id.is_empty())
            .map(|id| format!("discord:{id}"))
    } else {
        None
    }
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    for reset in [false, true] {
        let state = adapter.clone();
        let weak = window.as_weak();
        let commit = move |account: SharedString,
                           contact: SharedString,
                           name: Option<SharedString>|
              -> SharedString {
            let mut a = state.borrow_mut();
            // A stale editor cannot save into a new account or another profile.
            let visible = a
                .app
                .state
                .profile
                .as_ref()
                .is_some_and(|p| p.profile.id == contact.as_str())
                || a.app
                    .state
                    .panel_profile
                    .as_ref()
                    .is_some_and(|p| p.profile.id == contact.as_str());
            if !visible || owner(&a.app.state, &contact).as_deref() != Some(account.as_str()) {
                return "The account or contact changed. Reopen their profile.".into();
            }
            let mut next = a.layout.clone();
            if let Some(name) = name {
                if let Err(error) = next.local_timezones.set(&account, &contact, &name) {
                    return error.into();
                }
            } else {
                next.local_timezones.reset(&account, &contact);
            }
            if let Err(error) = next.try_write(&a.root) {
                return format!("Could not save the local timezone: {error}").into();
            }
            a.layout = next;
            if let Some(window) = weak.upgrade() {
                a.tick_timezones(&window);
            }
            SharedString::default()
        };
        if reset {
            window
                .global::<LocalTimezones>()
                .on_reset(move |account, contact| commit(account, contact, None));
        } else {
            window
                .global::<LocalTimezones>()
                .on_save(move |account, contact, name| commit(account, contact, Some(name)));
        }
    }
}

impl Adapter {
    fn timezone_labels(&self, window: &YapfastWindow) -> bool {
        let now = std::time::SystemTime::now();
        #[cfg(test)]
        let now = self.timezone_now.unwrap_or(now);
        let annotate = |mut data: ProfileData| {
            let account = owner(&self.app.state, &data.id);
            let saved = account
                .as_deref()
                .and_then(|account| self.layout.local_timezones.get(account, &data.id));
            data.local_timezone_owner = account.unwrap_or_default().into();
            data.local_timezone = saved.unwrap_or_default().into();
            data.local_time = if data.local_timezone_owner.is_empty() {
                "Local timezone unavailable — sign in to this contact's account".into()
            } else {
                local_timezones::at_system_time(saved, now).into()
            };
            data
        };
        let mut ticking = false;
        if let Some(view) = &self.app.state.profile {
            let data = annotate(window.get_profile());
            ticking |= data.id == view.profile.id && !data.local_timezone.is_empty();
            if data != window.get_profile() {
                window.set_profile(data);
            }
        }
        if let Some(view) = &self.app.state.panel_profile {
            let data = annotate(window.get_panel_profile());
            ticking |= data.id == view.profile.id && !data.local_timezone.is_empty();
            if data != window.get_panel_profile() {
                window.set_panel_profile(data);
            }
        }
        ticking
    }

    /// No timer while profiles are closed or no timezone was explicitly saved.
    /// The callback resolves the adapter's current window after recreation.
    pub(super) fn tick_timezones(&mut self, window: &YapfastWindow) {
        if !self.timezone_labels(window) {
            self.timezone_clock.stop();
        } else if !self.timezone_clock.running() {
            let weak = self.weak.clone();
            self.timezone_clock.start(
                slint::TimerMode::Repeated,
                Duration::from_secs(60),
                move || {
                    if let Some(adapter) = weak.upgrade() {
                        let a = adapter.borrow();
                        if let Some(window) = a.window.upgrade() {
                            a.timezone_labels(&window);
                        }
                    }
                },
            );
        }
    }
}
