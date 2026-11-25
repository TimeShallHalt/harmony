use image::{ImageReader, Limits};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::Cursor,
    sync::{Arc, Weak},
};
use yapfast_discord::service::Discord;

pub type Pictures = HashMap<String, Option<Arc<Thumbnail>>>;
pub struct Thumbnail {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}
// Slint images belong to the UI thread. Reuse the decoded image while its Rust thumbnail is live.
thread_local! {
    static DISPLAYED: RefCell<HashMap<usize,(Weak<Thumbnail>, Image)>> = RefCell::new(HashMap::new());
}
pub fn clear_displayed() {
    DISPLAYED.with_borrow_mut(HashMap::clear);
}
impl Thumbnail {
    pub fn image(self: &Arc<Self>) -> Image {
        DISPLAYED.with_borrow_mut(|cache| {
            let key = Arc::as_ptr(self) as usize;
            if let Some((owner, image)) = cache.get(&key) {
                if owner.strong_count() != 0 {
                    return image.clone();
                }
            }
            if cache.len() >= 192 {
                cache.retain(|_, (owner, _)| owner.strong_count() != 0);
            }
            let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &self.rgba,
                self.width,
                self.height,
            ));
            cache.insert(key, (Arc::downgrade(self), image.clone()));
            image
        })
    }
}
pub fn source(f: &yapfast_discord::message::Attachment, revealed: bool) -> Option<&str> {
    if f.spoiler && !revealed {
        return None;
    }
    f.still_url
        .as_deref()
        .or(f.preview_url.as_deref())
        .or_else(|| f.image.then_some(f.url.as_ref()))
}
pub async fn fetch(service: &Discord, url: &str, media: bool) -> Option<Arc<Thumbnail>> {
    let bytes = service
        .download_now(url, if media { 4 * 1024 * 1024 } else { 512 * 1024 })
        .await
        .ok()?;
    tokio::task::spawn_blocking(move || decode(&bytes, media))
        .await
        .ok()
        .flatten()
}
pub fn decode(bytes: &[u8], media: bool) -> Option<Arc<Thumbnail>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    let max_side = if media { 4096 } else { 512 };
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(if media {
        48 * 1024 * 1024
    } else {
        4 * 1024 * 1024
    });
    reader.limits(limits);
    let (width, height) = if media { (640, 400) } else { (96, 96) };
    let thumb = reader.decode().ok()?.thumbnail(width, height).to_rgba8();
    Some(Arc::new(Thumbnail {
        width: thumb.width(),
        height: thumb.height(),
        rgba: thumb.into_raw(),
    }))
}
