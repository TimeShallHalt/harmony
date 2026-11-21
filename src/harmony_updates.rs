//! Public Harmony release discovery and publisher-authenticated Windows installer downloads.
use anyhow::{Context, Result, anyhow, ensure};
use reqwest::Client;
use ring::{digest, signature};
use serde::Deserialize;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const REPO: &str = "TimeShallHalt/harmony";
const RELEASES: &str = "https://github.com/TimeShallHalt/harmony/releases";
const LATEST: &str = "https://api.github.com/repos/TimeShallHalt/harmony/releases/latest";
const PUBLIC_KEY: &str = include_str!("../packaging/keys/harmony-update.pub");
const MAX_INSTALLER: u64 = 512 * 1024 * 1024;
pub const WINDOWS_INSTALLER: bool = cfg!(all(windows, target_arch = "x86_64"));

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Build([u64; 3], u64);

impl Build {
    fn from_tag(tag: &str) -> Option<Self> {
        let (version, build) = tag.strip_prefix("harmony-v")?.split_once("-b")?;
        let parse = |s: &str| {
            let n: u64 = s.parse().ok()?;
            (n.to_string() == s).then_some(n)
        };
        let mut parts = version.split('.');
        let core = [
            parse(parts.next()?)?,
            parse(parts.next()?)?,
            parse(parts.next()?)?,
        ];
        let build = parse(build)?;
        (parts.next().is_none() && build > 0).then_some(Self(core, build))
    }
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub notes: String,
    pub has_installer: bool,
}

impl Release {
    pub fn page(&self) -> String {
        format!("{RELEASES}/tag/{}", self.tag)
    }
    fn url(&self, asset: &str) -> String {
        format!("{RELEASES}/download/{}/{asset}", self.tag)
    }
}

#[derive(Deserialize)]
struct Asset {
    name: String,
}

#[derive(Deserialize)]
struct Listing {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

fn installer_name(tag: &str) -> Option<String> {
    Build::from_tag(tag)?;
    let (version, _) = tag.strip_prefix("harmony-v")?.split_once("-b")?;
    Some(format!("Harmony-{version}-Setup.exe"))
}

fn available(bytes: &[u8], current: &str, build: u64) -> Result<Option<Release>> {
    let listing: Listing = serde_json::from_slice(bytes)?;
    let newer = Build::from_tag(&listing.tag_name).context("Unrecognized Harmony release tag")?;
    let mut installed = Build::from_tag(&format!("harmony-v{current}-b1"))
        .context("Unrecognized running version")?;
    installed.1 = build;
    if listing.draft
        || listing.prerelease
        || newer <= installed
        || (build == 0 && newer.0 == installed.0)
    {
        return Ok(None);
    }
    let installer = installer_name(&listing.tag_name).context("Invalid installer version")?;
    let has_installer = [
        installer.as_str(),
        "harmony-update.json",
        "harmony-update.json.sig",
    ]
    .iter()
    .all(|name| {
        listing
            .assets
            .iter()
            .filter(|asset| asset.name == *name)
            .count()
            == 1
    });
    Ok(Some(Release {
        tag: listing.tag_name,
        notes: listing
            .body
            .unwrap_or_default()
            .trim()
            .chars()
            .take(2000)
            .collect(),
        has_installer,
    }))
}

fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("Harmony/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let host = attempt.url().host_str().unwrap_or("");
            if attempt.previous().len() >= 5
                || attempt.url().scheme() != "https"
                || !(host == "github.com"
                    || host == "api.github.com"
                    || host.ends_with(".githubusercontent.com"))
            {
                attempt.error("Unexpected release download redirect")
            } else {
                attempt.follow()
            }
        }))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(45))
        .build()?)
}

async fn get_small(client: &Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(35))
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() <= limit - bytes.len(),
            "Release metadata exceeds its size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn check() -> Result<Option<Release>> {
    let current = crate::build_info::BuildInfo::current();
    available(
        &get_small(&client()?, LATEST, 512 * 1024).await?,
        current.version,
        current.build_id.parse()?,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    repository: String,
    tag: String,
    platform: String,
    installer: String,
    bytes: u64,
    sha256: String,
}

fn authenticate(bytes: &[u8], sig: &[u8], key: &[u8], release: &Release) -> Result<Manifest> {
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, key)
        .verify(bytes, sig)
        .map_err(|_| anyhow!("Harmony's release signature could not be verified"))?;
    let m: Manifest = serde_json::from_slice(bytes)?;
    ensure!(
        m.schema == 1
            && m.repository == REPO
            && m.tag == release.tag
            && m.platform == "windows-x64"
            && Some(&m.installer) == installer_name(&release.tag).as_ref(),
        "The signed installer belongs to a different release or platform"
    );
    ensure!(
        (64..=MAX_INSTALLER).contains(&m.bytes)
            && m.sha256.len() == 64
            && m.sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Invalid signed size or checksum"
    );
    Ok(m)
}

pub struct Staged {
    path: PathBuf,
    manifest: Manifest,
}

