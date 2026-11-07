use super::*;
const HASH: &str = "0123456789abcdef0123456789abcdef";
fn profile(avatar: Option<String>, banner: Option<String>) -> Profile {
    Profile {
        id: "200".into(),
        avatar,
        banner,
        ..Default::default()
    }
}
#[test]
fn server_avatar_and_banner_keep_the_exact_received_member_identity() {
    for (kind, directory) in [
        (ProfilePicture::Avatar, "avatars"),
        (ProfilePicture::Banner, "banners"),
    ] {
        let path = format!("guilds/300/users/200/{directory}/{HASH}");
        let url = format!("https://cdn.discordapp.com/{path}.png?size=128");
        let p = if kind == ProfilePicture::Avatar {
            profile(Some(url), None)
        } else {
            profile(None, Some(url))
        };
        let asset = Asset::from_profile("100", &p, kind).unwrap();
        assert_eq!(
            asset.url(),
            format!("https://cdn.discordapp.com/{path}.png?size=4096")
        );
        assert_eq!(asset.user(), "200");
        assert_eq!(asset.account(), "100");
        assert!(!asset.name().contains(['/', '\\', ':']));
    }
}
#[test]
fn animated_hash_uses_animated_webp_even_when_the_received_thumbnail_is_static_png() {
    let p = profile(
        None,
        Some(format!(
            "https://cdn.discordapp.com/banners/200/a_{HASH}.png?size=600"
        )),
    );
    let asset = Asset::from_profile("100", &p, ProfilePicture::Banner).unwrap();
    assert_eq!(
        asset.url(),
        format!("https://cdn.discordapp.com/banners/200/a_{HASH}.webp?size=4096&animated=true")
    );
    assert_eq!(asset.name(), "banner-200.webp");
    assert!(Asset::from_profile("100", &p, ProfilePicture::Avatar).is_none());
}
#[test]
fn default_avatars_are_real_cdn_assets_not_an_invented_personal_hash() {
    let p = profile(
        Some("https://cdn.discordapp.com/embed/avatars/3.png".into()),
        None,
    );
    let asset = Asset::from_profile("100", &p, ProfilePicture::Avatar).unwrap();
    assert_eq!(
        asset.url(),
        "https://cdn.discordapp.com/embed/avatars/3.png?size=4096"
    );
    assert!(Asset::from_profile("100", &p, ProfilePicture::Banner).is_none());
    assert!(Asset::from_profile("100", &profile(None, None), ProfilePicture::Avatar).is_none());
}
#[test]
fn untrusted_urls_never_become_a_privileged_open_url_fetch() {
    let good = format!("https://cdn.discordapp.com/avatars/200/{HASH}.png?size=128");
    let bad = [
        good.replace("cdn.discordapp.com", "cdn.discordapp.com.evil.invalid"),
        good.replace("https:", "http:"),
        good.replace("https:", "file:"),
        good.replace("cdn.discordapp.com", "user@cdn.discordapp.com"),
        good.replace("cdn.discordapp.com", "cdn.discordapp.com:444"),
        good.replace("/avatars/200/", "/avatars/201/"),
        good.replace("/avatars/200/", "/attachments/200/"),
        good.replace("/avatars/", "/avatars/../avatars/"),
        good.replace("/avatars/", "/%61vatars/"),
        good.replace(".png?", ".svg?"),
        format!("{good}#fragment"),
        format!("{good}&url=https://evil.invalid"),
        format!("{good}\n"),
        good.replace(HASH, "bad-hash"),
        good.replace("https:", "javascript:"),
        "https://127.0.0.1/avatars/200/fixture.png".into(),
    ];
    for url in bad {
        assert!(
            Asset::from_profile(
                "100",
                &profile(Some(url.clone()), None),
                ProfilePicture::Avatar
            )
            .is_none(),
            "accepted {url}"
        );
    }
    let mut p = profile(Some(good), None);
    p.id = "@matrix:test".into();
    assert!(Asset::from_profile("100", &p, ProfilePicture::Avatar).is_none());
}
#[test]
fn a_new_profile_hash_invalidates_old_bytes_even_for_the_same_user_and_account() {
    let mut p = profile(
        Some(format!(
            "https://cdn.discordapp.com/avatars/200/{HASH}.png?size=128"
        )),
        None,
    );
    let session = Session::open("100", &p, Slot::Profile, ProfilePicture::Avatar).unwrap();
    p.avatar = p
        .avatar
        .map(|url| url.replace(HASH, "fedcba9876543210fedcba9876543210"));
    assert!(!session.is_current(Some("100"), Some(&p)));
}
