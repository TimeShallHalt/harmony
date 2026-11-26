//! Bounded Ogg/Opus voice messages, using the call runtime's libopus.
use anyhow::Result;

use crate::opus::{Application, Decoder, Encoder};
use serde::{Deserialize, Serialize};

pub const RATE: usize = 48_000;
pub const FRAME: usize = 960;
pub const MAX_SECONDS: usize = 20 * 60;
pub const MAX_SAMPLES: usize = RATE * MAX_SECONDS;
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
pub const FILENAME: &str = "voice-message.ogg";
/// Discord Attach Files and Send Voice Messages; checked with channel overwrites.
pub const REQUIRED_PERMISSIONS: u64 = (1 << 15) | (1 << 46);

/// An immutable, reviewable recording. No capture or upload occurs on creation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    #[serde(with = "encoded_audio")]
    data: Vec<u8>,
    samples: usize,
    waveform: Vec<u8>,
}

// Encoded audio crosses the recorder pipe as base64 rather than JSON integers.
mod encoded_audio {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let value = <&str>::deserialize(deserializer)?;
        if value.len() > super::MAX_BYTES.div_ceil(3) * 4 {
            return Err(D::Error::custom(
                "Voice message exceeds the recording limit.",
            ));
        }
        STANDARD.decode(value).map_err(D::Error::custom)
    }
}

impl Note {
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn duration_secs(&self) -> f64 {
        self.samples as f64 / RATE as f64
    }
    pub fn waveform(&self) -> &[u8] {
        &self.waveform
    }

    /// Explicit review download; the caller has already shown a save dialog.
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        self.validate()?;
        std::fs::write(path, &self.data)?;
        Ok(())
    }

    /// Discord's dedicated voice-message body, never a generic file or Clip.
    pub fn message_body(&self, nonce: &str) -> Result<serde_json::Value> {
        use base64::Engine;
        self.validate()?;
        Ok(serde_json::json!({
            "flags": 8192, "nonce": nonce, "enforce_nonce": true,
            "attachments": [{
                "id": 0, "filename": FILENAME,
                "duration_secs": self.duration_secs(),
                "waveform": base64::engine::general_purpose::STANDARD.encode(&self.waveform),
            }],
        }))
    }

    /// Validate at every process/service boundary, before allocating a decoder.
    pub fn validate(&self) -> Result<()> {
        self.checked_pages().map(drop)
    }

    fn checked_pages(&self) -> Result<Vec<(u64, &[u8], usize)>> {
        anyhow::ensure!(
            (1..=MAX_SAMPLES).contains(&self.samples),
            "A voice message must contain up to 20 minutes of audio."
        );
        anyhow::ensure!(
            self.data.len() <= MAX_BYTES,
            "Voice message exceeds 10 MiB."
        );
        anyhow::ensure!(
            self.waveform.len() == self.samples.div_ceil(RATE / 10).min(256),
            "Invalid voice-note waveform."
        );
        let pages = self.pages()?;
        anyhow::ensure!(
            pages.len() >= 3 && pages[0].1.len() == 19 && &pages[0].1[..10] == b"OpusHead\x01\x01",
            "Expected mono Ogg/Opus."
        );
        anyhow::ensure!(pages[1].1.starts_with(b"OpusTags"), "Missing Opus tags.");
        let skip = u16::from_le_bytes(pages[0].1[10..12].try_into()?) as u64;
        let last = pages.last().unwrap();
        anyhow::ensure!(
            last.0 == self.samples as u64 + skip && self.data[last.2 + 5] == 4,
            "Invalid Opus duration."
        );
        Ok(pages)
    }

    fn pages(&self) -> Result<Vec<(u64, &[u8], usize)>> {
        let mut result = Vec::new();
        let mut at = 0;
        while at < self.data.len() {
            let header = self
                .data
                .get(at..at + 27)
                .ok_or_else(|| anyhow::anyhow!("Truncated Ogg page."))?;
            anyhow::ensure!(&header[..5] == b"OggS\0", "Invalid Ogg header.");
            anyhow::ensure!(
                u32::from_le_bytes(header[18..22].try_into()?) as usize == result.len(),
                "Invalid Ogg sequence."
            );
            let count = header[26] as usize;
            let lace = self
                .data
                .get(at + 27..at + 27 + count)
                .ok_or_else(|| anyhow::anyhow!("Truncated Ogg lacing."))?;
            anyhow::ensure!(
                count > 0 && lace.last() != Some(&255),
                "Continued Ogg packets are not supported here."
            );
            let size: usize = lace.iter().map(|&n| n as usize).sum();
            let start = at + 27 + count;
            let end = start + size;
            let packet = self
                .data
                .get(start..end)
                .ok_or_else(|| anyhow::anyhow!("Truncated Opus packet."))?;
            anyhow::ensure!(
                crc(&self.data[at..end]) == u32::from_le_bytes(header[22..26].try_into()?),
                "Ogg checksum mismatch."
            );
            result.push((u64::from_le_bytes(header[6..14].try_into()?), packet, at));
            at = end;
        }
        Ok(result)
    }

    /// Play each decoded frame, trimming the encoder delay and final padding.
    /// A cancelled consumer returns false to stop decoding immediately.
    pub fn play_frames(&self, mut play: impl FnMut(&[i16]) -> Result<bool>) -> Result<()> {
        let pages = self.checked_pages()?;
        let skip = u16::from_le_bytes(pages[0].1[10..12].try_into()?) as usize;
        let end = skip + self.samples;
        let mut decoded = 0;
        let mut decoder = Decoder::new(1)?;
        let mut frame = [0i16; FRAME];
        for (_, packet, _) in pages.into_iter().skip(2) {
            let n = decoder.decode(packet, &mut frame, false)?;
            let first = skip.saturating_sub(decoded).min(n);
            let last = end.saturating_sub(decoded).min(n);
            if first < last && !play(&frame[first..last])? {
                return Ok(());
            }
            decoded += n;
            if decoded >= end {
                return Ok(());
            }
        }
        anyhow::bail!("Truncated voice message.")
    }

    #[cfg(test)]
    fn decode(&self) -> Result<Vec<i16>> {
        let mut pcm = Vec::new();
        self.play_frames(|frame| {
            pcm.extend_from_slice(frame);
            Ok(true)
        })?;
        Ok(pcm)
    }
}

