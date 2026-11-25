use super::*;
#[test]
fn decoration_is_trusted_bounded_and_member_null_clears_global() {
    let mut input = serde_json::json!({"user":{"id":"200","username":"Ada","avatar_decoration_data":{"sku_id":"123","asset":"a_safe_12","expires_at":1000}}});
    let value = serde_json::to_value(parse(&input, None).unwrap()).unwrap();
    assert!(
        value["decoration"]["url"]
            .as_str()
            .is_some_and(|url| url.ends_with("/avatar-decoration-presets/a_safe_12.png")),
        "only fixed preset URL is constructed"
    );
    for bad in ["../unsafe", "https://evil.invalid/a", "bad?x", "a/b", ""] {
        input["user"]["avatar_decoration_data"]["asset"] = serde_json::json!(bad);
        assert!(
            serde_json::to_value(parse(&input, None).unwrap()).unwrap()["decoration"].is_null()
        );
    }
    input["user"]["avatar_decoration_data"]["asset"] = serde_json::json!("safe");
    input["user"]["avatar_decoration_data"]["expires_at"] = serde_json::json!("tomorrow");
    assert!(serde_json::to_value(parse(&input, None).unwrap()).unwrap()["decoration"].is_null());
    input["user"]["avatar_decoration_data"]["expires_at"] = serde_json::Value::Null;
    input["guild_member"] = serde_json::json!({"avatar_decoration_data":null});
    assert!(
        serde_json::to_value(parse(&input, Some("10")).unwrap()).unwrap()["decoration"].is_null(),
        "explicit member null must not revive a global preset"
    );
}
#[test]
fn received_name_style_is_bounded_and_persisted_without_mutation() {
    let mut input = serde_json::json!({"user":{"id":"200","username":"Ada","display_name_styles":{"font_id":1,"effect_id":8,"colors":[16711680,255]}}});
    let parsed = parse(&input, None).unwrap();
    let persisted = serde_json::to_value(parsed).unwrap();
    assert_eq!(
        persisted["name_style"]["font_id"], 1,
        "received style survives the production parser"
    );
    for invalid in [
        serde_json::json!([]),
        serde_json::json!([16777216]),
        serde_json::json!(["red"]),
        serde_json::json!([1, 2, 3, 4, 5, 6]),
    ] {
        input["user"]["display_name_styles"]["colors"] = invalid;
        let parsed = serde_json::to_value(parse(&input, None).unwrap()).unwrap();
        assert!(parsed["name_style"].is_null());
    }
    input["user"]["display_name_styles"] =
        serde_json::json!({"font_id":4294967295u64,"effect_id":1009,"colors":[0]});
    let parsed = serde_json::to_value(parse(&input, None).unwrap()).unwrap();
    assert_eq!(parsed["name_style"]["font_id"], 4294967295u64);
    input["user"]["display_name_styles"]["font_id"] = serde_json::json!(4294967296u64);
    assert!(serde_json::to_value(parse(&input, None).unwrap()).unwrap()["name_style"].is_null());
}
