//! Pixel-level original-image regression against the compiled production core.
//! This is also kept as a normal unit test in core chat.rs. This tiny executable
//! makes the RED/GREEN loop usable without rebuilding unrelated desktop crates.
use yapfast_core::chat::{PREVIEW_HEIGHT, PREVIEW_WIDTH, VIEW_HEIGHT, VIEW_WIDTH, decode_picture};

#[test]
fn viewer_source_pixels_are_not_downscaled_or_upscaled() {
    let original = image::RgbaImage::from_fn(3840, 1200, |x, y| {
        image::Rgba([(x & 255) as u8, (y & 255) as u8, ((x ^ y) & 255) as u8, 255])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    original
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let viewed = decode_picture(bytes.get_ref(), VIEW_WIDTH, VIEW_HEIGHT).unwrap();
    println!(
        "source={:?}; decoded={}x{}; viewer bounds={}x{}",
        original.dimensions(),
        viewed.width,
        viewed.height,
        VIEW_WIDTH,
        VIEW_HEIGHT
    );
    assert_eq!(
        (viewed.width, viewed.height),
        original.dimensions(),
        "viewer lost source resolution"
    );
    assert_eq!(
        &viewed.rgba[..],
        original.as_raw().as_slice(),
        "viewer altered original pixels"
    );
    let preview = decode_picture(bytes.get_ref(), PREVIEW_WIDTH, PREVIEW_HEIGHT).unwrap();
    assert_eq!(
        preview.width, PREVIEW_WIDTH,
        "timeline must still use cheap previews"
    );
    assert!(preview.height < original.height());
}

#[test]
fn attachment_view_requests_original_instead_of_a_resized_webp() {
    let original = "https://cdn.discordapp.com/attachments/11/12/grid.png?ex=aa&is=bb&hm=cc";
    let proxy = "https://media.discordapp.net/attachments/11/12/grid.png?ex=aa&is=bb&hm=cc";
    let message = yapfast_discord::message::parse(&serde_json::json!({
        "id": "1300000000000000000", "channel_id": "11", "content": "",
        "author": {"id": "1", "username": "fixture"},
        "attachments": [{"filename": "grid.png", "size": 1024,
            "url": original, "proxy_url": proxy, "content_type": "image/png",
            "width": 3840, "height": 1200}]
    }))
    .unwrap();
    let attachment = &message.attachments[0];
    assert_eq!(
        attachment.view_url.as_deref(),
        Some(original),
        "opening an attachment must request the original signed URL verbatim"
    );
    let preview = attachment.preview_url.as_ref().unwrap();
    assert!(preview.starts_with(proxy));
    assert!(preview.contains("width=550"));
    assert!(preview.contains("format=webp"));
}

#[test]
fn external_embeds_view_full_proxy_without_contacting_the_source() {
    let proxy =
        "https://images-ext-1.discordapp.net/external/hash/https/example.invalid/detail.png";
    let parsed = yapfast_discord::embed::parse(&serde_json::json!([{
        "type": "rich", "title": "Fixture", "image": {
            "url": "https://example.invalid/detail.png", "proxy_url": proxy,
            "width": 3840, "height": 2160
        }
    }]));
    assert_eq!(parsed.pictures[0].view_url.as_deref(), Some(proxy));
    assert!(
        parsed.pictures[0]
            .preview_url
            .as_ref()
            .unwrap()
            .contains("width=360")
    );
}

#[test]
fn component_gallery_views_original_discord_attachments() {
    let original = "https://cdn.discordapp.com/attachments/11/12/gallery.png?ex=aa&is=bb&hm=cc";
    let parsed = yapfast_discord::components::parse(&serde_json::json!([{
        "type": 12, "items": [{"media": {
            "url": original,
            "proxy_url": "https://media.discordapp.net/attachments/11/12/gallery.png?ex=aa&is=bb&hm=cc",
            "width": 3840, "height": 2160, "content_type": "image/png"
        }}]
    }]));
    assert_eq!(parsed.media[0].view_url.as_deref(), Some(original));
}

#[test]
fn supported_attachment_formats_keep_original_urls_without_preview_metadata() {
    for (kind, name) in [
        ("image/png", "still.png"),
        ("image/apng", "moving.apng"),
        ("image/jpeg; charset=binary", "photo.jpg"),
        ("image/gif", "moving.gif"),
        ("image/webp", "moving.webp"),
    ] {
        let original =
            format!("https://cdn.discordapp.com/attachments/11/12/{name}?ex=aa&is=bb&hm=cc%2Bdd");
        let message = yapfast_discord::message::parse(&serde_json::json!({
            "id": "1300000000000000000", "channel_id": "11", "content": "",
            "author": {"id": "1", "username": "fixture"},
            "attachments": [{"filename": name, "url": original, "content_type": kind}]
        }))
        .unwrap();
        assert_eq!(
            message.attachments[0].view_url.as_deref(),
            Some(original.as_str()),
            "{kind}"
        );
        assert!(message.attachments[0].preview_url.is_none());
    }
}

#[test]
fn external_and_lookalike_gallery_sources_never_bypass_the_proxy() {
    let proxy =
        "https://images-ext-1.discordapp.net/external/hash/https/example.invalid/detail.png";
    for source in [
        "https://example.invalid/detail.png",
        "https://cdn.discordapp.com.evil.invalid/attachments/11/12/detail.png",
        "https://cdn.discordapp.com@127.0.0.1/attachments/11/12/detail.png",
        "http://cdn.discordapp.com/attachments/11/12/detail.png",
    ] {
        let parsed = yapfast_discord::components::parse(&serde_json::json!([{
            "type": 12, "items": [{"media": {"url": source, "proxy_url": proxy,
                "content_type": "image/png", "width": 3840, "height": 2160}}]
        }]));
        assert_eq!(parsed.media[0].view_url.as_deref(), Some(proxy), "{source}");
    }
}

#[test]
fn unsupported_original_formats_keep_full_size_animated_conversion() {
    let proxy =
        "https://images-ext-1.discordapp.net/external/hash/https/example.invalid/moving.avif";
    let parsed = yapfast_discord::embed::parse(&serde_json::json!([{
        "type": "image", "url": "https://example.invalid/moving.avif",
        "thumbnail": {"url": "https://example.invalid/moving.avif", "proxy_url": proxy,
                      "width": 3840, "height": 2160}
    }]));
    let viewed = parsed.pictures[0].view_url.as_ref().unwrap();
    assert!(viewed.starts_with(proxy));
    assert!(viewed.contains("animated=true&format=webp"));
    assert!(viewed.contains("width=3840&height=2160"));
}

#[test]
fn original_gif_frames_keep_their_pixels_and_delays() {
    let first = image::RgbaImage::from_fn(3200, 2, |x, y| {
        let value = if (x + y) % 2 == 0 { 0 } else { 255 };
        image::Rgba([value, value, value, 255])
    });
    let second = image::RgbaImage::from_pixel(3200, 2, image::Rgba([255, 0, 0, 255]));
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        encoder
            .encode_frame(image::Frame::from_parts(
                first.clone(),
                0,
                0,
                image::Delay::from_numer_denom_ms(80, 1),
            ))
            .unwrap();
        encoder
            .encode_frame(image::Frame::from_parts(
                second.clone(),
                0,
                0,
                image::Delay::from_numer_denom_ms(120, 1),
            ))
            .unwrap();
    }
    let viewed = decode_picture(&bytes, VIEW_WIDTH, VIEW_HEIGHT).unwrap();
    assert_eq!((viewed.width, viewed.height), first.dimensions());
    assert_eq!(viewed.animation.as_ref().unwrap().delays, [80, 120]);
    let frames: Vec<_> = viewed.frames().collect();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].as_slice(), first.as_raw().as_slice());
    assert_eq!(frames[1].as_slice(), second.as_raw().as_slice());
}

#[test]
fn images_above_safety_bounds_are_refused_not_silently_shrunk() {
    let original = image::RgbaImage::new(VIEW_WIDTH + 1, 1);
    let mut bytes = std::io::Cursor::new(Vec::new());
    original
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    assert!(decode_picture(bytes.get_ref(), VIEW_WIDTH, VIEW_HEIGHT).is_none());
    assert!(decode_picture(b"not an image", VIEW_WIDTH, VIEW_HEIGHT).is_none());
}
