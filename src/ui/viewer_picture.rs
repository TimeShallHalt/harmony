//! The viewer's single converted image, not the timeline's preview cache.
use std::sync::Arc;
use yapfast_core::chat::Picture;

pub(super) fn image(
    shown: Option<&Arc<Picture>>,
    cache: &mut Option<(Arc<Picture>, slint::Image)>,
    make: impl FnOnce(&Picture) -> slint::Image,
) -> slint::Image {
    match (shown, cache.as_ref()) {
        (Some(shown), Some((cached, image))) if Arc::ptr_eq(shown, cached) => image.clone(),
        (Some(shown), _) => {
            let image = make(shown);
            *cache = Some((shown.clone(), image.clone()));
            image
        }
        (None, _) => {
            *cache = None;
            slint::Image::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_picture_without_a_preview_releases_the_previous_original() {
        let original = Arc::new(Picture::default());
        let pixels = Arc::downgrade(&original);
        let mut cache = None;
        image(Some(&original), &mut cache, |_| slint::Image::default());
        drop(original);
        assert!(pixels.upgrade().is_some());
        image(None, &mut cache, |_| panic!("nothing to convert"));
        assert!(
            pixels.upgrade().is_none(),
            "an attachment without a preview must not pin the inactive original"
        );
    }

    #[test]
    fn viewer_picture_reuses_only_the_current_source() {
        let original = Arc::new(Picture::default());
        let old = Arc::downgrade(&original);
        let mut cache = None;
        image(Some(&original), &mut cache, |_| slint::Image::default());
        image(Some(&original), &mut cache, |_| {
            panic!("same source is already converted")
        });
        drop(original);
        let next = Arc::new(Picture::default());
        image(Some(&next), &mut cache, |_| slint::Image::default());
        assert!(old.upgrade().is_none());
        assert!(Arc::ptr_eq(&cache.unwrap().0, &next));
    }
}
