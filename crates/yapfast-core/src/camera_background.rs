//! Offline person segmentation and background compositing for call cameras.
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, ensure};
use image::{GrayImage, ImageReader, RgbaImage, imageops::FilterType};
use livekit::webrtc::native::yuv_helper;

use crate::{camera::Sink, picture::I420};

pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
const WIDTH: u32 = 256;
const HEIGHT: u32 = 144;
const PIXELS: usize = WIDTH as usize * HEIGHT as usize;
const WEIGHTS: &[u8] =
    include_bytes!("../../../vendor/mediapipe-selfie/selfie_segmentation_landscape.onnx");

static MODEL: OnceLock<Result<rten::Model, String>> = OnceLock::new();
static WORKERS: OnceLock<Arc<rten::ThreadPool>> = OnceLock::new();
static BACKGROUND: Mutex<Option<Arc<RgbaImage>>> = Mutex::new(None);

pub fn prepare(bytes: &[u8]) -> Result<Arc<RgbaImage>> {
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Choose a picture up to 16 MiB."
    );
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    ensure!(
        matches!(
            reader.format(),
            Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
        ),
        "Choose a PNG or JPEG image."
    );
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()?
        .resize(1920, 1080, FilterType::Triangle)
        .into_rgba8();
    model()?;
    Ok(Arc::new(image))
}

fn model() -> Result<&'static rten::Model> {
    MODEL
        .get_or_init(|| rten::Model::load_static_slice(WEIGHTS).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| anyhow::anyhow!("The camera segmentation model could not load. {error}"))
}

pub fn set(image: Option<Arc<RgbaImage>>) {
    *BACKGROUND
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = image;
}

fn infer(input: &[f32], mask: &mut GrayImage) -> Result<()> {
    let input = rten::ValueView::from_shape([1, HEIGHT as usize, WIDTH as usize, 3], input)?;
    let workers = WORKERS.get_or_init(|| Arc::new(rten::ThreadPool::with_num_threads(2)));
    let options = rten::RunOptions::default().with_thread_pool(Some(workers.clone()));
    let output = model()?.run_one(input.into(), Some(options))?;
    let matte = output
        .as_tensor_view::<f32>()
        .context("Invalid camera mask type")?;
    let samples = matte.data().context("Camera mask is not contiguous")?;
    ensure!(samples.len() == PIXELS, "Invalid camera mask size");
    for (dst, &confidence) in mask.iter_mut().zip(samples) {
        *dst = ((confidence - 0.25).mul_add(2.0, 0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    Ok(())
}

pub(crate) fn wrap(mut sink: Sink) -> Sink {
    let mut active: Option<Matte> = None;
    Box::new(move |frame| {
        let selected = BACKGROUND.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let Some(selected) = selected else {
            active = None;
            sink(frame);
            return;
        };
        let size = (frame.width, frame.height);
        if active
            .as_ref()
            .is_none_or(|m| m.picture.dimensions() != size || !Arc::ptr_eq(&m.source, &selected))
        {
            active = Some(Matte::new(selected, size));
        }
        let processed = active.as_mut().unwrap().apply(frame);
        sink(&processed);
    })
}

struct Matte {
    source: Arc<RgbaImage>,
    picture: RgbaImage,
    mask: GrayImage,
    foreground: Vec<u8>,
    input: Vec<f32>,
    small: Vec<u8>,
    scratch: Vec<u8>,
    scaled_mask: GrayImage,
    planes: Vec<u8>,
    update_mask: bool,
    reported: bool,
}

impl Matte {
    fn new(source: Arc<RgbaImage>, size: (u32, u32)) -> Self {
        let (width, height) = size;
        let (sw, sh) = source.dimensions();
        let (cw, ch) = if sw * height > sh * width {
            ((sh * width / height).max(1), sh)
        } else {
            (sw, (sw * height / width).max(1))
        };
        let crop = image::imageops::crop_imm(&*source, (sw - cw) / 2, (sh - ch) / 2, cw, ch);
        let picture = image::imageops::resize(&*crop, width, height, FilterType::Triangle);
        let pixels = width as usize * height as usize;
        let chroma = width.div_ceil(2) as usize * height.div_ceil(2) as usize;
        Self {
            source,
            picture,
            mask: GrayImage::new(WIDTH, HEIGHT),
            foreground: vec![0; pixels * 4],
            input: vec![0.0; PIXELS * 3],
            small: vec![0; PIXELS * 4],
            scratch: Vec::new(),
            scaled_mask: GrayImage::new(width, height),
            planes: vec![0; pixels + 2 * chroma],
            update_mask: true,
            reported: false,
        }
    }

    fn apply(&mut self, frame: &I420<'_>) -> I420<'_> {
        let (w, h) = (frame.width, frame.height);
        if self.update_mask {
            frame.to_rgba(&mut self.scratch, &mut self.small, WIDTH, HEIGHT);
            for (rgb, rgba) in self
                .input
                .chunks_exact_mut(3)
                .zip(self.small.chunks_exact(4))
            {
                for (sample, &byte) in rgb.iter_mut().zip(rgba) {
                    *sample = f32::from(byte) / 255.0;
                }
            }
            if let Err(error) = infer(&self.input, &mut self.mask) {
                // Keep the replacement visible even if inference fails.
                self.mask.fill(0);
                if !self.reported {
                    tracing::warn!(%error, "could not segment a camera frame");
                    self.reported = true;
                }
            }
            self.scaled_mask = image::imageops::resize(&self.mask, w, h, FilterType::Triangle);
        }
        self.update_mask = !self.update_mask;
        frame.to_rgba(&mut self.scratch, &mut self.foreground, w, h);
        for ((pixel, background), &mask) in self
            .foreground
            .chunks_exact_mut(4)
            .zip(self.picture.pixels())
            .zip(self.scaled_mask.iter())
        {
            let alpha = u16::from(mask);
            for (pixel, &background) in pixel[..3].iter_mut().zip(background.0.iter()) {
                *pixel = ((u16::from(*pixel) * alpha + u16::from(background) * (255 - alpha) + 127)
                    / 255) as u8;
            }
        }
        let cw = w.div_ceil(2);
        let (y, chroma) = self.planes.split_at_mut(w as usize * h as usize);
        let (u, v) = chroma.split_at_mut(cw as usize * h.div_ceil(2) as usize);
        yuv_helper::abgr_to_i420(
            &self.foreground,
            w * 4,
            y,
            w,
            u,
            cw,
            v,
            cw,
            w as i32,
            h as i32,
        );
        I420 {
            width: w,
            height: h,
            y,
            u,
            v,
            strides: [w, cw, cw],
        }
    }
}

#[cfg(test)]
#[test]
fn bundled_model_runs_and_does_not_detect_a_person_in_black() {
    let mut mask = GrayImage::new(WIDTH, HEIGHT);
    infer(&vec![0.0; PIXELS * 3], &mut mask).unwrap();
    assert!(mask.iter().filter(|&&p| p > 127).count() < PIXELS / 10);
}
