//! Explicit, bounded local voice-note capture/review. Never used by a call.
//!
//! Native I/O uses the shared WASAPI/Pulse adapters. An isolated owned child
//! makes cancellation reliable even when an OS driver blocks in open/read/write.
//! Only this child's PID is ever stopped; no call device or global setting changes.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};
use yapfast_discord::voice_note::{Builder, FRAME, MAX_BYTES, MAX_SECONDS, Note, RATE};

pub const ARGUMENT: &str = "--voice-note-worker";
pub const AVAILABLE: bool = cfg!(any(target_os = "windows", target_os = "linux"));
const RUN: u8 = 0;
const STOP: u8 = 1;
const CANCEL: u8 = 2;
const MAX_WIRE: usize = MAX_BYTES.div_ceil(3) * 4 + 16_384;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Job {
    Record {
        device: String,
    },
    Play {
        note: Note,
        speakers: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Update {
    Progress(u32),
    Finished(std::result::Result<Option<Note>, String>),
}

/// Handle to exactly one explicitly started capture or playback process.
struct OwnedChild(Mutex<Child>);
impl OwnedChild {
    fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, Child>> {
        self.0.lock()
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Ok(child) = self.0.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub struct Worker {
    child: Arc<OwnedChild>,
    finished: Arc<AtomicBool>,
    stop: std::sync::mpsc::Sender<()>,
}

impl Worker {
    pub fn start(job: Job, update: impl FnMut(Update) + Send + 'static) -> Result<Self> {
        let mut command = Command::new(std::env::current_exe()?);
        command.arg(ARGUMENT);
        Self::with_command(job, command, update)
    }

    fn with_command(
        job: Job,
        mut command: Command,
        mut update: impl FnMut(Update) + Send + 'static,
    ) -> Result<Self> {
        anyhow::ensure!(
            AVAILABLE,
            "Native audio recording is unsupported on this OS."
        );
        if let Job::Record { device } = &job {
            anyhow::ensure!(
                !device.is_empty() && device.len() <= 4096,
                "Choose an existing microphone first."
            );
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000 | 0x0000_4000); // no console, not a hidden recorder UI
        }
        let mut child = command
            .spawn()
            .context("Could not start the native voice-note worker")?;
        let stdout = child
            .stdout
            .take()
            .context("Voice-note worker has no output")?;
        let mut stdin = child
            .stdin
            .take()
            .context("Voice-note worker has no input")?;
        let child = Arc::new(OwnedChild(Mutex::new(child)));
        let (stop, stopped) = std::sync::mpsc::channel();
        // A long review can fill the child's pipe. Keep that write off the UI thread
        // so Cancel and window closure can always release the device.
        std::thread::Builder::new()
            .name("harmony-voice-note-input".into())
            .spawn(move || {
                if serde_json::to_writer(&mut stdin, &job).is_ok()
                    && stdin.write_all(b"\n").is_ok()
                    && stopped.recv().is_ok()
                {
                    let _ = stdin.write_all(b"stop\n");
                }
            })?;
        let finished = Arc::new(AtomicBool::new(false));
        let (owned, done) = (child.clone(), finished.clone());
        std::thread::Builder::new()
            .name("harmony-voice-note-result".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    let mut stdout = BufReader::new(stdout);
                    loop {
                        let report = read_report(&mut stdout)?;
                        let terminal = matches!(&report, Update::Finished(_));
                        update(report);
                        if terminal {
                            return Ok(());
                        }
                    }
                })();
                if let Err(error) = result {
                    update(Update::Finished(Err(error.to_string())));
                }
                done.store(true, Ordering::SeqCst);
                if let Ok(mut child) = owned.lock() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            })?;
        Ok(Self {
            child,
            finished,
            stop,
        })
    }

    /// Stop and review. A blocked driver is killed after two seconds, not
    /// replaced with another device or a fabricated recording.
    pub fn stop(&self) {
        let _ = self.stop.send(());
        let (child, finished) = (self.child.clone(), self.finished.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            if !finished.load(Ordering::SeqCst) {
                if let Ok(mut child) = child.lock() {
                    let _ = child.kill();
                }
            }
        });
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if !self.finished.load(Ordering::SeqCst) {
            if let Ok(mut child) = self.child.lock() {
                let _ = child.kill();
            }
        }
    }
}

