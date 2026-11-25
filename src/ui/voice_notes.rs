//! Native composer recording adapter. No device opens while constructing UI.
use super::*;

#[cfg(feature = "discord")]
use std::sync::Mutex;
#[cfg(feature = "discord")]
use yapfast_core::{
    recording::{self, Job, Update, Worker},
    yapfast_discord::{message, voice_note::Note},
};

#[cfg(feature = "discord")]
#[derive(Default)]
struct Local {
    generation: u64,
    note: Option<Arc<Note>>,
    recording: bool,
    stopping: bool,
    playing: bool,
    millis: u32,
    problem: String,
}

#[cfg(feature = "discord")]
#[derive(Default)]
pub(super) struct Recorder {
    local: Arc<Mutex<Local>>,
    worker: Option<Worker>,
    devices: Vec<voice::Device>,
    room: Option<String>,
    account: Option<String>,
    request: String,
    sending: bool,
}

#[cfg(feature = "discord")]
impl Recorder {
    /// Also called when going to the tray: unseen recording must never continue.
    pub(super) fn discard(&mut self) {
        self.worker = None;
        let generation = self.local.lock().unwrap().generation.wrapping_add(1);
        *self.local.lock().unwrap() = Local {
            generation,
            ..Default::default()
        };
        self.room = None;
        self.account = None;
        self.sending = false;
    }

    fn spawn(&mut self, window: &YapfastWindow, job: Job) {
        self.worker = None;
        let (local, weak) = (self.local.clone(), window.as_weak());
        let generation = {
            let mut local = local.lock().unwrap();
            local.generation = local.generation.wrapping_add(1);
            local.problem.clear();
            local.recording = matches!(&job, Job::Record { .. });
            local.playing = !local.recording;
            local.stopping = false;
            local.millis = 0;
            local.generation
        };
        let result = Worker::start(job, move |update| {
            {
                let mut local = local.lock().unwrap();
                if local.generation != generation {
                    return;
                }
                match update {
                    Update::Progress(millis) => local.millis = millis,
                    Update::Finished(result) => {
                        local.recording = false;
                        local.playing = false;
                        local.stopping = false;
                        match result {
                            Ok(Some(note)) => local.note = Some(Arc::new(note)),
                            Ok(None) => {}
                            Err(error) => local.problem = error,
                        }
                    }
                }
            }
            // Only an event, not a polling loop. The callback owns the Rc adapter
            // on Slint's thread; no Rc crosses the worker/process boundary.
            let _ = weak.upgrade_in_event_loop(|window| {
                window.global::<VoiceNotes>().invoke_update_state()
            });
        });
        match result {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => {
                let mut local = self.local.lock().unwrap();
                local.recording = false;
                local.playing = false;
                local.problem = error.to_string();
            }
        }
    }
}

pub(super) fn bind(window: &YapfastWindow, adapter: &Rc<RefCell<Adapter>>) {
    #[cfg(not(feature = "discord"))]
    {
        let weak = window.as_weak();
        window.global::<VoiceNotes>().on_request_open(move || {
            if let Some(window) = weak.upgrade() {
                let ui = window.global::<VoiceNotes>();
                ui.set_open(true);
                ui.set_problem("This build has no Discord voice-message adapter.".into());
            }
        });
        let weak = window.as_weak();
        window.global::<VoiceNotes>().on_cancel(move || {
            if let Some(window) = weak.upgrade() {
                window.global::<VoiceNotes>().set_open(false);
            }
        });
        let _ = adapter;
    }
    #[cfg(feature = "discord")]
    {
        macro_rules! bind {
            ($callback:ident, $method:ident) => {
                let (adapter, weak) = (adapter.clone(), window.as_weak());
                window.global::<VoiceNotes>().$callback(move || {
                    if let Some(window) = weak.upgrade() {
                        adapter.borrow_mut().$method(&window);
                    }
                });
            };
        }
        bind!(on_request_open, open_voice_note);
        bind!(on_start_recording, start_voice_note);
        bind!(on_stop_recording, stop_voice_note);
        bind!(on_play_review, play_voice_note);
        bind!(on_stop_review, stop_voice_note_playback);
        bind!(on_save_review, save_voice_note);
        bind!(on_send_review, send_voice_note);
        bind!(on_cancel, discard_voice_note);
        bind!(on_update_state, show_voice_notes);
    }
}

