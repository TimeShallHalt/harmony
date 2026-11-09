//! Bounded received appearance only. No editor, Board, widget or entitlement API.
use serde_json::Value;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Decoration {
    pub sku_id: String,
    pub asset: String,
    pub url: String,
    pub expires_at: Option<u64>,
}
pub(super) fn decoration(value: &Value) -> Option<Decoration> {
    let sku = value["sku_id"].as_str()?;
    let asset = value["asset"].as_str()?;
    if sku.is_empty()
        || sku.len() > 20
        || !sku.bytes().all(|b| b.is_ascii_digit())
        || sku.parse::<u64>().is_err()
        || asset.is_empty()
        || asset.len() > 128
        || !asset
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let expires_at = match &value["expires_at"] {
        Value::Null => None,
        v => Some(v.as_u64()?),
    };
    Some(Decoration {
        sku_id: sku.into(),
        asset: asset.into(),
        url: format!(
            "{}/avatar-decoration-presets/{asset}.png",
            crate::ready::cdn()
        ),
        expires_at,
    })
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NameStyle {
    pub font_id: u32,
    pub effect_id: u32,
    pub colors: Vec<u32>,
}
pub(super) fn name_style(value: &Value) -> Option<NameStyle> {
    let colors = value["colors"].as_array()?;
    if colors.is_empty() || colors.len() > 5 {
        return None;
    }
    Some(NameStyle {
        font_id: value["font_id"].as_u64()?.try_into().ok()?,
        effect_id: value["effect_id"].as_u64()?.try_into().ok()?,
        colors: colors
            .iter()
            .map(|v| v.as_u64().filter(|v| *v <= 0xffffff).map(|v| v as u32))
            .collect::<Option<Vec<_>>>()?,
    })
}