#[cfg(windows)]
fn hold_update_directories(path: &Path) -> Result<Vec<fs::File>> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    ensure!(path.is_absolute(), "Expected an absolute update cache path");
    let parents: Vec<_> = path
        .ancestors()
        .skip(1)
        .filter(|p| p.parent().is_some())
        .collect();
    let mut guards = Vec::new();
    for parent in parents.into_iter().rev() {
        let handle = fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent)?;
        let meta = handle.metadata()?;
        ensure!(
            meta.is_dir() && meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "The update cache must be on a local path without junctions"
        );
        guards.push(handle);
    }
    Ok(guards)
}

fn verified_file(path: &Path, manifest: &Manifest) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    // Keep this exact image read-only and in place until CreateProcess has opened it.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options.open(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        ensure!(
            file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "Expected an ordinary installer file"
        );
    }
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() == manifest.bytes,
        "Installer size changed"
    );
    let mut hash = digest::Context::new(&digest::SHA256);
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    ensure!(
        hex::encode(hash.finish()) == manifest.sha256,
        "Installer checksum mismatch"
    );
    Ok(file)
}

pub async fn download(release: &Release, cache: &Path) -> Result<Staged> {
    ensure!(
        WINDOWS_INSTALLER && release.has_installer,
        "No signed Windows x64 installer is available"
    );
    let client = client()?;
    let bytes = get_small(&client, &release.url("harmony-update.json"), 16 * 1024).await?;
    let sig = get_small(&client, &release.url("harmony-update.json.sig"), 64).await?;
    let manifest = authenticate(&bytes, &sig, &hex::decode(PUBLIC_KEY.trim())?, release)?;
    fs::create_dir_all(cache)?;
    let path = cache.join("Harmony-Setup.exe");
    if verified_file(&path, &manifest).is_err() {
        let mut response = client
            .get(release.url(&manifest.installer))
            .timeout(Duration::from_secs(15 * 60))
            .send()
            .await?
            .error_for_status()?;
        let mut temporary = tempfile::NamedTempFile::new_in(cache)?;
        let mut count = 0;
        let mut hash = digest::Context::new(&digest::SHA256);
        while let Some(chunk) = response.chunk().await? {
            count += chunk.len() as u64;
            ensure!(
                count <= manifest.bytes,
                "Download exceeds signed installer size"
            );
            hash.update(&chunk);
            temporary.write_all(&chunk)?;
        }
        ensure!(
            count == manifest.bytes && hex::encode(hash.finish()) == manifest.sha256,
            "Downloaded installer failed verification"
        );
        temporary.as_file().sync_all()?;
        temporary.persist(&path)?;
    }
    Ok(Staged { path, manifest })
}

/// Select silent mode only for the exact location and scope registered by Harmony Setup.
#[cfg(windows)]
fn installed_scope(program: &Path) -> Option<&'static str> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
        RegGetValueW,
    };
    let key: Vec<_> = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{46969CAC-C472-58C4-89BD-069E35950175}_is1"
        .encode_utf16().chain([0]).collect();
    let value = |hive: HKEY, name: &str| -> Option<OsString> {
        let name: Vec<_> = name.encode_utf16().chain([0]).collect();
        let mut buffer = vec![0u16; 32768];
        let mut size = (buffer.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                hive,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if result != 0 || size < 2 || size % 2 != 0 {
            return None;
        }
        Some(OsString::from_wide(&buffer[..size as usize / 2 - 1]))
    };
    let folder = program.parent()?.canonicalize().ok()?;
    let mut scopes = [
        (HKEY_CURRENT_USER, "/CURRENTUSER"),
        (HKEY_LOCAL_MACHINE, "/ALLUSERS"),
    ]
    .into_iter()
    .filter_map(|(hive, mode)| {
        if value(hive, "HarmonySimpleInstaller")? != "1" {
            return None;
        }
        (PathBuf::from(value(hive, "InstallLocation")?)
            .canonicalize()
            .ok()?
            == folder)
            .then_some(mode)
    });
    let scope = scopes.next()?;
    scopes.next().is_none().then_some(scope)
}