fn read_report(reader: &mut impl BufRead) -> Result<Update> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_WIRE + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    anyhow::ensure!(
        !bytes.is_empty(),
        "Native voice-note worker stopped before finishing (device unavailable or stop timed out)."
    );
    anyhow::ensure!(
        bytes.len() <= MAX_WIRE && bytes.last() == Some(&b'\n'),
        "Invalid or oversized voice-note worker response."
    );
    let report: Update = serde_json::from_slice(&bytes)?;
    if let Update::Finished(Ok(Some(note))) = &report {
        note.validate()?;
    }
    Ok(report)
}

trait SampleSource {
    fn read(&mut self, frame: &mut [f32]) -> Result<()>;
}
struct Microphone(crate::audio_io::FixedInput);
impl SampleSource for Microphone {
    fn read(&mut self, frame: &mut [f32]) -> Result<()> {
        self.0.read(frame)
    }
}

fn record_from(
    open: impl FnOnce() -> Result<Box<dyn SampleSource>>,
    control: &AtomicU8,
    mut progress: impl FnMut(u32),
) -> Result<Option<Note>> {
    if control.load(Ordering::SeqCst) != RUN {
        return Ok(None);
    }
    let mut encoder = Builder::new()?;
    let mut input = open()?;
    let started = Instant::now();
    let mut frame = [0.0; FRAME];
    while control.load(Ordering::SeqCst) == RUN {
        input.read(&mut frame)?;
        if control.load(Ordering::SeqCst) != RUN {
            break;
        }
        let full = encoder.push(&frame)?;
        if encoder.samples() % (RATE / 5) == 0 {
            progress((encoder.samples() * 1000 / RATE) as u32);
        }
        if full || started.elapsed() >= Duration::from_secs(MAX_SECONDS as u64) {
            break;
        }
    }
    // Close the microphone before finalizing/returning anything for review.
    drop(input);
    if control.load(Ordering::SeqCst) == CANCEL {
        return Ok(None);
    }
    Ok(Some(encoder.finish()?))
}

fn report(update: &Update) -> Result<()> {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, update)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

/// Worker-only entry point, routed before UI/TLS/login/call initialization.
/// Invoked solely by a user pressing Record or Play in the recording dialog.
pub fn run() -> Result<()> {
    let mut first = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAX_WIRE + 1) as u64)
        .read_until(b'\n', &mut first)?;
    anyhow::ensure!(
        first.len() <= MAX_WIRE && first.last() == Some(&b'\n'),
        "Invalid voice-note job."
    );
    let job: Job = serde_json::from_slice(&first)?;
    let control = Arc::new(AtomicU8::new(RUN));
    let commands = control.clone();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        commands.store(
            if line.trim() == "stop" { STOP } else { CANCEL },
            Ordering::SeqCst,
        );
    });
    // Hard lifetime bound also covers a hung device open/read/drain.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(MAX_SECONDS as u64 + 5));
        std::process::exit(2);
    });
    let result = match job {
        Job::Record { device } => record_from(
            || {
                anyhow::ensure!(
                    !device.is_empty() && device.len() <= 4096,
                    "Choose an existing microphone first."
                );
                // Strict open: deliberately NOT LastingInput/open_or_default, which
                // silently recover by recording a different microphone.
                let input = crate::audio_io::FixedInput::open(
                    Some(&device),
                    "Harmony voice note (recording)",
                    RATE as u32,
                    1,
                    Duration::from_millis(20),
                )?;
                Ok(Box::new(Microphone(input)))
            },
            &control,
            |millis| {
                let _ = report(&Update::Progress(millis));
            },
        ),
        Job::Play { note, speakers } => (|| -> Result<Option<Note>> {
            if control.load(Ordering::SeqCst) != RUN {
                return Ok(None);
            }
            let mut output = crate::audio_io::Output::<i16>::open(
                speakers.as_deref(),
                "Harmony voice-note review",
                RATE as u32,
                1,
                Some(Duration::from_millis(40)),
            )?;
            note.play_frames(|frame| {
                if control.load(Ordering::SeqCst) != RUN {
                    return Ok(false);
                }
                output.write(frame)?;
                Ok(true)
            })?;
            if control.load(Ordering::SeqCst) == RUN {
                output.drain()?;
            } else {
                output.flush();
            }
            Ok(None)
        })(),
    };
    report(&Update::Finished(
        result.map_err(|error| format!("{error:#}")),
    ))
}

#[cfg(test)]
#[path = "recording_tests.rs"]
mod tests;
