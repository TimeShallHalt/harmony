//! Bounded received appearance only. No editor, Board, widget or entitlement API.
use serde_json::Value;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Decoration {
    pub sku_id: String,
    pub asset: String,
    pub url: String,
    pub expires_at: Option<u64>,
}
impl Decoration {
    /// The shop still of this accepted decoration, never a Names fallback.
    /// Recheck cached/deserialized SKUs before embedding them in a URL.
    pub fn shop_picture(&self) -> Option<String> {
        valid_sku(&self.sku_id).then(|| {
            format!(
                "{}/media/v1/collectibles-shop/{}/static",
                crate::ready::cdn(),
                self.sku_id
            )
        })
    }
}

fn valid_sku(sku: &str) -> bool {
    !sku.is_empty()
        && sku.len() <= 20
        && sku.bytes().all(|b| b.is_ascii_digit())
        && sku.parse::<u64>().is_ok()
}

pub(super) fn decoration(value: &Value) -> Option<Decoration> {
    let sku = value["sku_id"].as_str()?;
    let asset = value["asset"].as_str()?;
    if !valid_sku(sku)
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
