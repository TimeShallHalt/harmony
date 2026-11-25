use super::*;

thread_local! {
    // One-shot, per-test-thread seams; never compiled into the application.
    static BEFORE_LOGO_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {
        std::cell::RefCell::new(None)
    };
    static BEFORE_ASSET_FINALIZE: std::cell::RefCell<Option<Box<dyn FnOnce(&Path)>>> = const {
        std::cell::RefCell::new(None)
    };
}

pub(super) fn before_logo_read() {
    if let Some(hook) = BEFORE_LOGO_READ.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
}

pub(super) fn before_asset_finalize(path: &Path) {
    if let Some(hook) = BEFORE_ASSET_FINALIZE.with(|slot| slot.borrow_mut().take()) {
        hook(path);
    }
}

#[cfg(windows)]
#[test]
fn logo_finalization_failure_does_not_replace_an_existing_export() {
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let path = write_logo_pack(source.path(), "logo.png", &png(2, 3));
    let applied = Applied::load(&path).unwrap();
    let output = dir.path().join("export.harmony.json");
    applied.export(&output).unwrap();
    let old_json = std::fs::read(&output).unwrap();
    let old_export = Applied::load(&output).unwrap();
    BEFORE_ASSET_FINALIZE.with(|slot| {
        // Make tempfile::keep fail for real on Windows (SetFileAttributesW).
        *slot.borrow_mut() = Some(Box::new(|path| std::fs::remove_file(path).unwrap()));
    });
    let error = applied.export(&output).unwrap_err();
    assert!(!error.is_empty());
    assert!(BEFORE_ASSET_FINALIZE.with(|slot| slot.borrow().is_none()));
    assert_eq!(
        std::fs::read(&output).unwrap(),
        old_json,
        "JSON changed before logo finalization succeeded"
    );
    assert_eq!(Applied::load(&output).unwrap(), old_export);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn manifest_failure_cleans_finalized_logo_and_preserves_the_old_export() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let input = write_logo_pack(source.path(), "logo.png", &png(3, 2));
    let applied = Applied::load(&input).unwrap();
    let output = dest.path().join("export.harmony.json");
    applied.export(&output).unwrap();
    let old_json = std::fs::read(&output).unwrap();
    let old_snapshot = Applied::load(&output).unwrap();
    // Persisting over a directory is a real deterministic filesystem failure
    // on both OSes, injected only at the manifest-writer boundary.
    let blocked = dest.path().join("occupied");
    std::fs::create_dir(&blocked).unwrap();
    let mut before: Vec<_> = std::fs::read_dir(dest.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    before.sort();
    let mut new_logo = None;
    let error = applied
        .export_with_writer(&output, |path, bytes| {
            assert_eq!(path, output);
            assert_eq!(std::fs::read(path).unwrap(), old_json);
            let pack = ThemePack::parse(bytes).unwrap();
            let logo = dest.path().join(pack.logo.unwrap());
            assert_eq!(std::fs::read(&logo).unwrap(), applied.logo_png);
            #[cfg(windows)]
            {
                use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
                assert_eq!(
                    std::fs::metadata(&logo).unwrap().file_attributes() & 0x100,
                    0,
                    "logo still has FILE_ATTRIBUTE_TEMPORARY at JSON commit"
                );
                // No temporary writer may remain open when JSON is published.
                let guard = std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1)
                    .open(&logo)
                    .unwrap();
                drop(guard);
            }
            new_logo = Some(logo);
            write_atomic(&blocked, bytes)
        })
        .unwrap_err();
    assert!(!error.is_empty());
    assert!(
        !new_logo.unwrap().exists(),
        "failed manifest left a new logo behind"
    );
    assert_eq!(std::fs::read(&output).unwrap(), old_json);
    assert_eq!(Applied::load(&output).unwrap(), old_snapshot);
    let mut after: Vec<_> = std::fs::read_dir(dest.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    after.sort();
    assert_eq!(after, before, "failed export leaked a temporary file");
}

#[cfg(windows)]
#[test]
fn locked_manifest_failure_preserves_old_export_and_removes_the_new_logo() {
    use std::os::windows::fs::OpenOptionsExt;
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let input = write_logo_pack(source.path(), "logo.png", &png(3, 2));
    let applied = Applied::load(&input).unwrap();
    let output = dest.path().join("export.harmony.json");
    applied.export(&output).unwrap();
    let old_json = std::fs::read(&output).unwrap();
    let old_snapshot = Applied::load(&output).unwrap();
    let guard = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&output)
        .unwrap();
    assert!(applied.export(&output).is_err(), "locked JSON was replaced");
    drop(guard);
    assert_eq!(std::fs::read(&output).unwrap(), old_json);
    assert_eq!(Applied::load(&output).unwrap(), old_snapshot);
    assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 2);
}

