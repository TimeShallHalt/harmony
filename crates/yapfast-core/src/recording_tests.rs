use super::*;

struct Generated {
    frames: usize,
    control: Arc<AtomicU8>,
    cancel: bool,
    fail: bool,
}
impl SampleSource for Generated {
    fn read(&mut self, frame: &mut [f32]) -> anyhow::Result<()> {
        self.frames += 1;
        if self.fail {
            anyhow::bail!("fixture device disconnected");
        }
        frame.fill(0.2);
        if self.frames == 10 {
            self.control
                .store(if self.cancel { CANCEL } else { STOP }, Ordering::SeqCst);
        }
        Ok(())
    }
}

#[test]
fn explicit_stop_returns_review_audio_without_uploading() {
    let control = Arc::new(AtomicU8::new(RUN));
    let device = Generated {
        frames: 0,
        control: control.clone(),
        cancel: false,
        fail: false,
    };
    let note = record_from(|| Ok(Box::new(device)), &control, |_| {})
        .unwrap()
        .unwrap();
    assert_eq!(
        note.duration_secs(),
        0.18,
        "exclude the read that observes Stop"
    );
    let mut samples = 0;
    note.play_frames(|frame| {
        samples += frame.len();
        Ok(true)
    })
    .unwrap();
    assert_eq!(samples, 8640);
}

#[test]
fn cancellation_discards_every_sample_and_does_not_reopen() {
    let control = Arc::new(AtomicU8::new(RUN));
    let device = Generated {
        frames: 0,
        control: control.clone(),
        cancel: true,
        fail: false,
    };
    assert!(
        record_from(|| Ok(Box::new(device)), &control, |_| {})
            .unwrap()
            .is_none()
    );
    assert!(
        record_from(
            || panic!("must not open a cancelled device"),
            &control,
            |_| {}
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn unavailable_device_fails_instead_of_using_another_microphone() {
    let control = Arc::new(AtomicU8::new(RUN));
    let result = record_from(
        || anyhow::bail!("fixture selected device is gone"),
        &control,
        |_| {},
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("selected device is gone")
    );
    let device = Generated {
        frames: 0,
        control: control.clone(),
        cancel: false,
        fail: true,
    };
    let result = record_from(|| Ok(Box::new(device)), &control, |_| {});
    assert!(result.unwrap_err().to_string().contains("disconnected"));
}

#[test]
fn owned_fixture_worker_can_stop_review_and_cancel_without_touching_devices() {
    use std::sync::mpsc;
    let mut builder = Builder::new().unwrap();
    builder.push(&[0.25; FRAME]).unwrap();
    let note = builder.finish().unwrap();
    let terminal = serde_json::to_string(&Update::Finished(Ok(Some(note.clone())))).unwrap();
    for cancel in [false, true] {
        let script = format!(
            "import sys\nsys.stdin.readline()\nprint('{{\"Progress\":0}}', flush=True)\nsys.stdin.readline()\nprint({terminal:?}, flush=True)\n"
        );
        let mut command = Command::new(if cfg!(windows) { "python" } else { "python3" });
        command.args(["-c", &script]);
        let (tx, rx) = mpsc::channel();
        let worker = Worker::with_command(
            Job::Record {
                device: "GENERATED FIXTURE ONLY".into(),
            },
            command,
            move |update| {
                let _ = tx.send(update);
            },
        )
        .unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Update::Progress(0)
        ));
        if cancel {
            drop(worker);
            assert!(matches!(
                rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                Update::Finished(Err(_))
            ));
        } else {
            worker.stop();
            let Update::Finished(Ok(Some(review))) =
                rx.recv_timeout(Duration::from_secs(5)).unwrap()
            else {
                panic!("fixture review did not finish")
            };
            assert_eq!(review.data(), note.data());
        }
    }
}

#[test]
fn unresponsive_protocol_is_size_bounded() {
    let mut reader = std::io::Cursor::new(vec![b'x'; MAX_WIRE + 2]);
    assert!(read_report(&mut reader).is_err());
}
