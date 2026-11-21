//! Compile-time public build identity only; no account or installation state.
#[derive(Clone, Debug)]
pub struct BuildInfo {
    pub version: &'static str,
    pub build_id: &'static str,
    pub commit: &'static str,
    pub dirty: bool,
}
impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            build_id: option_env!("HARMONY_BUILD_ID").unwrap_or("0"),
            commit: option_env!("HARMONY_BUILD_COMMIT").unwrap_or("unknown (source archive)"),
            dirty: option_env!("HARMONY_BUILD_DIRTY") != Some("false"),
        }
    }
    pub fn diagnostics(&self) -> String {
        format!(
            "Harmony {}\nRunning build: {}{}\nCommit: {}\nPlatform: {}\nArchitecture: {}\nUpstream: Yapfast 0.4.1\nLicense: GPL-3.0-or-later\nSource: https://github.com/TimeShallHalt/harmony\nUpstream source: https://github.com/yapfast/yapfast\nUpdates: Harmony public releases with publisher-signed Windows installers.",
            self.version,
            self.build_id,
            if self.dirty {
                " (development / dirty tree)"
            } else {
                ""
            },
            self.commit,
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }
}

// Channel metadata for the version footer and command-line reports. The
// full, unshortened Harmony source identity above remains in diagnostics.

const VERSION: &str = env!("CARGO_PKG_VERSION");
const COMMIT: Option<&str> = option_env!("YAPFAST_BUILD_COMMIT");
const DATE: Option<&str> = option_env!("YAPFAST_BUILD_DATE");
const RELEASE: bool = option_env!("YAPFAST_BUILD_RELEASE").is_some();

/// How many characters of a commit's hash Harmony shows.
const SHORT_COMMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// A release with no prerelease part in its version.
    Stable,
    /// A build of `main` that the Nightly workflow published.
    Nightly,
    /// Any other release with a prerelease part, such as `0.5.0-rc.1`.
    Prerelease,
    /// A build from a checkout, such as `cargo run`.
    Development,
}

#[cfg_attr(
    not(feature = "desktop"),
    allow(dead_code, reason = "only the window shows the parts")
)]
impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Channel::Stable => "Stable",
            Channel::Nightly => "Nightly",
            Channel::Prerelease => "Pre-release",
            Channel::Development => "Development build",
        }
    }

    /// How loud the window draws the label: 0 for a release, 1 for a
    /// prerelease, 2 for a development build.
    pub fn tone(self) -> i32 {
        match self {
            Channel::Stable => 0,
            Channel::Nightly | Channel::Prerelease => 1,
            Channel::Development => 2,
        }
    }
}

pub struct ChannelInfo<'a> {
    version: &'a str,
    release: bool,
    commit: Option<&'a str>,
    date: Option<&'a str>,
}

/// This build's own.
pub fn current() -> ChannelInfo<'static> {
    ChannelInfo::new(VERSION, RELEASE, COMMIT, DATE)
}

#[cfg_attr(
    not(feature = "desktop"),
    allow(dead_code, reason = "only the window shows the parts")
)]
impl<'a> ChannelInfo<'a> {
    pub fn new(
        version: &'a str,
        release: bool,
        commit: Option<&'a str>,
        date: Option<&'a str>,
    ) -> Self {
        Self {
            version,
            release,
            // Cut to its short form, as Git shows it, but not through a
            // character: a hash is ASCII, and anything else is left whole.
            commit: commit
                .map(str::trim)
                .filter(|commit| !commit.is_empty())
                .map(|commit| commit.get(..SHORT_COMMIT).unwrap_or(commit)),
            date: date.map(str::trim).filter(|date| !date.is_empty()),
        }
    }

    pub fn version(&self) -> &str {
        self.version
    }

    pub fn channel(&self) -> Channel {
        if self.version.contains("-nightly.") {
            Channel::Nightly
        } else if !self.release {
            Channel::Development
        } else if self.version.contains('-') {
            Channel::Prerelease
        } else {
            Channel::Stable
        }
    }

    /// The first line: a release's version, as its tag names it, or a
    /// development build's commit, which says more than the version of the
    /// release it grew from.
    pub fn headline(&self) -> String {
        match (self.channel(), self.commit) {
            (Channel::Development, Some(commit)) => format!("Harmony {commit}"),
            (Channel::Development, None) => "Harmony".into(),
            _ => format!("Harmony v{}", self.version),
        }
    }

