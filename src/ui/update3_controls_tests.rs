//! Real native editor + real local preference store; no service or account.
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc};
use yapfast_core::local_timezones;
slint::slint! {
    import { TimezoneEditor } from "../../ui/personal-controls.slint";
    export { LocalTimezones } from "../../ui/personal-controls.slint";
    export component TimezoneHarness inherits Window {
        preferred-width: 320px; preferred-height: 220px;
        in-out property <string> owner: "discord:fixture-a";
        in-out property <string> contact: "fixture-contact";
        in-out property <string> saved;
        in-out property <string> entry <=> editor.entry;
        out property <string> error: editor.error;
        out property <string> clock: editor.clock;
        public function submit() { editor.submit(); }
        public function reset() { editor.reset(); }
        public function focus-entry() { editor.focus-entry(); }
        editor := TimezoneEditor {
            owner: root.owner; contact: root.contact; saved: root.saved;
            clock: "Timezone not set (local only)";
        }
    }
}
fn make() -> TimezoneHarness {
    thread_local! { static INIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    INIT.with(|init| {
        if !init.replace(true) {
            i_slint_backend_testing::init_no_event_loop();
        }
    });
    let w = TimezoneHarness::new().unwrap();
    w.show().unwrap();
    w
}
#[test]
fn native_timezone_edit_enter_validation_reset_and_target_change() {
    let w = make();
    let prefs = Rc::new(RefCell::new(local_timezones::Preferences::default()));
    let out = prefs.clone();
    w.global::<LocalTimezones>()
        .on_save(move |owner, contact, text| {
            out.borrow_mut()
                .set(&owner, &contact, &text)
                .err()
                .unwrap_or_default()
                .into()
        });
    let out = prefs.clone();
    w.global::<LocalTimezones>()
        .on_reset(move |owner, contact| {
            out.borrow_mut().reset(&owner, &contact);
            "".into()
        });
    w.set_entry("EST".into());
    w.invoke_submit();
    assert!(!w.get_error().is_empty());
    assert_eq!(
        prefs.borrow().get("discord:fixture-a", "fixture-contact"),
        None
    );
    w.set_entry("Europe/London".into());
    w.invoke_focus_entry();
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    w.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "\n".into() });
    assert!(w.get_error().is_empty());
    assert_eq!(
        prefs.borrow().get("discord:fixture-a", "fixture-contact"),
        Some("Europe/London")
    );
    w.set_entry("not/saved".into());
    w.set_owner("discord:fixture-b".into());
    i_slint_core::properties::ChangeTracker::run_change_handlers();
    assert_eq!(w.get_entry(), "");
    w.invoke_reset();
    assert_eq!(
        prefs.borrow().get("discord:fixture-a", "fixture-contact"),
        Some("Europe/London")
    );
    w.set_owner("discord:fixture-a".into());
    w.invoke_reset();
    assert_eq!(
        prefs.borrow().get("discord:fixture-a", "fixture-contact"),
        None
    );
    assert!(w.get_clock().contains("not set"));
}
