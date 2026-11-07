//! Staging and recoverable swaps. Never terminate processes or run downloads.
use super::*;
use release::{Candidate, MAX_ASSET, MAX_METADATA, Manifest, Transport, validate_manifest};
use safe_fs::Dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Phase {
    Staged,
    Applying,
    Installed,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: u32,
    pub target: String,
    pub previous_sha256: String,
    pub manifest: Manifest,
    pub armed: bool,
    pub phase: Phase,
}
fn location(install: &Path) -> Result<(Dir, Dir, String)> {
    let parent = install
        .parent()
        .ok_or(Error::Invalid("installation directory"))?;
    let target = install
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(Error::Invalid("installation filename"))?
        .to_owned();
    safe_fs::leaf(&target)?;
    let root = Dir::open(parent)?;
    let updates = root.child(".harmony-updates", true)?;
    Ok((root, updates, target))
}
pub fn hash(file: &mut File, cancel: &Cancel) -> Result<String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| Error::Io("seek executable"))?;
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut bytes = [0u8; 64 * 1024];
    loop {
        cancel.check()?;
        let n = file
            .read(&mut bytes)
            .map_err(|_| Error::Io("read executable"))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_ASSET {
            return Err(Error::Size);
        }
        digest.update(&bytes[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
struct BoundedWriter<'a> {
    file: &'a mut File,
    limit: u64,
    written: u64,
    exceeded: bool,
    cancel: &'a Cancel,
}
impl Write for BoundedWriter<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.cancel.check().is_err() {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        if b.len() as u64 > self.limit.saturating_sub(self.written) {
            self.exceeded = true;
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let n = self.file.write(b)?;
        self.written += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
pub fn validate_executable(file: &mut File, platform: &str) -> Result<()> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| Error::Io("seek executable header"))?;
    let mut header = [0u8; 64];
    file.read_exact(&mut header)
        .map_err(|_| Error::Invalid("short executable header"))?;
    let valid = if platform.starts_with("windows-") {
        let offset = u32::from_le_bytes(header[0x3c..0x40].try_into().unwrap()) as u64;
        if &header[..2] != b"MZ" || offset < 64 || offset > 1024 * 1024 {
            false
        } else {
            file.seek(SeekFrom::Start(offset))
                .map_err(|_| Error::Io("seek PE header"))?;
            let mut pe = [0u8; 6];
            file.read_exact(&mut pe)
                .map_err(|_| Error::Invalid("PE header"))?;
            &pe[..4] == b"PE\0\0"
                && u16::from_le_bytes([pe[4], pe[5]])
                    == if platform.ends_with("x86_64") {
                        0x8664
                    } else {
                        0xaa64
                    }
        }
    } else {
        &header[..4] == b"\x7fELF"
            && header[4] == 2
            && header[5] == 1
            && header[6] == 1
            && &header[8..11] == b"AI\x02"
            && u16::from_le_bytes([header[18], header[19]])
                == if platform.ends_with("x86_64") {
                    62
                } else {
                    183
                }
    };
    if valid {
        Ok(())
    } else {
        Err(Error::Invalid(
            "executable platform/architecture (raw PE or type-2 AppImage required)",
        ))
    }
}
fn write_plan(dir: &Dir, plan: &Plan) -> Result<()> {
    let json = serde_json::to_vec(plan).map_err(|_| Error::Invalid("update plan"))?;
    dir.atomic("plan.json", &json)
}
fn read_plan(dir: &Dir, target: &str) -> Result<Plan> {
    let plan: Plan = serde_json::from_slice(&dir.read("plan.json", MAX_METADATA)?)
        .map_err(|_| Error::Invalid("saved update plan"))?;
    if plan.schema != 1
        || plan.target != target
        || plan.previous_sha256.len() != 64
        || !plan
            .previous_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error::Invalid("saved installation identity"));
    }
    safe_fs::leaf(&plan.target)?;
    validate_manifest(&plan.manifest, &BuildInfo::current().platform_key())?;
    Ok(plan)
}
fn pending_name(updates: &Dir) -> Result<Option<String>> {
    if !updates.exists("pending.json")? {
        return Ok(None);
    }
    let name: String = serde_json::from_slice(&updates.read("pending.json", 256)?)
        .map_err(|_| Error::Invalid("saved pending transaction"))?;
    safe_fs::leaf(&name)?;
    if !name.starts_with("txn-") {
        return Err(Error::Invalid("saved transaction namespace"));
    }
    Ok(Some(name))
}
pub fn pending(install: &Path) -> Result<Option<(String, Plan)>> {
    let (_, updates, target) = location(install)?;
    let Some(name) = pending_name(&updates)? else {
        return Ok(None);
    };
    let plan = read_plan(&updates.child(&name, false)?, &target)?;
    Ok(Some((name, plan)))
}
pub fn stage(
    install: &Path,
    candidate: &Candidate,
    transport: &dyn Transport,
    cancel: &Cancel,
) -> Result<String> {
    let artifact = validate_manifest(&candidate.manifest, &BuildInfo::current().platform_key())?;
    if candidate.asset.name != artifact.name
        || candidate.asset.size != artifact.size
        || candidate.release.tag_name != candidate.manifest.tag
    {
        return Err(Error::Invalid("candidate changed"));
    }
    let (root, updates, target) = location(install)?;
    let _lock = updates.lock()?;
    if let Some((_, p)) = pending(install)? {
        if p.phase != Phase::Installed {
            return Err(Error::Conflict);
        }
    }
    let previous_sha256 = hash(&mut root.file(&target, false)?, cancel)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Io("system clock"))?
        .as_nanos();
    let name = format!("txn-{}-{nonce}", std::process::id());
    if updates.exists(&name)? {
        return Err(Error::Conflict);
    }
    let dir = updates.child(&name, true)?;
    let result = (|| {
        let mut file = dir.file("payload", true)?;
        let mut bounded = BoundedWriter {
            file: &mut file,
            limit: artifact.size,
            written: 0,
            exceeded: false,
            cancel,
        };
        let downloaded = transport.asset(candidate.asset.id, &mut bounded, artifact.size, cancel);
        cancel.check()?;
        if bounded.exceeded || (downloaded.is_ok() && bounded.written != artifact.size) {
            return Err(Error::Size);
        }
        downloaded?;
        file.sync_all()
            .map_err(|_| Error::Io("sync staged executable"))?;
        if hash(&mut file, cancel)? != artifact.sha256 {
            return Err(Error::Hash);
        }
        validate_executable(&mut file, &artifact.platform)?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Error::Io("make staged AppImage executable"))?;
        }
        drop(file);
        cancel.check()?;
        let plan = Plan {
            schema: 1,
            target,
            previous_sha256,
            manifest: candidate.manifest.clone(),
            armed: false,
            phase: Phase::Staged,
        };
        write_plan(&dir, &plan)?;
        cancel.check()?;
        updates.atomic(
            "pending.json",
            &serde_json::to_vec(&name).map_err(|_| Error::Invalid("pending transaction"))?,
        )?;
        Ok(name.clone())
    })();
    if result.is_err() {
        for file in ["payload", "plan.json"] {
            if dir.exists(file)? && dir.remove(file).is_err() {
                return Err(Error::Io(
                    "partial staging cleanup failed; owned transaction retained",
                ));
            }
        }
    }
    result
}
pub fn arm(install: &Path, enabled: bool) -> Result<()> {
    let (_, updates, target) = location(install)?;
    let _lock = updates.lock()?;
    let name = pending_name(&updates)?.ok_or(Error::Invalid("no staged update"))?;
    let dir = updates.child(&name, false)?;
    let mut plan = read_plan(&dir, &target)?;
    if plan.phase != Phase::Staged {
        return Err(Error::Invalid("update not staged"));
    }
    plan.armed = enabled;
    write_plan(&dir, &plan)
}
pub fn discard(install: &Path) -> Result<()> {
    let (_, updates, target) = location(install)?;
    let _lock = updates.lock()?;
    let Some(name) = pending_name(&updates)? else {
        return Ok(());
    };
    let dir = updates.child(&name, false)?;
    let plan = read_plan(&dir, &target)?;
    if plan.phase == Phase::Applying {
        return Err(Error::Busy);
    }
    for file in ["payload", "helper.exe", "helper.AppImage"] {
        if dir.exists(file)? {
            dir.remove(file)?;
        }
    }
    updates.remove("pending.json")?;
    Ok(()) // Journal and rollback are intentionally retained; no recursive deletion.
}
pub fn activate(install: &Path) -> Result<()> {
    activate_inner(install, || {})
}
fn copy_verified(source: &mut File, dir: &Dir, name: &str, expected: &str) -> Result<()> {
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| Error::Io("seek backup source"))?;
    let mut out = dir.file(name, true)?;
    std::io::copy(&mut source.take(MAX_ASSET + 1), &mut out)
        .map_err(|_| Error::Io("write owned backup"))?;
    out.sync_all().map_err(|_| Error::Io("sync owned backup"))?;
    if hash(&mut out, &Cancel::default())? != expected {
        return Err(Error::Hash);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        out.set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Io("backup executable permissions"))?;
    }
    Ok(())
}
fn activate_inner(install: &Path, before_publish: impl FnOnce()) -> Result<()> {
    let (root, updates, target) = location(install)?;
    let _lock = updates.lock()?;
    let name = pending_name(&updates)?.ok_or(Error::Invalid("no staged update"))?;
    let dir = updates.child(&name, false)?;
    let mut plan = read_plan(&dir, &target)?;
    if !plan.armed || !matches!(plan.phase, Phase::Staged | Phase::Applying) {
        return Err(Error::Invalid("installation not authorized on full exit"));
    }
    let artifact = validate_manifest(&plan.manifest, &BuildInfo::current().platform_key())?;
    let current_hash = hash(&mut root.file(&target, false)?, &Cancel::default())?;
    if plan.phase == Phase::Applying && current_hash == artifact.sha256 {
        if hash(&mut dir.file("rollback", false)?, &Cancel::default())? != plan.previous_sha256 {
            return Err(Error::Hash);
        }
        plan.phase = Phase::Installed;
        plan.armed = false;
        return write_plan(&dir, &plan);
    }
    if current_hash != plan.previous_sha256 {
        return Err(Error::Conflict);
    }
    let mut payload = dir.file("payload", false)?;
    if payload
        .metadata()
        .map_err(|_| Error::Io("staged size"))?
        .len()
        != artifact.size
    {
        return Err(Error::Size);
    }
    if hash(&mut payload, &Cancel::default())? != artifact.sha256 {
        return Err(Error::Hash);
    }
    validate_executable(&mut payload, &artifact.platform)?;
    // Back up first, sync and rehash. Never create a window where the launch
    // target is absent: only the final candidate rename replaces it atomically.
    if !dir.exists("rollback")? {
        copy_verified(
            &mut root.file(&target, false)?,
            &dir,
            "rollback",
            &plan.previous_sha256,
        )?;
    }
    if hash(&mut dir.file("rollback", false)?, &Cancel::default())? != plan.previous_sha256 {
        return Err(Error::Hash);
    }
    plan.phase = Phase::Applying;
    write_plan(&dir, &plan)?;
    before_publish();
    let result = (|| {
        // Revalidate the leaf after any intervening I/O; no symlink is followed.
        if hash(&mut dir.file("payload", false)?, &Cancel::default())? != artifact.sha256 {
            return Err(Error::Hash);
        }
        dir.rename("payload", &root, &target, true)?;
        if hash(&mut root.file(&target, false)?, &Cancel::default())? != artifact.sha256 {
            return Err(Error::Hash);
        }
        plan.phase = Phase::Installed;
        plan.armed = false;
        write_plan(&dir, &plan)
    })();
    if result.is_err() {
        // If publication never happened the old target is untouched. Otherwise
        // restore from a verified copy without consuming the rollback itself.
        let baseline = root
            .file(&target, false)
            .and_then(|mut f| hash(&mut f, &Cancel::default()))
            .ok();
        if baseline.as_deref() != Some(&plan.previous_sha256) {
            let restore = format!("restore-{}.tmp", std::process::id());
            copy_verified(
                &mut dir.file("rollback", false)?,
                &root,
                &restore,
                &plan.previous_sha256,
            )?;
            root.rename(&restore, &root, &target, true)?;
            if hash(&mut root.file(&target, false)?, &Cancel::default())? != plan.previous_sha256 {
                return Err(Error::Hash);
            }
        }
        plan.phase = Phase::Failed;
        plan.armed = false;
        write_plan(&dir, &plan)?;
    }
    result
}
pub fn spawn_helper(install: &Path) -> Result<()> {
    let (root, updates, target) = location(install)?;
    let _lock = updates.lock()?;
    let name = pending_name(&updates)?.ok_or(Error::Invalid("no staged update"))?;
    let dir = updates.child(&name, false)?;
    let plan = read_plan(&dir, &target)?;
    if plan.phase != Phase::Staged || !plan.armed {
        return Err(Error::Invalid("full-exit install not authorized"));
    }
    let helper_name = if cfg!(windows) {
        "helper.exe"
    } else {
        "helper.AppImage"
    };
    if !dir.exists(helper_name)? {
        copy_verified(
            &mut root.file(&target, false)?,
            &dir,
            helper_name,
            &plan.previous_sha256,
        )?;
    }
    let mut helper = dir.file(helper_name, false)?;
    if hash(&mut helper, &Cancel::default())? != plan.previous_sha256 {
        return Err(Error::Hash);
    }
    #[cfg(target_os = "linux")]
    let executable = {
        use std::os::fd::AsRawFd;
        std::path::PathBuf::from(format!("/proc/self/fd/{}", helper.as_raw_fd()))
    };
    #[cfg(windows)]
    let executable = dir.path.join(helper_name);
    #[cfg(not(any(windows, target_os = "linux")))]
    let executable = {
        return Err(Error::Unsupported);
        std::path::PathBuf::new()
    };
    // Windows CreateProcess uses a path. Freeze/revalidate ALL its ancestors
    // against reparse mutations just for spawn (no more filesystem writes).
    #[cfg(windows)]
    let _guards = {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let mut guards = Vec::new();
        let mut path = std::path::PathBuf::new();
        for c in dir.path.components() {
            path.push(c.as_os_str());
            if matches!(c, std::path::Component::Prefix(_)) {
                continue;
            }
            let f = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .custom_flags(0x0020_0000 | 0x0200_0000)
                .open(&path)
                .map_err(|_| Error::Io("guard helper launch directory"))?;
            let m = f
                .metadata()
                .map_err(|_| Error::Io("helper launch metadata"))?;
            if !m.is_dir() || m.file_attributes() & 0x400 != 0 {
                return Err(Error::Invalid("helper launch reparse directory"));
            }
            guards.push(f);
        }
        let f = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(&executable)
            .map_err(|_| Error::Io("guard helper image"))?;
        let m = f
            .metadata()
            .map_err(|_| Error::Io("helper image metadata"))?;
        if !m.is_file() || m.file_attributes() & 0x400 != 0 {
            return Err(Error::Invalid("helper image link"));
        }
        guards.push(f);
        guards
    };
    let mut cmd = std::process::Command::new(executable);
    cmd.arg("--harmony-update-helper")
        .arg(install)
        .arg(&name)
        .arg(std::process::id().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_DEBUG",
    ] {
        cmd.env_remove(key);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map_err(|_| Error::Io("launch full-exit updater helper"))?;
    Ok(()) // Handoff only, not an installation success claim.
}
pub fn record_helper_failure(install: &Path) -> Result<()> {
    let (_, updates, target) = location(install)?;
    let _lock = updates.lock()?;
    let Some(name) = pending_name(&updates)? else {
        return Ok(());
    };
    let dir = updates.child(&name, false)?;
    let mut plan = read_plan(&dir, &target)?;
    if plan.phase == Phase::Staged {
        plan.phase = Phase::Failed;
        plan.armed = false;
        write_plan(&dir, &plan)?;
    }
    Ok(())
}
#[cfg(test)]
pub fn activate_with_hook(install: &Path, hook: impl FnOnce()) -> Result<()> {
    activate_inner(install, hook)
}
