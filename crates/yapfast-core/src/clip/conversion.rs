//! Transcode a local video or audio clip with the codecs already in Harmony.
//! The same process boundary as the player keeps native decoding off the UI.
use std::{fs::File, io::Write, path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, ensure};
use yapfast_discord::opus::{Application, Encoder};

use super::{Clip, Encoded, Pixels, webm};
use crate::{
    video::{Next, Source, Video, fit},
    video_player::Resampler,
};

pub const ARGUMENT: &str = "--convert-clip";
pub const MAX_OUTPUT: u64 = 128 * 1024 * 1024;
const MAX_INPUT: u64 = 512 * 1024 * 1024;
const MAX_TIME: u64 = 5 * 60 * 1000;
const RATE: u32 = 48_000;
const SAMPLES_PER_MS: usize = 48;
const OPUS_FRAME: usize = 960;
const VIDEO_STEP: u64 = 33;

#[derive(Clone, Copy)]
pub enum Format {
    WebM,
    Ogg,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::WebM => "webm",
            Self::Ogg => "ogg",
        }
    }
}

/// Run a bounded worker. Cancelling drops its handle and stops only this child.
pub async fn convert(input: &Path, output: File, format: Format, limit: u64) -> Result<()> {
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .arg(ARGUMENT)
        .arg(input)
        .arg(format.extension())
        .arg(limit.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(output))
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = command.spawn().context("Could not start clip conversion")?;
    let result = tokio::time::timeout(Duration::from_secs(5 * 60), child.wait_with_output())
        .await
        .context("Clip conversion timed out")??;
    if !result.status.success() {
        let error = String::from_utf8_lossy(&result.stderr);
        let detail: String = error
            .trim()
            .trim_start_matches("Error: ")
            .chars()
            .take(300)
            .collect();
        anyhow::bail!("Could not convert the clip. {detail}");
    }
    Ok(())
}

pub fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(2);
    let input = args.next().context("A clip file is required")?;
    let format = match args.next().as_deref().and_then(|s| s.to_str()) {
        Some("webm") => Format::WebM,
        Some("ogg") => Format::Ogg,
        _ => anyhow::bail!("Unknown clip format"),
    };
    let limit = args
        .next()
        .and_then(|s| s.to_str()?.parse::<u64>().ok())
        .context("Invalid byte limit")?;
    ensure!(args.next().is_none(), "Unexpected clip arguments");
    let file = File::open(input)?;
    let info = file.metadata()?;
    ensure!(
        info.is_file() && (1..=MAX_INPUT).contains(&info.len()),
        "Choose a regular media file up to 512 MiB."
    );
    let bytes = transcode(file, format, limit)?;
    std::io::stdout().lock().write_all(&bytes)?;
    Ok(())
}

impl Source for File {
    fn size(&mut self) -> Option<u64> {
        self.metadata().ok().map(|info| info.len())
    }
}

fn transcode(source: impl Source, format: Format, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        (64 * 1024..=MAX_OUTPUT).contains(&limit),
        "The destination's file limit is too small or unavailable."
    );
    let has_video = matches!(format, Format::WebM);
    let mut video = if has_video {
        Video::open(source, true)
    } else {
        Video::open_sound(source)
    }
    .context("Harmony could not read this media format.")?;
    let duration = video
        .duration_ms
        .filter(|ms| (1..=MAX_TIME).contains(ms))
        .context("Choose a clip with a known duration of up to five minutes.")?;
    // Leave space for the container, variable bitrate peaks, and short clips'
    // first keyframe. Limit quality to the native desktop playback range.
    let budget = (limit.saturating_sub(32 * 1024) * 8 * 75 / 100 / duration).min(6_000) as u32;
    let audio_rate = match (has_video, budget) {
        (true, 0..=180) => 32,
        (_, 0..=100) => 48,
        _ => 96,
    };
    let mut audio = video
        .sound
        .map(|(rate, channels)| Audio::new(rate, channels, audio_rate))
        .transpose()?;
    let mut screen = if has_video {
        let bit_rate = budget.saturating_sub(if audio.is_some() { audio_rate } else { 0 });
        ensure!(
            bit_rate >= 50,
            "This clip is too long to fit. Choose a shorter clip."
        );
        Some(Screen::new(&video, bit_rate)?)
    } else {
        ensure!(
            audio.is_some() && budget >= 32,
            "The audio clip is too long to fit."
        );
        None
    };
    let mut frames = Vec::new();
    let mut bytes = 0u64;
    loop {
        let before = frames.len();
        match video.decode()? {
            Next::Picture(time) => {
                if let Some(screen) = &mut screen {
                    screen.push(&mut video, time, &mut frames)?;
                }
            }
            Next::Sound(time) => {
                if let Some(audio) = &mut audio {
                    audio.push(video.sound(), time, &mut frames)?;
                }
            }
            Next::End => break,
        }
        bytes += frames[before..]
            .iter()
            .map(|frame| frame.data.len() as u64 + 12)
            .sum::<u64>();
        ensure!(
            bytes <= limit,
            "The converted clip is still too large. Choose a shorter clip."
        );
    }
    if let Some(audio) = &mut audio {
        audio.finish(&mut frames)?;
    }
    let converted = if let Some(screen) = &mut screen {
        let end = screen.finish(&mut frames)?;
        let (w, h) = screen.size;
        let duration = end.max(audio.as_ref().map_or(0, Audio::duration));
        frames.sort_by_key(|frame| (frame.time, frame.track));
        webm(w, h, &frames, duration, audio.map(|a| a.skip))
    } else {
        ogg(&frames, &audio.context("There is no audio in this clip")?)
    };
    ensure!(
        converted.len() as u64 <= limit,
        "The converted clip is still too large. Choose a shorter clip."
    );
    Ok(converted)
}

