//! Headless native production scheduler callbacks; no OS input or live server.
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};
use yapfast_core::{application::Application, chat::Command};

slint::slint! {
    #[edition = "1.19"]
    import { Scheduling, ScheduledDialog, ScheduledRow } from "../../ui/scheduled.slint";
    export { Scheduling, ScheduledRow }
    export component YapfastWindow inherits Window {
        preferred-width: 900px; preferred-height: 700px;
        in property <string> notice;
        if Scheduling.open: ScheduledDialog { width: parent.width; height: parent.height; }
    }
}
#[path = "scheduled.rs"]
mod scheduled;
struct FakeBackend(Rc<RefCell<Vec<Command>>>);
impl FakeBackend {
    fn send(&self, command: Command) -> Result<(), Command> {
        self.0.borrow_mut().push(command);
        Ok(())
    }
}
struct Adapter {
    app: Application,
    scheduled: scheduled::Controller,
    backend: Option<FakeBackend>,
    weak: std::rc::Weak<RefCell<Self>>,
    window: slint::Weak<YapfastWindow>,
}
impl Adapter {
    fn show_composer(&mut self, _window: &YapfastWindow) {}
    fn execute(&mut self, effects: Vec<yapfast_core::application::Effect>) {
        for e in effects {
            if let yapfast_core::application::Effect::Backend(c) = e
                && let Some(b) = &self.backend
            {
                let _ = b;
                if self.scheduled_before_command(&c).is_ok() {
                    let _ = self.backend.as_ref().unwrap().send(c);
                }
            }
        }
    }
}
struct Time(Cell<i64>);
impl yapfast_core::scheduled::Clock for Time {
    fn now_ms(&self) -> i64 {
        self.0.get()
    }
}
fn fixture(
    root: &std::path::Path,
) -> (
    YapfastWindow,
    Rc<RefCell<Adapter>>,
    Rc<Time>,
    Rc<RefCell<Vec<Command>>>,
) {
    thread_local! { static READY: Cell<bool> = const { Cell::new(false) }; }
    READY.with(|ready| {
        if !ready.replace(true) {
            i_slint_backend_testing::init_no_event_loop();
        }
    });
    let window = YapfastWindow::new().unwrap();
    let clock = Rc::new(Time(Cell::new(
        yapfast_core::scheduled::parse_due("2026-11-07 12:00", "UTC").unwrap(),
    )));
    let commands = Rc::new(RefCell::new(vec![]));
    let adapter = Rc::new_cyclic(|weak| {
        let mut app = Application::default();
        app.dispatch(yapfast_core::application::Action::Preview);
        app.state.preview = false;
        app.state.discord_user = Some("123".into());
        app.state.discord_online = true;
        app.state.selected = Some("discord:10:11".into());
        app.state.room_ready = true;
        let mut room = (*app.state.rooms[0]).clone();
        room.id = "discord:10:11".into();
        room.network = yapfast_core::network::Network::Discord;
        room.forum = false;
        room.voice = false;
        room.read_only = false;
        room.locked = false;
        app.state.rooms = vec![std::sync::Arc::new(room)];
        app.dispatch(yapfast_core::application::Action::SetDraft(
            "Only after review".into(),
        ));
        let mut controller =
            scheduled::Controller::with_store(root, yapfast_core::scheduled::fixtures::keys());
        controller.clock = clock.clone();
        RefCell::new(Adapter {
            app,
            scheduled: controller,
            backend: Some(FakeBackend(commands.clone())),
            weak: weak.clone(),
            window: window.as_weak(),
        })
    });
    scheduled::bind(&window, &adapter);
    (window, adapter, clock, commands)
}
fn schedule_fixture(window: &YapfastWindow) {
    let ui = window.global::<Scheduling>();
    ui.invoke_compose();
    ui.set_time("2026-11-07 13:00".into());
    ui.set_zone("UTC".into());
    ui.invoke_review();
    assert!(ui.get_reviewed(), "{}", ui.get_error());
    ui.invoke_save();
    assert_eq!(ui.get_error(), "");
}
#[test]
fn native_unlink_command_purges_before_forwarding_and_clears_sensitive_review() {
    use yapfast_core::{
        application::Action,
        chat::DiscordCommand,
        scheduled::{Clock, State},
    };
    let root = tempfile::tempdir().unwrap();
    let (w, a, time, commands) = fixture(root.path());
    schedule_fixture(&w);
    {
        let mut a = a.borrow_mut();
        a.scheduled
            .queue
            .as_mut()
            .unwrap()
            .schedule(
                "456",
                "discord:20:21",
                "other account",
                time.now_ms() + 3_600_000,
                &[],
                &*time,
            )
            .unwrap();
    }
    w.global::<Scheduling>().invoke_compose();
    w.global::<Scheduling>().invoke_review();
    w.global::<Scheduling>()
        .invoke_request_storage("rotate".into());
    {
        let mut a = a.borrow_mut();
        a.app.state.discord_linked = true;
        a.app.state.account = Some("123".into());
        let effects = a.app.dispatch(Action::UnlinkDiscord);
        assert!(!effects.is_empty());
        a.execute(effects);
        let q = a.scheduled.queue.as_ref().unwrap();
        assert_eq!(q.items().len(), 1);
        assert_eq!(q.items()[0].account, "456");
        assert_eq!(q.items()[0].state, State::Pending);
    }
    assert_eq!(w.global::<Scheduling>().get_body(), "");
    assert!(!w.global::<Scheduling>().get_reviewed());
    assert!(
        w.global::<Scheduling>().get_confirmation().is_empty(),
        "unlink invalidates pending storage consent"
    );
    assert!(
        commands
            .borrow()
            .iter()
            .any(|c| matches!(c, Command::Discord(DiscordCommand::Unlink)))
    );
    time.0.set(time.now_ms() + 3_600_000);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(3600));
    slint::platform::update_timers_and_animations();
    assert!(
        commands
            .borrow()
            .iter()
            .all(|c| !matches!(c, Command::ScheduledSend(_)))
    );
}
#[test]
fn native_rotation_requires_confirmation_preserves_record_and_discloses_retention_and_frozen_files()
{
    let root = tempfile::tempdir().unwrap();
    let (w, a, _time, _commands) = fixture(root.path());
    schedule_fixture(&w);
    let ui = w.global::<Scheduling>();
    let before = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
    ui.invoke_request_storage("rotate".into());
    assert_eq!(
        std::fs::read(root.path().join("scheduled/queue.json")).unwrap(),
        before,
        "asking is not consent"
    );
    assert!(ui.get_confirmation().contains("Rotate"));
    ui.invoke_confirm_storage();
    assert_eq!(ui.get_error(), "");
    assert_ne!(
        std::fs::read(root.path().join("scheduled/queue.json")).unwrap(),
        before
    );
    assert_eq!(
        a.borrow().scheduled.queue.as_ref().unwrap().items().len(),
        1
    );
    assert!(ui.get_retention().contains("OS secure storage"));
    assert!(ui.get_retention().contains("until dismissed"));
    assert!(ui.get_attachment_policy().contains("frozen"));
}
#[test]
fn native_iana_picker_converts_review_and_rejects_dst_without_guessing() {
    let root = tempfile::tempdir().unwrap();
    let (w, _a, _time, _commands) = fixture(root.path());
    let ui = w.global::<Scheduling>();
    ui.invoke_compose();
    let zones = ui.get_zones();
    let index = (0..zones.row_count())
        .find(|i| zones.row_data(*i).unwrap() == "America/New_York")
        .expect("real IANA list");
    ui.invoke_pick_zone(index as i32);
    assert_eq!(ui.get_zone(), "America/New_York");
    ui.set_time("2026-11-08 12:00".into());
    ui.invoke_review();
    assert!(ui.get_reviewed(), "{}", ui.get_error());
    assert_eq!(ui.get_utc(), "2026-11-08 17:00:00 UTC");
    ui.set_time("2026-11-01 01:30".into());
    ui.invoke_changed();
    ui.invoke_review();
    assert!(!ui.get_reviewed());
    assert!(ui.get_error().contains("ambiguous"));
    ui.invoke_pick_zone(-1);
    assert_eq!(
        ui.get_zone(),
        "America/New_York",
        "invalid selection must not silently choose Local"
    );
}
#[test]
fn native_test_constructor_never_uses_a_real_os_key_facility() {
    let root = tempfile::tempdir().unwrap();
    let mut controller = scheduled::Controller::new(root.path());
    assert!(controller.queue.is_none());
    assert!(!root.path().join("scheduled").exists());
    controller.open_on_demand().unwrap();
    assert!(controller.queue.is_some());
    let bytes = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
    assert!(bytes.starts_with(b"YFS2"));
    let id = std::str::from_utf8(&bytes[4..36]).unwrap();
    assert!(
        yapfast_core::scheduled::SecretStore::get(&*yapfast_core::scheduled::fixtures::keys(), id)
            .is_ok(),
        "cfg(test) must inject fixture secrets, including full-app tests"
    );
}
#[test]
fn native_unlinked_event_purges_owned_claim_before_identity_is_cleared() {
    use yapfast_core::{chat::Event, scheduled::Clock};
    let root = tempfile::tempdir().unwrap();
    let (w, a, time, commands) = fixture(root.path());
    schedule_fixture(&w);
    {
        let mut a = a.borrow_mut();
        a.scheduled
            .queue
            .as_mut()
            .unwrap()
            .schedule(
                "456",
                "discord:20:21",
                "preserved",
                time.now_ms() + 3_600_000,
                &[],
                &*time,
            )
            .unwrap();
    }
    time.0.set(time.now_ms() + 3_600_000);
    a.borrow_mut().drive_scheduled();
    let claim = commands
        .borrow()
        .iter()
        .find_map(|c| match c {
            Command::ScheduledSend(p) => Some(p.claim.clone()),
            _ => None,
        })
        .unwrap();
    {
        let mut a = a.borrow_mut();
        let event = Event::DiscordUnlinked;
        a.scheduled_before_event(&event);
        a.app.apply(event);
        a.scheduled_context_changed();
        a.show_scheduled(&w);
        assert_eq!(a.scheduled.queue.as_ref().unwrap().items().len(), 1);
        assert_eq!(
            a.scheduled.queue.as_ref().unwrap().items()[0].account,
            "456"
        );
    }
    assert!(claim.is_cancelled());
    assert_eq!(w.global::<Scheduling>().get_body(), "");
    assert_eq!(w.global::<Scheduling>().get_entries().row_count(), 0);
}
#[test]
fn native_failed_purge_blocks_live_unlink_and_reports_durable_storage_pause() {
    use yapfast_core::{application::Action, chat::DiscordCommand, scheduled::SecretStore};
    let root = tempfile::tempdir().unwrap();
    let (w, a, _time, commands) = fixture(root.path());
    schedule_fixture(&w);
    let bytes = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
    yapfast_core::scheduled::fixtures::keys()
        .remove(std::str::from_utf8(&bytes[4..36]).unwrap())
        .unwrap();
    {
        let mut a = a.borrow_mut();
        a.app.state.discord_linked = true;
        a.app.state.account = Some("123".into());
        let effects = a.app.dispatch(Action::UnlinkDiscord);
        a.execute(effects);
        a.show_scheduled(&w);
    }
    assert!(!w.global::<Scheduling>().get_error().is_empty());
    assert!(
        commands
            .borrow()
            .iter()
            .all(|c| !matches!(c, Command::Discord(DiscordCommand::Unlink)))
    );
    assert!(root.path().join("scheduled/maintenance.json").exists());
    assert!(
        a.borrow()
            .scheduled
            .queue
            .as_ref()
            .unwrap()
            .problem()
            .is_some()
    );
}
#[test]
fn native_explicit_legacy_encryption_action_blocks_old_pending_authorization() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("scheduled");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("queue.json"),br#"{"version":1,"namespace":"0123456789abcdef","next":2,"items":[{"id":"0123456789abcdef00000001","account":"123","room":"discord:10:11","body":"generated migration fixture","due_ms":1060000,"state":"Pending","files":[]}]}"#).unwrap();
    let (w, a, _time, commands) = fixture(root.path());
    let ui = w.global::<Scheduling>();
    assert!(!ui.get_error().is_empty());
    ui.invoke_request_storage("migrate".into());
    assert!(ui.get_confirmation().contains("legacy"));
    ui.invoke_confirm_storage();
    assert_eq!(ui.get_error(), "");
    assert!(
        std::fs::read(folder.join("queue.json"))
            .unwrap()
            .starts_with(b"YFS2")
    );
    assert!(matches!(
        a.borrow().scheduled.queue.as_ref().unwrap().items()[0].state,
        yapfast_core::scheduled::State::Blocked(_)
    ));
    assert!(
        commands
            .borrow()
            .iter()
            .all(|c| !matches!(c, Command::ScheduledSend(_)))
    );
}
#[test]
fn native_key_unavailable_at_due_time_has_a_visible_notice_without_opening_queue() {
    use yapfast_core::scheduled::{Clock, SecretStore};
    let root = tempfile::tempdir().unwrap();
    let (w, a, time, commands) = fixture(root.path());
    schedule_fixture(&w);
    w.global::<Scheduling>().invoke_close();
    let bytes = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
    yapfast_core::scheduled::fixtures::keys()
        .remove(std::str::from_utf8(&bytes[4..36]).unwrap())
        .unwrap();
    time.0.set(time.now_ms() + 3_600_000);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(3600));
    slint::platform::update_timers_and_animations();
    assert!(
        commands
            .borrow()
            .iter()
            .all(|c| !matches!(c, Command::ScheduledSend(_)))
    );
    assert!(
        a.borrow()
            .app
            .state
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("key") || n.contains("storage")),
        "a hidden queue error is not visible failure"
    );
    assert!(
        !w.get_notice().is_empty(),
        "the production window banner must update on the timer, not wait for another backend event"
    );
}
#[test]
fn native_timer_rechecks_wall_clock_then_dispatches_real_command_once_with_visible_failure() {
    let root = tempfile::tempdir().unwrap();
    let (w, a, time, commands) = fixture(root.path());
    schedule_fixture(&w);
    let id = a.borrow().scheduled.queue.as_ref().unwrap().items()[0]
        .id
        .clone();
    time.0.set(time.0.get() - 86_400_000);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(3600));
    slint::platform::update_timers_and_animations();
    assert!(
        commands
            .borrow()
            .iter()
            .all(|c| !matches!(c, Command::ScheduledSend(_)))
    );
    time.0
        .set(yapfast_core::scheduled::parse_due("2026-11-07 13:00", "UTC").unwrap());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(300));
    slint::platform::update_timers_and_animations();
    assert_eq!(commands.borrow().iter().filter(|c| matches!(c, Command::ScheduledSend(p) if p.id == id && p.account == "123" && p.room == "discord:10:11")).count(), 1);
    a.borrow_mut()
        .scheduled_result("123", &id, Err("fixture transport timeout".into()));
    a.borrow().show_scheduled(&w);
    let row = w.global::<Scheduling>().get_entries().row_data(0).unwrap();
    assert!(row.status.contains("timeout"));
    assert!(!row.editable && !row.cancellable);
    assert!(
        a.borrow()
            .app
            .state
            .notice
            .as_deref()
            .is_some_and(|n| n.contains(&id)),
        "failure must be visible outside the queue"
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(3600));
    slint::platform::update_timers_and_animations();
    assert_eq!(
        commands
            .borrow()
            .iter()
            .filter(|c| matches!(c, Command::ScheduledSend(_)))
            .count(),
        1
    );
}
#[test]
fn native_switch_disconnect_reconnect_scopes_the_queue_without_duplicate_commands() {
    let root = tempfile::tempdir().unwrap();
    let (w, a, time, commands) = fixture(root.path());
    schedule_fixture(&w);
    {
        let mut a = a.borrow_mut();
        a.app.state.discord_user = Some("456".into());
        a.scheduled_context_changed();
        a.show_scheduled(&w);
    }
    assert_eq!(w.global::<Scheduling>().get_entries().row_count(), 0);
    time.0
        .set(yapfast_core::scheduled::parse_due("2026-11-07 13:00", "UTC").unwrap());
    {
        let mut a = a.borrow_mut();
        a.app.state.discord_user = Some("123".into());
        a.app.state.discord_online = false;
        a.scheduled_context_changed();
        a.show_scheduled(&w);
    }
    assert!(
        commands
            .borrow()
            .iter()
            .all(|c| !matches!(c, Command::ScheduledSend(_)))
    );
    {
        let mut a = a.borrow_mut();
        a.app.state.discord_online = true;
        a.scheduled_context_changed();
        a.scheduled_context_changed();
    }
    assert_eq!(
        commands
            .borrow()
            .iter()
            .filter(|c| matches!(c, Command::ScheduledSend(_)))
            .count(),
        1
    );
}
#[test]
fn native_queue_load_failure_stays_visible_after_opening_the_list() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("scheduled")).unwrap();
    std::fs::write(root.path().join("scheduled/queue.json"), b"not json").unwrap();
    let (w, _a, _t, _commands) = fixture(root.path());
    let ui = w.global::<Scheduling>();
    assert!(!ui.get_error().is_empty());
    ui.invoke_list();
    assert!(!ui.get_error().is_empty());
    assert!(!ui.get_can_schedule());
}
#[test]
fn native_attachment_review_cannot_silently_send_changed_source_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (w, a, _time, _commands) = fixture(root.path());
    let source = root.path().join("review.txt");
    std::fs::write(&source, b"reviewed").unwrap();
    a.borrow_mut()
        .app
        .dispatch(yapfast_core::application::Action::Stage(vec![
            source.clone(),
        ]));
    let ui = w.global::<Scheduling>();
    ui.invoke_compose();
    ui.set_time("2026-11-07 13:00".into());
    ui.set_zone("UTC".into());
    ui.invoke_review();
    assert!(ui.get_reviewed(), "{}", ui.get_error());
    std::fs::write(&source, b"replaced").unwrap();
    ui.invoke_save();
    assert!(!ui.get_error().is_empty());
    assert!(
        a.borrow()
            .scheduled
            .queue
            .as_ref()
            .unwrap()
            .items()
            .is_empty()
    );
}
#[test]
fn native_schedule_preserves_silent_control_and_normal_composer_tracking_policy() {
    let root = tempfile::tempdir().unwrap();
    let (w, a, _t, _commands) = fixture(root.path());
    {
        let mut a = a.borrow_mut();
        a.app
            .dispatch(yapfast_core::application::Action::StripTracking(true));
        a.app.dispatch(yapfast_core::application::Action::SetDraft(
            "@silent see https://a.example/p?utm_source=x&id=2".into(),
        ));
    }
    schedule_fixture(&w);
    assert_eq!(
        a.borrow().scheduled.queue.as_ref().unwrap().items()[0].body,
        "@silent see https://a.example/p?id=2"
    );
}
#[test]
fn native_review_normalizes_message_edits_before_save_even_from_another_conversation() {
    let root = tempfile::tempdir().unwrap();
    let (w, a, _t, _commands) = fixture(root.path());
    a.borrow_mut()
        .app
        .dispatch(yapfast_core::application::Action::StripTracking(true));
    let ui = w.global::<Scheduling>();
    ui.invoke_compose();
    ui.set_time("2026-11-07 13:00".into());
    ui.set_zone("UTC".into());
    let edited = "@silent see https://a.example/p?utm_source=x&id=2";
    ui.set_body(edited.into());
    ui.invoke_review();
    assert_eq!(ui.get_body(), edited);
    assert_eq!(ui.get_review_body(), "@silent see https://a.example/p?id=2");
    ui.invoke_save();
    let saved = a.borrow().scheduled.queue.as_ref().unwrap().items()[0].clone();
    assert_eq!(saved.body, "@silent see https://a.example/p?id=2");

    a.borrow_mut().app.state.selected = Some("discord:20:21".into());
    ui.invoke_edit(saved.id.into());
    ui.set_body("new https://a.example/p?utm_medium=x&id=3".into());
    ui.invoke_review();
    assert_eq!(ui.get_review_body(), "new https://a.example/p?id=3");
    ui.invoke_save();
    let a = a.borrow();
    let saved = &a.scheduled.queue.as_ref().unwrap().items()[0];
    assert_eq!(saved.body, "new https://a.example/p?id=3");
    assert_eq!(saved.room, "discord:10:11");
}
#[test]
fn native_backend_stop_has_a_visible_uncertain_record_and_never_requeues_it() {
    let root = tempfile::tempdir().unwrap();
    let (w, a, t, commands) = fixture(root.path());
    schedule_fixture(&w);
    t.0.set(yapfast_core::scheduled::parse_due("2026-11-07 13:00", "UTC").unwrap());
    a.borrow_mut().drive_scheduled();
    {
        let mut a = a.borrow_mut();
        a.backend = None;
        a.scheduled_context_changed();
        a.show_scheduled(&w);
    }
    assert!(matches!(
        a.borrow().scheduled.queue.as_ref().unwrap().items()[0].state,
        yapfast_core::scheduled::State::Uncertain(_)
    ));
    assert!(
        w.global::<Scheduling>()
            .get_entries()
            .row_data(0)
            .unwrap()
            .status
            .contains("uncertain")
    );
    {
        let mut a = a.borrow_mut();
        a.backend = Some(FakeBackend(commands.clone()));
        a.scheduled_context_changed();
    }
    assert_eq!(
        commands
            .borrow()
            .iter()
            .filter(|c| matches!(c, Command::ScheduledSend(_)))
            .count(),
        1
    );
}
#[test]
fn native_local_missing_attachment_failure_is_visible_without_opening_queue() {
    let root = tempfile::tempdir().unwrap();
    let (w, a, t, commands) = fixture(root.path());
    let source = root.path().join("snapshot.txt");
    std::fs::write(&source, b"fixture").unwrap();
    a.borrow_mut()
        .app
        .dispatch(yapfast_core::application::Action::Stage(vec![source]));
    schedule_fixture(&w);
    let item = a.borrow().scheduled.queue.as_ref().unwrap().items()[0].clone();
    std::fs::remove_file(&item.files[0].path).unwrap();
    t.0.set(yapfast_core::scheduled::parse_due("2026-11-07 13:00", "UTC").unwrap());
    a.borrow_mut().drive_scheduled();
    assert!(
        a.borrow()
            .app
            .state
            .notice
            .as_deref()
            .is_some_and(|n| n.contains(&item.id))
    );
    assert!(
        commands
            .borrow()
            .iter()
            .all(|c| !matches!(c, Command::ScheduledSend(_)))
    );
}
#[test]
fn native_persistence_fault_does_not_disappear_when_the_queue_is_reopened() {
    let root = tempfile::tempdir().unwrap();
    let (w, a, _t, _commands) = fixture(root.path());
    schedule_fixture(&w);
    let id = a.borrow().scheduled.queue.as_ref().unwrap().items()[0]
        .id
        .clone();
    let path = root.path().join("scheduled/queue.json");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(path).unwrap();
    let ui = w.global::<Scheduling>();
    ui.invoke_edit(id.into());
    ui.invoke_review();
    ui.invoke_save();
    assert!(!ui.get_error().is_empty());
    ui.invoke_list();
    assert!(!ui.get_error().is_empty());
}
#[test]
fn native_compose_review_save_list_edit_cancel_callbacks_are_reachable_and_durable() {
    let root = tempfile::tempdir().unwrap();
    let (window, adapter, _clock, commands) = fixture(root.path());
    let ui = window.global::<Scheduling>();
    ui.invoke_compose();
    assert!(ui.get_open() && ui.get_composing());
    assert_eq!(ui.get_body(), "Only after review");
    ui.set_time("2026-11-07 13:00".into());
    ui.set_zone("UTC".into());
    ui.invoke_save();
    assert!(
        adapter
            .borrow()
            .scheduled
            .queue
            .as_ref()
            .unwrap()
            .items()
            .is_empty(),
        "requires explicit review"
    );
    ui.invoke_review();
    assert!(ui.get_reviewed());
    assert!(ui.get_utc().contains("13:00:00 UTC"));
    ui.invoke_save();
    let id = adapter.borrow().scheduled.queue.as_ref().unwrap().items()[0]
        .id
        .clone();
    assert!(!ui.get_composing());
    assert!(
        commands
            .borrow()
            .iter()
            .all(|c| !matches!(c, Command::ScheduledSend(_)))
    );
    ui.invoke_list();
    assert_eq!(ui.get_entries().row_count(), 1);
    ui.invoke_edit(id.clone().into());
    ui.set_body("Edited local schedule".into());
    ui.invoke_changed();
    assert!(!ui.get_reviewed());
    ui.invoke_review();
    ui.invoke_save();
    assert_eq!(
        adapter.borrow().scheduled.queue.as_ref().unwrap().items()[0].body,
        "Edited local schedule"
    );
    ui.invoke_cancel(id.into());
    assert_eq!(
        adapter.borrow().scheduled.queue.as_ref().unwrap().items()[0].state,
        yapfast_core::scheduled::State::Cancelled
    );
    drop(adapter);
    drop(window);
    let reloaded = yapfast_core::scheduled::Queue::open_with(
        root.path(),
        yapfast_core::scheduled::fixtures::keys(),
    )
    .unwrap();
    assert_eq!(
        reloaded.items()[0].state,
        yapfast_core::scheduled::State::Cancelled
    );
}