fn directory_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // Junction creation needs no Developer Mode / symlink privilege.
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "junction creation failed: {output:?}"
        );
    }
}

fn remove_directory_link(path: &Path) {
    #[cfg(windows)]
    std::fs::remove_dir(path).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(path).unwrap();
}

#[test]
fn logo_file_symlinks_and_nonregular_directories_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = write_logo_pack(dir.path(), "logo.png", &png(2, 3));
    let logo = dir.path().join("logo.png");
    let target = outside.path().join("outside.png");
    std::fs::write(&target, png(4, 5)).unwrap();
    std::fs::remove_file(&logo).unwrap();
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&target, &logo);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(&target, &logo);
    match result {
        Ok(()) => {
            assert!(Applied::load(&path).is_err(), "logo symlink accepted");
            std::fs::remove_file(&logo).unwrap();
        }
        #[cfg(windows)]
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("file-symlink subcase needs Windows symlink privilege: {error}");
        }
        Err(error) => panic!("cannot create logo symlink: {error}"),
    }
    std::fs::create_dir(&logo).unwrap();
    assert!(Applied::load(&path).is_err(), "directory accepted as logo");
}

#[test]
fn intermediate_logo_links_and_selected_directory_links_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = write_logo_pack(dir.path(), "assets/logo.png", &png(2, 3));
    std::fs::write(outside.path().join("logo.png"), png(4, 5)).unwrap();
    std::fs::remove_dir_all(dir.path().join("assets")).unwrap();
    directory_link(outside.path(), &dir.path().join("assets"));
    assert!(
        Applied::load(&path).is_err(),
        "intermediate symlink/junction accepted"
    );
    remove_directory_link(&dir.path().join("assets"));

    let input = write_logo_pack(outside.path(), "logo.png", &png(2, 3));
    let linked = dir.path().join("linked-pack");
    directory_link(outside.path(), &linked);
    assert!(
        Applied::load(&linked.join(input.file_name().unwrap())).is_err(),
        "selected pack ancestor symlink/junction accepted"
    );
    remove_directory_link(&linked);
}

#[test]
fn a_held_pack_directory_cannot_be_redirected_by_replacing_its_name() {
    let parent = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let dir = parent.path().join("pack");
    let old = parent.path().join("old-pack");
    std::fs::create_dir(&dir).unwrap();
    let original = png(2, 3);
    let path = write_logo_pack(&dir, "logo.png", &original);
    std::fs::write(outside.path().join("logo.png"), png(4, 5)).unwrap();
    let (mut folder, manifest, _) = pack_fs::open_pack(&path).unwrap();
    assert!(ThemePack::parse(&read_bounded(manifest, MAX_PACK_BYTES).unwrap()).is_ok());
    let replaced = match std::fs::rename(&dir, &old) {
        Ok(()) => {
            directory_link(outside.path(), &dir);
            true
        }
        #[cfg(windows)]
        Err(error) => {
            assert!(
                matches!(error.raw_os_error(), Some(5 | 32)),
                "unexpected rename error: {error}"
            );
            false
        }
        #[cfg(not(windows))]
        Err(error) => panic!("cannot replace pack directory: {error}"),
    };
    let logo = folder.open_logo("logo.png").unwrap();
    assert_eq!(read_bounded(logo, MAX_LOGO_BYTES).unwrap(), original);
    drop(folder);
    if replaced {
        remove_directory_link(&dir);
    }
}