/// Streaming encoder: one PCM frame and a bounded peak summary, not raw audio.
pub struct Builder {
    encoder: Encoder,
    data: Vec<u8>,
    sequence: u32,
    pending: Vec<f32>,
    peaks: Vec<u8>,
    samples: usize,
    encoded: u64,
    skip: u16,
}

impl Builder {
    pub fn new() -> Result<Self> {
        let mut encoder = Encoder::new(1, Application::Voice, 32_000)?;
        let skip = encoder.lookahead()?;
        let mut this = Self {
            encoder,
            data: Vec::new(),
            sequence: 0,
            pending: Vec::with_capacity(FRAME),
            peaks: Vec::new(),
            samples: 0,
            encoded: 0,
            skip,
        };
        let mut head = b"OpusHead\x01\x01".to_vec();
        head.extend_from_slice(&skip.to_le_bytes());
        head.extend_from_slice(&(RATE as u32).to_le_bytes());
        head.extend_from_slice(&[0, 0, 0]);
        page(&mut this.data, &mut this.sequence, 2, 0, &head);
        page(
            &mut this.data,
            &mut this.sequence,
            0,
            0,
            b"OpusTags\x07\0\0\0Harmony\0\0\0\0",
        );
        Ok(this)
    }

    pub fn samples(&self) -> usize {
        self.samples
    }

    /// Returns true at the duration cap; nothing beyond it is retained.
    pub fn push(&mut self, samples: &[f32]) -> Result<bool> {
        let samples = &samples[..samples.len().min(MAX_SAMPLES - self.samples)];
        anyhow::ensure!(
            samples.iter().all(|s| s.is_finite()),
            "Invalid microphone samples."
        );
        for &sample in samples {
            self.pending.push(sample.clamp(-1.0, 1.0));
            self.samples += 1;
            if self.pending.len() == FRAME {
                self.peaks.push(
                    (self.pending.iter().fold(0f32, |a, s| a.max(s.abs())) * 255.0).round() as u8,
                );
                self.packet(false)?;
            }
        }
        Ok(self.samples == MAX_SAMPLES)
    }

    fn packet(&mut self, last: bool) -> Result<()> {
        self.pending.resize(FRAME, 0.0);
        let mut packet = [0u8; 1500];
        let size = self.encoder.encode_float(&self.pending, &mut packet)?;
        self.encoded += FRAME as u64;
        let granule = if last {
            self.samples as u64 + u64::from(self.skip)
        } else {
            self.encoded
        };
        page(
            &mut self.data,
            &mut self.sequence,
            if last { 4 } else { 0 },
            granule,
            &packet[..size],
        );
        self.pending.clear();
        anyhow::ensure!(
            self.data.len() <= MAX_BYTES,
            "Voice message exceeds 10 MiB."
        );
        Ok(())
    }

