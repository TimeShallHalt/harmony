//! Optional stable native entry point. Creating this layout is a separate,
//! explicit manual install into an EMPTY owned directory, never a self-update.
//! The updater must never replace the stable entry or this installation anchor.
use super::*;
use safe_fs::Dir;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(super) const STATE: &str = ".harmony-launch";
pub fn names() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("Harmony.exe", "harmony-payload.exe")
    } else {
        ("Harmony", "harmony-payload")
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Anchor {
    schema: u32,
    entry: String,
    target: String,
    bootstrap_sha256: String,
    initial_sha256: String,
}
fn digest_token(s: &str) -> Result<()> {
    if s.len() != 64
        || !s
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(Error::Invalid("canonical SHA-256"));
    }
    Ok(())
}
fn source(path: &Path) -> Result<File> {
    let parent = Dir::open(path.parent().ok_or(Error::Invalid("source parent"))?)?;
    parent.file(
        path.file_name()
            .and_then(|s| s.to_str())
            .ok_or(Error::Invalid("source filename"))?,
        false,
    )
}
/// Native payloads only. A mounted AppImage's inner ELF cannot attest its outer
/// file; installing that format here would silently reintroduce finding 2.
pub(super) fn validate_native(file: &mut File) -> Result<()> {
    #[cfg(windows)]
    {
        return transaction::validate_executable(file, &BuildInfo::current().platform_key());
    }
    #[cfg(target_os = "linux")]
    {
        file.seek(SeekFrom::Start(0))
            .map_err(|_| Error::Io("native ELF header"))?;
        let mut h = [0u8; 64];
        file.read_exact(&mut h)
            .map_err(|_| Error::Invalid("native ELF header"))?;
        let machine = if std::env::consts::ARCH == "x86_64" {
            62
        } else if std::env::consts::ARCH == "aarch64" {
            183
        } else {
            return Err(Error::Unsupported);
        };
        if &h[..4] != b"\x7fELF"
            || h[4..7] != [2, 1, 1]
            || &h[8..11] == b"AI\x02"
            || !matches!(u16::from_le_bytes([h[16], h[17]]), 2 | 3)
            || u16::from_le_bytes([h[18], h[19]]) != machine
        {
            return Err(Error::Invalid(
                "direct native ELF required, not mounted AppImage",
            ));
        }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Err(Error::Unsupported)
    }
}
/// Parent-owned manual migration contract: independently verify both supplied
/// digests/source provenance; create a NEW directory; change shortcuts only
/// after a successful read-back and an owned launch test. Old layout untouched.
pub fn create(
    root: &Path,
    bootstrap: &Path,
    bootstrap_hash: &str,
    payload: &Path,
    payload_hash: &str,
) -> Result<()> {
    digest_token(bootstrap_hash)?;
    digest_token(payload_hash)?;
    let mut boot = source(bootstrap)?;
    let mut app = source(payload)?;
    for (file, expected) in [(&mut boot, bootstrap_hash), (&mut app, payload_hash)] {
        if transaction::hash(file, &Cancel::default())? != expected {
            return Err(Error::Hash);
        }
        validate_native(file)?;
    }
    let root = Dir::open(root)?;
    root.require_private()?;
    if std::fs::read_dir(&root.path)
        .map_err(|_| Error::Io("inspect new layout"))?
        .next()
        .is_some()
    {
        return Err(Error::Conflict);
    }
    let state = root.child(STATE, true)?;
    let (entry, target) = names();
    transaction::copy_verified(&mut app, &root, target, payload_hash)?;
    transaction::copy_verified(&mut boot, &state, "bootstrap", bootstrap_hash)?;
    let anchor = Anchor {
        schema: 1,
        entry: entry.into(),
        target: target.into(),
        bootstrap_sha256: bootstrap_hash.into(),
        initial_sha256: payload_hash.into(),
    };
    state.atomic(
        "layout.json",
        &serde_json::to_vec(&anchor).map_err(|_| Error::Invalid("layout anchor"))?,
    )?;
    // NOREPLACE and LAST. A failed/partial creation cannot look like an installed
    // shortcut, and no existing installation or unrelated file is overwritten.
    state.rename("bootstrap", &root, entry, false)?;
    if transaction::hash(&mut root.file(entry, false)?, &Cancel::default())? != bootstrap_hash
        || transaction::hash(&mut root.file(target, false)?, &Cancel::default())? != payload_hash
    {
        return Err(Error::Hash);
    }
    Ok(())
}
pub fn install_cli(args: &[OsString]) -> Option<Result<()>> {
    if args.first()?.to_str()? != "--harmony-create-launch-layout" {
        return None;
    }
    Some((|| {
        if args.len() != 7 || args[6] != "create-new-layout-v1" {
            return Err(Error::Invalid("explicit new-layout arguments"));
        }
        create(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[3].to_str().ok_or(Error::Invalid("bootstrap digest"))?,
            Path::new(&args[4]),
            args[5].to_str().ok_or(Error::Invalid("payload digest"))?,
        )
    })())
}
pub(super) fn anchor(install: &Path) -> Result<Anchor> {
    if install.file_name().and_then(|n| n.to_str()) != Some(names().1) {
        return Err(Error::Unsupported);
    }
    let root = Dir::open(install.parent().ok_or(Error::Unsupported)?)?;
    root.require_private()?;
    let state = root.child(STATE, false)?;
    let anchor: Anchor = serde_json::from_slice(&state.read("layout.json", 4096)?)
        .map_err(|_| Error::Invalid("native layout anchor"))?;
    if anchor.schema != 1 || anchor.entry != names().0 || anchor.target != names().1 {
        return Err(Error::Invalid("fixed native layout names"));
    }
    digest_token(&anchor.bootstrap_sha256)?;
    digest_token(&anchor.initial_sha256)?;
    let mut boot = root.file(names().0, false)?;
    checked_native(&mut boot)?;
    if transaction::hash(&mut boot, &Cancel::default())? != anchor.bootstrap_sha256 {
        return Err(Error::Hash);
    }
    Ok(anchor)
}
pub(super) fn checked_native(file: &mut File) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file
            .metadata()
            .map_err(|_| Error::Io("native image ownership"))?;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 || m.mode() & 0o100 == 0 {
            return Err(Error::Invalid("private executable image required"));
        }
    }
    validate_native(file)
}
/// Normal user launch only. Selection is read under the SAME OS lock used by
/// publication and is executed from the held, hash-verified file, not a pathname
/// reopened after verification. No background monitor or automatic restart.
pub fn launch(args: &[OsString]) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let entry = std::env::current_exe().map_err(|_| Error::UnsupportedImageIdentity)?;
        if entry.file_name().and_then(|n| n.to_str()) != Some(names().0) {
            return Err(Error::Unsupported);
        }
        let install = entry.with_file_name(names().1);
        let a = anchor(&install)?;
        let mut running = image_identity::running(&entry)?;
        if transaction::hash(&mut running, &Cancel::default())? != a.bootstrap_sha256 {
            return Err(Error::Hash);
        }
        let (_, updates, _) = transaction::location(&install)?;
        let _lock = updates.lock()?;
        let (mut selected, _) = transaction::launch_image(&install, &a.initial_sha256)?;
        checked_native(&mut selected)?;
        let error = std::process::Command::new(format!("/proc/self/fd/{}", selected.as_raw_fd()))
            .args(args)
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .exec();
        let _ = error;
        Err(Error::Io("execute verified native payload"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        Err(Error::UnsupportedImageIdentity)
    }
}
/// Discover this explicit layout from kernel current_exe and fixed structure,
/// never from APPIMAGE, argv, the working directory or an environment override.
pub(super) fn current_install() -> Result<Option<std::path::PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        let executed = std::env::current_exe().map_err(|_| Error::UnsupportedImageIdentity)?;
        let Some(leaf) = executed.file_name().and_then(|n| n.to_str()) else {
            return Ok(None);
        };
        let install = if leaf == names().1 {
            executed.clone()
        } else if matches!(leaf, "displaced" | "rollback") {
            let txn = executed.parent().ok_or(Error::Unsupported)?;
            let updates = txn.parent().ok_or(Error::Unsupported)?;
            if updates.file_name().and_then(|n| n.to_str()) != Some(".harmony-updates") {
                return Ok(None);
            }
            updates.parent().ok_or(Error::Unsupported)?.join(names().1)
        } else {
            return Ok(None);
        };
        let a = anchor(&install)?;
        let (_, updates, _) = transaction::location(&install)?;
        let _lock = updates.lock()?;
        let (_file, path) = transaction::launch_image(&install, &a.initial_sha256)?;
        let _kernel_image = image_identity::running(&path)?;
        Ok(Some(install))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(None)
    }
}
