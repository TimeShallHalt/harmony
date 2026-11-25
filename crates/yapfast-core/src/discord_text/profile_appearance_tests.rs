//! Received parser -> production appearance projection -> UI accessor.
use super::*;
use serde_json::{Value, json};
use yapfast_discord::message::Names;

fn received() -> Value {
    json!({"user": {"id": "200", "username": "Ada",
        "avatar_decoration_data": {"sku_id": "123", "asset": "a_safe", "expires_at": null},
        "display_name_styles": {"font_id": 8, "effect_id": 2, "colors": [16711680, 255]}
    }})
}

fn projected(value: &Value) -> Profile {
    let received = yapfast_discord::profile::parse(value, Some("10")).unwrap();
    profile_appearance(&received)
}

#[test]
fn projected_member_null_cannot_revive_the_names_global_decoration() {
    let mut value = received();
    let mut names = Names::default();
    names.learn_user(&value["user"]);
    assert!(
        names.decoration(Some("10"), "200").is_some(),
        "the independent global cache still has art"
    );
    value["guild_member"] = json!({"avatar_decoration_data": null});
    let shown = projected(&value);
    assert!(shown.decoration.is_none());
    assert!(shown.avatar_decoration.is_none());
    assert_eq!(
        shown.avatar_decoration_url(),
        None,
        "the root adapter has no shop URL to resurrect"
    );
}

#[test]
fn projected_malformed_expiry_cannot_revive_a_shop_url() {
    let mut value = received();
    let mut names = Names::default();
    names.learn_user(&value["user"]);
    for expiry in [json!("tomorrow"), json!(-1), json!(1.5), json!({})] {
        value["user"]["avatar_decoration_data"]["expires_at"] = expiry;
        // The loose Names SKU cache would still return the shop artwork.
        names.learn_user(&value["user"]);
        assert!(names.decoration(Some("10"), "200").is_some());
        let shown = projected(&value);
        assert!(shown.decoration.is_none() && shown.avatar_decoration.is_none());
        assert_eq!(shown.avatar_decoration_url(), None);
    }
}

#[test]
fn projected_accepted_decoration_keeps_shop_art_but_both_routes_obey_expiry() {
    let mut shown = projected(&received());
    assert!(
        shown
            .avatar_decoration_url()
            .unwrap()
            .ends_with("/collectibles-shop/123/static")
    );
    assert!(
        shown
            .decoration
            .as_ref()
            .unwrap()
            .url
            .ends_with("/avatar-decoration-presets/a_safe.png")
    );
    assert!(shown.decoration.as_ref().unwrap().visible_at(1));
    shown.decoration.as_mut().unwrap().expires_at = Some(1);
    assert!(!shown.decoration.as_ref().unwrap().visible_at(1));
    assert_eq!(shown.avatar_decoration_url(), None);
}

#[test]
fn projected_cached_invalid_sku_never_becomes_a_shop_url() {
    let mut received = yapfast_discord::profile::parse(&received(), None).unwrap();
    for invalid in [
        "",
        "../unsafe",
        "https://evil.invalid",
        "18446744073709551616",
        " 123",
        "+123",
    ] {
        received.decoration.as_mut().unwrap().sku_id = invalid.into();
        let shown = profile_appearance(&received);
        assert!(
            shown.avatar_decoration.is_none(),
            "revalidate cached SKU {invalid:?}"
        );
        assert_eq!(shown.avatar_decoration_url(), None);
    }
}

#[test]
fn projected_style_uses_the_accepted_response_not_an_independent_names_cache() {
    let mut value = received();
    let mut names = Names::default();
    names.learn_user(&value["user"]);
    assert!(names.style(Some("10"), "200").is_some());
    let shown = projected(&value);
    let style = shown.style.as_ref().unwrap();
    assert_eq!(
        (style.font, style.effect, style.colors()),
        (8, 2, &[16711680, 255][..])
    );
    value["user"]["display_name_styles"]["colors"] = json!([16777216]);
    let shown = projected(&value);
    assert!(shown.name_style.is_none() && shown.style.is_none());
    assert!(
        names.style(Some("10"), "200").is_some(),
        "cached style may outlive this response"
    );
}
