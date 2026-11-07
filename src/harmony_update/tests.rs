use super::*;
#[path = "../../scripts/harmony_build_stamp.rs"]
mod build_stamp;
use release::*;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Write};
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
    assert!(transaction::activate(&install).is_err());
    transaction::arm(&install, true).unwrap();
    transaction::activate(&install).unwrap();
    assert_eq!(std::fs::read(&install).unwrap(), f.assets[&2]);
    let rollback = tmp.0.join(".harmony-updates").join(txn).join("rollback");
    assert_eq!(std::fs::read(rollback).unwrap(), b"old fixture executable");
    assert_eq!(
        transaction::pending(&install).unwrap().unwrap().1.phase,
        transaction::Phase::Installed
    );
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
    assert_eq!(transaction::activate(&install), Err(Error::Conflict));
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
    assert!(preferences.check_on_startup);
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
    use std::{
        process::Command,
        time::{Duration, Instant},
    };
    let program = std::env::var_os("HARMONY_TEST_PROCESS_FIXTURE").expect("runner fixture path");
    let make = |mode: &str, value: &str| {
        let mut c = Command::new(&program);
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
            Command::new("__no_such_harmony_gh_fixture__"),
            &mut Vec::new(),
            8,
            &Cancel::default(),
            Duration::from_secs(1),
            Error::Network
        ),
        Err(Error::MissingGh)
    );
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
    let mut parent = std::process::Command::new(&install)
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
        let output = std::process::Command::new("icacls.exe")
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
fn windows_junction_staging_cannot_redirect_writes() {
    let tmp = Temp::new();
    let outside = Temp::new();
    let install = tmp.install();
    let output = std::process::Command::new("cmd.exe")
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
        std::process::Command::new(&program)
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
fn recovery_finishes_an_interrupted_journal_after_a_verified_atomic_publish() {
    let tmp = Temp::new();
    let install = tmp.install();
    let f = fixture();
    let c = check_release(&f, &current_fixture(), &Cancel::default())
        .unwrap()
        .unwrap();
    let txn = transaction::stage(&install, &c, &f, &Cancel::default()).unwrap();
    transaction::arm(&install, true).unwrap();
    transaction::activate(&install).unwrap();
    let mut plan = transaction::pending(&install).unwrap().unwrap().1;
    plan.phase = transaction::Phase::Applying;
    plan.armed = true;
    std::fs::write(
        tmp.0.join(".harmony-updates").join(&txn).join("plan.json"),
        serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
    transaction::activate(&install).unwrap();
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
