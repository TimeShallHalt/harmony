use super::*;

#[test]
fn viewer_lifecycle_shelving_channel_cancels_only_its_original_task() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut channel = OpenChannel::new("discord:10:20").unwrap();
            let request = crate::original::Request::new();
            let (started, waiting) = tokio::sync::oneshot::channel();
            let (preview_release, preview_wait) = tokio::sync::oneshot::channel::<()>();
            let (preview_done, preview_result) = tokio::sync::oneshot::channel();
            channel.tasks.spawn(async move {
                preview_wait.await.unwrap();
                let _ = preview_done.send(());
            });
            channel.viewer_task.start(
                request.clone(),
                async move {
                    let _ = started.send(());
                    std::future::pending::<()>().await;
                    Ok(Vec::new())
                },
                |_| panic!("a shelved channel must not decode its original"),
                |_| panic!("a shelved channel must not publish its original"),
            );
            tokio::time::timeout(Duration::from_secs(2), waiting)
                .await
                .unwrap()
                .unwrap();
            channel.shelve();
            assert!(request.is_cancelled());
            preview_release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), preview_result)
                .await
                .unwrap()
                .unwrap();
            assert!(
                channel.tasks.join_next().await.unwrap().is_ok(),
                "shelving the viewer must not abort unrelated channel preview/message tasks"
            );
        });
}

#[test]
fn viewer_lifecycle_forbidden_original_is_actionable_without_blind_retry() {
    let error = anyhow::Error::new(Refused("403".parse().unwrap()));
    let message = original_download_error(&error);
    assert!(
        message.contains("Reopen") && message.contains("conversation"),
        "{message}"
    );
    assert!(
        message.contains("link") && message.contains("access"),
        "{message}"
    );
    assert!(
        !message.contains("Try again"),
        "do not retry an expired or inaccessible signed URL in a loop"
    );
}

#[test]
fn viewer_lifecycle_too_large_names_the_safe_limit_and_save_original() {
    let error = anyhow::Error::new(yapfast_discord::api::TooLarge)
        .context("transport refused an oversized original");
    let message = original_download_error(&error);
    assert!(
        message.contains("safe") && message.contains("limit"),
        "{message}"
    );
    assert!(
        message.contains("Save") && message.contains("original"),
        "{message}"
    );
    assert!(
        !message.contains("Try again"),
        "a permanent byte limit is not a retryable stall"
    );
    assert_eq!(
        MAX_PREVIEW_DOWNLOAD_BYTES,
        20 * 1024 * 1024,
        "this slice does not relax the preview/transport policy"
    );
}

fn assert_original_provider_still(format: image::ImageFormat, extension: &str) {
    let source = image::RgbImage::from_fn(3840, 8, |x, y| {
        image::Rgb([(x & 255) as u8, ((x ^ y) & 255) as u8, (y * 31) as u8])
    });
    let mut data = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(source)
        .write_to(&mut data, format)
        .unwrap();
    // JPEG is lossy: compare the provider with the decoded encoded source,
    // not with pre-encoding RGB values that JPEG was never able to preserve.
    let expected = image::load_from_memory(data.get_ref()).unwrap().to_rgba8();
    let shown = super::super::render::decode(
        &format!("https://cdn.discordapp.com/attachments/11/12/original.{extension}"),
        data.get_ref(),
        (crate::chat::VIEW_WIDTH, crate::chat::VIEW_HEIGHT),
    )
    .expect("the production Discord renderer must accept the original");
    assert_eq!((shown.width, shown.height), expected.dimensions());
    assert_eq!(&shown.rgba[..], expected.as_raw().as_slice());
}

#[test]
fn original_provider_renderer_preserves_png_pixels_above_the_old_cap() {
    assert_original_provider_still(image::ImageFormat::Png, "png");
}

#[test]
fn original_provider_renderer_preserves_decoded_jpeg_pixels_above_the_old_cap() {
    assert_original_provider_still(image::ImageFormat::Jpeg, "jpg");
}

#[test]
fn original_provider_renderer_preserves_gif_frame_pixels_and_delays() {
    let first = image::RgbaImage::from_fn(3200, 2, |x, y| {
        let value = if (x + y) % 2 == 0 { 0 } else { 255 };
        image::Rgba([value, value, value, 255])
    });
    let second = image::RgbaImage::from_pixel(3200, 2, image::Rgba([0, 0, 255, 255]));
    let mut data = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut data);
        for (pixels, delay) in [(first.clone(), 80), (second.clone(), 120)] {
            encoder
                .encode_frame(image::Frame::from_parts(
                    pixels,
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(delay, 1),
                ))
                .unwrap();
        }
    }
    let shown = super::super::render::decode(
        "https://cdn.discordapp.com/attachments/11/12/original.gif",
        &data,
        (crate::chat::VIEW_WIDTH, crate::chat::VIEW_HEIGHT),
    )
    .expect("the production provider renderer must preserve the animation");
    assert_eq!((shown.width, shown.height), first.dimensions());
    assert_eq!(shown.animation.as_ref().unwrap().delays, [80, 120]);
    let frames: Vec<_> = shown.frames().collect();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].as_slice(), first.as_raw().as_slice());
    assert_eq!(frames[1].as_slice(), second.as_raw().as_slice());
}

#[test]
fn original_provider_missing_conversion_metadata_has_an_actionable_decoder_failure() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let original = "https://cdn.discordapp.com/attachments/11/12/original.svg";
            for proxy in [
                None,
                Some("https://media.discordapp.net/attachments/11/12/original.svg"),
            ] {
                let mut media = serde_json::json!({
                    "filename": "original.svg", "url": original,
                    "content_type": "image/svg+xml"
                });
                if let Some(proxy) = proxy {
                    media["proxy_url"] = serde_json::json!(proxy);
                }
                let message = yapfast_discord::message::parse(&serde_json::json!({
                    "id": "1300000000000000000", "channel_id": "11", "content": "",
                    "author": {"id": "1", "username": "fixture"},
                    "attachments": [media]
                }))
                .unwrap();
                assert_eq!(message.attachments[0].view_url.as_deref(), Some(original));
                assert!(message.attachments[0].preview_url.is_none());
                let url = message.attachments[0].view_url.clone().unwrap();
                let mut task = crate::original::Task::default();
                let (done, received) = tokio::sync::oneshot::channel();
                task.start(
                    crate::original::Request::new(),
                    async {
                        Ok(br#"<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160"><rect width="3840" height="2160" fill="white"/></svg>"#.to_vec())
                    },
                    move |data| {
                        super::super::render::decode(
                            &url,
                            &data,
                            (crate::chat::VIEW_WIDTH, crate::chat::VIEW_HEIGHT),
                        )
                    },
                    move |loaded| {
                        let _ = done.send(loaded);
                    },
                );
                let error = tokio::time::timeout(Duration::from_secs(2), received)
                    .await
                    .unwrap()
                    .unwrap()
                    .expect_err("unsupported source must fail explicitly, not become a blurry success");
                assert!(error.contains("Save") && error.contains("open"), "{error}");
            }
        });
}