    pub fn finish(mut self) -> Result<Note> {
        anyhow::ensure!(self.samples > 0, "No audio was recorded.");
        let mut finished = false;
        if !self.pending.is_empty() {
            self.peaks.push(
                (self.pending.iter().fold(0f32, |a, s| a.max(s.abs())) * 255.0).round() as u8,
            );
            finished = self.encoded + FRAME as u64 >= self.samples as u64 + u64::from(self.skip);
            self.packet(finished)?;
        }
        if !finished {
            while self.encoded + (FRAME as u64) < self.samples as u64 + u64::from(self.skip) {
                self.packet(false)?;
            }
            self.packet(true)?;
        }
        let count = self.samples.div_ceil(RATE / 10).min(256);
        let waveform = (0..count)
            .map(|i| {
                let first = i * self.peaks.len() / count;
                let end = ((i + 1) * self.peaks.len() / count)
                    .max(first + 1)
                    .min(self.peaks.len());
                self.peaks[first..end].iter().copied().max().unwrap_or(0)
            })
            .collect();
        let note = Note {
            data: self.data,
            samples: self.samples,
            waveform,
        };
        note.validate()?;
        Ok(note)
    }
}

/// Deterministic entry point used by encoder tests, never a device.
#[cfg(test)]
pub fn encode(samples: &[f32]) -> Result<Vec<u8>> {
    let mut builder = Builder::new()?;
    builder.push(samples)?;
    Ok(builder.finish()?.data)
}

/// Appends one complete packet in an Ogg page. Use a fresh sequence for each stream.
pub fn page(bytes: &mut Vec<u8>, sequence: &mut u32, flags: u8, granule: u64, packet: &[u8]) {
    let start = bytes.len();
    bytes.extend_from_slice(b"OggS\0");
    bytes.push(flags);
    bytes.extend_from_slice(&granule.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&sequence.to_le_bytes());
    *sequence += 1;
    bytes.extend_from_slice(&[0; 4]);
    bytes.push((packet.len() / 255 + 1) as u8);
    bytes.extend(std::iter::repeat_n(255, packet.len() / 255));
    bytes.push((packet.len() % 255) as u8);
    bytes.extend_from_slice(packet);
    let crc = crc(&bytes[start..]);
    bytes[start + 22..start + 26].copy_from_slice(&crc.to_le_bytes());
}

