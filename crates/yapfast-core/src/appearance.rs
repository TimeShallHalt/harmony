//! Portable read-only appearance data; no backend mutation capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoration {
    pub url: String,
    pub expires_at: Option<u64>,
}
impl Decoration {
    pub fn visible_at(&self, now: u64) -> bool {
        self.expires_at.is_none_or(|expiry| expiry > now)
    }
}
#[cfg(feature = "discord")]
impl From<yapfast_discord::profile::appearance::Decoration> for Decoration {
    fn from(value: yapfast_discord::profile::appearance::Decoration) -> Self {
        Self {
            url: value.url,
            expires_at: value.expires_at,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameStyle {
    pub font_id: u32,
    pub effect_id: u32,
    pub colors: Vec<u32>,
}
#[cfg(feature = "discord")]
impl From<yapfast_discord::profile::appearance::NameStyle> for NameStyle {
    fn from(value: yapfast_discord::profile::appearance::NameStyle) -> Self {
        Self {
            font_id: value.font_id,
            effect_id: value.effect_id,
            colors: value.colors,
        }
    }
}