struct Screen {
    size: (u32, u32),
    clip: Clip,
    pixels: Pixels,
    rgba: Vec<u8>,
    time: Option<u64>,
}

impl Screen {
    fn new(video: &Video<'_>, kbps: u32) -> Result<Self> {
        ensure!(
            (2..=8192).contains(&video.width) && (2..=8192).contains(&video.height),
            "Unsupported video dimensions."
        );
        let bounds = match kbps {
            0..=150 => (426, 240),
            151..=400 => (640, 360),
            401..=950 => (854, 480),
            _ => (1280, 720),
        };
        let size = fit(video.width, video.height, bounds.0, bounds.1);
        let (w, h) = size;
        let clip = Clip::new(w, h, kbps).context("Could not start the WebM encoder")?;
        Ok(Self {
            size,
            clip,
            pixels: Pixels::new(w, h),
            rgba: vec![0; (w * h * 4) as usize],
            time: None,
        })
    }

    fn push(
        &mut self,
        video: &mut Video<'_>,
        time: Option<u64>,
        frames: &mut Vec<Encoded>,
    ) -> Result<()> {
        let time = time.unwrap_or_else(|| self.time.map_or(0, |last| last + VIDEO_STEP));
        ensure!(time <= MAX_TIME, "Choose a clip up to five minutes.");
        if self.time.is_some_and(|last| last + VIDEO_STEP - 1 > time) {
            return Ok(());
        }
        video.picture(&mut self.rgba, self.size.0, self.size.1)?;
        self.pixels
            .encode(
                &self.clip,
                Some(&self.rgba),
                time,
                VIDEO_STEP as u32,
                frames,
            )
            .context("Could not encode a video frame")?;
        self.time = Some(time);
        Ok(())
    }

    fn finish(&mut self, frames: &mut Vec<Encoded>) -> Result<u64> {
        self.pixels
            .encode(&self.clip, None, 0, 0, frames)
            .context("Could not finish the video")?;
        Ok(self.time.context("This clip has no video frames")? + VIDEO_STEP)
    }
}

struct Audio {
    encoder: Encoder,
    resampler: Resampler,
    pcm: Vec<f32>,
    samples: usize,
    encoded: usize,
    skip: u16,
}

impl Audio {
    fn new(rate: u32, channels: usize, kbps: u32) -> Result<Self> {
        let mut encoder = Encoder::new(2, Application::Audio, (kbps * 1000) as i32)?;
        let skip = encoder.lookahead()?;
        Ok(Self {
            encoder,
            resampler: Resampler::new(rate, channels),
            pcm: Vec::new(),
            samples: 0,
            encoded: 0,
            skip,
        })
    }

    fn push(
        &mut self,
        sound: crate::video::Sound<'_>,
        time: Option<u64>,
        out: &mut Vec<Encoded>,
    ) -> Result<()> {
        self.resampler.follow(sound.rate, sound.channels);
        let decoded = self.resampler.run(sound.samples);
        if let Some(time) = time {
            ensure!(time <= MAX_TIME, "Choose a clip up to five minutes.");
            let at = time as usize * SAMPLES_PER_MS;
            if at > self.samples + SAMPLES_PER_MS * 40 {
                let gap = at - self.samples;
                // Fill gaps on the original timeline without holding a full clip of silence.
                let silence = [0f32; OPUS_FRAME * 2];
                for start in (0..gap).step_by(OPUS_FRAME) {
                    self.queue(&silence[..2 * (gap - start).min(OPUS_FRAME)], out)?;
                }
            } else if self.samples > at + SAMPLES_PER_MS * 40 {
                return self.queue(
                    &decoded[((self.samples - at) * 2).min(decoded.len())..],
                    out,
                );
            }
        }
        self.queue(&decoded, out)
    }

