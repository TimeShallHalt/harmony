//! Exact entry on the production native volume control; no audio backend.
use slint::ComponentHandle;
use std::{cell::RefCell, rc::Rc};
slint::slint! {
    import { ExactVolume } from "../../ui/personal-controls.slint";
    export component VolumeHarness inherits Window {
        preferred-width: 320px; preferred-height: 280px;
        in-out property <int> voice: 100;
        in-out property <int> stream: 54;
        in-out property <string> voice-entry <=> voice-control.entry;
        in-out property <string> stream-entry <=> stream-control.entry;
        out property <string> error: voice-control.error;
        callback parse(string) -> int;
        callback commit(bool, int);
        public function focus-voice() { voice-control.focus-entry(); }
        public function focus-stream() { stream-control.focus-entry(); }
        VerticalLayout {
            voice-control := ExactVolume {
                label: "User volume"; target: "fixture"; value <=> root.voice;
                parse(s) => { return root.parse(s); }
                committed(v) => { root.commit(false, v); }
            }
            stream-control := ExactVolume {
                label: "Stream volume"; target: "fixture"; value <=> root.stream;
                parse(s) => { return root.parse(s); }
                committed(v) => { root.commit(true, v); }
            }
        }
    }
}
#[test]
fn native_exact_entries_commit_independently_on_enter_and_reject_invalid_input() {
    i_slint_backend_testing::init_no_event_loop();
    let w = VolumeHarness::new().unwrap();
    w.on_parse(|text| yapfast_core::volume::parse_percentage(&text).unwrap_or(-1));
    let commits = Rc::new(RefCell::new(Vec::new()));
    let out = commits.clone();
    w.on_commit(move |stream, value| out.borrow_mut().push((stream, value)));
    w.show().unwrap();
    let enter = || {
        i_slint_core::properties::ChangeTracker::run_change_handlers();
        w.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "\n".into() });
    };
    w.set_voice_entry("NaN".into());
    w.invoke_focus_voice();
    enter();
    assert_eq!(w.get_voice(), 100);
    assert!(!w.get_error().is_empty());
    assert!(commits.borrow().is_empty());
    w.set_voice_entry("137".into());
    assert_eq!(
        w.get_voice(),
        100,
        "partially typed text never changes a gain"
    );
    enter();
    w.set_stream_entry("176".into());
    w.invoke_focus_stream();
    enter();
    assert_eq!((w.get_voice(), w.get_stream()), (137, 176));
    assert_eq!(&*commits.borrow(), &[(false, 137), (true, 176)]);
    for boundary in [0, 200] {
        w.set_voice_entry(boundary.to_string().into());
        w.invoke_focus_voice();
        enter();
        assert_eq!(w.get_voice(), boundary);
        assert_eq!(w.get_stream(), 176);
    }
}
