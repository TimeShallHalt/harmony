//! Only compile-time allowlisted fields are copied; never paths/env/account state.
#[derive(Clone, Debug, serde::Serialize)]
pub struct BuildInfo {
    pub version: String,
    pub build_id: u64,
    pub commit: String,
    pub dirty: bool,
    pub platform: String,
    pub architecture: String,
    pub upstream: &'static str,
    pub license: &'static str,
}
impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            build_id: option_env!("HARMONY_BUILD_ID")
                .unwrap_or("0")
                .parse()
                .unwrap_or(0),
            commit: option_env!("HARMONY_BUILD_COMMIT")
                .unwrap_or("unknown (source archive)")
                .into(),
            dirty: option_env!("HARMONY_BUILD_DIRTY") != Some("false"),
            platform: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
            upstream: "Yapfast 0.3.0",
            license: "GPL-3.0-or-later",
        }
    }
    pub fn platform_key(&self) -> String {
        format!("{}-{}", self.platform, self.architecture)
    }
    pub fn diagnostics(&self) -> String {
        format!(
            "Harmony {}\nRunning build: {}{}\nCommit: {}\nPlatform: {}\nArchitecture: {}\nUpstream: {}\nLicense: {}\nSource: https://github.com/TimeShallHalt/harmony\nUpstream source: https://github.com/yapfast/yapfast\nUpdates: private GitHub releases via optional official gh; SHA-256 integrity, no independent release signature.\nThis describes the running image, not another installed or staged build.",
            self.version,
            self.build_id,
            if self.dirty {
                " (development / dirty tree)"
            } else {
                ""
            },
            self.commit,
            self.platform,
            self.architecture,
            self.upstream,
            self.license
        )
    }
}
