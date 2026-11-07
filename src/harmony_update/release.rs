use super::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
pub const REPOSITORY: &str = "TimeShallHalt/harmony";
pub const MAX_ASSET: u64 = 1_500_000_000;
pub const MAX_METADATA: u64 = 262_144;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub platform: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub format: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub repository: String,
    pub tag: String,
    pub version: String,
    pub build_id: u64,
    pub commit: String,
    pub artifacts: Vec<Artifact>,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub manifest: Manifest,
    pub release: Release,
    pub asset: Asset,
    pub artifact: Artifact,
}
pub trait Transport: Send + Sync {
    fn releases(&self, cancel: &Cancel) -> Result<Vec<Release>>;
    fn asset(&self, id: u64, output: &mut dyn Write, limit: u64, cancel: &Cancel) -> Result<()>;
}
pub const MANIFEST_NAME: &str = "harmony-update-v1.json";
pub fn version_key(version: &str, build: u64) -> Result<(semver::Version, u64)> {
    let v = semver::Version::parse(version).map_err(|_| Error::Invalid("semantic version"))?;
    if !v.pre.is_empty() || !v.build.is_empty() || version != v.to_string() {
        return Err(Error::Invalid("stable canonical version required"));
    }
    Ok((v, build))
}
pub fn tag(version: &str, build: u64) -> String {
    format!("harmony-v{version}-b{build}")
}
fn tag_key(text: &str) -> Result<(semver::Version, u64)> {
    let (version, build) = text
        .strip_prefix("harmony-v")
        .and_then(|s| s.rsplit_once("-b"))
        .ok_or(Error::Invalid("release tag"))?;
    let build = build
        .parse::<u64>()
        .map_err(|_| Error::Invalid("build number"))?;
    if text != tag(version, build) || build == 0 {
        return Err(Error::Invalid("release tag"));
    }
    version_key(version, build)
}
pub fn validate_manifest(m: &Manifest, platform: &str) -> Result<Artifact> {
    if m.schema != 1
        || m.repository != REPOSITORY
        || m.build_id == 0
        || m.tag != tag(&m.version, m.build_id)
    {
        return Err(Error::Invalid("manifest identity"));
    }
    version_key(&m.version, m.build_id)?;
    if m.commit.len() != 40
        || !m
            .commit
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error::Invalid("full lowercase Git commit"));
    }
    if m.artifacts.is_empty() || m.artifacts.len() > 4 {
        return Err(Error::Invalid("artifact count"));
    }
    let mut seen = std::collections::HashSet::new();
    for a in &m.artifacts {
        let (format, extension) = match a.platform.as_str() {
            "windows-x86_64" | "windows-aarch64" => ("exe", "exe"),
            "linux-x86_64" | "linux-aarch64" => ("appimage", "AppImage"),
            _ => return Err(Error::Unsupported),
        };
        if !seen.insert(&a.platform)
            || a.format != format
            || a.name != format!("Harmony-{}-{}.{extension}", m.version, a.platform)
        {
            return Err(Error::Invalid("platform artifact name or format"));
        }
        if a.size < 4 || a.size > MAX_ASSET {
            return Err(Error::Size);
        }
        if a.sha256.len() != 64
            || !a
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::Invalid("SHA-256"));
        }
    }
    m.artifacts
        .iter()
        .find(|a| a.platform == platform)
        .cloned()
        .ok_or(Error::Unsupported)
}
fn validate_release(r: &Release) -> Result<()> {
    tag_key(&r.tag_name)?;
    if r.draft
        || r.prerelease
        || r.html_url
            != format!(
                "https://github.com/{REPOSITORY}/releases/tag/{}",
                r.tag_name
            )
        || r.assets.len() > 16
    {
        return Err(Error::Invalid("release repository or state"));
    }
    let mut names = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    for a in &r.assets {
        if a.id == 0
            || !ids.insert(a.id)
            || !names.insert(&a.name)
            || a.url
                != format!(
                    "https://api.github.com/repos/{REPOSITORY}/releases/assets/{}",
                    a.id
                )
        {
            return Err(Error::Invalid("asset repository or duplicate identity"));
        }
    }
    Ok(())
}
pub fn check_release(
    transport: &dyn Transport,
    current: &BuildInfo,
    cancel: &Cancel,
) -> Result<Option<Candidate>> {
    cancel.check()?;
    let releases = transport.releases(cancel)?;
    if releases.len() > 100 {
        return Err(Error::Invalid("release count"));
    }
    let mut stable: Vec<_> = releases
        .iter()
        .filter(|r| !r.draft && !r.prerelease && r.tag_name.starts_with("harmony-v"))
        .collect();
    stable.sort_by_key(|r| tag_key(&r.tag_name).ok());
    let Some(r) = stable.last() else {
        return Ok(None);
    };
    validate_release(r)?;
    let manifest_asset = r
        .assets
        .iter()
        .find(|a| a.name == MANIFEST_NAME)
        .ok_or(Error::Invalid("release manifest missing"))?;
    if manifest_asset.size == 0 || manifest_asset.size > MAX_METADATA {
        return Err(Error::Size);
    }
    let mut bytes = Vec::new();
    transport.asset(manifest_asset.id, &mut bytes, MAX_METADATA, cancel)?;
    if bytes.len() as u64 != manifest_asset.size {
        return Err(Error::Size);
    }
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("release manifest JSON"))?;
    let artifact = validate_manifest(&manifest, &current.platform_key())?;
    if manifest.tag != r.tag_name {
        return Err(Error::Invalid("manifest/release tag mismatch"));
    }
    let asset = r
        .assets
        .iter()
        .find(|a| a.name == artifact.name)
        .ok_or(Error::Invalid("platform asset missing"))?
        .clone();
    if asset.size != artifact.size {
        return Err(Error::Size);
    }
    let next = version_key(&manifest.version, manifest.build_id)?;
    let old = version_key(&current.version, current.build_id)?;
    if next == old && manifest.commit != current.commit {
        return Err(Error::Conflict);
    }
    cancel.check()?;
    if next <= old {
        return Ok(None);
    }
    Ok(Some(Candidate {
        manifest,
        release: (*r).clone(),
        asset,
        artifact,
    }))
}
