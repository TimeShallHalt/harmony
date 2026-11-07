//! Profile media is resolved only from a profile already received for this
//! account. There is deliberately no arbitrary-URL viewer action.
use crate::chat::{Profile, ProfilePicture};

/// Discord's documented maximum CDN request size, not a promise of source detail.
pub const MAX_CDN_SIZE: u32 = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    account: String,
    user: String,
    kind: ProfilePicture,
    received: String,
    url: String,
    name: String,
}

impl Asset {
    pub fn from_profile(account: &str, profile: &Profile, kind: ProfilePicture) -> Option<Self> {
        if !identifier(account) || !identifier(&profile.id) {
            return None;
        }
        let received = match kind {
            ProfilePicture::Avatar => profile.avatar.as_deref()?,
            ProfilePicture::Banner => profile.banner.as_deref()?,
        };
        if received.len() > 2048 {
            return None;
        }
        // Exact hosts, HTTPS, no userinfo/port/escaped paths/fragments. A profile
        // CDN resource is stricter than the general attachment/proxy allowlist.
        let path = received
            .strip_prefix("https://cdn.discordapp.com/")
            .or_else(|| received.strip_prefix("https://media.discordapp.net/"))?;
        if received
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
            || path.contains(['#', '%', '\\'])
        {
            return None;
        }
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        if !query.is_empty()
            && !query.split('&').all(|pair| {
                pair.strip_prefix("size=").is_some_and(identifier)
                    || matches!(pair, "animated=true" | "animated=false")
            })
        {
            return None;
        }
        let parts: Vec<_> = path.split('/').collect();
        let role = match kind {
            ProfilePicture::Avatar => "avatars",
            ProfilePicture::Banner => "banners",
        };
        let file = match parts.as_slice() {
            ["embed", "avatars", file] if kind == ProfilePicture::Avatar => {
                if !matches!(
                    *file,
                    "0.png" | "1.png" | "2.png" | "3.png" | "4.png" | "5.png"
                ) {
                    return None;
                }
                return Some(Self {
                    account: account.into(),
                    user: profile.id.clone(),
                    kind,
                    received: received.into(),
                    url: format!("https://cdn.discordapp.com/{path}?size={MAX_CDN_SIZE}"),
                    name: format!("avatar-{}.png", profile.id),
                });
            }
            [directory, user, file] if *directory == role && *user == profile.id => *file,
            ["guilds", guild, "users", user, directory, file]
                if identifier(guild) && *user == profile.id && *directory == role =>
            {
                *file
            }
            _ => return None,
        };
        let (hash, ext) = file.rsplit_once('.')?;
        if !matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif") {
            return None;
        }
        let animated = hash.starts_with("a_");
        let digest = hash.strip_prefix("a_").unwrap_or(hash);
        if digest.len() != 32 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        // Animated WebP handles modern WebP/AVIF uploads that cannot always be
        // converted to GIF. The existing native animation decoder supports it.
        let (ext, animation) = if animated {
            ("webp", "&animated=true")
        } else {
            ("png", "")
        };
        let directory = path.rsplit_once('/')?.0;
        let noun = match kind {
            ProfilePicture::Avatar => "avatar",
            ProfilePicture::Banner => "banner",
        };
        Some(Self {
            account: account.into(),
            user: profile.id.clone(),
            kind,
            received: received.into(),
            url: format!(
                "https://cdn.discordapp.com/{directory}/{hash}.{ext}?size={MAX_CDN_SIZE}{animation}"
            ),
            name: format!("{noun}-{}.{ext}", profile.id),
        })
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn user(&self) -> &str {
        &self.user
    }
    pub fn kind(&self) -> ProfilePicture {
        self.kind
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

fn identifier(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Profile,
    Panel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    /// Original downloaded bytes, also available for Save after a decode error.
    pub bytes: Option<std::sync::Arc<[u8]>>,
    pub picture: Result<std::sync::Arc<crate::chat::Picture>, &'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub request: u64,
    pub slot: Slot,
    pub asset: Asset,
    pub loaded: Option<Loaded>,
    pub save_note: Option<&'static str>,
}

impl Session {
    pub fn open(
        account: &str,
        profile: &Profile,
        slot: Slot,
        kind: ProfilePicture,
    ) -> Option<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Some(Self {
            request: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            slot,
            asset: Asset::from_profile(account, profile, kind)?,
            loaded: None,
            save_note: None,
        })
    }
    pub fn is_current(&self, account: Option<&str>, profile: Option<&Profile>) -> bool {
        account == Some(self.asset.account())
            && profile
                .and_then(|profile| {
                    Asset::from_profile(self.asset.account(), profile, self.asset.kind())
                })
                .as_ref()
                == Some(&self.asset)
    }
    pub fn accept(&mut self, request: u64, asset: &Asset, loaded: Loaded) -> bool {
        if request != self.request || *asset != self.asset {
            return false;
        }
        self.loaded = Some(loaded);
        true
    }
}

#[cfg(test)]
#[path = "profile_media_url_tests.rs"]
mod url_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requests_the_supported_maximum_for_the_received_avatar_identity() {
        let profile = Profile {
            id: "200".into(),
            avatar: Some("https://cdn.discordapp.com/avatars/200/0123456789abcdef0123456789abcdef.png?size=128".into()),
            ..Default::default()
        };
        let asset =
            Asset::from_profile("100", &profile, ProfilePicture::Avatar).expect("received avatar");
        assert_eq!(
            asset.url(),
            "https://cdn.discordapp.com/avatars/200/0123456789abcdef0123456789abcdef.png?size=4096"
        );
    }

    fn profile() -> Profile {
        Profile { id: "200".into(), avatar: Some("https://cdn.discordapp.com/avatars/200/0123456789abcdef0123456789abcdef.png?size=128".into()), ..Default::default() }
    }
    #[test]
    fn stale_requests_cannot_fill_a_reopened_viewer() {
        let old = Session::open("100", &profile(), Slot::Profile, ProfilePicture::Avatar).unwrap();
        let mut current =
            Session::open("100", &profile(), Slot::Profile, ProfilePicture::Avatar).unwrap();
        assert_ne!(old.request, current.request);
        assert!(!current.accept(
            old.request,
            &old.asset,
            Loaded {
                bytes: None,
                picture: Err("stale")
            }
        ));
        assert!(current.loaded.is_none());
    }
    #[test]
    fn source_changes_invalidate_the_identity_and_account_bound_viewer() {
        let profile = profile();
        let session = Session::open("100", &profile, Slot::Panel, ProfilePicture::Avatar).unwrap();
        assert!(session.is_current(Some("100"), Some(&profile)));
        assert!(!session.is_current(Some("101"), Some(&profile)));
        assert!(!session.is_current(None, Some(&profile)));
        assert!(!session.is_current(Some("100"), None));
        let mut changed = profile.clone();
        changed.id = "201".into();
        assert!(!session.is_current(Some("100"), Some(&changed)));
        changed = profile.clone();
        changed.avatar = None;
        assert!(!session.is_current(Some("100"), Some(&changed)));
    }
}
