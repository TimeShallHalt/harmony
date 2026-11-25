//! Native compose/review/list callbacks and one-shot local queue wakeup.
use super::*;
use yapfast_core::scheduled::{self as queue, Clock, Queue, State, SystemClock};
struct Draft {
    account: String,
    room: String,
    id: Option<String>,
    sources: Vec<(PathBuf, bool)>,
    original: String,
}
struct Review {
    input: String,
    body: String,
    time: String,
    zone: String,
    due: i64,
    files: Vec<queue::Snapshot>,
}
pub(super) struct Controller {
    pub(super) queue: Option<Queue>,
    pub(super) clock: Rc<dyn Clock>,
    timer: slint::Timer,
    context: Option<(String, bool)>,
    draft: Option<Draft>,
    review: Option<Review>,
    error: String,
    root: PathBuf,
    /// A fresh profile creates its scheduler directory and OS key on first use.
    unopened: bool,
    store: std::sync::Arc<dyn queue::SecretStore>,
    unlinking: Option<String>,
    maintenance: Option<(String, String)>,
}
impl Controller {
    pub(super) fn new(root: &std::path::Path) -> Self {
        #[cfg(not(test))]
        let store = std::sync::Arc::new(queue::OsSecrets);
        #[cfg(test)]
        let store = queue::fixtures::keys();
        Self::with_store(root, store)
    }
    pub(super) fn with_store(
        root: &std::path::Path,
        store: std::sync::Arc<dyn queue::SecretStore>,
    ) -> Self {
        let (queue, error, unopened) = match root.join("scheduled").try_exists() {
            Ok(false) => (None, String::new(), true),
            Ok(true) => match Queue::open_with(root, store.clone()) {
                Ok(queue) => (Some(queue), String::new(), false),
                Err(error) => (None, error, false),
            },
            Err(error) => (None, error.to_string(), false),
        };
        Self {
            queue,
            clock: Rc::new(SystemClock),
            timer: slint::Timer::default(),
            context: None,
            draft: None,
            review: None,
            error,
            root: root.to_owned(),
            unopened,
            store,
            unlinking: None,
            maintenance: None,
        }
    }