impl Adapter {
    #[cfg(feature = "discord")]
    fn open_voice_note(&mut self, window: &YapfastWindow) {
        self.voice_notes.discard();
        self.voice_notes.room = self.app.state.selected.clone();
        self.voice_notes.account = self.app.state.discord_user.clone();
        self.voice_notes.request = message::nonce();
        self.voice_notes.devices.clear();
        let ui = window.global::<VoiceNotes>();
        ui.set_selected_device(-1);
        ui.set_open(true);
        self.show_voice_notes(window);
        if self.voice_note_capture_problem().is_none() {
            self.action(window, Action::Voice(voice::Command::Devices));
        }
    }

    #[cfg(feature = "discord")]
    fn sync_voice_note_devices(&mut self, window: &YapfastWindow) {
        let devices = &self.app.state.voice.microphones;
        if self.voice_notes.devices == *devices {
            return;
        }
        let ui = window.global::<VoiceNotes>();
        let previous = usize::try_from(ui.get_selected_device())
            .ok()
            .and_then(|index| self.voice_notes.devices.get(index))
            .map(|device| device.id.as_str());
        let selected = match previous.or(self.app.state.voice.microphone.as_deref()) {
            Some(id) => devices.iter().position(|device| device.id == id),
            None if self.voice_notes.devices.is_empty() && !devices.is_empty() => Some(0),
            None => None,
        };
        self.voice_notes.devices.clone_from(devices);
        ui.set_devices(ModelRc::new(VecModel::from(
            devices
                .iter()
                .map(|device| SharedString::from(device.name.as_str()))
                .collect::<Vec<_>>(),
        )));
        ui.set_selected_device(selected.map_or(-1, |index| index as i32));
    }

    #[cfg(feature = "discord")]
    fn discard_voice_note(&mut self, window: &YapfastWindow) {
        if self.voice_notes.sending {
            return;
        } // Send is irreversible once requested.
        self.voice_notes.discard();
        let ui = window.global::<VoiceNotes>();
        ui.set_open(false);
        ui.set_recording(false);
        ui.set_stopping(false);
        ui.set_playing(false);
        ui.set_ready(false);
        ui.set_waveform(ModelRc::default());
    }

    #[cfg(feature = "discord")]
    fn voice_note_capture_problem(&self) -> Option<String> {
        if !recording::AVAILABLE {
            return Some("Native audio recording is unavailable on this OS.".into());
        }
        if self.app.state.voice.phase != Phase::Disconnected {
            return Some("Recording and review playback are disabled during calls to preserve the call's audio devices.".into());
        }
        match self.voice_notes.room.as_deref() {
            Some(room) if self.voice_notes.account == self.app.state.discord_user => {
                self.app.voice_note_problem(room)
            }
            Some(_) => Some("Reopen the recorder for your current account.".into()),
            None => Some("Open an authorized conversation before recording.".into()),
        }
    }

    #[cfg(feature = "discord")]
    fn start_voice_note(&mut self, window: &YapfastWindow) {
        if self.voice_note_capture_problem().is_some() {
            self.show_voice_notes(window);
            return;
        }
        let index = window.global::<VoiceNotes>().get_selected_device();
        let Some(device) = usize::try_from(index)
            .ok()
            .and_then(|index| self.voice_notes.devices.get(index))
        else {
            self.voice_notes.local.lock().unwrap().problem =
                "Choose an existing microphone first.".into();
            self.show_voice_notes(window);
            return;
        };
        if !self
            .app
            .state
            .voice
            .microphones
            .iter()
            .any(|current| current.id == device.id)
        {
            self.voice_notes.local.lock().unwrap().problem = "That microphone is no longer listed. Close and reopen to refresh the list; no fallback device was opened.".into();
            self.show_voice_notes(window);
            return;
        }
        {
            let local = self.voice_notes.local.lock().unwrap();
            if local.recording || local.playing || local.note.is_some() || self.voice_notes.sending
            {
                return;
            }
        }
        self.voice_notes.spawn(
            window,
            Job::Record {
                device: device.id.clone(),
            },
        );
        self.show_voice_notes(window);
    }