/// Ogg's non-reflected CRC-32, with the page's checksum field cleared.
fn crc(page: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0; 256];
        let mut index = 0;
        while index < table.len() {
            let mut value = (index as u32) << 24;
            let mut bit = 0;
            while bit < 8 {
                value = if value & 0x8000_0000 == 0 {
                    value << 1
                } else {
                    (value << 1) ^ 0x04c1_1db7
                };
                bit += 1;
            }
            table[index] = value;
            index += 1;
        }
        table
    };
    page[..22]
        .iter()
        .chain(&[0; 4])
        .chain(&page[26..])
        .fold(0u32, |crc, &byte| {
            (crc << 8) ^ TABLE[((crc >> 24) as u8 ^ byte) as usize]
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_note_has_duration_waveform_delay_and_decodable_audio() {
        let mut builder = Builder::new().unwrap();
        for frame in 0..50 {
            let samples: Vec<f32> = (0..960)
                .map(|at| {
                    ((frame * 960 + at) as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()
                        * 0.5
                })
                .collect();
            assert!(!builder.push(&samples).unwrap());
        }
        let note = builder.finish().unwrap();
        assert_eq!(note.duration_secs(), 1.0);
        assert_eq!(note.waveform().len(), 10, "at most one point per 100 ms");
        assert!(
            note.waveform()
                .iter()
                .all(|&peak| (120..=129).contains(&peak))
        );
        let pcm = note.decode().unwrap();
        assert_eq!(pcm.len(), 48_000, "trim Opus delay and padding");
        assert!(pcm.iter().any(|sample| sample.unsigned_abs() > 10_000));
        assert!(u16::from_le_bytes(note.data()[38..40].try_into().unwrap()) > 0);
        note.validate().unwrap();
    }

    #[test]
    fn voice_upload_has_explicit_flag_and_sample_derived_metadata() {
        use base64::Engine;
        let mut builder = Builder::new().unwrap();
        builder.push(&[0.25; 1001]).unwrap();
        let note = builder.finish().unwrap();
        let body = note.message_body("note-nonce").unwrap();
        assert_eq!(body["flags"], 8192);
        assert!(body.get("content").is_none());
        assert!(body.get("message_reference").is_none());
        assert_eq!(body["nonce"], "note-nonce");
        assert_eq!(body["attachments"].as_array().unwrap().len(), 1);
        let attachment = &body["attachments"][0];
        assert_eq!(attachment["filename"], FILENAME);
        assert_eq!(attachment["id"], 0);
        assert_eq!(
            attachment["duration_secs"].as_f64().unwrap(),
            1001.0 / 48_000.0
        );
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(attachment["waveform"].as_str().unwrap())
                .unwrap(),
            note.waveform()
        );
    }

    #[test]
    fn review_download_writes_the_actual_encoded_audio() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("generated-voice-message.ogg");
        let mut builder = Builder::new().unwrap();
        builder.push(&[0.25; 960]).unwrap();
        let note = builder.finish().unwrap();
        note.save(&path).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), note.data());
    }

    #[test]
    #[ignore = "requires ffmpeg; generated samples only, never a device"]
    fn generated_ogg_decodes_in_independent_ffmpeg() {
        let mut builder = Builder::new().unwrap();
        let samples: Vec<f32> = (0..RATE)
            .map(|at| (at as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.5)
            .collect();
        builder.push(&samples).unwrap();
        let note = builder.finish().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("generated.ogg");
        note.save(&path).unwrap();
        let decoded = std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-threads",
                "1",
                "-filter_threads",
                "1",
                "-filter_complex_threads",
                "1",
                "-v",
                "error",
                "-i",
            ])
            .arg(&path)
            .args(["-f", "s16le", "-ac", "1", "-ar", "48000", "pipe:1"])
            .output()
            .unwrap();
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        assert_eq!(
            decoded.stdout.len(),
            RATE * 2,
            "independent decoder trims delay and EOS padding"
        );
        assert!(
            decoded
                .stdout
                .chunks_exact(2)
                .any(|pair| i16::from_le_bytes([pair[0], pair[1]]).unsigned_abs() > 10_000)
        );
    }

    #[test]
    fn subframe_notes_do_not_emit_a_backwards_eos_granule() {
        for count in [1, 100, 648, 649, 959, 960, 961, 1001] {
            let mut builder = Builder::new().unwrap();
            builder.push(&vec![0.25; count]).unwrap();
            let note = builder.finish().unwrap();
            let pages = note.pages().unwrap();
            assert!(
                pages.windows(2).all(|pair| pair[0].0 <= pair[1].0),
                "backwards granule for {count} samples"
            );
            assert_eq!(note.decode().unwrap().len(), count);
        }
    }

    #[test]
    fn limits_empty_input_corruption_and_non_finite_samples_are_rejected() {
        assert!(Builder::new().unwrap().finish().is_err());
        assert!(Builder::new().unwrap().push(&[f32::NAN]).is_err());
        let mut builder = Builder::new().unwrap();
        for _ in 0..MAX_SECONDS * 50 {
            builder.push(&[0.0; FRAME]).unwrap();
        }
        assert!(builder.push(&[0.9; FRAME]).unwrap());
        assert_eq!(builder.samples(), MAX_SAMPLES);
        let note = builder.finish().unwrap();
        assert_eq!(note.duration_secs(), MAX_SECONDS as f64);
        assert!(note.data.len() <= MAX_BYTES);
        assert!(note.waveform.iter().all(|&sample| sample == 0));
        let mut corrupt = note.clone();
        corrupt.data[50] ^= 1;
        assert!(corrupt.validate().is_err());
        let mut oversized = note.clone();
        oversized.data.resize(MAX_BYTES + 1, 0);
        assert!(oversized.validate().is_err());
        let mut too_long = note;
        too_long.samples = MAX_SAMPLES + 1;
        assert!(too_long.validate().is_err());
    }

    #[test]
    fn generated_speech_is_a_real_ogg_opus_stream() {
        let samples: Vec<f32> = (0..48_000)
            .map(|at| (at as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
            .collect();
        let data = encode(&samples).expect("encode generated samples without opening a device");
        assert_eq!(&data[..4], b"OggS");
        assert!(data.windows(8).any(|magic| magic == b"OpusHead"));
        assert!(data.windows(8).any(|magic| magic == b"OpusTags"));
    }
}