#[test]
fn logo_reads_use_the_verified_handle_even_if_the_leaf_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let original = png(2, 3);
    let path = write_logo_pack(dir.path(), "logo.png", &original);
    let logo = dir.path().join("logo.png");
    let renamed = dir.path().join("old-logo.png");
    BEFORE_LOGO_READ.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || match std::fs::rename(&logo, &renamed) {
            Ok(()) => std::fs::write(&logo, png(4, 5)).unwrap(),
            #[cfg(windows)]
            Err(error) => assert!(
                matches!(error.raw_os_error(), Some(5 | 32)),
                "unexpected rename error: {error}"
            ),
            #[cfg(not(windows))]
            Err(error) => panic!("cannot replace logo: {error}"),
        }));
    });
    assert_eq!(Applied::load(&path).unwrap().logo_png, original);
    assert!(BEFORE_LOGO_READ.with(|slot| slot.borrow().is_none()));
}

fn write_logo_pack(folder: &Path, logo_path: &str, bytes: &[u8]) -> PathBuf {
    let logo = folder.join(logo_path);
    std::fs::create_dir_all(logo.parent().unwrap()).unwrap();
    std::fs::write(logo, bytes).unwrap();
    let path = folder.join("local.harmony.json");
    let manifest =
        serde_json::json!({"version":1,"name":"Confined","base":"nord","logo":logo_path});
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    path
}

#[test]
fn replacing_a_logo_directory_after_validation_cannot_redirect_the_read() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let original = png(2, 3);
    let path = write_logo_pack(dir.path(), "assets/logo.png", &original);
    std::fs::write(outside.path().join("logo.png"), png(4, 5)).unwrap();
    let assets = dir.path().join("assets");
    let moved = dir.path().join("old-assets");
    let target = outside.path().to_owned();
    BEFORE_LOGO_READ.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || match std::fs::rename(&assets, &moved) {
            Ok(()) => directory_link(&target, &assets),
            #[cfg(windows)]
            Err(error) => assert!(
                matches!(error.raw_os_error(), Some(5 | 32)),
                "expected held-directory sharing denial, got {error}"
            ),
            #[cfg(not(windows))]
            Err(error) => panic!("directory replacement failed: {error}"),
        }));
    });
    let loaded = Applied::load(&path).unwrap();
    assert_eq!(
        loaded.logo_png, original,
        "read followed the replacement directory"
    );
    assert!(BEFORE_LOGO_READ.with(|slot| slot.borrow().is_none()));
    // Remove the link, not its target, before TempDir cleanup.
    if dir.path().join("old-assets").exists() {
        #[cfg(windows)]
        std::fs::remove_dir(dir.path().join("assets")).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(dir.path().join("assets")).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_data_handles_are_nonblocking_and_unix_sockets_are_rejected() {
    use std::os::fd::AsRawFd;
    let dir = tempfile::tempdir().unwrap();
    let path = write_logo_pack(dir.path(), "logo.png", &png(2, 3));
    let (mut folder, manifest, _) = pack_fs::open_pack(&path).unwrap();
    let logo = folder.open_logo("logo.png").unwrap();
    for file in [&manifest, &logo] {
        // SAFETY: file owns a live descriptor; F_GETFL has no third argument.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_ne!(
            flags & libc::O_NONBLOCK,
            0,
            "regular data read lost O_NONBLOCK"
        );
        assert!(file.metadata().unwrap().is_file());
    }
    std::fs::remove_file(dir.path().join("logo.png")).unwrap();
    let _socket = std::os::unix::net::UnixListener::bind(dir.path().join("logo.png")).unwrap();
    assert!(
        Applied::load(&path).is_err(),
        "UNIX socket accepted as logo"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn fifo_logo_is_rejected_without_waiting_for_a_writer() {
    const CHILD_PATH: &str = "HARMONY_FIFO_CHILD_PATH";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        assert!(Applied::load(Path::new(&path)).is_err());
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = write_logo_pack(dir.path(), "logo.png", &png(1, 1));
    let fifo = dir.path().join("logo.png");
    std::fs::remove_file(&fifo).unwrap();
    use std::os::unix::ffi::OsStrExt;
    let fifo = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: fifo is a valid NUL-terminated local path.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "fifo_logo_is_rejected_without_waiting_for_a_writer",
            "--nocapture",
        ])
        .env(CHILD_PATH, &path)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (done, wait) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut output = String::new();
        stdout.read_to_string(&mut output).unwrap();
        let _ = done.send(output);
    });
    // One blocking deadline, not polling. Do not reap before kill: an exited
    // child stays a zombie, so even a timeout/exit race cannot kill a reused PID.
    let finished = wait.recv_timeout(std::time::Duration::from_secs(5));
    if finished.is_err() {
        child.kill().unwrap();
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert!(finished.is_ok(), "opening a FIFO waited for a writer");
    print!("{}", finished.unwrap());
    assert!(status.success(), "FIFO rejection child failed: {status}");
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([120, 50, 200, 255]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    bytes.into_inner()
}

#[test]
fn loading_is_bounded_and_snapshots_only_the_relative_png() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    let logo = png(32, 24);
    std::fs::write(dir.path().join("assets/logo.png"), &logo).unwrap();
    let file = dir.path().join("test.harmony.json");
    std::fs::write(
        &file,
        br#"{"version":1,"name":"Local","base":"bonfire","logo":"assets/logo.png"}"#,
    )
    .unwrap();
    let loaded =
        Applied::load(&file).expect("chosen local pack and bounded relative logo must load");
    assert_eq!(loaded.logo_png, logo);
    assert_eq!(loaded.source, Some(file.canonicalize().unwrap()));
    // No source file/asset needs to remain accessible for saved appearance.
    std::fs::remove_file(&file).unwrap();
    std::fs::remove_file(dir.path().join("assets/logo.png")).unwrap();
    let saved = serde_json::to_vec(&loaded).unwrap();
    assert_eq!(serde_json::from_slice::<Applied>(&saved).unwrap(), loaded);

    for bytes in [
        vec![b'x'; MAX_LOGO_BYTES + 1],
        png(513, 1),
        b"not a PNG".to_vec(),
    ] {
        std::fs::write(dir.path().join("assets/logo.png"), bytes).unwrap();
        std::fs::write(
            &file,
            br#"{"version":1,"name":"Local","base":"bonfire","logo":"assets/logo.png"}"#,
        )
        .unwrap();
        assert!(Applied::load(&file).is_err());
    }
    std::fs::write(&file, vec![b' '; MAX_PACK_BYTES + 1]).unwrap();
    assert!(Applied::load(&file).is_err());
    assert!(Applied::load(dir.path()).is_err());
}

