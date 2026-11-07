use super::*;
#[path = "../../scripts/harmony_build_stamp.rs"]
mod build_stamp;
use release::*;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Write};
fn owned_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_4000);
    }
    cmd
}
fn layout_names() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("Harmony.exe", "harmony-payload.exe")
    } else {
        ("Harmony", "harmony-payload")
    }
}
fn create_owned_layout(tmp: &Temp) -> (std::path::PathBuf, std::path::PathBuf) {
    let inputs = Temp::new();
    let program = inputs.0.join("payload-source");
    let boot = inputs.0.join("bootstrap-source");
    std::fs::copy(
        std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap(),
        &program,
    )
    .unwrap();
    std::fs::copy(
        std::env::var_os("HARMONY_TEST_BOOTSTRAP")
            .unwrap_or(std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap()),
        &boot,
    )
    .unwrap();
    let sha = |p: &std::path::Path| format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()));
    let result = owned_command(&program)
        .arg("--harmony-create-launch-layout")
        .arg(&tmp.0)
        .arg(&boot)
        .arg(sha(&boot))
        .arg(&program)
        .arg(sha(&program))
        .arg("create-new-layout-v1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let (entry, target) = layout_names();
    (tmp.0.join(entry), tmp.0.join(target))
}
#[cfg(target_os = "linux")]
fn native_payload_fixture() -> Fixture {
    let mut f = fixture();
    let mut bytes =
        std::fs::read(std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap()).unwrap();
    bytes.extend_from_slice(b"owned next-version ELF fixture");
    let mut manifest: Manifest = serde_json::from_slice(&f.assets[&1]).unwrap();
    let a = &mut manifest.artifacts[0];
    a.format = "elf".into();
    a.name = format!("Harmony-1.0.1-{}.elf", BuildInfo::current().platform_key());
    a.size = bytes.len() as u64;
    a.sha256 = format!("{:x}", Sha256::digest(&bytes));
    f.releases[0].assets[1].name = a.name.clone();
    f.releases[0].assets[1].size = a.size;
    f.assets.insert(2, bytes);
    f.assets.insert(1, serde_json::to_vec(&manifest).unwrap());
    f.releases[0].assets[0].size = f.assets[&1].len() as u64;
    f
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "owned native-ELF packaging boundary; use runner"]
fn native_elf_updates_require_the_explicit_stable_layout() {
    let tmp = Temp::new();
    let (entry, install) = create_owned_layout(&tmp);
    let f = native_payload_fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .expect("native ELF manifest")
        .unwrap();
    transaction::stage(&install, &c, &f, &Cancel::default())
        .expect("native ELF stage in stable layout");
    let direct = Temp::new();
    let legacy = direct.install();
    assert_eq!(
        transaction::stage(&legacy, &c, &f, &Cancel::default()),
        Err(Error::Unsupported)
    );
    assert_eq!(std::fs::read(&legacy).unwrap(), b"old fixture executable");
    assert!(entry.exists());
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "actual stable entry -> owned payload -> normal Controller; use runner"]
fn stable_entry_launches_verified_native_payload_without_appimage_environment() {
    let tmp = Temp::new();
    let (entry, install) = create_owned_layout(&tmp);
    let marker = tmp.0.join("normal-result");
    let result = owned_command(&entry)
        .arg("normal-startup")
        .arg(&marker)
        .env_remove("APPIMAGE")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "normal entry failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(marker).unwrap()).unwrap();
    assert_eq!(value["install"], install.to_str().unwrap());
    assert!(value["phase"].is_null());
    assert_eq!(value["armed"], false);
}
#[test]
#[ignore = "explicit new-layout installer, owned executable inputs only; use runner"]
fn stable_layout_creation_requires_explicit_verified_inputs_and_refuses_migration_in_place() {
    let inputs = Temp::new();
    let program = inputs.0.join("payload-source");
    std::fs::copy(
        std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap(),
        &program,
    )
    .unwrap();
    let digest = format!("{:x}", Sha256::digest(std::fs::read(&program).unwrap()));
    let tmp = Temp::new();
    let create = |dest: &std::path::Path, expected: &str| {
        owned_command(&program)
            .arg("--harmony-create-launch-layout")
            .arg(dest)
            .arg(&program)
            .arg(&digest)
            .arg(&program)
            .arg(expected)
            .arg("create-new-layout-v1")
            .output()
            .unwrap()
    };
    let result = create(&tmp.0, &digest);
    assert!(
        result.status.success(),
        "explicit new-layout creation unavailable: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let (entry, payload) = layout_names();
    assert_eq!(
        std::fs::read(tmp.0.join(entry)).unwrap(),
        std::fs::read(&program).unwrap()
    );
    assert_eq!(
        std::fs::read(tmp.0.join(payload)).unwrap(),
        std::fs::read(&program).unwrap()
    );
    assert!(tmp.0.join(".harmony-launch/layout.json").is_file());
    let before = std::fs::read(tmp.0.join(".harmony-launch/layout.json")).unwrap();
    assert!(
        !create(&tmp.0, &digest).status.success(),
        "no in-place migration or clobber"
    );
    assert_eq!(
        std::fs::read(tmp.0.join(".harmony-launch/layout.json")).unwrap(),
        before
    );
    let bad = Temp::new();
    assert!(!create(&bad.0, &"0".repeat(64)).status.success());
    assert!(
        !bad.0.join(entry).exists(),
        "entry must be published LAST, only after validation"
    );
    let occupied = Temp::new();
    std::fs::write(occupied.0.join("unrelated"), b"keep").unwrap();
    assert!(!create(&occupied.0, &digest).status.success());
    assert_eq!(
        std::fs::read(occupied.0.join("unrelated")).unwrap(),
        b"keep"
    );
}
fn activate_pending(install: &std::path::Path) -> Result<()> {
    let name = transaction::pending(install)?.ok_or(Error::NotFound)?.0;
    transaction::activate(install, &name)
}
fn current_fixture() -> BuildInfo {
    let mut info = BuildInfo::current();
    info.version = "1.0.0".into();
    info.build_id = 0;
    info
}
fn fixture_controller(install: Result<std::path::PathBuf>) -> controller::Controller {
    let mut c = controller::Controller::new(install);
    c.current = current_fixture();
    c
}
struct Fixture {
    releases: Vec<Release>,
    assets: HashMap<u64, Vec<u8>>,
}
impl Transport for Fixture {
    fn releases(&self, c: &Cancel) -> Result<Vec<Release>> {
        c.check()?;
        Ok(self.releases.clone())
    }
    fn asset(&self, id: u64, w: &mut dyn Write, limit: u64, c: &Cancel) -> Result<()> {
        c.check()?;
        let b = self.assets.get(&id).ok_or(Error::Network)?;
        if b.len() as u64 > limit {
            return Err(Error::Size);
        }
        w.write_all(b).map_err(|_| Error::Io("fixture write"))
    }
}
fn fixture() -> Fixture {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let ext = if cfg!(windows) { "exe" } else { "AppImage" };
    let mut bytes = vec![0u8; 128];
    if cfg!(windows) {
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(
            &(if std::env::consts::ARCH == "aarch64" {
                0xaa64u16
            } else {
                0x8664u16
            })
            .to_le_bytes(),
        );
    } else {
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[8..11].copy_from_slice(b"AI\x02");
        bytes[18..20].copy_from_slice(
            &(if std::env::consts::ARCH == "aarch64" {
                183u16
            } else {
                62u16
            })
            .to_le_bytes(),
        );
    }
    // Synthetic executable-format fixture only; no downloaded payload is run.
    let artifact = Artifact {
        platform: platform.clone(),
        name: format!("Harmony-1.0.1-{platform}.{ext}"),
        size: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        format: if cfg!(windows) { "exe" } else { "appimage" }.into(),
    };
    let manifest = Manifest {
        schema: 1,
        repository: REPOSITORY.into(),
        tag: "harmony-v1.0.1-b1".into(),
        version: "1.0.1".into(),
        build_id: 1,
        commit: "0123456789abcdef0123456789abcdef01234567".into(),
        artifacts: vec![artifact.clone()],
    };
    let json = serde_json::to_vec(&manifest).unwrap();
    let release = Release {
        tag_name: manifest.tag.clone(),
        html_url: format!(
            "https://github.com/{REPOSITORY}/releases/tag/{}",
            manifest.tag
        ),
        draft: false,
        prerelease: false,
        assets: vec![
            Asset {
                id: 1,
                name: "harmony-update-v1.json".into(),
                size: json.len() as u64,
                url: format!("https://api.github.com/repos/{REPOSITORY}/releases/assets/1"),
            },
            Asset {
                id: 2,
                name: artifact.name,
                size: artifact.size,
                url: format!("https://api.github.com/repos/{REPOSITORY}/releases/assets/2"),
            },
        ],
    };
    Fixture {
        releases: vec![release],
        assets: HashMap::from([(1, json), (2, bytes)]),
    }
}
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "harmony-update-fixture-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn install(&self) -> std::path::PathBuf {
        let p = self.0.join("harmony.exe");
        std::fs::write(&p, b"old fixture executable").unwrap();
        p
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn fixture_download_stages_real_bytes_without_replacing_running_target() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    assert_eq!(std::fs::read(&install).unwrap(), b"old fixture executable");
    let staged = std::fs::read(tmp.0.join(".harmony-updates").join(txn).join("payload")).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&staged)), c.artifact.sha256);
    assert_eq!(staged, f.assets[&2]);
}
#[test]
fn private_release_fixture_resolves_manifest_to_current_platform() {
    let c = check_release(&fixture(), &current_fixture(), &Cancel::default()).unwrap();
    assert!(c.is_some(), "a newer verified release must be offered");
    let c = c.unwrap();
    assert_eq!(c.manifest.version, "1.0.1");
    assert_eq!(c.asset.id, 2);
}

