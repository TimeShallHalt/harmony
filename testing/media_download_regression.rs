//! Real production HTTP downloader against generated loopback-only fixtures.
//! No saved login, cookie file, Discord host or real account is contacted.
use yapfast_discord::api::{Api, TooLarge};

fn url(path: &str) -> String {
    format!(
        "{}{path}",
        std::env::var("YAPFAST_DISCORD_TEST_SERVER").unwrap()
    )
}

fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn media_redirect_cannot_contact_an_unapproved_destination() {
    run(async {
        let api = Api::anonymous().unwrap();
        let error = api
            .download_now(&url("/redirect-outside?hm=fixture-secret"), 1024)
            .await
            .expect_err("downloader followed a redirect outside its allowlist");
        assert!(
            !format!("{error:#}").contains("fixture-secret"),
            "signed URLs must not leak in errors"
        );
        let count = api.download_now(&url("/target-count"), 1024).await.unwrap();
        assert_eq!(
            count, b"0",
            "redirect target was contacted before validation"
        );
    });
}

#[test]
fn media_redirect_can_follow_an_approved_relative_destination() {
    run(async {
        let api = Api::anonymous().unwrap();
        assert_eq!(
            api.download_now(&url("/redirect-safe"), 1024)
                .await
                .unwrap(),
            b"original-fixture"
        );
    });
}

#[test]
fn media_redirect_chain_is_bounded() {
    run(async {
        let api = Api::anonymous().unwrap();
        assert!(api.download_now(&url("/loop"), 1024).await.is_err());
        let count = api.download_now(&url("/loop-count"), 1024).await.unwrap();
        let count: usize = std::str::from_utf8(&count).unwrap().parse().unwrap();
        assert!(count <= 6, "too many redirected requests: {count}");
    });
}

#[test]
fn declared_oversized_download_has_a_typed_limit_error() {
    run(async {
        let api = Api::anonymous().unwrap();
        let error = api.download_now(&url("/sized"), 16).await.unwrap_err();
        assert!(error.is::<TooLarge>(), "expected TooLarge, got {error:#}");
    });
}

#[test]
fn chunked_oversized_download_has_a_typed_limit_error() {
    run(async {
        let api = Api::anonymous().unwrap();
        let error = api.download_now(&url("/chunked"), 16).await.unwrap_err();
        assert!(error.is::<TooLarge>(), "expected TooLarge, got {error:#}");
    });
}

#[test]
fn hostile_length_is_refused_before_reading_the_body() {
    run(async {
        let api = Api::anonymous().unwrap();
        let error = api
            .download_now(&url("/hostile-length"), 16)
            .await
            .unwrap_err();
        assert!(
            error.is::<TooLarge>(),
            "expected immediate TooLarge, got {error:#}"
        );
    });
}

#[test]
fn download_exactly_at_the_limit_keeps_every_byte() {
    run(async {
        let api = Api::anonymous().unwrap();
        assert_eq!(
            api.download_now(&url("/sized"), 64).await.unwrap(),
            vec![b'x'; 64]
        );
    });
}