#[test]
fn export_writes_a_portable_snapshot_without_touching_the_source() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let file = source.path().join("local.harmony.json");
    std::fs::write(source.path().join("logo.png"), png(12, 16)).unwrap();
    std::fs::write(
        &file,
        br#"{"version":1,"name":"Portable","base":"nord","logo":"logo.png"}"#,
    )
    .unwrap();
    let applied = Applied::load(&file).unwrap();
    let output = dest.path().join("shared.harmony.json");
    applied
        .export(&output)
        .expect("snapshot must export with its own relative PNG");
    let again = Applied::load(&output).unwrap();
    assert_eq!(again.logo_png, applied.logo_png);
    assert_eq!(again.pack.name, applied.pack.name);
    assert!(source.path().join("logo.png").exists());
    // An output failure cannot truncate an existing chosen pack.
    let original = std::fs::read(&output).unwrap();
    let mut invalid = applied.clone();
    invalid.pack.version = 99;
    assert!(invalid.export(&output).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), original);
}

#[test]
fn invalid_saved_pack_is_discarded_without_reading_its_source() {
    #[derive(serde::Deserialize)]
    struct Saved {
        #[serde(default, deserialize_with = "deserialize_saved")]
        pack: Option<Applied>,
        unrelated: String,
    }
    let pack = ThemePack::parse(br#"{"version":1,"name":"Saved","base":"bonfire"}"#).unwrap();
    let snapshot = Applied {
        pack,
        source: Some(PathBuf::from("does-not-exist.harmony.json")),
        logo_png: Vec::new(),
    };
    let valid = serde_json::json!({"pack":snapshot,"unrelated":"kept"});
    assert!(
        serde_json::from_value::<Saved>(valid.clone())
            .unwrap()
            .pack
            .is_some()
    );
    let mut invalid = valid;
    invalid["pack"]["pack"]["version"] = 100.into();
    let saved = serde_json::from_value::<Saved>(invalid).unwrap();
    assert!(saved.pack.is_none());
    assert_eq!(saved.unrelated, "kept");
}

#[test]
fn selected_files_are_regular_and_logo_paths_cannot_escape_via_links() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("linked.harmony.json");
    let target = dir.path().join("actual.harmony.json");
    std::fs::write(&target, br#"{"version":1,"name":"Link","base":"bonfire"}"#).unwrap();
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(&target, &file);
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&target, &file);
    if result.is_ok() {
        assert!(Applied::load(&file).is_err(), "selected symlink accepted");
    }
    assert!(Applied::load(Path::new(r"\\server\share\pack.harmony.json")).is_err());
    assert!(reject_network_path(Path::new(r"C:\safe\pack.harmony.json:stream")).is_err());
    assert!(reject_network_path(Path::new(r"\\?\GLOBALROOT\Device\pack.harmony.json")).is_err());
}