    #[cfg(feature = "discord")]
    fn stop_voice_note(&mut self, window: &YapfastWindow) {
        if let Some(worker) = &self.voice_notes.worker {
            worker.stop();
            self.voice_notes.local.lock().unwrap().stopping = true;
        }
        self.show_voice_notes(window);
    }

    #[cfg(feature = "discord")]
    fn play_voice_note(&mut self, window: &YapfastWindow) {
        if self.voice_note_capture_problem().is_some() || self.voice_notes.sending {
            return;
        }
        let note = self.voice_notes.local.lock().unwrap().note.clone();
        if let Some(note) = note {
            self.voice_notes.spawn(
                window,
                Job::Play {
                    note: (*note).clone(),
                    speakers: self.app.state.voice.speaker.clone(),
                },
            );
            self.show_voice_notes(window);
        }
    }

    #[cfg(feature = "discord")]
    fn stop_voice_note_playback(&mut self, window: &YapfastWindow) {
        self.voice_notes.worker = None;
        let mut local = self.voice_notes.local.lock().unwrap();
        local.generation = local.generation.wrapping_add(1);
        local.playing = false;
        drop(local);
        self.show_voice_notes(window);
    }

    #[cfg(feature = "discord")]
    fn send_voice_note(&mut self, window: &YapfastWindow) {
        if self.voice_notes.account != self.app.state.discord_user {
            self.discard_voice_note(window);
            return;
        }
        let local = self.voice_notes.local.lock().unwrap();
        if self.voice_notes.sending || local.recording || local.playing {
            return;
        }
        let (Some(note), Some(room)) = (local.note.clone(), self.voice_notes.room.clone()) else {
            return;
        };
        drop(local);
        if let Some(problem) = self.app.voice_note_problem(&room) {
            self.voice_notes.local.lock().unwrap().problem = problem;
            self.show_voice_notes(window);
            return;
        }
        let request = self.voice_notes.request.clone();
        self.action(
            window,
            Action::SendVoiceNote {
                room,
                account: self.voice_notes.account.clone().unwrap_or_default(),
                request: request.clone(),
                note,
            },
        );
        self.voice_notes.sending = self
            .app
            .state
            .voice_note_delivery
            .as_ref()
            .is_some_and(|delivery| delivery.request == request);
        if !self.voice_notes.sending
            && let Some(problem) = &self.app.state.notice
        {
            self.voice_notes
                .local
                .lock()
                .unwrap()
                .problem
                .clone_from(problem);
        }
        self.show_voice_notes(window);
    }

    #[cfg(feature = "discord")]
    fn save_voice_note(&mut self, window: &YapfastWindow) {
        if self.voice_notes.sending {
            return;
        }
        let local = self.voice_notes.local.lock().unwrap();
        let Some(note) = local.note.clone() else {
            return;
        };
        let generation = local.generation;
        drop(local);
        let state = self.voice_notes.local.clone();
        let (runtime, weak) = (self.runtime.clone(), window.as_weak());
        self.dialog(async move {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("voice-message.ogg")
                .add_filter("Ogg Opus audio", &["ogg"])
                .save_file()
                .await
            {
                if state.lock().unwrap().generation != generation {
                    return;
                }
                let path = file.path().to_owned();
                let result = runtime.spawn_blocking(move || note.save(&path)).await;
                if let Some(window) = weak.upgrade() {
                    window.global::<VoiceNotes>().set_status(match result {
                        Ok(Ok(())) => "Saved Ogg/Opus audio.".into(),
                        _ => "Could not save the voice note.".into(),
                    });
                }
            }
        });
    }