#[test]
fn activation_is_user_armed_preserves_rollback_and_verifies_both_files() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    assert!(activate_pending(&install).is_err());
    transaction::arm(&install, true).unwrap();
    activate_pending(&install).unwrap();
    assert_eq!(std::fs::read(&install).unwrap(), f.assets[&2]);
    let rollback = tmp.0.join(".harmony-updates").join(txn).join("rollback");
    assert_eq!(std::fs::read(rollback).unwrap(), b"old fixture executable");
    assert_eq!(
        transaction::pending(&install).unwrap().unwrap().1.phase,
        transaction::Phase::Installed
    );
}
#[cfg(target_os = "linux")]
#[test]
fn committed_pending_pointer_sync_failure_retains_payload_and_reachable_recovery() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    safe_fs::FAIL_SYNC_FOR.with(|slot| *slot.borrow_mut() = Some("pending.json".into()));
    assert!(
        transaction::stage(&install, &c, &f, &Cancel::default()).is_err(),
        "durability error must be reported"
    );
    let pending = transaction::pending(&install)
        .expect("published pending pointer must not dangle")
        .unwrap();
    assert_eq!(
        std::fs::read(
            tmp.0
                .join(".harmony-updates")
                .join(&pending.0)
                .join("payload")
        )
        .unwrap(),
        f.assets[&2]
    );
    let controller = fixture_controller(Ok(install.clone()));
    controller.load_pending().unwrap();
    assert!(controller.snapshot().staged.is_some());
    controller.arm(true).unwrap();
    controller.arm(false).unwrap();
    controller.discard().unwrap();
    assert!(transaction::pending(&install).unwrap().is_none());
    assert_eq!(std::fs::read(install).unwrap(), b"old fixture executable");
}
#[cfg(target_os = "linux")]
#[test]
fn post_pointer_sync_failure_is_reachable_in_the_original_controller_too() {
    let tmp = Temp::new();
    let install = tmp.install();
    let controller = fixture_controller(Ok(install.clone()));
    safe_fs::FAIL_SYNC_FOR.with(|slot| *slot.borrow_mut() = Some("pending.json".into()));
    assert_eq!(controller.check(&fixture(), true), Err(Error::Durability));
    assert!(!controller.busy());
    assert!(controller.snapshot().staged.is_some());
    controller.arm(true).unwrap();
    controller.arm(false).unwrap();
    controller.discard().unwrap();
    assert!(transaction::pending(&install).unwrap().is_none());
}
#[cfg(target_os = "linux")]
#[test]
fn pre_pointer_plan_sync_failure_does_not_claim_a_new_pending_publication() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let old = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    activate_pending(&install).unwrap();
    let controller = fixture_controller(Ok(install.clone()));
    safe_fs::FAIL_SYNC_FOR.with(|slot| *slot.borrow_mut() = Some("plan.json".into()));
    assert!(controller.check(&f, true).is_err());
    assert!(
        controller.snapshot().staged.is_none(),
        "an old Installed pointer is not a new committed staged update"
    );
    let saved = transaction::pending(&install).unwrap().unwrap();
    assert_eq!(saved.0, old);
    assert_eq!(saved.1.phase, transaction::Phase::Installed);
}
#[test]
fn checksum_failure_does_not_publish_pending_or_change_installation() {
    let tmp = Temp::new();
    let install = tmp.install();
    let mut f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    f.assets.get_mut(&2).unwrap()[4] ^= 0x20;
    assert_eq!(
        transaction::stage(&install, &c, &f, &Cancel::default()),
        Err(Error::Hash)
    );
    assert!(transaction::pending(&install).unwrap().is_none());
    assert_eq!(std::fs::read(install).unwrap(), b"old fixture executable");
}
#[test]
fn release_repository_and_asset_url_are_not_delegated_to_manifest_hosts() {
    for change in [0, 1] {
        let mut f = fixture();
        if change == 0 {
            f.releases[0].html_url = "https://evil.invalid/release".into();
        } else {
            f.releases[0].assets[1].url = "https://evil.invalid/asset".into();
        }
        assert!(check_release(&f, &current_fixture(), &Cancel::default()).is_err());
    }
}
#[test]
fn artifact_paths_archives_and_duplicate_platforms_are_rejected() {
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    for name in [
        "../harm.exe",
        "C:\\harm.exe",
        "file:secret",
        "//server/harm",
        "update.zip",
        "safe.exe/../../x",
    ] {
        let mut m = c.manifest.clone();
        m.artifacts[0].name = name.into();
        assert!(
            validate_manifest(&m, &current_fixture().platform_key()).is_err(),
            "{name}"
        );
    }
    let mut m = c.manifest.clone();
    m.artifacts.push(m.artifacts[0].clone());
    assert!(validate_manifest(&m, &current_fixture().platform_key()).is_err());
}
#[test]
fn semantic_order_uses_numeric_versions_then_explicit_build_ids() {
    assert!(version_key("1.10.0", 1).unwrap() > version_key("1.9.9", 900).unwrap());
    assert!(version_key("1.0.0", 2).unwrap() > version_key("1.0.0", 1).unwrap());
    for v in ["01.0.0", "1.0", "v1.0.0", "1.0.0+commit", "1.0.0-rc.1"] {
        assert!(version_key(v, 1).is_err());
    }
}
#[test]
fn release_no_release_cancel_and_same_build_commit_conflict_are_truthful() {
    assert!(
        check_release(
            &Fixture {
                releases: vec![],
                assets: HashMap::new()
            },
            &current_fixture(),
            &Cancel::default()
        )
        .unwrap()
        .is_none()
    );
    let cancel = Cancel::default();
    cancel.0.store(true, Ordering::Release);
    assert_eq!(
        check_release(&fixture(), &current_fixture(), &cancel).unwrap_err(),
        Error::Cancelled
    );
    let mut current = BuildInfo::current();
    current.version = "1.0.1".into();
    current.build_id = 1;
    assert_eq!(
        check_release(&fixture(), &current, &Cancel::default()).unwrap_err(),
        Error::Conflict
    );
    current.commit = "0123456789abcdef0123456789abcdef01234567".into();
    assert!(
        check_release(&fixture(), &current, &Cancel::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn automatic_checks_are_event_driven_and_at_most_daily() {
    assert!(controller::startup_due(true, 100_000, None));
    assert!(!controller::startup_due(false, 100_000, None));
    assert!(!controller::startup_due(true, 100_000, Some(99_999)));
    assert!(controller::startup_due(true, 100_000, Some(1)));
}

fn rewrite_manifest(f: &mut Fixture, change: impl FnOnce(&mut Manifest)) {
    let mut m: Manifest = serde_json::from_slice(&f.assets[&1]).unwrap();
    change(&mut m);
    f.assets.insert(1, serde_json::to_vec(&m).unwrap());
    f.releases[0].assets[0].size = f.assets[&1].len() as u64;
}
#[test]
fn correct_checksum_wrong_executable_architecture_is_rejected_before_staging() {
    let tmp = Temp::new();
    let install = tmp.install();
    let mut f = fixture();
    f.assets.get_mut(&2).unwrap()[0] = b'X';
    let sha = format!("{:x}", Sha256::digest(&f.assets[&2]));
    rewrite_manifest(&mut f, |m| m.artifacts[0].sha256 = sha);
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    assert!(
        transaction::stage(&install, &c, &f, &Cancel::default()).is_err(),
        "a correctly hashed non-executable must never stage"
    );
}
#[test]
fn real_publish_failure_retains_target_and_verified_rollback() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    let dir = tmp.0.join(".harmony-updates").join(txn);
    assert!(
        transaction::activate_with_hook(&install, || std::fs::remove_file(dir.join("payload"))
            .unwrap())
        .is_err()
    );
    assert_eq!(std::fs::read(install).unwrap(), b"old fixture executable");
    assert_eq!(
        std::fs::read(dir.join("rollback")).unwrap(),
        b"old fixture executable"
    );
    assert_eq!(
        transaction::pending(&tmp.0.join("harmony.exe"))
            .unwrap()
            .unwrap()
            .1
            .phase,
        transaction::Phase::Failed
    );
}
#[test]
fn concurrent_creation_after_displacement_is_never_overwritten_or_restored_over() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    let path = install.clone();
    transaction::AFTER_DISPLACE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            std::fs::write(path, b"concurrent creation").unwrap()
        }))
    });
    assert_eq!(activate_pending(&install), Err(Error::Conflict));
    assert_eq!(std::fs::read(&install).unwrap(), b"concurrent creation");
    let saved = tmp.0.join(".harmony-updates").join(txn);
    assert_eq!(
        std::fs::read(saved.join("displaced")).unwrap(),
        b"old fixture executable"
    );
    assert_eq!(
        std::fs::read(saved.join("rollback")).unwrap(),
        b"old fixture executable"
    );
    assert_eq!(std::fs::read(saved.join("payload")).unwrap(), f.assets[&2]);
}
#[test]
fn readback_conflict_never_restores_over_a_later_installation() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    let path = install.clone();
    let later = tmp.0.join("later.tmp");
    std::fs::write(&later, b"later installation").unwrap();
    transaction::AFTER_PUBLISH.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            // A separate installer's atomic rename, not an in-place write
            // blocked by Windows sharing while our payload handle is held.
            std::fs::rename(later, path).unwrap()
        }))
    });
    assert_eq!(activate_pending(&install), Err(Error::Conflict));
    assert_eq!(std::fs::read(&install).unwrap(), b"later installation");
    assert_eq!(
        transaction::pending(&install).unwrap().unwrap().1.phase,
        transaction::Phase::Applying
    );
}
#[test]
fn concurrent_target_change_at_publication_is_preserved_not_overwritten() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    assert_eq!(
        transaction::activate_with_hook(&install, || std::fs::write(
            &install,
            b"unrelated concurrent image"
        )
        .unwrap()),
        Err(Error::Conflict)
    );
    assert_eq!(
        std::fs::read(&install).unwrap(),
        b"unrelated concurrent image"
    );
}
#[test]
fn publication_error_never_restores_over_an_unrelated_concurrent_target() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    let dir = tmp.0.join(".harmony-updates").join(txn);
    assert!(
        transaction::activate_with_hook(&install, || {
            std::fs::write(&install, b"unrelated concurrent image").unwrap();
            std::fs::remove_file(dir.join("payload")).unwrap();
        })
        .is_err()
    );
    assert_eq!(
        std::fs::read(&install).unwrap(),
        b"unrelated concurrent image",
        "must not restore rollback over an image this transaction never published"
    );
}
#[test]
fn changed_installed_executable_is_never_overwritten_by_old_plan() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    std::fs::write(&install, b"another installation").unwrap();
    assert_eq!(activate_pending(&install), Err(Error::Conflict));
    assert_eq!(std::fs::read(install).unwrap(), b"another installation");
}
#[test]
fn staging_size_mismatch_and_cancellation_never_arm_an_update() {
    for oversized in [true, false] {
        let tmp = Temp::new();
        let install = tmp.install();
        let mut f = fixture();
        let c = check_release(&f, &current_fixture(), &Cancel::default())
            .unwrap()
            .unwrap();
        if oversized {
            f.assets.get_mut(&2).unwrap().push(1);
        } else {
            f.assets.get_mut(&2).unwrap().pop();
        }
        assert_eq!(
            transaction::stage(&install, &c, &f, &Cancel::default()),
            Err(Error::Size)
        );
        assert!(transaction::pending(&install).unwrap().is_none());
    }
    struct CancelDownload(Fixture);
    impl Transport for CancelDownload {
        fn releases(&self, c: &Cancel) -> Result<Vec<Release>> {
            self.0.releases(c)
        }
        fn asset(&self, id: u64, w: &mut dyn Write, limit: u64, c: &Cancel) -> Result<()> {
            if id == 2 {
                w.write_all(b"MZ").unwrap();
                c.0.store(true, Ordering::Release);
                c.check()
            } else {
                self.0.asset(id, w, limit, c)
            }
        }
    }
    let tmp = Temp::new();
    let install = tmp.install();
    let f = CancelDownload(fixture());
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        transaction::stage(&install, &c, &f, &Cancel::default()),
        Err(Error::Cancelled)
    );
    assert!(transaction::pending(&install).unwrap().is_none());
}
#[test]
#[ignore = "owned provider/race subprocess; use testing/run_updater_tests.py"]
fn cancel_between_check_and_chunk_is_nonretryable_and_cleans_provider_and_busy() {
    use std::time::{Duration, Instant};
    if std::env::var_os("HARMONY_CANCEL_RACE_CHILD").is_none() {
        let mut cmd = owned_command(std::env::current_exe().unwrap());
        cmd.args(["--exact", "harmony_update::tests::cancel_between_check_and_chunk_is_nonretryable_and_cleans_provider_and_busy", "--include-ignored", "--nocapture"])
            .env("HARMONY_CANCEL_RACE_CHILD", "1");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_4000);
        }
        let mut child = cmd.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("write_all spun after cancellation instead of timely provider/busy cleanup");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        return;
    }
    struct Race(Fixture);
    impl Transport for Race {
        fn releases(&self, c: &Cancel) -> Result<Vec<Release>> {
            self.0.releases(c)
        }
        fn asset(&self, id: u64, w: &mut dyn Write, limit: u64, c: &Cancel) -> Result<()> {
            if id != 2 {
                return self.0.asset(id, w, limit, c);
            }
            let cancel = c.clone();
            gh::BEFORE_WRITE.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || cancel.0.store(true, Ordering::Release)))
            });
            let mut cmd = owned_command(std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap());
            cmd.arg("chunk-and-sleep");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_4000);
            }
            let result = gh::run_command(cmd, w, limit, c, Duration::from_secs(1), Error::Network);
            assert_eq!(
                result,
                Err(Error::Cancelled),
                "runner must translate the writer cancellation truthfully"
            );
            result
        }
    }
    let tmp = Temp::new();
    let install = tmp.install();
    let controller = fixture_controller(Ok(install.clone()));
    let start = Instant::now();
    assert_eq!(
        controller.check(&Race(fixture()), true),
        Err(Error::Cancelled)
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(!controller.busy());
    assert!(transaction::pending(&install).unwrap().is_none());
    assert_eq!(std::fs::read(install).unwrap(), b"old fixture executable");
    controller.check(&fixture(), false).unwrap();
}
#[test]
fn filesystem_locks_exclude_other_update_transactions() {
    let tmp = Temp::new();
    let dir = safe_fs::Dir::open(&tmp.0).unwrap();
    let first = dir.lock().unwrap();
    assert!(matches!(dir.lock(), Err(Error::Busy)));
    drop(first);
    assert!(dir.lock().is_ok());
}
#[test]
fn windows_and_linux_path_tokens_fail_closed() {
    for n in [
        "..",
        ".",
        "../payload",
        "/tmp/payload",
        "C:payload",
        "payload:ads",
        "payload\\x",
        "NUL.exe",
        "COM1",
        "x.",
        "",
    ] {
        assert!(safe_fs::leaf(n).is_err(), "{n}");
    }
    assert!(safe_fs::Dir::open(std::path::Path::new("relative")).is_err());
}
#[test]
#[ignore = "actual inherited OS capabilities and owned parents; use runner"]
fn launch_bound_parent_capability_rejects_wrong_identity_and_handles_early_exit() {
    use std::time::{Duration, Instant};
    let tmp = Temp::new();
    for mode in ["valid", "wrong-birth", "wrong-pid", "early"] {
        let marker = tmp.0.join(format!("{mode}.txt"));
        let mut cmd = owned_command(std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap());
        cmd.arg("lease-parent").arg(&marker).arg(mode);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_4000);
        }
        let mut parent = cmd.spawn().unwrap();
        if mode == "valid" {
            std::thread::sleep(Duration::from_millis(80));
            assert!(!marker.exists(), "must wait for its owned parent");
        }
        assert!(parent.wait().unwrap().success());
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() {
            assert!(Instant::now() < deadline, "capability child failed: {mode}");
            std::thread::sleep(Duration::from_millis(20));
        }
        let expected = if mode == "wrong-birth"
            || mode == "wrong-pid"
            || (cfg!(target_os = "linux") && mode == "early")
        {
            b"rejected".as_slice()
        } else {
            b"exited".as_slice()
        };
        assert_eq!(std::fs::read(&marker).unwrap(), expected, "{mode}");
    }
}
#[test]
#[ignore = "owned already-exited parent; use runner"]
fn bare_pid_after_parent_exit_is_not_an_identity_bound_handoff() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let a = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    let mut cmd = owned_command(std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap());
    cmd.args(["sleep", "1"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_4000);
    }
    let mut parent = cmd.spawn().unwrap();
    let pid = parent.id();
    parent.wait().unwrap();
    let args = [
        "--harmony-update-helper".into(),
        install.as_os_str().into(),
        a.into(),
        pid.to_string().into(),
    ];
    assert!(
        helper::cli_main(&args).unwrap().is_err(),
        "bare exited PID is not evidence of a spawning parent"
    );
    assert_eq!(std::fs::read(install).unwrap(), b"old fixture executable");
}
#[test]
#[ignore = "owned parent process plus transaction race seam; use runner"]
fn stale_helper_cannot_activate_or_fail_a_replacement_transaction() {
    for replacement_armed in [true, false] {
        let tmp = Temp::new();
        let install = tmp.install();
        let f = fixture();
        let c = check_release(&f, &current_fixture(), &Cancel::default())
            .unwrap()
            .unwrap();
        let a = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
        transaction::arm(&install, true).unwrap();
        let mut parent = owned_command(std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap())
            .args(["sleep", "1"])
            .spawn()
            .unwrap();
        assert!(parent.wait().unwrap().success());
        let path = install.clone();
        helper::BEFORE_ACTIVATE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                transaction::discard(&path).unwrap();
                let f = fixture();
                let c = check_release(&f, &current_fixture(), &Cancel::default())
                    .unwrap()
                    .unwrap();
                transaction::stage(&path, &c, &f, &Cancel::default()).unwrap();
                transaction::arm(&path, replacement_armed).unwrap();
            }))
        });
        // Unit scheduling seam is after the real parent wait. The separate
        // owned helper test exercises OS launch/wait as well.
        assert_eq!(helper::after_parent(&install, &a), Err(Error::Conflict));
        let b = transaction::pending(&install).unwrap().unwrap();
        assert_eq!(
            b.1.phase,
            transaction::Phase::Staged,
            "stale failure must not mutate B"
        );
        assert_eq!(b.1.armed, replacement_armed);
        assert_eq!(std::fs::read(&install).unwrap(), b"old fixture executable");
        assert_eq!(
            std::fs::read(tmp.0.join(".harmony-updates").join(b.0).join("payload")).unwrap(),
            f.assets[&2]
        );
    }
}
#[test]
fn controller_persists_staged_state_and_does_not_arm_automatically() {
    let tmp = Temp::new();
    let install = tmp.install();
    let controller = fixture_controller(Ok(install.clone()));
    controller.check(&fixture(), true).unwrap();
    assert!(controller.snapshot().staged.is_some());
    assert!(!controller.snapshot().staged.unwrap().1.armed);
    assert!(controller.handoff_on_full_exit().is_ok()); // No helper is spawned.
    controller.arm(true).unwrap();
    controller.arm(false).unwrap();
    let restored = fixture_controller(Ok(install));
    restored.load_pending().unwrap();
    assert!(!restored.snapshot().staged.unwrap().1.armed);
    restored.discard().unwrap();
    assert!(restored.snapshot().staged.is_none());
    let preferences: controller::Preferences = serde_json::from_str("{}").unwrap();
    assert!(!preferences.check_on_startup);
    assert!(!preferences.download_automatically);
}
#[test]
fn concurrent_checks_share_one_guard_and_cancel_releases_it() {
    use std::sync::{Mutex, mpsc};
    struct Gate {
        entered: mpsc::Sender<()>,
        leave: Mutex<mpsc::Receiver<()>>,
    }
    impl Transport for Gate {
        fn releases(&self, c: &Cancel) -> Result<Vec<Release>> {
            self.entered.send(()).unwrap();
            self.leave.lock().unwrap().recv().unwrap();
            c.check()?;
            Ok(vec![])
        }
        fn asset(&self, _: u64, _: &mut dyn Write, _: u64, _: &Cancel) -> Result<()> {
            unreachable!()
        }
    }
    let (entered, waiting) = mpsc::channel();
    let (leave, release) = mpsc::channel();
    let controller = Arc::new(fixture_controller(Err(Error::Unsupported)));
    let worker = controller.clone();
    let thread = std::thread::spawn(move || {
        worker.check(
            &Gate {
                entered,
                leave: Mutex::new(release),
            },
            false,
        )
    });
    waiting
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert_eq!(controller.check(&fixture(), false), Err(Error::Busy));
    controller.cancel();
    leave.send(()).unwrap();
    assert_eq!(thread.join().unwrap(), Err(Error::Cancelled));
    assert!(!controller.busy());
    controller.check(&fixture(), false).unwrap();
}
#[test]
fn gh_commands_never_use_manifest_hosts_or_expose_token_arguments() {
    for endpoint in [
        "https://evil.invalid/x",
        "//evil.invalid/x",
        "repos/other/repo/releases/assets/1",
        "repos/TimeShallHalt/harmony/releases/assets/01",
        "repos/TimeShallHalt/harmony/releases/assets/0",
    ] {
        assert!(gh::api_command(endpoint, true).is_err());
    }
    let command = gh::api_command("repos/TimeShallHalt/harmony/releases/assets/2", true).unwrap();
    let args: Vec<_> = command.get_args().map(|s| s.to_string_lossy()).collect();
    assert!(args.iter().any(|s| s == "github.com"));
    assert!(
        !args
            .iter()
            .any(|s| s.contains("token") || s.contains("Authorization"))
    );
    for key in ["GH_TOKEN", "GITHUB_TOKEN", "GH_DEBUG"] {
        assert!(command.get_envs().any(|(k, v)| k == key && v.is_none()));
    }
    assert!(Error::MissingGh.to_string().contains("official GitHub CLI"));
    assert!(Error::Auth.to_string().contains("offline"));
}
#[test]
#[ignore = "standalone owned process fixture; use testing/run_updater_tests.py"]
fn provider_process_limits_failure_cancel_timeout_and_missing_cli_are_real() {
    use std::time::{Duration, Instant};
    let program = std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").expect("runner fixture path");
    let make = |mode: &str, value: &str| {
        let mut c = owned_command(&program);
        c.args([mode, value]);
        c
    };
    let c = Cancel::default();
    assert_eq!(
        gh::run_command(
            make("bytes", "16"),
            &mut Vec::new(),
            8,
            &c,
            Duration::from_secs(2),
            Error::Network
        ),
        Err(Error::Size)
    );
    assert_eq!(
        gh::run_command(
            make("fail", "0"),
            &mut Vec::new(),
            8,
            &c,
            Duration::from_secs(2),
            Error::Auth
        ),
        Err(Error::Auth)
    );
    assert_eq!(
        gh::run_command(
            make("sleep", "4000"),
            &mut Vec::new(),
            8,
            &c,
            Duration::from_millis(100),
            Error::Network
        ),
        Err(Error::Timeout)
    );
    let cancel = c.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        cancel.0.store(true, Ordering::Release);
    });
    let start = Instant::now();
    assert_eq!(
        gh::run_command(
            make("sleep", "4000"),
            &mut Vec::new(),
            8,
            &c,
            Duration::from_secs(3),
            Error::Network
        ),
        Err(Error::Cancelled)
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    t.join().unwrap();
    assert_eq!(
        gh::run_command(
            owned_command("__no_such_harmony_gh_fixture__"),
            &mut Vec::new(),
            8,
            &Cancel::default(),
            Duration::from_secs(1),
            Error::Network
        ),
        Err(Error::MissingGh)
    );
}
#[cfg(target_os = "linux")]
fn wait_owned_marker(path: &std::path::Path) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "owned fixture marker missing: {}",
            path.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::fs::read_to_string(path).unwrap()
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "actual kernel-bound owned helper replaced A/B at handoff; use runner"]
fn owned_stale_helper_cannot_activate_or_fail_replacement_b() {
    for armed in [true, false] {
        let tmp = Temp::new();
        let install = tmp.0.join("harmony.exe");
        std::fs::copy(
            std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap(),
            &install,
        )
        .unwrap();
        let old = std::fs::read(&install).unwrap();
        let f = fixture();
        let c = check_release(&f, &current_fixture(), &Cancel::default())
            .unwrap()
            .unwrap();
        let a = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
        transaction::arm(&install, true).unwrap();
        let ready = tmp.0.join("ready");
        let release = tmp.0.join("release");
        let result = tmp.0.join("helper-result");
        let mut parent = owned_command(&install)
            .arg("handoff-hold")
            .arg(&install)
            .arg(&ready)
            .arg(&release)
            .env("HARMONY_FIXTURE_HELPER_RESULT", &result)
            .spawn()
            .unwrap();
        assert_eq!(wait_owned_marker(&ready), "ready");
        let updates = safe_fs::Dir::open(&tmp.0.join(".harmony-updates")).unwrap();
        {
            let _lock = updates.lock().unwrap();
            updates.remove("pending.json").unwrap();
        }
        let b = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
        assert_ne!(a, b);
        transaction::arm(&install, armed).unwrap();
        let before =
            std::fs::read(tmp.0.join(".harmony-updates").join(&b).join("plan.json")).unwrap();
        std::fs::write(&release, b"natural fixture exit").unwrap();
        assert!(parent.wait().unwrap().success());
        assert_eq!(wait_owned_marker(&result), "Err(Conflict)");
        assert_eq!(
            std::fs::read(tmp.0.join(".harmony-updates").join(&b).join("plan.json")).unwrap(),
            before,
            "stale activation/failure must not mutate B"
        );
        assert_eq!(std::fs::read(&install).unwrap(), old);
    }
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "normal Controller plus owned helper after real journal cut; use runner"]
fn owned_recovery_handoff_uses_controller_authorization_and_natural_parent_exit() {
    let tmp = Temp::new();
    let install = tmp.0.join("harmony.exe");
    std::fs::copy(
        std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap(),
        &install,
    )
    .unwrap();
    let old = std::fs::read(&install).unwrap();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    cut_activation(&install, "journal");
    let result = tmp.0.join("helper-result");
    let mut parent = owned_command(&install)
        .arg("recover-handoff")
        .arg(&install)
        .env("HARMONY_FIXTURE_HELPER_RESULT", &result)
        .spawn()
        .unwrap();
    assert!(parent.wait().unwrap().success());
    assert_eq!(wait_owned_marker(&result), "Ok(())");
    assert_eq!(
        transaction::pending(&install).unwrap().unwrap().1.phase,
        transaction::Phase::Installed
    );
    assert_eq!(std::fs::read(&install).unwrap(), f.assets[&2]);
    assert_eq!(
        std::fs::read(tmp.0.join(".harmony-updates").join(txn).join("rollback")).unwrap(),
        old
    );
}
#[test]
#[ignore = "owned image rename/substitution, no substituted image executed; use runner"]
fn running_image_identity_rejects_same_hash_path_substitution() {
    for mode in ["identify-substituted", "identify-current"] {
        let tmp = Temp::new();
        let install = tmp.0.join("harmony.exe");
        let marker = tmp.0.join("identity.txt");
        let program = std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap();
        std::fs::copy(&program, &install).unwrap();
        let mut cmd = owned_command(&install);
        cmd.arg(mode)
            .arg(&program)
            .arg(&marker)
            .env("APPIMAGE", &install);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_4000);
        }
        assert!(cmd.status().unwrap().success());
        let expected = if mode == "identify-current" && cfg!(target_os = "linux") {
            b"accepted".as_slice()
        } else {
            b"rejected".as_slice()
        };
        assert_eq!(
            std::fs::read(&marker).unwrap(),
            expected,
            "kernel identity, not another hash of the supplied path: {mode}"
        );
    }
}
#[test]
#[ignore = "standalone owned helper fixture; use testing/run_updater_tests.py"]
fn native_helper_waits_for_its_owned_parent_then_installs_without_relaunch() {
    let tmp = Temp::new();
    let install = tmp.0.join("harmony.exe");
    let program = std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").expect("runner fixture path");
    std::fs::copy(&program, &install).unwrap();
    let old = std::fs::read(&install).unwrap();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    let mut parent = owned_command(&install)
        .arg("handoff")
        .arg(&install)
        .spawn()
        .unwrap();
    // While its parent deliberately remains alive, helper cannot publish.
    std::thread::sleep(std::time::Duration::from_millis(80));
    assert_eq!(std::fs::read(&install).unwrap(), old);
    assert!(parent.wait().unwrap().success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        if transaction::pending(&install).unwrap().unwrap().1.phase == transaction::Phase::Installed
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "helper never completed: {:?}",
            transaction::pending(&install)
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(std::fs::read(&install).unwrap(), f.assets[&2]);
    let rollback = tmp.0.join(".harmony-updates").join(txn).join("rollback");
    assert_eq!(std::fs::read(rollback).unwrap(), old);
}

#[test]
fn staging_is_owned_and_not_writable_by_other_users() {
    let tmp = Temp::new();
    let install = tmp.install();
    let staging = tmp.0.join(".harmony-updates");
    std::fs::create_dir(&staging).unwrap();
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o777)).unwrap();
    }
    #[cfg(windows)]
    {
        let output = owned_command("icacls.exe")
            .arg(&staging)
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        assert!(output.status.success());
    }
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    assert!(
        transaction::stage(&install, &c, &f, &Cancel::default()).is_err(),
        "another user must not be able to write update staging"
    );
    assert_eq!(std::fs::read(install).unwrap(), b"old fixture executable");
}
#[cfg(windows)]
#[test]
fn windows_object_inherit_only_write_grants_are_rejected_on_actual_child_files() {
    let tmp = Temp::new();
    let staging = tmp.0.join("owned-staging");
    std::fs::create_dir(&staging).unwrap();
    let output = owned_command("icacls.exe")
        .arg(&staging)
        .args(["/grant", "*S-1-1-0:(OI)(IO)W"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    // This ACE doesn't grant effective directory writes. NTFS does turn it
    // into an effective write ACE on the child; inspect those actual objects.
    let dir = safe_fs::Dir::open(&staging).unwrap();
    for name in ["payload", "plan.json", "helper.exe"] {
        let path = staging.join(name);
        std::fs::write(&path, b"fixture").unwrap();
        assert!(
            dir.file(name, false).is_err(),
            "accepted inherited child write ACL: {name}"
        );
    }
    assert!(
        safe_fs::Dir::open(&tmp.0)
            .unwrap()
            .child("owned-staging", false)
            .is_err(),
        "unsafe prospective inheritance must also fail before creating data"
    );
    let safe = safe_fs::Dir::open(&tmp.0)
        .unwrap()
        .child("safe-staging", true)
        .unwrap();
    for name in ["payload", "plan.json", "helper.exe"] {
        drop(safe.file(name, true).unwrap());
        assert!(
            safe.file(name, false).is_ok(),
            "private valid child rejected"
        );
    }
}
#[cfg(windows)]
#[test]
fn windows_junction_staging_cannot_redirect_writes() {
    let tmp = Temp::new();
    let outside = Temp::new();
    let install = tmp.install();
    let output = owned_command("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(tmp.0.join(".harmony-updates"))
        .arg(&outside.0)
        .output()
        .unwrap();
    assert!(output.status.success());
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    assert!(transaction::stage(&install, &c, &f, &Cancel::default()).is_err());
    assert_eq!(std::fs::read_dir(&outside.0).unwrap().count(), 0);
}
#[cfg(target_os = "linux")]
#[test]
fn linux_links_fifos_and_sockets_are_rejected_without_blocking_or_following() {
    use std::{
        ffi::CString,
        os::unix::{fs::symlink, net::UnixListener},
    };
    let tmp = Temp::new();
    let outside = Temp::new();
    let dir = safe_fs::Dir::open(&tmp.0).unwrap();
    std::fs::write(outside.0.join("target"), b"outside").unwrap();
    symlink(outside.0.join("target"), tmp.0.join("link")).unwrap();
    assert!(dir.file("link", false).is_err());
    let fifo = CString::new(tmp.0.join("fifo").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(dir.file("fifo", false).is_err());
    let _socket = UnixListener::bind(tmp.0.join("socket")).unwrap();
    assert!(dir.file("socket", false).is_err());
    symlink(&outside.0, tmp.0.join("dirlink")).unwrap();
    assert!(dir.child("dirlink", false).is_err());
    assert_eq!(std::fs::read(outside.0.join("target")).unwrap(), b"outside");
}
#[cfg(target_os = "linux")]
#[test]
fn linux_directory_and_file_handles_remain_authoritative_after_name_swaps() {
    use std::{io::Read, os::unix::fs::symlink};
    let tmp = Temp::new();
    let outside = Temp::new();
    let base = tmp.0.join("base");
    std::fs::create_dir(&base).unwrap();
    std::fs::write(base.join("payload"), b"owned").unwrap();
    std::fs::write(outside.0.join("payload"), b"outside").unwrap();
    let dir = safe_fs::Dir::open(&base).unwrap();
    let mut file = dir.file("payload", false).unwrap();
    std::fs::rename(&base, tmp.0.join("moved")).unwrap();
    symlink(&outside.0, &base).unwrap();
    assert_eq!(dir.read("payload", 128).unwrap(), b"owned");
    dir.atomic("new.json", b"owned state").unwrap();
    assert!(!outside.0.join("new.json").exists());
    std::fs::rename(tmp.0.join("moved/payload"), tmp.0.join("moved/old")).unwrap();
    symlink(outside.0.join("payload"), tmp.0.join("moved/payload")).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"owned");
}

#[test]
#[ignore = "standalone owned stamp fixture; use testing/run_updater_tests.py"]
fn build_stamp_emission_uses_real_git_identity_and_rejects_bad_release_ids() {
    let tmp = Temp::new();
    let program = std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap();
    let source = std::path::Path::new(file!())
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let run = |id: &str| {
        owned_command(&program)
            .arg("emit-stamp")
            .env("CARGO_MANIFEST_DIR", source)
            .env("CARGO_PKG_VERSION", env!("CARGO_PKG_VERSION"))
            .env("CARGO_CFG_TARGET_OS", std::env::consts::OS)
            .env("CARGO_CFG_TARGET_ARCH", std::env::consts::ARCH)
            .env("OUT_DIR", &tmp.0)
            .env("HARMONY_BUILD_ID", id)
            .output()
            .unwrap()
    };
    let emitted = run("0");
    assert!(
        emitted.status.success(),
        "{}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    let bytes = std::fs::read(tmp.0.join("harmony-build-info.json")).unwrap();
    let info: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let actual = build_stamp::inspect(source, 0).unwrap();
    assert_eq!(info["commit"], actual.commit);
    assert_eq!(info["dirty"], actual.dirty);
    assert_eq!(info["build_id"], 0);
    assert_eq!(info["architecture"], std::env::consts::ARCH);
    assert!(!run("01").status.success());
    assert_eq!(
        std::fs::read(tmp.0.join("harmony-build-info.json")).unwrap(),
        bytes
    );
    if actual.dirty {
        assert!(!run("42").status.success());
    }
}
#[test]
#[ignore = "owned crash-cut subprocess; use runner"]
fn crash_cut_child() {
    let Some(path) = std::env::var_os("HARMONY_CRASH_INSTALL") else {
        return;
    };
    let install = std::path::PathBuf::from(path);
    let cut = std::env::var("HARMONY_CRASH_CUT").unwrap();
    if let Some((point, to)) = cut.split_once(':') {
        let point = point.to_owned();
        let to = to.to_owned();
        safe_fs::RENAME_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |p, name| {
                if p == point && name == to {
                    std::process::exit(92);
                }
            }))
        });
    }
    if cut == "displaced" {
        transaction::AFTER_DISPLACE
            .with(|hook| *hook.borrow_mut() = Some(Box::new(|| std::process::exit(92))));
    }
    if cut == "published" {
        transaction::AFTER_PUBLISH
            .with(|hook| *hook.borrow_mut() = Some(Box::new(|| std::process::exit(92))));
    }
    transaction::activate_with_hook(&install, || {
        if cut == "journal" {
            std::process::exit(92);
        }
    })
    .unwrap();
    panic!("crash cut was not reached");
}
fn cut_activation(install: &std::path::Path, cut: &str) {
    let mut cmd = owned_command(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "harmony_update::tests::crash_cut_child",
        "--ignored",
    ])
    .env("HARMONY_CRASH_INSTALL", install)
    .env("HARMONY_CRASH_CUT", cut);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_4000);
    }
    assert_eq!(cmd.status().unwrap().code(), Some(92));
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "six actual exit cuts + stable normal entry + production Controller/helper; use runner"]
fn stable_normal_entry_recovers_every_publication_and_durability_cut() {
    for cut in [
        "before:displaced",
        "visible:displaced",
        "durable:displaced",
        "before:harmony-payload",
        "visible:harmony-payload",
        "durable:harmony-payload",
    ] {
        let tmp = Temp::new();
        let (entry, install) = create_owned_layout(&tmp);
        let entry_bytes = std::fs::read(&entry).unwrap();
        let old = std::fs::read(&install).unwrap();
        let f = native_payload_fixture();
        let c = check_release(&f, &current_fixture(), &Cancel::default())
            .unwrap()
            .unwrap();
        let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
        transaction::arm(&install, true).unwrap();
        cut_activation(&install, cut);
        let published = cut == "visible:harmony-payload" || cut == "durable:harmony-payload";
        let absent = cut != "before:displaced" && !published;
        assert_eq!(!install.exists(), absent, "cut {cut}");
        let marker = tmp.0.join("normal-result");
        let result = owned_command(&entry)
            .arg("normal-startup")
            .arg(&marker)
            .env_remove("APPIMAGE")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "entry unusable after {cut}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
        assert_eq!(value["install"], install.to_str().unwrap());
        assert_eq!(value["armed"], false, "cut {cut} retained authorization");
        assert_eq!(
            !install.exists(),
            absent,
            "startup cannot publish or blindly restore: {cut}"
        );
        assert_eq!(
            std::fs::read(&entry).unwrap(),
            entry_bytes,
            "stable entry must never change"
        );
        let p = transaction::pending(&install).unwrap().unwrap().1;
        if published {
            assert_eq!(p.phase, transaction::Phase::Installed);
            assert!(value["phase"].is_null());
        } else {
            assert_eq!(p.phase, transaction::Phase::Applying, "never reset journal");
            assert_eq!(value["phase"], "Applying");
            let ready = tmp.0.join("ready");
            let release = tmp.0.join("release");
            let result_path = tmp.0.join("helper-result");
            let mut parent = owned_command(&entry)
                .arg("normal-authorize-hold")
                .arg(&marker)
                .arg(&ready)
                .arg(&release)
                .env_remove("APPIMAGE")
                .env("HARMONY_FIXTURE_HELPER_RESULT", &result_path)
                .spawn()
                .unwrap();
            assert_eq!(
                wait_owned_marker(&ready),
                "authorized helper ready; parent alive"
            );
            assert_eq!(
                !install.exists(),
                absent,
                "helper published before natural exit: {cut}"
            );
            assert!(
                !result_path.exists(),
                "helper returned before parent exited"
            );
            std::fs::write(&release, b"natural owned-process exit").unwrap();
            assert!(parent.wait().unwrap().success());
            assert_eq!(
                wait_owned_marker(&result_path),
                "Ok(())",
                "recovery helper at {cut}"
            );
            assert_eq!(
                transaction::pending(&install).unwrap().unwrap().1.phase,
                transaction::Phase::Installed
            );
        }
        assert_eq!(std::fs::read(&install).unwrap(), f.assets[&2]);
        assert_eq!(
            std::fs::read(tmp.0.join(".harmony-updates").join(&txn).join("rollback")).unwrap(),
            old
        );
        // Only this second explicit user launch runs the new payload; neither
        // startup, authorization nor the helper relaunches it.
        let result = owned_command(&entry)
            .arg("normal-startup")
            .arg(&marker)
            .env_remove("APPIMAGE")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "next normal launch after {cut}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "stable entry must not silently roll back an Installed selection; use runner"]
fn stable_entry_rejects_previous_bytes_after_completed_installation() {
    let tmp = Temp::new();
    let (entry, install) = create_owned_layout(&tmp);
    let old = std::fs::read(&install).unwrap();
    let f = native_payload_fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    activate_pending(&install).unwrap();
    std::fs::write(&install, old).unwrap();
    let marker = tmp.0.join("must-not-run");
    let result = owned_command(&entry)
        .arg("normal-startup")
        .arg(&marker)
        .env_remove("APPIMAGE")
        .output()
        .unwrap();
    assert!(
        !result.status.success(),
        "Installed requires NEW authoritative bytes; never silently roll back"
    );
    assert!(!marker.exists());
    assert_eq!(
        transaction::pending(&install).unwrap().unwrap().1.phase,
        transaction::Phase::Installed
    );
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "failed absent-target recovery must still launch verified old app; use runner"]
fn stable_entry_survives_recovery_payload_failure_without_blind_rollback_or_phase_reset() {
    let tmp = Temp::new();
    let (entry, install) = create_owned_layout(&tmp);
    let f = native_payload_fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    cut_activation(&install, "displaced");
    let controller = fixture_controller(Ok(install.clone()));
    controller.load_pending().unwrap();
    controller.arm(true).unwrap();
    let payload = tmp.0.join(".harmony-updates").join(txn).join("payload");
    assert!(
        transaction::activate_with_hook(&install, || std::fs::remove_file(payload).unwrap())
            .is_err()
    );
    let p = transaction::pending(&install).unwrap().unwrap().1;
    assert_eq!(
        p.phase,
        transaction::Phase::Applying,
        "absent payload must retain recoverable old selection"
    );
    assert!(!p.armed);
    assert!(
        !install.exists(),
        "no blind restore after a failed publication attempt"
    );
    let marker = tmp.0.join("failure-status");
    let result = owned_command(&entry)
        .arg("normal-startup-status")
        .arg(&marker)
        .env_remove("APPIMAGE")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "old app remains launchable: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(marker).unwrap()).unwrap();
    assert_eq!(value["phase"], "Applying");
    assert!(
        value["load_error"].as_str().is_some(),
        "real Controller error, not fake success"
    );
    assert!(!install.exists());
}
#[test]
#[ignore = "owned evacuation crash cut; documents an unresolved layout blocker"]
fn evacuation_crash_retains_data_but_normal_shortcut_recovery_is_unsupported() {
    let tmp = Temp::new();
    let install = tmp.0.join("harmony.exe");
    std::fs::copy(
        std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").unwrap(),
        &install,
    )
    .unwrap();
    let old = std::fs::read(&install).unwrap();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    cut_activation(&install, "displaced");
    assert!(!install.exists());
    let saved = tmp.0.join(".harmony-updates").join(txn);
    assert_eq!(std::fs::read(saved.join("displaced")).unwrap(), old);
    assert_eq!(std::fs::read(saved.join("rollback")).unwrap(), old);
    assert_eq!(std::fs::read(saved.join("payload")).unwrap(), f.assets[&2]);
    assert_eq!(
        owned_command(&install)
            .arg("normal-startup")
            .arg(tmp.0.join("never-launched"))
            .spawn()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound,
        "preserved REAL portable failure: normal entry is absent, retained bytes are not recovery"
    );
    let restored = fixture_controller(Ok(install.clone()));
    assert_eq!(restored.load_pending(), Err(Error::Unsupported));
    assert_eq!(
        restored.snapshot().staged.as_ref().unwrap().1.phase,
        transaction::Phase::Applying
    );
    assert!(!restored.snapshot().staged.as_ref().unwrap().1.armed);
    assert_eq!(restored.arm(true), Err(Error::Unsupported));
    assert_eq!(restored.discard(), Err(Error::Busy));
    assert!(
        !install.exists(),
        "startup must not blindly restore or run a journal-supplied path"
    );
}
#[test]
#[ignore = "owned crash-cut subprocess; use runner"]
fn normal_controller_startup_reconciles_completed_publication_without_replacement() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    cut_activation(&install, "published");
    assert_eq!(
        transaction::pending(&install).unwrap().unwrap().1.phase,
        transaction::Phase::Applying
    );
    let restored = fixture_controller(Ok(install.clone()));
    restored.load_pending().unwrap();
    assert!(
        restored.snapshot().staged.is_none(),
        "normal startup must reconcile authoritative completed bytes"
    );
    assert_eq!(
        transaction::pending(&install).unwrap().unwrap().1.phase,
        transaction::Phase::Installed
    );
    assert_eq!(std::fs::read(install).unwrap(), f.assets[&2]);
}
#[test]
#[ignore = "owned crash-cut subprocess; use runner"]
fn normal_controller_requires_new_authorization_for_unfinished_applying() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    cut_activation(&install, "journal");
    let restored = fixture_controller(Ok(install.clone()));
    restored.load_pending().unwrap();
    assert!(
        !restored.snapshot().staged.unwrap().1.armed,
        "startup must revoke interrupted authorization, not blindly reset the journal"
    );
    restored.handoff_on_full_exit().unwrap();
    assert_eq!(std::fs::read(&install).unwrap(), b"old fixture executable");
    restored.arm(true).unwrap();
    assert_eq!(
        restored.snapshot().staged.unwrap().1.phase,
        transaction::Phase::Applying
    );
    restored.arm(false).unwrap();
}
#[test]
fn recovery_finishes_an_interrupted_journal_after_a_verified_atomic_publish() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    activate_pending(&install).unwrap();
    let mut plan = transaction::pending(&install).unwrap().unwrap().1;
    plan.phase = transaction::Phase::Applying;
    plan.armed = true;
    std::fs::write(
        tmp.0.join(".harmony-updates").join(&txn).join("plan.json"),
        serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
    activate_pending(&install).unwrap();
    assert_eq!(
        transaction::pending(&install).unwrap().unwrap().1.phase,
        transaction::Phase::Installed
    );
    assert_eq!(std::fs::read(&install).unwrap(), f.assets[&2]);
    assert_eq!(
        std::fs::read(tmp.0.join(".harmony-updates").join(txn).join("rollback")).unwrap(),
        b"old fixture executable"
    );
}

#[test]
fn harmony_identity_is_distinct_from_upstream_and_diagnostics_are_allowlisted() {
    let text = BuildInfo::current().diagnostics();
    assert!(
        text.contains(&format!("Harmony {}", env!("CARGO_PKG_VERSION"))),
        "running package identity: {text}"
    );
    assert_ne!(
        env!("CARGO_PKG_VERSION"),
        "0.3.0",
        "custom builds need an independent Harmony version"
    );
    assert!(text.contains("Upstream: Yapfast 0.3.0"));
    assert!(text.contains("GPL-3.0-or-later"));
    assert!(!text.contains("TOKEN"));
}