#[test]
fn semantic_schema_matches_every_mutable_slint_color_and_rust_palette() {
    let mut slint_names: Vec<_> = include_str!("../../ui/theme.slint")
        .lines()
        .filter_map(|line| line.trim().strip_prefix("in-out property <color> "))
        .filter_map(|line| line.split(':').next())
        .collect();
    let mut pack_names = COLOR_NAMES.to_vec();
    slint_names.sort();
    pack_names.sort();
    assert_eq!(
        pack_names, slint_names,
        "packs must cover the live native color tokens"
    );
    let source = include_str!("theme.rs");
    let colors = source
        .split("theme! {")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    let mut rust_names: Vec<_> = colors
        .lines()
        .filter_map(|line| {
            line.trim()
                .split_once('(')
                .map(|(name, _)| name.replace('_', "-"))
        })
        .collect();
    rust_names.sort();
    assert_eq!(rust_names, pack_names);
}

#[test]
fn static_png_only_rejects_animation_and_windows_device_names() {
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut output = (data.len() as u32).to_be_bytes().to_vec();
        output.extend(kind);
        output.extend(data);
        let mut crc = !0u32;
        for b in kind.iter().chain(data) {
            crc ^= *b as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(crc & 1));
            }
        }
        output.extend((!crc).to_be_bytes());
        output
    }
    let ordinary = png(1, 1);
    let mut apng = ordinary[..33].to_vec();
    apng.extend(chunk(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]));
    apng.extend(chunk(
        b"fcTL",
        &[
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 10, 0, 0,
        ],
    ));
    apng.extend(&ordinary[33..]);
    assert!(
        decode_logo(&apng).is_err(),
        "animated PNG accepted as a static logo"
    );
    for name in [
        "NUL.png",
        "assets/CON.png",
        "com1.png",
        "aux.txt.png",
        "LPT9.png",
    ] {
        assert!(
            validate_logo_path(name).is_err(),
            "accepted device name {name}"
        );
    }
}

#[test]
fn shipped_example_loads_as_the_documented_vertical_slice() {
    let path = Path::new(file!())
        .parent()
        .unwrap()
        .join("../../examples/themes/orchid.harmony.json");
    let applied = Applied::load(&path).expect("the shipped example and its local logo must import");
    assert_eq!(applied.pack.name, "Orchid");
    assert_eq!(applied.pack.layout.title_height, Some(36.0));
    assert!(!applied.logo_png.is_empty());
    assert_eq!(
        decode_logo(&applied.logo_png).unwrap().dimensions(),
        (128, 32)
    );
}

#[test]
fn version_one_round_trips_a_partial_native_theme() {
    let text = br##"{"version":1,"name":"Orchid","base":"bonfire","dark":true,"colors":{"accent":"#cab8ff","accent-soft":"#cab8ff26"},"layout":{"title-height":36,"sidebar-width":280,"composer-height":54,"message-group-gap":12},"typography":{"font-family":"DM Sans","text-size":16,"title-text-size":13},"motion":{"transition-ms":90,"popup-ms":80,"reduced-motion":true}}"##;
    let pack = ThemePack::parse(text).expect("a supported native theme pack must import");
    assert_eq!(pack.name, "Orchid");
    assert_eq!(pack.colors["accent"], "#cab8ff");
    assert_eq!(pack.layout.sidebar_width, Some(280.0));
    assert_eq!(pack.typography.font_family.as_deref(), Some("DM Sans"));
    assert!(pack.motion.reduced_motion);
    let exported = pack.to_bytes().unwrap();
    assert_eq!(ThemePack::parse(&exported).unwrap(), pack);
}

