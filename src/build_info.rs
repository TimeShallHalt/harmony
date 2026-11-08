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
            "Harmony {}\nRunning build: {}{}\nCommit: {}\nPlatform: {}\nArchitecture: {}\nUpstream: Yapfast 0.3.0\nLicense: GPL-3.0-or-later\nSource: https://github.com/TimeShallHalt/harmony\nUpstream source: https://github.com/yapfast/yapfast\nUpdater not included in this build.",
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