    fn queue(&mut self, samples: &[f32], out: &mut Vec<Encoded>) -> Result<()> {
        self.samples += samples.len() / 2;
        ensure!(
            self.duration() <= MAX_TIME + 40,
            "Choose a clip up to five minutes."
        );
        self.pcm.extend(samples.iter().map(|&s| {
            if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
        let mut encoded = 0;
        while self.pcm.len() - encoded >= OPUS_FRAME * 2 {
            let next = encoded + OPUS_FRAME * 2;
            let mut data = [0; 1500];
            let count = self
                .encoder
                .encode_float(&self.pcm[encoded..next], &mut data)?;
            out.push(Encoded {
                time: (self.encoded / SAMPLES_PER_MS) as u64,
                track: 2,
                keyframe: true,
                data: data[..count].to_vec(),
            });
            self.encoded += OPUS_FRAME;
            encoded = next;
        }
        self.pcm.drain(..encoded);
        Ok(())
    }

    fn duration(&self) -> u64 {
        self.samples.div_ceil(SAMPLES_PER_MS) as u64
    }

    fn finish(&mut self, out: &mut Vec<Encoded>) -> Result<()> {
        ensure!(self.samples > 0, "This clip has no decodable sound.");
        let samples = self.samples;
        let padded = (samples + usize::from(self.skip)).div_ceil(OPUS_FRAME) * OPUS_FRAME;
        let silence = vec![0.0; (padded - samples) * 2];
        self.queue(&silence, out)?;
        self.samples = samples;
        Ok(())
    }
}

pub(super) fn opus_head(skip: u16) -> Vec<u8> {
    let mut head = b"OpusHead\x01\x02".to_vec();
    head.extend(skip.to_le_bytes());
    head.extend(RATE.to_le_bytes());
    head.extend([0, 0, 0]); // gain and channel mapping family
    head
}

fn ogg(frames: &[Encoded], audio: &Audio) -> Vec<u8> {
    use yapfast_discord::voice_note::page;
    let mut out = Vec::new();
    let mut sequence = 0;
    page(&mut out, &mut sequence, 2, 0, &opus_head(audio.skip));
    page(
        &mut out,
        &mut sequence,
        0,
        0,
        b"OpusTags\x07\0\0\0Harmony\0\0\0\0",
    );
    for (i, frame) in frames.iter().enumerate() {
        let last = i + 1 == frames.len();
        let granule = if last {
            audio.samples + usize::from(audio.skip)
        } else {
            (i + 1) * OPUS_FRAME
        };
        page(
            &mut out,
            &mut sequence,
            if last { 4 } else { 0 },
            granule as u64,
            &frame.data,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn clip_conversion_preserves_video_and_sound_and_respects_the_byte_limit() {
        let input = include_bytes!("../../../../testing/fixtures/video-aac.mp4");
        let webm = transcode(Cursor::new(input), Format::WebM, 2 * 1024 * 1024).unwrap();
        assert!(webm.len() < 2 * 1024 * 1024);
        let mut decoded = Video::open(Cursor::new(webm), true).unwrap();
        assert!(decoded.sound.is_some());
        let (mut pictures, mut sound) = (0, 0);
        loop {
            match decoded.decode().unwrap() {
                Next::Picture(_) => pictures += 1,
                Next::Sound(_) => sound += decoded.sound().samples.len(),
                Next::End => break,
            }
        }
        assert!(pictures > 1 && sound > OPUS_FRAME * 2);
        assert!(transcode(Cursor::new(input), Format::WebM, 32).is_err());
    }

    #[test]
    fn clip_conversion_writes_playable_ogg_opus() {
        let input = include_bytes!("../../../../testing/fixtures/voice-tone.ogg");
        let ogg = transcode(Cursor::new(input), Format::Ogg, 1024 * 1024).unwrap();
        assert!(ogg.starts_with(b"OggS"));
        let mut sound = Video::open_sound(Cursor::new(ogg)).unwrap();
        assert!((0..12).any(|_| sound.decode_sound().unwrap().is_some()
            && sound.sound().samples.iter().any(|f| f.abs() > 0.01)));
    }
}
