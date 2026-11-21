//! Production recording dialog, using Slint's in-memory backend only.
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::Cell, rc::Rc};

slint::slint! {
    #[edition = "1.19"]
    import { VoiceNotes, VoiceNoteDialog } from "../../ui/voice_notes.slint";
    export { VoiceNotes } from "../../ui/voice_notes.slint";
    export component VoiceNoteHarness inherits Window {
        preferred-width: 720px; preferred-height: 640px;
        public function record() { dialog.request-record(); }
        public function send() { dialog.request-send(); }
        dialog := VoiceNoteDialog { width: parent.width; height: parent.height; }
    }
}
fn make() -> VoiceNoteHarness {
    thread_local! { static INIT: Cell<bool> = const { Cell::new(false) }; }
    INIT.with(|init| {
        if !init.replace(true) {
            i_slint_backend_testing::init_no_event_loop();
        }
    });
    VoiceNoteHarness::new().unwrap()
}
#[test]
fn voice_note_dialog_never_starts_merely_by_opening_or_choosing_device() {
    let ui = make();
    let starts = Rc::new(Cell::new(0));
    let count = starts.clone();
    ui.global::<VoiceNotes>()
        .on_start_recording(move || count.set(count.get() + 1));
    let state = ui.global::<VoiceNotes>();
    state.set_open(true);
    state.set_devices(ModelRc::new(VecModel::from(vec![
        "Generated fixture device".into(),
    ])));
    state.set_selected_device(0);
    ui.show().unwrap();
    assert_eq!(starts.get(), 0);
    ui.invoke_record();
    assert_eq!(starts.get(), 0, "missing capability cannot record");
    state.set_can_record(true);
    ui.invoke_record();
    assert_eq!(starts.get(), 1, "only an explicit action may start");
    state.set_recording(true);
    ui.invoke_record();
    assert_eq!(starts.get(), 1, "cannot start a second recording");
}
#[test]
fn voice_note_dialog_upload_requires_review_and_explicit_send() {
    let ui = make();
    let sends = Rc::new(Cell::new(0));
    let count = sends.clone();
    ui.global::<VoiceNotes>()
        .on_send_review(move || count.set(count.get() + 1));
    let state = ui.global::<VoiceNotes>();
    state.set_open(true);
    state.set_can_send(true);
    ui.invoke_send();
    assert_eq!(sends.get(), 0);
    state.set_ready(true);
    assert_eq!(sends.get(), 0, "finishing recording must not upload");
    ui.invoke_send();
    assert_eq!(sends.get(), 1);
    state.set_sending(true);
    ui.invoke_send();
    assert_eq!(sends.get(), 1, "no duplicate while uploading");
}