    pub(super) fn open_on_demand(&mut self) -> Result<(), String> {
        if self.unopened {
            self.unopened = false;
            self.queue = Some(Queue::open_with(&self.root, self.store.clone())?);
        }
        Ok(())
    }
}
impl Adapter {
    fn clear_scheduled_sensitive(&mut self) {
        self.scheduled.timer.stop();
        self.scheduled.draft = None;
        self.scheduled.review = None;
        self.scheduled.maintenance = None;
        if let Some(w) = self.window.upgrade() {
            let ui = w.global::<Scheduling>();
            ui.set_confirmation("".into());
            ui.set_body("".into());
            ui.set_review_body("".into());
            ui.set_attachments("".into());
            ui.set_destination("".into());
            ui.set_time("".into());
            ui.set_utc("".into());
            ui.set_reviewed(false);
            ui.set_composing(false);
            ui.set_entries(ModelRc::default());
        }
    }
    fn request_scheduled_storage(&mut self, w: &YapfastWindow, kind: &str) -> Result<(), String> {
        let note = match kind {
            "rotate" => {
                "Rotate the scheduler-only OS key and re-encrypt all accounts transactionally? Accepted sends are cancelled locally and remain uncertain; no retry. Other accounts are preserved."
            }
            "purge" => {
                "Permanently remove this account’s local queue and owned file copies? Other accounts are preserved. Accepted delivery may already have happened; purge cannot unsend it."
            }
            "recover" => {
                "Authenticate committed storage and finish an interrupted transaction, or reopen after unlocking the OS key facility? No ambiguous send is retried; pending work from an interrupted transaction requires review/reschedule. Keep backups if data is corrupt."
            }
            "migrate" => {
                "Encrypt the legacy plaintext queue and owned files using a new OS-secure scheduler key? Original files are removed only after commit; migrated pending messages require explicit review/reschedule. Old backups may still contain plaintext."
            }
            _ => return Err("Unknown scheduler storage action.".into()),
        };
        self.scheduled.maintenance = Some((kind.into(), self.scheduled_account().into()));
        w.global::<Scheduling>().set_confirmation(note.into());
        Ok(())
    }
    fn confirm_scheduled_storage(&mut self, w: &YapfastWindow) -> Result<(), String> {
        let (kind, account) = self
            .scheduled
            .maintenance
            .take()
            .ok_or("Choose and confirm a scheduler storage action first.")?;
        w.global::<Scheduling>().set_confirmation("".into());
        if account != self.scheduled_account() {
            return Err("Account changed; storage action cancelled.".into());
        }
        self.clear_scheduled_sensitive();
        match kind.as_str() {
            "rotate" => self
                .scheduled
                .queue
                .as_mut()
                .ok_or("Queue unavailable.")?
                .rotate()?,
            "purge" => {
                if account.is_empty() {
                    return Err("Sign into the owning account before purging.".into());
                }
                self.scheduled
                    .queue
                    .as_mut()
                    .ok_or("Queue unavailable.")?
                    .purge(&account)?;
            }
            "recover" | "migrate" => {
                self.scheduled.queue = None;
                let root = self.scheduled.root.clone();
                let store = self.scheduled.store.clone();
                if kind == "migrate" {
                    Queue::migrate(&root, store.clone())?;
                } else {
                    Queue::recover(&root, store.clone())?;
                }
                self.scheduled.queue = Some(Queue::open_with(&root, store)?);
            }
            _ => return Err("Unknown scheduler storage action.".into()),
        }
        self.scheduled.error.clear();
        self.drive_scheduled();
        Ok(())
    }
    pub(super) fn scheduled_before_command(&mut self, command: &Command) -> Result<(), String> {
        if matches!(
            command,
            Command::Discord(yapfast_core::chat::DiscordCommand::Unlink)
        ) {
            let account = self.scheduled_account().to_owned();
            self.clear_scheduled_sensitive();
            self.scheduled.unlinking = Some(account.clone());
            if self.scheduled.unopened {
                return Ok(());
            }
            let result = self
                .scheduled
                .queue
                .as_mut()
                .ok_or(
                    "Scheduler unavailable: restore/recover its storage before unlinking."
                        .to_owned(),
                )
                .and_then(|q| q.purge(&account));
            if let Err(error) = result {
                self.scheduled.error = error.clone();
                self.app.state.notice = Some(error.clone());
                return Err(error);
            }
        }
        Ok(())
    }
    pub(super) fn scheduled_before_event(&mut self, event: &yapfast_core::chat::Event) {
        if matches!(event, yapfast_core::chat::Event::DiscordUnlinked) {
            if self.scheduled.unlinking.take().is_none() {
                let account = self.scheduled_account().to_owned();
                self.clear_scheduled_sensitive();
                if let Some(q) = self.scheduled.queue.as_mut() {
                    if let Err(error) = q.purge(&account) {
                        self.scheduled.error = error.clone();
                        self.app.state.notice = Some(error);
                    }
                }
            } else {
                self.clear_scheduled_sensitive();
            }
        }
    }
    fn scheduled_account(&self) -> &str {
        if self.app.state.preview {
            ""
        } else {
            self.app.state.discord_user.as_deref().unwrap_or_default()
        }
    }
    fn can_schedule(&self) -> bool {
        let s = &self.app.state;
        (self.scheduled.unopened
            || self
                .scheduled
                .queue
                .as_ref()
                .is_some_and(|q| q.problem().is_none()))
            && !self.scheduled_account().is_empty()
            && self.scheduled.unlinking.as_deref() != Some(self.scheduled_account())
            && s.can_write()
            && s.selected_room()
                .is_some_and(|room| room.network == yapfast_core::network::Network::Discord)
            && s.forum().is_none()
            && s.compose().is_none()
            && s.edit().is_none()
            && self.backend.is_some()
    }
    /// Adapter-owned: close-to-tray keeps the timer. Monotonic firing never
    /// authorizes an early send. No timer when empty/offline/another account.
    pub(super) fn drive_scheduled(&mut self) {
        self.scheduled.timer.stop();
        let account = self.scheduled_account().to_owned();
        let online = self.app.state.discord_online
            && !self.app.state.busy
            && self.backend.is_some()
            && !account.is_empty()
            && self.scheduled.unlinking.as_deref() != Some(account.as_str());
        if let Some((old_account, old_online)) = self.scheduled.context.clone()
            && old_online
            && (!online || old_account != account)
            && let Some(q) = self.scheduled.queue.as_mut()
        {
            match q.interrupt(
                &old_account,
                "Account disconnected or backend stopped after dispatch",
            ) {
                Ok(count) if count > 0 && old_account == account => {
                    if let Some(item) = q
                        .items()
                        .iter()
                        .find(|i| i.account == account && matches!(i.state, State::Uncertain(_)))
                    {
                        self.app.state.notice = Some(format!(
                            "Scheduled message {}: delivery uncertain after disconnect. Check Local scheduled messages; no automatic retry.",
                            item.id
                        ));
                    }
                }
                Err(error) => self.scheduled.error = error,
                _ => {}
            }
        }
        if self
            .scheduled
            .context
            .as_ref()
            .is_some_and(|old| old.0 != account)
        {
            self.scheduled.draft = None;
            self.scheduled.review = None;
            self.scheduled.maintenance = None;
            if let Some(w) = self.window.upgrade() {
                let ui = w.global::<Scheduling>();
                ui.set_composing(false);
                ui.set_reviewed(false);
                ui.set_body("".into());
                ui.set_review_body("".into());
                ui.set_attachments("".into());
                ui.set_destination("".into());
                ui.set_utc("".into());
                ui.set_time("".into());
                ui.set_zone("Local".into());
                ui.set_entries(ModelRc::default());
                ui.set_confirmation("".into());
            }
        }
        self.scheduled.context = Some((account.clone(), online));
        let clock = self.scheduled.clock.clone();
        let backend = &self.backend;
        let Some(q) = self.scheduled.queue.as_mut() else {
            if !account.is_empty() && !self.scheduled.error.is_empty() {
                self.app.state.notice = Some(format!(
                    "Scheduler storage unavailable: {}. Open Local scheduled messages; nothing will dispatch.",
                    self.scheduled.error
                ));
            }
            return;
        };
        let pending: Vec<_> = q
            .items()
            .iter()
            .filter(|i| i.account == account && i.state == State::Pending)
            .map(|i| i.id.clone())
            .collect();
        if let Err(e) = q.dispatch_due(&account, online, &*clock, |payload| match backend {
            Some(b) => b.send(Command::ScheduledSend(payload)).map_err(|_| {
                "Backend did not accept the command. Review and reschedule.".to_owned()
            }),
            None => Err("Authorized backend unavailable.".into()),
        }) {
            self.app.state.notice = Some(format!(
                "Scheduler storage/claim failure: {e}. Open Local scheduled messages; dispatch is paused."
            ));
            self.scheduled.error = e;
        }
        if let Some(item) = q
            .items()
            .iter()
            .find(|i| pending.contains(&i.id) && matches!(i.state, State::Blocked(_)))
            && let State::Blocked(error) = &item.state
        {
            self.app.state.notice = Some(format!(
                "Scheduled message {} was not sent: {error}. Open Local scheduled messages.",
                item.id
            ));
        }
        if let Some(delay) = q.next_delay(&account, online, &*clock) {
            let weak = self.weak.clone();
            self.scheduled.timer.start(
                slint::TimerMode::SingleShot,
                delay.max(Duration::from_millis(1)),
                move || {
                    if let Some(a) = weak.upgrade() {
                        let mut a = a.borrow_mut();
                        a.drive_scheduled();
                        if let Some(w) = a.window.upgrade() {
                            a.show_scheduled(&w);
                            a.show_composer(&w);
                        }
                    }
                },
            );
        }
    }
    pub(super) fn scheduled_context_changed(&mut self) {
        let current = (
            self.scheduled_account().to_owned(),
            self.app.state.discord_online
                && !self.app.state.busy
                && self.backend.is_some()
                && !self.scheduled_account().is_empty(),
        );
        if self.scheduled.context.as_ref() != Some(&current) {
            self.drive_scheduled();
        }
    }
    pub(super) fn scheduled_result(&mut self, account: &str, id: &str, result: Result<(), String>) {
        if let Err(e) = &result
            && account == self.scheduled_account()
        {
            let diagnostic: String = e.chars().take(1000).collect();
            self.app.state.notice = Some(format!(
                "Scheduled message {id}: {diagnostic}. Check Local scheduled messages; no automatic retry."
            ));
        }
        if let Some(q) = self.scheduled.queue.as_mut()
            && let Err(e) = q.complete(account, id, result)
        {
            self.scheduled.error = e;
        }
        self.drive_scheduled();
    }
    pub(super) fn show_scheduled(&self, window: &YapfastWindow) {
        window.set_notice(self.app.state.notice.as_deref().unwrap_or_default().into());
        let ui = window.global::<Scheduling>();
        let account = self.scheduled_account();
        ui.set_review_body(
            self.scheduled
                .review
                .as_ref()
                .map_or("", |r| r.body.as_str())
                .into(),
        );
        ui.set_available(!account.is_empty());
        ui.set_storage_ready(
            self.scheduled
                .queue
                .as_ref()
                .is_some_and(|q| q.problem().is_none()),
        );
        ui.set_can_schedule(self.can_schedule());
        ui.set_error(
            self.scheduled
                .queue
                .as_ref()
                .and_then(|q| q.problem())
                .unwrap_or(self.scheduled.error.as_str())
                .into(),
        );
        ui.set_connection(if account.is_empty() {
            "Sign in to the owning Discord account to view its local queue.".into()
        } else {
            format!(
                "Local queue · account {account} · {}",
                if self.app.state.discord_online && self.backend.is_some() {
                    "connected"
                } else {
                    "offline: pending messages wait for reconnect"
                }
            )
            .into()
        });
        let rows: Vec<_> = self
            .scheduled
            .queue
            .iter()
            .flat_map(|q| q.items())
            .filter(|i| i.account == account)
            .map(|i| {
                let status = match &i.state {
                    State::Pending => {
                        if self.app.state.discord_online {
                            "Pending — app must stay running".into()
                        } else {
                            "Pending — waiting for this account to reconnect".into()
                        }
                    }
                    State::InFlight => {
                        "Dispatched — too late to cancel; waiting for service result".into()
                    }
                    State::Sent => "Sent — service confirmed".into(),
                    State::Cancelled => "Cancelled locally — never dispatched".into(),
                    State::Blocked(e) => format!("Not sent: {e}"),
                    State::Uncertain(e) => format!("Delivery uncertain: {e}"),
                };
                ScheduledRow {
                    id: i.id.as_str().into(),
                    destination: self.schedule_destination(&i.room).into(),
                    due: format!(
                        "{} · local {}",
                        queue::utc_label(i.due_ms),
                        queue::local_input(i.due_ms)
                    )
                    .into(),
                    body: format!("{}{}", i.body, attachment_summary(&i.files)).into(),
                    status: status.into(),
                    editable: matches!(i.state, State::Pending | State::Blocked(_)),
                    cancellable: matches!(i.state, State::Pending | State::Blocked(_)),
                    dismissible: matches!(
                        i.state,
                        State::Sent | State::Cancelled | State::Uncertain(_)
                    ),
                }
            })
            .collect();
        ui.set_entries(ModelRc::new(VecModel::from(rows)));
    }
    fn schedule_destination(&self, room: &str) -> String {
        self.app
            .state
            .rooms
            .iter()
            .find(|r| r.id == room)
            .map_or_else(
                || format!("{room} (not currently available)"),
                |r| format!("{} · {room}", r.name),
            )
    }
    fn open_schedule(&mut self, window: &YapfastWindow, id: Option<&str>) -> Result<(), String> {
        let ui = window.global::<Scheduling>();
        self.scheduled.review = None;
        let account = self.scheduled_account().to_owned();
        if account.is_empty() {
            return Err("Sign into Discord to schedule a message.".into());
        }
        if let Some(id) = id {
            let i = self
                .scheduled
                .queue
                .iter()
                .flat_map(|q| q.items())
                .find(|i| {
                    i.id == id
                        && i.account == account
                        && matches!(i.state, State::Pending | State::Blocked(_))
                })
                .cloned()
                .ok_or("Only this account's undispatched messages can be edited.")?;
            ui.set_body(i.body.as_str().into());
            ui.set_time(queue::local_input(i.due_ms).into());
            ui.set_zone("Local".into());
            ui.set_attachments(attachment_summary(&i.files).into());
            self.scheduled.draft = Some(Draft {
                account,
                room: i.room,
                id: Some(i.id),
                sources: vec![],
                original: i.body,
            });
        } else {
            if !self.can_schedule() {
                return Err("Select a writable Discord conversation, with no reply/edit active. Preview cannot schedule account messages.".into());
            }
            self.scheduled.open_on_demand()?;
            let s = &self.app.state;
            let room = s.selected.clone().unwrap();
            ui.set_body(s.draft().into());
            ui.set_time(
                queue::local_input(self.scheduled.clock.now_ms().saturating_add(3_600_000)).into(),
            );
            ui.set_zone("Local".into());
            let files = s
                .staged()
                .iter()
                .map(|f| (f.path.clone(), f.spoiler))
                .collect::<Vec<_>>();
            ui.set_attachments(source_summary(&files).into());
            self.scheduled.draft = Some(Draft {
                account,
                room,
                id: None,
                sources: files,
                original: s.draft().to_owned(),
            });
        }
        ui.set_destination(
            self.schedule_destination(&self.scheduled.draft.as_ref().unwrap().room)
                .into(),
        );
        ui.set_composing(true);
        ui.set_open(true);
        ui.set_reviewed(false);
        ui.set_utc("".into());
        Ok(())
    }
    fn review_schedule(&mut self, w: &YapfastWindow) -> Result<(), String> {
        let ui = w.global::<Scheduling>();
        self.scheduled.review = None;
        ui.set_reviewed(false);
        let d = self
            .scheduled
            .draft
            .as_ref()
            .ok_or("No scheduled draft is open.")?;
        if d.account != self.scheduled_account() {
            return Err("Account changed; open a new schedule.".into());
        }
        let due = queue::parse_due(ui.get_time().as_str(), ui.get_zone().as_str())?;
        let count = if let Some(id) = &d.id {
            self.scheduled
                .queue
                .iter()
                .flat_map(|q| q.items())
                .find(|i| &i.id == id)
                .map_or(0, |i| i.files.len())
        } else {
            d.sources.len()
        };
        let input = ui.get_body();
        let (text, silent) = queue::sending_text(input.as_str());
        // Only resolve names against the intended recipient. Unedited entries
        // already carry the exact mentions that were originally reviewed.
        let already_wired = d.id.is_some() && input.as_str() == d.original;
        let content =
            if !already_wired && self.app.state.selected.as_deref() == Some(d.room.as_str()) {
                self.app.outgoing_body(&d.room, text)
            } else if self.app.state.strip_tracking {
                yapfast_core::links::without_tracking(text).into_owned()
            } else {
                text.to_owned()
            };
        let body = if silent {
            format!("@silent {content}")
        } else {
            content
        };
        queue::validate(
            &d.account,
            &d.room,
            &body,
            due,
            count,
            &*self.scheduled.clock,
        )?;
        let files = queue::inspect_sources(&d.sources)?;
        if !files.is_empty() {
            ui.set_attachments(attachment_summary(&files).into());
        }
        self.scheduled.review = Some(Review {
            input: input.to_string(),
            body,
            time: ui.get_time().to_string(),
            zone: ui.get_zone().to_string(),
            due,
            files,
        });
        ui.set_utc(queue::utc_label(due).into());
        ui.set_reviewed(true);
        Ok(())
    }
    fn save_schedule(&mut self, w: &YapfastWindow) -> Result<(), String> {
        let ui = w.global::<Scheduling>();
        let r = self
            .scheduled
            .review
            .as_ref()
            .filter(|r| {
                r.input == ui.get_body().as_str()
                    && r.time == ui.get_time().as_str()
                    && r.zone == ui.get_zone().as_str()
            })
            .ok_or("Review the content, attachments and UTC due time before saving.")?;
        let d = self
            .scheduled
            .draft
            .as_ref()
            .ok_or("No scheduled draft is open.")?;
        if d.account != self.scheduled_account() {
            return Err("Account changed; open a new schedule.".into());
        }
        queue::read_attachments(&r.files)?;
        let q = self
            .scheduled
            .queue
            .as_mut()
            .ok_or("Durable local queue unavailable.")?;
        if let Some(id) = &d.id {
            q.edit(&d.account, id, &r.body, r.due, &*self.scheduled.clock)?;
        } else {
            q.schedule_reviewed(
                &d.account,
                &d.room,
                &r.body,
                r.due,
                &r.files,
                &*self.scheduled.clock,
            )?;
            let room = d.room.clone();
            let original = d.original.clone();
            let paths = d.sources.clone();
            // Clear only captured content after durable save; new edits/files
            // stay. Nothing calls the immediate Send action.
            if self.app.state.selected.as_deref() == Some(room.as_str()) {
                if self.app.state.draft() == original {
                    let effects = self
                        .app
                        .dispatch(yapfast_core::application::Action::SetDraft(String::new()));
                    self.execute(effects);
                }
                let indices: Vec<_> = self
                    .app
                    .state
                    .staged()
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| paths.iter().any(|(p, s)| p == &f.path && *s == f.spoiler))
                    .map(|(i, _)| i)
                    .collect();
                for i in indices.into_iter().rev() {
                    self.app
                        .dispatch(yapfast_core::application::Action::Unstage(i));
                }
                self.show_composer(w);
            }
        }
        self.scheduled.draft = None;
        self.scheduled.review = None;
        ui.set_composing(false);
        ui.set_reviewed(false);
        ui.set_body("".into());
        self.drive_scheduled();
        Ok(())
    }
}
fn source_summary(files: &[(PathBuf, bool)]) -> String {
    if files.is_empty() {
        return "No attachments.".into();
    }
    files
        .iter()
        .map(|(p, spoiler)| {
            format!(
                "{}{} — {}",
                if *spoiler { "[spoiler] " } else { "" },
                p.file_name().unwrap_or_default().to_string_lossy(),
                std::fs::metadata(p)
                    .ok()
                    .filter(|m| m.is_file())
                    .map_or_else(
                        || "unavailable".into(),
                        |m| format!("{} bytes; local copy saved on Schedule", m.len())
                    )
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn attachment_summary(files: &[queue::Snapshot]) -> String {
    if files.is_empty() {
        return String::new();
    }
    format!(
        "\nAttachments: {}",
        files
            .iter()
            .map(|f| format!(
                "{}{} ({} bytes; {})",
                if f.spoiler { "[spoiler] " } else { "" },
                f.name,
                f.bytes,
                if f.path.is_file() {
                    "local snapshot"
                } else {
                    "unavailable"
                }
            ))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    window
        .global::<Scheduling>()
        .set_zones(ModelRc::new(VecModel::from(
            queue::timezone_choices()
                .into_iter()
                .map(slint::SharedString::from)
                .collect::<Vec<_>>(),
        )));
    {
        let weak = window.as_weak();
        let a = Rc::downgrade(adapter);
        window.global::<Scheduling>().on_pick_zone(move |index| {
            if let (Some(w), Some(a)) = (weak.upgrade(), a.upgrade()) {
                let ui = w.global::<Scheduling>();
                let zone = usize::try_from(index)
                    .ok()
                    .and_then(|index| ui.get_zones().row_data(index));
                let mut a = a.borrow_mut();
                a.scheduled.review = None;
                ui.set_reviewed(false);
                if let Some(zone) = zone {
                    ui.set_zone(zone);
                    ui.set_zone_index(index);
                    a.scheduled.error.clear();
                } else {
                    a.scheduled.error = "Choose a valid timezone; no selection was guessed.".into();
                }
                a.show_scheduled(&w);
            }
        });
    }
    macro_rules! bind {
        ($callback:ident, $body:expr) => {{
            let weak = window.as_weak();
            let a = Rc::downgrade(adapter);
            window.global::<Scheduling>().$callback(move || {
                if let (Some(w), Some(a)) = (weak.upgrade(), a.upgrade()) {
                    let mut a = a.borrow_mut();
                    let result: Result<(), String> = ($body)(&mut *a, &w);
                    if let Err(e) = result {
                        a.scheduled.error = e;
                    } else if a.scheduled.queue.is_some() {
                        a.scheduled.error.clear();
                    }
                    a.show_scheduled(&w);
                }
            });
        }};
    }
    {
        let weak = window.as_weak();
        let a = Rc::downgrade(adapter);
        window
            .global::<Scheduling>()
            .on_request_storage(move |kind| {
                if let (Some(w), Some(a)) = (weak.upgrade(), a.upgrade()) {
                    let mut a = a.borrow_mut();
                    if let Err(error) = a.request_scheduled_storage(&w, kind.as_str()) {
                        a.scheduled.error = error;
                    }
                    a.show_scheduled(&w);
                }
            });
    }
    bind!(on_confirm_storage, |a: &mut Adapter, w: &YapfastWindow| a
        .confirm_scheduled_storage(w));
    bind!(on_abort_storage, |a: &mut Adapter, w: &YapfastWindow| {
        a.scheduled.maintenance = None;
        w.global::<Scheduling>().set_confirmation("".into());
        Ok(())
    });
    bind!(on_compose, |a: &mut Adapter, w: &YapfastWindow| {
        w.global::<Scheduling>().set_open(true);
        a.open_schedule(w, None)
    });
    bind!(on_list, |a: &mut Adapter, w: &YapfastWindow| {
        a.scheduled.draft = None;
        a.scheduled.review = None;
        let ui = w.global::<Scheduling>();
        ui.set_composing(false);
        ui.set_open(true);
        if !a.scheduled_account().is_empty() {
            a.scheduled.open_on_demand()?;
        }
        Ok(())
    });
    bind!(on_close, |a: &mut Adapter, w: &YapfastWindow| {
        a.scheduled.draft = None;
        a.scheduled.review = None;
        w.global::<Scheduling>().set_open(false);
        Ok(())
    });
    bind!(on_changed, |a: &mut Adapter, w: &YapfastWindow| {
        a.scheduled.review = None;
        w.global::<Scheduling>().set_reviewed(false);
        Ok(())
    });
    bind!(on_review, |a: &mut Adapter, w: &YapfastWindow| a
        .review_schedule(w));
    bind!(on_save, |a: &mut Adapter, w: &YapfastWindow| a
        .save_schedule(w));
    {
        let weak = window.as_weak();
        let a = Rc::downgrade(adapter);
        window.global::<Scheduling>().on_edit(move |id| {
            if let (Some(w), Some(a)) = (weak.upgrade(), a.upgrade()) {
                let mut a = a.borrow_mut();
                a.scheduled.error = a
                    .open_schedule(&w, Some(id.as_str()))
                    .err()
                    .unwrap_or_default();
                a.show_scheduled(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        let a = Rc::downgrade(adapter);
        window.global::<Scheduling>().on_cancel(move |id| {
            if let (Some(w), Some(a)) = (weak.upgrade(), a.upgrade()) {
                let mut a = a.borrow_mut();
                let account = a.scheduled_account().to_owned();
                a.scheduled.error = a
                    .scheduled
                    .queue
                    .as_mut()
                    .ok_or("Queue unavailable.".to_owned())
                    .and_then(|q| q.cancel(&account, id.as_str()))
                    .err()
                    .unwrap_or_default();
                a.drive_scheduled();
                a.show_scheduled(&w);
            }
        });
    }
    {
        let weak = window.as_weak();
        let a = Rc::downgrade(adapter);
        window.global::<Scheduling>().on_dismiss(move |id| {
            if let (Some(w), Some(a)) = (weak.upgrade(), a.upgrade()) {
                let mut a = a.borrow_mut();
                let account = a.scheduled_account().to_owned();
                a.scheduled.error = a
                    .scheduled
                    .queue
                    .as_mut()
                    .ok_or("Queue unavailable.".to_owned())
                    .and_then(|q| q.dismiss(&account, id.as_str()))
                    .err()
                    .unwrap_or_default();
                a.show_scheduled(&w);
            }
        });
    }
    adapter.borrow_mut().scheduled_context_changed();
    adapter.borrow().show_scheduled(window);
}