impl Staged {
    pub fn launch(&self) -> Result<()> {
        ensure!(WINDOWS_INSTALLER, "Installer requires Windows x64");
        #[cfg(windows)]
        let _directories = hold_update_directories(&self.path)?;
        let _guard = verified_file(&self.path, &self.manifest)?;
        // The signed installer waits for this process and checks for other running copies.
        // Its wizard preserves the previous installation's scope, location and registrations.
        let mut setup = std::process::Command::new(&self.path);
        setup
            .arg(format!("/HARMONYWAITPID={}", std::process::id()))
            .args(["/NORESTART", "/NOCLOSEAPPLICATIONS"]);
        #[cfg(windows)]
        if let Ok(program) = std::env::current_exe()
            && let Some(scope) = installed_scope(&program)
            && let Some(folder) = program.parent()
        {
            let mut arg = std::ffi::OsString::from("/HARMONYINSTALLED=");
            arg.push(folder);
            setup.args([scope, "/SILENT"]).arg(arg);
        }
        setup.spawn().context("Could not start Harmony Setup")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{rand::SystemRandom, signature::KeyPair};

    const TAG: &str = "harmony-v1.2.1-b10";
    fn release() -> Release {
        Release {
            tag: TAG.into(),
            notes: "Changes".into(),
            has_installer: true,
        }
    }
    fn manifest(content: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema":1, "repository":REPO, "tag":TAG, "platform":"windows-x64",
            "installer":"Harmony-1.2.1-Setup.exe", "bytes":content.len(),
            "sha256":hex::encode(digest::digest(&digest::SHA256,content))
        }))
        .unwrap()
    }

    #[test]
    fn released_builds_order_numerically_and_reject_foreign_tags() {
        let listing = |tag: &str, prerelease: bool| {
            serde_json::to_vec(&serde_json::json!({
            "tag_name":tag, "body":"Notes", "draft":false, "prerelease":prerelease,
            "assets":[{"name":"Harmony-1.2.1-Setup.exe"},{"name":"harmony-update.json"},{"name":"harmony-update.json.sig"}]
        })).unwrap()
        };
        let bytes = listing(TAG, false);
        assert!(
            available(&bytes, "1.2.1", 9)
                .unwrap()
                .unwrap()
                .has_installer
        );
        assert!(available(&bytes, "1.2.1", 10).unwrap().is_none());
        assert!(available(&bytes, "1.2.1", 0).unwrap().is_none());
        assert!(available(&bytes, "1.10.0", 1).unwrap().is_none());
        assert!(
            available(&listing(TAG, true), "1.2.0", 1)
                .unwrap()
                .is_none()
        );
        for tag in [
            "v1.2.1",
            "harmony-v01.2.1-b1",
            "harmony-v1.2.1-b0",
            "harmony-v1.2.1-b1/x",
        ] {
            assert!(available(&listing(tag, false), "1.2.0", 1).is_err());
        }
    }

    #[test]
    #[ignore = "requires HARMONY_SIGNED_UPDATE_DIR with the matching local release artifacts"]
    fn packaged_setup_matches_the_embedded_publisher_key_and_build() {
        let folder = PathBuf::from(std::env::var_os("HARMONY_SIGNED_UPDATE_DIR").unwrap());
        let build = crate::build_info::BuildInfo::current();
        let release = Release {
            tag: format!("harmony-v{}-b{}", build.version, build.build_id),
            notes: String::new(),
            has_installer: true,
        };
        let manifest = authenticate(
            &fs::read(folder.join("harmony-update.json")).unwrap(),
            &fs::read(folder.join("harmony-update.json.sig")).unwrap(),
            &hex::decode(PUBLIC_KEY.trim()).unwrap(),
            &release,
        )
        .unwrap();
        let path = folder.join(&manifest.installer);
        #[cfg(windows)]
        let _parents = hold_update_directories(&path).unwrap();
        verified_file(&path, &manifest).unwrap();
    }

    #[test]
    fn signatures_bind_the_publisher_release_platform_and_installer_bytes() {
        let rng = SystemRandom::new();
        let alg = &signature::ECDSA_P256_SHA256_FIXED_SIGNING;
        let pkcs8 = signature::EcdsaKeyPair::generate_pkcs8(alg, &rng).unwrap();
        let pair = signature::EcdsaKeyPair::from_pkcs8(alg, pkcs8.as_ref(), &rng).unwrap();
        let key = pair.public_key().as_ref();
        let payload = vec![42; 512];
        let bytes = manifest(&payload);
        let sig = pair.sign(&rng, &bytes).unwrap();
        let m = authenticate(&bytes, sig.as_ref(), key, &release()).unwrap();
        assert!(
            authenticate(
                &bytes,
                sig.as_ref(),
                &hex::decode(PUBLIC_KEY.trim()).unwrap(),
                &release()
            )
            .is_err()
        );
        assert!(
            authenticate(
                &[bytes.clone(), vec![b' ']].concat(),
                sig.as_ref(),
                key,
                &release()
            )
            .is_err()
        );
        assert!(
            authenticate(
                &bytes,
                sig.as_ref(),
                key,
                &Release {
                    tag: "harmony-v1.2.1-b11".into(),
                    ..release()
                }
            )
            .is_err()
        );
        for (field, wrong) in [
            ("platform", "linux-x64"),
            ("repository", "another/repo"),
            ("installer", "another.exe"),
        ] {
            let mut data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            data[field] = wrong.into();
            let changed = serde_json::to_vec(&data).unwrap();
            let signed = pair.sign(&rng, &changed).unwrap();
            assert!(authenticate(&changed, signed.as_ref(), key, &release()).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("installer");
        fs::write(&path, &payload).unwrap();
        assert!(verified_file(&path, &m).is_ok());
        fs::write(&path, vec![41; 512]).unwrap();
        assert!(verified_file(&path, &m).is_err());
        fs::write(&path, [42; 64]).unwrap();
        assert!(verified_file(&path, &m).is_err());
    }
}
