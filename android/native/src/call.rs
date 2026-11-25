//! Foreground voice and Go Live use the same sessions and transport as desktop.
use crate::{platform::Platform, shell::Command};
use anyhow::{Context, Result, ensure};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use yapfast_discord::{
    rtp::Codec,
    service::Discord,
    stream_connection::{Frame, StreamSender},
    voice::{self, Call, Listener, Microphone, Track},
};

#[derive(Default)]
pub struct Devices {
    microphone: Mutex<Option<(i64, Microphone)>>,
    stream: Mutex<Option<(i64, StreamSender)>>,
}
impl Devices {
    pub fn samples(&self, serial: i64, samples: &[i16]) {
        let mic = self.microphone.lock().unwrap();
        if let Some((_, input)) = mic.as_ref().filter(|(id, _)| *id == serial) {
            input.push(
                &samples
                    .iter()
                    .map(|v| *v as f32 / 32768.0)
                    .collect::<Vec<_>>(),
            );
        }
    }
    pub fn frame(&self, serial: i64, bytes: Vec<u8>, time_us: i64, keyframe: bool) -> bool {
        let stream = self.stream.lock().unwrap();
        let Some((_, sender)) = stream.as_ref().filter(|(id, _)| *id == serial) else {
            return false;
        };
        sender.send(Frame {
            data: bytes,
            codec: Codec::H264,
            timestamp: ((time_us as u64) * 9 / 100) as u32,
            keyframe,
        });
        sender.take_keyframe_request()
    }
}
struct Speaker {
    platform: Arc<Platform>,
    events: UnboundedSender<Command>,
    serial: i64,
}
impl Listener for Speaker {
    fn on_call_thread(&self) {
        self.platform.attach_audio_thread();
    }
    fn disconnected(&self, reason: &str) {
        let _ = self
            .events
            .send(Command::VoiceLost(self.serial, reason.into()));
    }
    fn tick(&self, voices: &[(u64, &[i16])]) {
        if voices.is_empty() {
            return;
        }
        let mut mix = [0i32; voice::FRAME * 2];
        for (_, voice) in voices {
            for (out, sample) in mix.iter_mut().zip(*voice) {
                *out += i32::from(*sample);
            }
        }
        let pcm = mix.map(|s| s.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
        let _ = self.platform.play(self.serial, &pcm);
    }
}
pub struct Session {
    pub guild: String,
    pub channel: String,
    pub name: String,
    pub muted: bool,
    pub serial: i64,
    pub sharing: Option<i64>,
    share_key: Option<String>,
    pub call: Call,
    track: Track,
    service: Discord,
    platform: Arc<Platform>,
    gateway_leave: bool,
}
impl Session {
    pub async fn join(
        service: Discord,
        platform: Arc<Platform>,
        events: UnboundedSender<Command>,
        serial: i64,
        guild: String,
        channel: String,
        name: String,
    ) -> Result<Self> {
        let result = tokio::time::timeout(Duration::from_secs(25), async {
            let server = service.join(&guild, &channel).await?;
            let speaker = Speaker {
                platform: platform.clone(),
                events,
                serial,
            };
            Call::connect(&server, &guild, &channel, Arc::new(speaker)).await
        })
        .await
        .context("The voice server timed out")
        .and_then(|r| r);
        let mut call = match result {
            Ok(call) => call,
            Err(e) => {
                service.leave(&guild);
                return Err(e);
            }
        };
        let (mic, input) = voice::live_microphone();
        let track = call.play(input);
        track.pause()?;
        if let Err(e) = platform.with_id("openCallAudio", serial) {
            service.leave(&guild);
            return Err(e);
        }
        *platform.devices.microphone.lock().unwrap() = Some((serial, mic));
        Ok(Self {
            guild,
            channel,
            name,
            muted: true,
            serial,
            sharing: None,
            share_key: None,
            call,
            track,
            service,
            platform,
            gateway_leave: true,
        })
    }
    pub fn mute(&mut self, muted: bool) -> Result<()> {
        if muted {
            self.track.pause()?;
            self.platform.with_id("muteCallAudio", self.serial)?;
            if let Some((_, m)) = self.platform.devices.microphone.lock().unwrap().as_ref() {
                m.clear();
            }
        } else {
            self.track.play()?;
        }
        self.muted = muted;
        self.service
            .set_self(&self.guild, &self.channel, muted, false, false);
        Ok(())
    }
    pub fn ask_mic(&self) -> Result<()> {
        self.platform.with_id("requestCallMic", self.serial)
    }
    pub fn request_share(&mut self, id: i64) -> Result<()> {
        ensure!(self.sharing.is_none(), "Screen sharing already started.");
        self.platform.with_id("requestProjection", id)?;
        self.sharing = Some(id);
        Ok(())
    }
    pub async fn share(&mut self, id: i64, width: u32, height: u32) -> Result<()> {
        ensure!(self.sharing == Some(id), "Screen sharing was cancelled.");
        let server = tokio::time::timeout(
            Duration::from_secs(16),
            self.service.go_live(&self.guild, &self.channel),
        )
        .await
        .context("No stream server answered")??;
        self.share_key = Some(server.stream_key.clone());
        let sender = StreamSender::start(server, width, height, 15, vec![Codec::H264])?;
        *self.platform.devices.stream.lock().unwrap() = Some((id, sender));
        Ok(())
    }
    pub fn stop_share(&mut self) {
        if self.sharing.take().is_some() {
            let _ = self.platform.simple("stopProjection");
        }
        self.platform.devices.stream.lock().unwrap().take();
        if let Some(key) = self.share_key.take() {
            self.service.end_stream(&key);
        }
    }
    pub fn stream_key(&self) -> Option<&str> {
        self.share_key.as_deref()
    }
    pub fn taken_over(&mut self) {
        self.gateway_leave = false;
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop_share();
        let _ = self.platform.with_id("closeCallAudio", self.serial);
        self.platform.devices.microphone.lock().unwrap().take();
        self.call.leave();
        if self.gateway_leave {
            self.service.leave(&self.guild);
        }
    }
}