    /// What follows the channel's label on the second line: the commit, which
    /// the first line has already given for a development build, and the day
    /// it was made.
    pub fn details(&self) -> String {
        let commit = self
            .commit
            .filter(|_| self.channel() != Channel::Development);
        [commit, self.date]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// Everything on one line, for a bug report and `--version`.
    pub fn summary(&self) -> String {
        let version = format!("Harmony v{}", self.version);
        [
            Some(version.as_str()),
            Some(self.channel().label()),
            self.commit,
            self.date,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build<'a>(
        version: &'a str,
        release: bool,
        commit: Option<&'a str>,
        date: Option<&'a str>,
    ) -> ChannelInfo<'a> {
        ChannelInfo::new(version, release, commit, date)
    }

    #[test]
    fn harmony_diagnostics_keep_full_source_identity_and_upstream_attribution() {
        let info = BuildInfo::current();
        let text = info.diagnostics();
        assert!(text.starts_with("Harmony "));
        assert!(text.contains(info.commit));
        assert!(text.contains("Upstream: Yapfast 0.4.1"));
        assert!(text.contains("GPL-3.0-or-later"));
        assert!(text.contains("publisher-signed Windows installers"));
    }

    const COMMIT_FULL: &str = "59391f96a1b2c3d4e5f60718293a4b5c6d7e8f90";

    #[test]
    fn a_release_without_a_prerelease_part_is_stable() {
        let info = build("0.4.0", true, Some(COMMIT_FULL), Some("2026-11-07"));
        assert_eq!(info.channel(), Channel::Stable);
        assert_eq!(info.headline(), "Harmony v0.4.0");
        assert_eq!(info.channel().label(), "Stable");
        assert_eq!(info.details(), "59391f96 · 2026-11-07");
        assert_eq!(
            info.summary(),
            "Harmony v0.4.0 · Stable · 59391f96 · 2026-11-07"
        );
    }

    #[test]
    fn a_nightly_names_itself_in_its_version() {
        let info = build(
            "0.4.0-nightly.20261107.12",
            true,
            Some(COMMIT_FULL),
            Some("2026-11-07"),
        );
        assert_eq!(info.channel(), Channel::Nightly);
        assert_eq!(info.headline(), "Harmony v0.4.0-nightly.20261107.12");
        assert_eq!(info.details(), "59391f96 · 2026-11-07");
        assert_eq!(
            info.summary(),
            "Harmony v0.4.0-nightly.20261107.12 · Nightly · 59391f96 · 2026-11-07"
        );
    }

    #[test]
    fn a_nightly_is_one_even_if_the_release_flag_was_lost() {
        let info = build("0.4.0-nightly.20261107.12", false, None, None);
        assert_eq!(info.channel(), Channel::Nightly);
    }

    #[test]
    fn another_prerelease_is_a_prerelease() {
        let info = build("0.5.0-rc.1", true, None, None);
        assert_eq!(info.channel(), Channel::Prerelease);
        assert_eq!(info.channel().label(), "Pre-release");
        assert_eq!(info.channel().tone(), 1);
    }

    #[test]
    fn a_build_outside_the_release_packaging_is_a_development_build() {
        // Cargo.toml's version is a stable release's, so it can't tell.
        let info = build("0.4.0", false, Some(COMMIT_FULL), Some("2026-11-09"));
        assert_eq!(info.channel(), Channel::Development);
        assert_eq!(info.channel().label(), "Development build");
        assert_eq!(info.channel().tone(), 2);
        assert_eq!(info.headline(), "Harmony 59391f96");
        // The commit is in the headline already.
        assert_eq!(info.details(), "2026-11-09");
        assert_eq!(
            info.summary(),
            "Harmony v0.4.0 · Development build · 59391f96 · 2026-11-09"
        );
    }

    #[test]
    fn a_build_that_knows_no_commit_leaves_it_out() {
        let development = build("0.4.0", false, None, None);
        assert_eq!(development.headline(), "Harmony");
        assert_eq!(development.details(), "");
        assert_eq!(development.summary(), "Harmony v0.4.0 · Development build");
        let release = build("0.4.0", true, None, Some("2026-11-07"));
        assert_eq!(release.details(), "2026-11-07");
        assert_eq!(release.summary(), "Harmony v0.4.0 · Stable · 2026-11-07");
    }

    #[test]
    fn a_commit_is_shortened_without_breaking_a_character() {
        assert_eq!(build("1.0.0", true, Some("abc"), None).commit, Some("abc"));
        assert_eq!(build("1.0.0", true, Some("  "), None).commit, None);
        assert_eq!(
            build("1.0.0", true, Some("0123456789\n"), None).commit,
            Some("01234567")
        );
        // Not a hash, but never a panic.
        assert!(build("1.0.0", true, Some("ééééé"), None).commit.is_some());
    }

    #[test]
    fn this_build_describes_itself() {
        let info = current();
        assert!(info.summary().starts_with("Harmony v"));
        assert!(info.summary().contains(info.channel().label()));
    }
}
