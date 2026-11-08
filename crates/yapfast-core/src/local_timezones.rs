//! Per-contact timezone annotations. This module has no network or friend-list API.

use std::collections::BTreeMap;

/// Nested by provider-qualified owning account, then the exact contact ID.
/// Only the desktop Layout stores this; it is never a provider setting.
#[derive(Clone, Default, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Preferences(BTreeMap<String, BTreeMap<String, String>>);

fn zone(name: &str) -> Option<chrono_tz::Tz> {
    // Reject ambiguous abbreviations/fixed offsets rather than guessing a
    // person's location. UTC is the explicit universal IANA exception.
    if name.len() > 128 || (name != "UTC" && !name.contains('/')) {
        return None;
    }
    name.parse().ok()
}

impl Preferences {
    pub fn reset(&mut self, account: &str, contact: &str) {
        if let Some(contacts) = self.0.get_mut(account) {
            contacts.remove(contact);
            if contacts.is_empty() {
                self.0.remove(account);
            }
        }
    }

    pub fn get(&self, account: &str, contact: &str) -> Option<&str> {
        self.0.get(account)?.get(contact).map(String::as_str)
    }

    pub fn set(&mut self, account: &str, contact: &str, name: &str) -> Result<(), &'static str> {
        if account.is_empty() || contact.is_empty() {
            return Err("A signed-in owning account and contact are required.");
        }
        zone(name).ok_or("Enter an exact IANA timezone, for example Europe/London or UTC.")?;
        self.0
            .entry(account.into())
            .or_default()
            .insert(contact.into(), name.into());
        Ok(())
    }
}

/// Convert an instant, never an ambiguous local wall time. Re-evaluate the
/// IANA rules on each visible-minute tick instead of caching a fixed offset.
pub fn display(saved: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> String {
    let Some(saved) = saved else {
        return "Timezone not set (local only)".into();
    };
    let Some(zone) = zone(saved) else {
        return "Invalid saved timezone — edit or reset (local only)".into();
    };
    format!(
        "{} · {}",
        now.with_timezone(&zone)
            .format("%Y-%m-%d %H:%M %Z (UTC%:z)"),
        saved
    )
}

/// Current display uses the same injected-instant path as deterministic tests.
pub fn current(saved: Option<&str>) -> String {
    at_system_time(saved, std::time::SystemTime::now())
}

/// Inject a UTC instant without changing the operating system clock.
pub fn at_system_time(saved: Option<&str>, now: std::time::SystemTime) -> String {
    display(saved, now.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_tracks_dst_transitions_and_fractional_zones_from_utc() {
        let at = |s: &str| {
            chrono::DateTime::parse_from_rfc3339(s)
                .unwrap()
                .with_timezone(&chrono::Utc)
        };
        assert_eq!(
            display(Some("America/New_York"), at("2026-03-08T06:59:00Z")),
            "2026-03-08 01:59 EST (UTC-05:00) · America/New_York"
        );
        assert_eq!(
            display(Some("America/New_York"), at("2026-03-08T07:01:00Z")),
            "2026-03-08 03:01 EDT (UTC-04:00) · America/New_York"
        );
        assert_eq!(
            display(Some("America/New_York"), at("2026-11-01T05:30:00Z")),
            "2026-11-01 01:30 EDT (UTC-04:00) · America/New_York"
        );
        assert_eq!(
            display(Some("America/New_York"), at("2026-11-01T06:30:00Z")),
            "2026-11-01 01:30 EST (UTC-05:00) · America/New_York"
        );
        assert_eq!(
            display(Some("Asia/Kathmandu"), at("2026-01-01T23:30:00Z")),
            "2026-01-02 05:15 +0545 (UTC+05:45) · Asia/Kathmandu"
        );
        assert_eq!(
            display(None, at("2026-01-01T00:00:00Z")),
            "Timezone not set (local only)"
        );
        assert_eq!(
            display(Some("bad/zone"), at("2026-01-01T00:00:00Z")),
            "Invalid saved timezone — edit or reset (local only)"
        );
    }

    #[test]
    fn persistence_roundtrip_reset_and_invalid_saved_values_remain_explicit() {
        let mut prefs = Preferences::default();
        prefs
            .set(
                "matrix:@fixture:example.org",
                "@contact:example.org",
                "Europe/London",
            )
            .unwrap();
        prefs.set("discord:owner", "other", "UTC").unwrap();
        let bytes = serde_json::to_vec(&prefs).unwrap();
        let mut restored: Preferences = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, prefs);
        restored.reset("matrix:@fixture:example.org", "@contact:example.org");
        assert_eq!(
            restored.get("matrix:@fixture:example.org", "@contact:example.org"),
            None
        );
        assert_eq!(restored.get("discord:owner", "other"), Some("UTC"));
        assert_eq!(current(None), "Timezone not set (local only)");
        let broken: Preferences =
            serde_json::from_str(r#"{"discord:owner":{"contact":"bad/zone"}}"#).unwrap();
        assert_eq!(
            current(broken.get("discord:owner", "contact")),
            "Invalid saved timezone — edit or reset (local only)"
        );
    }

    #[test]
    fn saves_only_iana_names_and_keeps_contacts_separate_per_account() {
        let mut prefs = Preferences::default();
        assert!(
            prefs
                .set("discord:owner-a", "contact", "America/New_York")
                .is_ok()
        );
        assert_eq!(
            prefs.get("discord:owner-a", "contact"),
            Some("America/New_York")
        );
        assert_eq!(prefs.get("discord:owner-b", "contact"), None);
        for invalid in [
            "",
            "EST",
            "UTC+05:30",
            "+03:00",
            "america/new_york",
            " Europe/London",
            "Europe/London\n",
            "not/a/zone",
        ] {
            assert!(
                prefs.set("discord:owner-a", "contact", invalid).is_err(),
                "{invalid:?}"
            );
            assert_eq!(
                prefs.get("discord:owner-a", "contact"),
                Some("America/New_York")
            );
        }
    }
}
