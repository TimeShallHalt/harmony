//! Non-movable PulseAudio input for explicitly chosen local recordings.
//! Calls keep using their existing recoverable streams; this one fails closed.
use anyhow::{Context as _, Result, anyhow, bail, ensure};
use libpulse_binding::{
    context::{Context, FlagSet as ContextFlags, State as ContextState},
    def::BufferAttr,
    mainloop::standard::{IterateResult, Mainloop},
    sample::{Format, Spec},
    stream::{FlagSet, PeekResult, State, Stream},
};
use std::{
    thread,
    time::{Duration, Instant},
};

fn recording_flags() -> FlagSet {
    FlagSet::DONT_MOVE
}

/// Independent local capture. Unlike Pulse's simple API, DONT_MOVE prevents a
/// hot-unplug from silently continuing on another microphone/monitor source.
pub(crate) struct PinnedInput {
    stream: Stream,
    context: Context,
    mainloop: Mainloop,
    left: Vec<u8>,
}

impl PinnedInput {
    pub fn open(
        microphone: Option<&str>,
        name: &'static str,
        rate: u32,
        channels: u16,
        fragment: Duration,
    ) -> Result<Self> {
        let id = microphone.context("Choose an existing microphone first")?;
        ensure!(
            id.starts_with(crate::voice::NAMED),
            "Unrecognized PulseAudio microphone identifier"
        );
        let source = super::pulse::source(Some(id))?;
        let mut mainloop = Mainloop::new().context("No PulseAudio main loop")?;
        let mut context =
            Context::new(&mainloop, "Harmony voice note").context("No PulseAudio context")?;
        context
            .connect(None, ContextFlags::NOFLAGS, None)
            .map_err(|e| anyhow!("{e}"))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match context.get_state() {
                ContextState::Ready => break,
                ContextState::Failed | ContextState::Terminated => {
                    bail!("The sound server refused voice-note recording")
                }
                _ => pump(&mut mainloop, deadline)?,
            }
        }
        let spec = Spec {
            format: Format::FLOAT32NE,
            channels: u8::try_from(channels)?,
            rate,
        };
        let mut stream = Stream::new(&mut context, name, &spec, None)
            .context("No PulseAudio recording stream")?;
        let bytes = super::Format::of::<f32>(rate, channels).bytes(fragment);
        let attributes = BufferAttr {
            maxlength: u32::try_from(bytes * 4)?,
            tlength: u32::MAX,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: u32::try_from(bytes)?,
        };
        stream
            .connect_record(source.as_deref(), Some(&attributes), recording_flags())
            .map_err(|e| anyhow!("{e}"))?;
        loop {
            match stream.get_state() {
                State::Ready => break,
                State::Failed | State::Terminated => {
                    bail!("The selected microphone is unavailable")
                }
                _ => pump(&mut mainloop, deadline)?,
            }
        }
        Ok(Self {
            stream,
            context,
            mainloop,
            left: Vec::new(),
        })
    }

    pub fn read(&mut self, samples: &mut [f32]) -> Result<()> {
        let bytes = super::bytes_mut(samples);
        let mut filled = self.left.len().min(bytes.len());
        bytes[..filled].copy_from_slice(&self.left[..filled]);
        self.left.drain(..filled);
        let deadline = Instant::now() + Duration::from_secs(2);
        while filled < bytes.len() {
            ensure!(
                self.stream.get_state() == State::Ready,
                "The selected microphone disconnected"
            );
            let packet = match self.stream.peek().map_err(|e| anyhow!("{e}"))? {
                PeekResult::Empty => {
                    pump(&mut self.mainloop, deadline)?;
                    continue;
                }
                PeekResult::Hole(_) => bail!("Microphone audio was interrupted"),
                PeekResult::Data(data) => {
                    ensure!(data.len() <= 65_536, "Microphone buffer exceeded its bound");
                    data.to_vec()
                }
            };
            self.stream.discard().map_err(|e| anyhow!("{e}"))?;
            let wanted = packet.len().min(bytes.len() - filled);
            bytes[filled..filled + wanted].copy_from_slice(&packet[..wanted]);
            filled += wanted;
            self.left.extend_from_slice(&packet[wanted..]);
        }
        Ok(())
    }
}

fn pump(mainloop: &mut Mainloop, deadline: Instant) -> Result<()> {
    ensure!(
        Instant::now() < deadline,
        "The selected microphone or sound server stopped responding"
    );
    match mainloop.iterate(false) {
        IterateResult::Success(_) => {}
        _ => bail!("The PulseAudio recording main loop stopped"),
    }
    thread::sleep(Duration::from_millis(5));
    Ok(())
}

impl Drop for PinnedInput {
    fn drop(&mut self) {
        let _ = self.stream.disconnect();
        self.context.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recordings_cannot_be_moved_to_a_different_source() {
        assert!(recording_flags().contains(FlagSet::DONT_MOVE));
    }
}