    /// Called on native events and composer changes, never global polling.
    pub(super) fn show_voice_notes(&mut self, window: &YapfastWindow) {
        #[cfg(feature = "discord")]
        {
            let ui = window.global::<VoiceNotes>();
            ui.set_available(
                self.app
                    .state
                    .selected_room()
                    .is_some_and(|room| room.network == Network::Discord)
                    && self.app.state.can_write()
                    && !self.app.state.preview,
            );
            if !ui.get_open() {
                return;
            }
            if self.voice_notes.room != self.app.state.selected
                || self.voice_notes.account != self.app.state.discord_user
            {
                self.voice_notes.discard();
                ui.set_open(false);
                return;
            }
            if self.voice_notes.sending {
                if let Some(delivery) = self
                    .app
                    .state
                    .voice_note_delivery
                    .as_ref()
                    .filter(|delivery| delivery.request == self.voice_notes.request)
                {
                    if let Some(result) = &delivery.result {
                        self.voice_notes.sending = false;
                        match result {
                            Ok(()) => {
                                self.voice_notes.discard();
                                ui.set_open(false);
                                return;
                            }
                            Err(error) => {
                                self.voice_notes.local.lock().unwrap().problem = error.clone()
                            }
                        }
                    }
                }
            }
            self.sync_voice_note_devices(window);
            let capture_problem = self.voice_note_capture_problem();
            let mut local = self.voice_notes.local.lock().unwrap();
            if capture_problem.is_some() && (local.recording || local.playing) {
                self.voice_notes.worker = None;
                local.generation = local.generation.wrapping_add(1);
                local.recording = false;
                local.playing = false;
                local.stopping = false;
                local.problem = "Stopped local recording/playback because the call or conversation capability changed. No audio was uploaded.".into();
            }
            if !local.recording && !local.playing && !local.stopping {
                self.voice_notes.worker = None;
            }
            ui.set_destination(self.app.state.upload_destination().into());
            ui.set_can_record(capture_problem.is_none());
            ui.set_can_send(
                self.voice_notes
                    .room
                    .as_deref()
                    .is_some_and(|room| self.app.voice_note_problem(room).is_none()),
            );
            ui.set_recording(local.recording);
            ui.set_stopping(local.stopping);
            ui.set_playing(local.playing);
            ui.set_ready(local.note.is_some());
            ui.set_sending(self.voice_notes.sending);
            ui.set_problem(if local.problem.is_empty() {
                capture_problem.unwrap_or_default().into()
            } else {
                local.problem.as_str().into()
            });
            let seconds = local.millis / 1000;
            ui.set_status(if let Some(note) = &local.note {
                format!(
                    "{:.1} seconds  ·  {}",
                    note.duration_secs(),
                    yapfast_core::chat::file_size(note.data().len() as u64)
                )
                .into()
            } else if local.recording {
                format!("{}:{:02} / 20:00", seconds / 60, seconds % 60).into()
            } else {
                if self.voice_notes.devices.is_empty() {
                    "No microphone available.".into()
                } else {
                    "Ready to record.".into()
                }
            });
            if let Some(note) = &local.note {
                // A maximum of 256 tiny peaks; no PCM or frames enter Slint.
                ui.set_waveform(ModelRc::new(VecModel::from(
                    note.waveform()
                        .iter()
                        .map(|&peak| i32::from(peak))
                        .collect::<Vec<_>>(),
                )));
            }
        }
        #[cfg(not(feature = "discord"))]
        let _ = window;
    }
}

#[cfg(all(test, feature = "discord"))]
mod tests {
    use super::*;
    #[test]
    fn voice_note_review_is_discarded_when_the_connected_account_changes() {
        i_slint_backend_testing::init_no_event_loop();
        let data = tempfile::tempdir().unwrap();
        let window = YapfastWindow::new().unwrap();
        let adapter = Adapter::new(
            Arc::new(BackgroundRuntime::new().unwrap()),
            data.path().into(),
            &window,
        )
        .unwrap();
        bind(&window, &adapter);
        {
            let mut a = adapter.borrow_mut();
            a.app.state.selected = Some("discord:@me:30".into());
            a.app.state.discord_user = Some("1".into());
        }
        let ui = window.global::<VoiceNotes>();
        ui.invoke_request_open();
        assert!(ui.get_open());
        let mut encoder = yapfast_core::yapfast_discord::voice_note::Builder::new().unwrap();
        encoder.push(&[0.25; 960]).unwrap();
        let mut a = adapter.borrow_mut();
        a.voice_notes.local.lock().unwrap().note = Some(Arc::new(encoder.finish().unwrap()));
        a.show_voice_notes(&window);
        assert!(ui.get_ready());
        a.app.state.discord_user = Some("2".into());
        a.show_voice_notes(&window);
        assert!(!ui.get_open());
        assert!(a.voice_notes.local.lock().unwrap().note.is_none());
    }
}
