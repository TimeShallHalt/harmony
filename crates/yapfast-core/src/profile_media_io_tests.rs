use super::*;

#[tokio::test]
async fn typed_safe_limit_and_access_failures_reach_the_profile_loader_sink() {
    for error in [
        anyhow::Error::new(yapfast_discord::api::TooLarge).context("fixture byte bound"),
        anyhow::Error::new(yapfast_discord::api::Refused("403".parse().unwrap())),
    ] {
        let oversized = error.is::<yapfast_discord::api::TooLarge>();
        let mut loader = Loader::default();
        let (done, result) = tokio::sync::oneshot::channel();
        loader.start(
            1,
            Request::new(),
            async move { Err(download_error(&error)) },
            move |loaded| {
                let _ = done.send(loaded);
            },
        );
        let loaded = tokio::time::timeout(Duration::from_secs(2), result)
            .await
            .unwrap()
            .unwrap()
            .into_inner();
        let text = loaded.picture.unwrap_err();
        assert!(loaded.bytes.is_none());
        if oversized {
            assert!(text.contains("safe") && text.contains("limit"), "{text}");
        } else {
            assert!(
                text.contains("access") && text.contains("profile"),
                "{text}"
            );
        }
        assert!(
            !text.contains("try again"),
            "permanent policy errors must not suggest blind retry: {text}"
        );
    }
}

async fn loaded(data: Vec<u8>) -> Loaded {
    let mut loader = Loader::default();
    let (done, result) = tokio::sync::oneshot::channel();
    loader.start(1, Request::new(), async { Ok(data) }, move |loaded| {
        let _ = done.send(loaded);
    });
    tokio::time::timeout(Duration::from_secs(2), result)
        .await
        .unwrap()
        .unwrap()
        .into_inner()
}
#[tokio::test]
async fn animation_decodes_from_the_real_webp_fixture_without_losing_its_frame_delays() {
    let bytes = include_bytes!("../../../testing/fixtures/gifs/wave.webp");
    let loaded = loaded(bytes.to_vec()).await;
    let picture = loaded.picture.unwrap();
    let animation = picture.animation.as_ref().expect("animated WebP fixture");
    assert!(animation.delays.len() > 1);
    assert!(animation.delays.iter().all(|delay| *delay > 0));
    assert_eq!(loaded.bytes.as_deref(), Some(bytes.as_slice()));
}
#[tokio::test]
async fn still_pixels_are_not_upscaled_and_original_bytes_survive_decode_errors() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(9, 7, image::Rgba([30, 70, 110, 255]))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let loaded = loaded(encoded.into_inner()).await;
    let picture = loaded.picture.unwrap();
    assert_eq!((picture.width, picture.height), (9, 7));
    assert_eq!(&picture.rgba[..4], &[30, 70, 110, 255]);
    let malformed = b"not a picture".to_vec();
    let result = self::loaded(malformed.clone()).await;
    assert!(result.picture.is_err());
    assert_eq!(result.bytes.as_deref(), Some(malformed.as_slice()));
}
#[tokio::test]
async fn size_limit_and_failed_network_deliver_explicit_errors_without_retaining_bytes() {
    let result = loaded(vec![0; MAX_BYTES + 1]).await;
    assert!(result.picture.unwrap_err().contains("too large"));
    assert!(result.bytes.is_none());
    let mut loader = Loader::default();
    let (done, result) = tokio::sync::oneshot::channel();
    loader.start(
        1,
        Request::new(),
        async { Err("fixture download failure") },
        move |loaded| {
            let _ = done.send(loaded);
        },
    );
    let loaded = result.await.unwrap().into_inner();
    assert_eq!(loaded.picture.unwrap_err(), "fixture download failure");
    assert!(loaded.bytes.is_none());
}
#[tokio::test]
async fn total_deadline_drops_a_stalled_fetch_and_surfaces_an_error() {
    let mut loader = Loader::default();
    let (done, result) = tokio::sync::oneshot::channel();
    loader.start_with_deadline(
        1,
        Request::new(),
        std::future::pending(),
        move |loaded| {
            let _ = done.send(loaded);
        },
        Duration::from_millis(20),
    );
    let loaded = tokio::time::timeout(Duration::from_secs(1), result)
        .await
        .unwrap()
        .unwrap()
        .into_inner();
    assert!(loaded.picture.unwrap_err().contains("too long"));
    assert!(loaded.bytes.is_none());
}
#[tokio::test]
async fn cancelling_a_replaced_request_does_not_abort_the_new_one() {
    let mut loader = Loader::default();
    loader.start(1, Request::new(), std::future::pending(), |_| {
        panic!("replaced request completed")
    });
    let (release, wait) = tokio::sync::oneshot::channel();
    let (done, result) = tokio::sync::oneshot::channel();
    loader.start(
        2,
        Request::new(),
        async {
            wait.await.unwrap();
            Ok(vec![1, 2, 3])
        },
        move |loaded| {
            let _ = done.send(loaded);
        },
    );
    loader.cancel(1);
    release.send(()).unwrap();
    let loaded = tokio::time::timeout(Duration::from_secs(1), result)
        .await
        .unwrap()
        .unwrap()
        .into_inner();
    assert_eq!(loaded.bytes.as_deref(), Some([1, 2, 3].as_slice()));
}
#[tokio::test]
async fn the_decoder_gate_blocks_new_decodes_until_the_existing_one_releases() {
    let mut loader = Loader::default();
    let permit = crate::original::admission().acquire_owned().await.unwrap();
    let (done, mut result) = tokio::sync::oneshot::channel();
    loader.start(1, Request::new(), async { Ok(vec![0]) }, move |loaded| {
        let _ = done.send(loaded);
    });
    tokio::task::yield_now().await;
    assert!(result.try_recv().is_err());
    drop(permit);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), result)
            .await
            .unwrap()
            .is_ok()
    );
}