#[test]
fn untrusted_pack_validation_rejects_unsupported_or_unsafe_data() {
    let base = serde_json::json!({"version":1,"name":"Safe","base":"bonfire"});
    let cases = [
        ("version", serde_json::json!(2)),
        ("base", serde_json::json!("unknown")),
        ("name", serde_json::json!("")),
        ("name", serde_json::json!("x".repeat(81))),
        ("name", serde_json::json!("a\nb")),
        ("css", serde_json::json!("anything")),
        ("colors", serde_json::json!({"unknown":"#ffffff"})),
        ("colors", serde_json::json!({"accent":"red"})),
        ("colors", serde_json::json!({"accent":" #ffffff"})),
        ("layout", serde_json::json!({"title-height":23})),
        ("layout", serde_json::json!({"sidebar-width":381})),
        ("layout", serde_json::json!({"composer-height":999})),
        (
            "typography",
            serde_json::json!({"font-family":"https://example.org/font"}),
        ),
        ("typography", serde_json::json!({"text-size":9})),
        ("motion", serde_json::json!({"transition-ms":1001})),
        ("motion", serde_json::json!({"popup-ms":-1})),
        ("motion", serde_json::json!({"script":"alert(1)"})),
        ("logo", serde_json::json!("../logo.png")),
        ("logo", serde_json::json!("/logo.png")),
        ("logo", serde_json::json!("C:\\logo.png")),
        ("logo", serde_json::json!("https://example.org/logo.png")),
        ("logo", serde_json::json!("file:logo.png")),
        ("logo", serde_json::json!("assets/logo.svg")),
        ("logo", serde_json::json!("assets/./logo.png")),
    ];
    for (key, value) in cases {
        let mut input = base.clone();
        input[key] = value;
        assert!(
            ThemePack::parse(&serde_json::to_vec(&input).unwrap()).is_err(),
            "accepted {input}"
        );
    }
    assert!(ThemePack::parse(&vec![b' '; 65537]).is_err());
    assert!(
        ThemePack::parse(
            br#"{"version":1,"name":"Safe","base":"bonfire","layout":{"title-height":1e999}}"#
        )
        .is_err()
    );
}

#[test]
fn every_supported_metric_accepts_only_its_documented_finite_range() {
    let ranges = [
        ("title-height", 24.0, 64.0),
        ("title-inset", 0.0, 40.0),
        ("rail-tile", 32.0, 48.0),
        ("sidebar-width", 180.0, 380.0),
        ("header-height", 36.0, 80.0),
        ("account-height", 48.0, 96.0),
        ("profile-width", 240.0, 420.0),
        ("members-width", 180.0, 360.0),
        ("composer-height", 40.0, 96.0),
        ("composer-padding", 4.0, 32.0),
        ("composer-gutter", 0.0, 32.0),
        ("message-gap", 0.0, 16.0),
        ("message-group-gap", 0.0, 40.0),
        ("channel-row-height", 24.0, 48.0),
    ];
    for (key, min, max) in ranges {
        for edge in [min, max] {
            let input =
                serde_json::json!({"version":1,"name":"Edges","base":"nord","layout":{key:edge}});
            assert!(
                ThemePack::parse(&serde_json::to_vec(&input).unwrap()).is_ok(),
                "rejected {input}"
            );
        }
        for outside in [min - 0.1, max + 0.1] {
            let input = serde_json::json!({"version":1,"name":"Edges","base":"nord","layout":{key:outside}});
            assert!(
                ThemePack::parse(&serde_json::to_vec(&input).unwrap()).is_err(),
                "accepted {input}"
            );
        }
    }
    let mut pack = ThemePack::parse(br#"{"version":1,"name":"Finite","base":"yap"}"#).unwrap();
    for nonfinite in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        pack.layout.title_height = Some(nonfinite);
        assert!(pack.to_bytes().is_err());
    }
}
