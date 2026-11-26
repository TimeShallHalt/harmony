//! Per-contact timezone annotations. This module has no network or friend-list API.

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, NaiveTime, Timelike, Utc};

// Public-domain IANA tzdb tables. zone.tab preserves the country-to-zone
// mapping even when two countries currently use the same rules.
const COUNTRIES: &str = include_str!("timezone_data/iso3166.tab");
const LOCATIONS: &str = include_str!("timezone_data/zone.tab");

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
        zone(name).ok_or("Choose a time zone or enter an IANA name such as Europe/London.")?;
        self.store(account, contact, name.into())
    }

    pub fn set_observed(
        &mut self,
        account: &str,
        contact: &str,
        time: &str,
    ) -> Result<(), &'static str> {
        let now = chrono::Local::now();
        let offset = observed_offset(time, now.with_timezone(&Utc), *now.offset())
            .ok_or("Enter their current time as HH:MM, or a difference such as +05:30.")?;
        self.store(
            account,
            contact,
            format!("offset:{}", offset.local_minus_utc() / 60),
        )
    }

    fn store(&mut self, account: &str, contact: &str, value: String) -> Result<(), &'static str> {
        if account.is_empty() || contact.is_empty() {
            return Err("A signed-in owning account and contact are required.");
        }
        self.0
            .entry(account.into())
            .or_default()
            .insert(contact.into(), value);
        Ok(())
    }
}

pub fn lookup(country: &str, city: &str) -> Vec<String> {
    let clean = |value: &str| {
        value
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let country = clean(country);
    let city = clean(city);
    if country.is_empty() && city.is_empty() {
        return Vec::new();
    }
    let code = COUNTRIES
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .find(|(code, name)| clean(code) == country || clean(name) == country)
        .map(|(code, _)| code);
    let mut matches = LOCATIONS
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let region = columns.next()?;
            columns.next()?;
            let zone_name = columns.next()?;
            let name = zone_name.rsplit('/').next()?;
            ((country.is_empty() || code == Some(region))
                && (city.is_empty() || clean(name) == city)
                && zone(zone_name).is_some())
            .then(|| zone_name.to_owned())
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches
}

/// A wall clock without a date is interpreted on the nearest local day.
/// The observation is rounded to a civil 15-minute offset. An explicit
/// difference is kept to the minute and disambiguates distant time zones.
fn observed_offset(input: &str, now: DateTime<Utc>, here: FixedOffset) -> Option<FixedOffset> {
    let input = input.trim();
    let delta = if let Some(value) = input.strip_prefix(['+', '-']) {
        let (hours, minutes) = value.split_once(':').unwrap_or((value, "0"));
        let hours = hours.parse::<u16>().ok()?;
        let minutes = minutes.parse::<u16>().ok()?;
        if hours > 26 || minutes > 59 {
            return None;
        }
        let sign = if input.starts_with('-') { -1 } else { 1 };
        (60 * i32::from(hours) + i32::from(minutes)) * sign
    } else {
        let time = NaiveTime::parse_from_str(input, "%H:%M").ok()?;
        let local = now.with_timezone(&here).time();
        let minutes = (time.num_seconds_from_midnight() as i32 / 60)
            - (local.num_seconds_from_midnight() as i32 / 60);
        let nearest = (minutes + 720).rem_euclid(1440) - 720;
        let utc_minutes = here.local_minus_utc() / 60;
        (nearest + utc_minutes + 7).div_euclid(15) * 15 - utc_minutes
    };
    let minutes = here.local_minus_utc() / 60 + delta;
    fixed_offset(minutes)
}

fn fixed_offset(minutes: i32) -> Option<FixedOffset> {
    (-720..=840)
        .contains(&minutes)
        .then(|| FixedOffset::east_opt(minutes * 60))
        .flatten()
}

/// Convert an instant, never an ambiguous local wall time. Re-evaluate the
/// IANA rules on each visible-minute tick instead of caching a fixed offset.
pub fn display(saved: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> String {
    let Some(saved) = saved else {
        return "Timezone not set (local only)".into();
    };
    if let Some(offset) = saved
        .strip_prefix("offset:")
        .and_then(|value| value.parse().ok())
        .and_then(fixed_offset)
    {
        return format!(
            "{} · fixed offset",
            now.with_timezone(&offset).format("%Y-%m-%d %H:%M (UTC%:z)")
        );
    }
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
    fn location_and_observed_clock_handle_country_filters_and_midnight() {
        assert_eq!(lookup("Canada", "Toronto"), ["America/Toronto"]);
        assert!(lookup("US", "Toronto").is_empty());
        assert!(lookup("Canada", "").len() > 1);
        let now = DateTime::parse_from_rfc3339("2026-11-27T04:50:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let local = FixedOffset::west_opt(5 * 3600).unwrap();
        assert_eq!(
            observed_offset("00:50", now, local)
                .unwrap()
                .local_minus_utc(),
            -4 * 3600
        );
        assert_eq!(
            observed_offset("+10:45", now, local)
                .unwrap()
                .local_minus_utc(),
            5 * 3600 + 45 * 60
        );
        assert!(observed_offset("+99:00", now, local).is_none());
    }

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
