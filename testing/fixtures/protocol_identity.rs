//! Source fixture: actual production modules, IN-MEMORY registry only.
//! No Windows registration API is linked by this executable.
#![allow(
    non_snake_case,
    non_camel_case_types,
    dead_code,
    unused_variables,
    unused_imports,
    unsafe_op_in_unsafe_fn
)]
extern crate self as tempfile;
extern crate self as tracing;
extern crate self as windows_sys;
extern crate self as yapfast_core;

pub struct TempDir(std::path::PathBuf);
impl TempDir {
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn tempdir() -> std::io::Result<TempDir> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("isolated fixture root"))
        .join(format!(
            "linux-fixture-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    std::fs::create_dir(&path)?;
    Ok(TempDir(path))
}

#[macro_export]
macro_rules! warn {
    ($($tokens:tt)*) => {{}};
}
#[macro_export]
macro_rules! info {
    ($($tokens:tt)*) => {{}};
}

pub const OPEN_LINK: &str = "--open-link";
pub fn default_directory() -> std::io::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::from(r"C:\fixture\Harmony data"))
}
// @AUTOSTART@
// @PROTOCOL@

mod icon {
    pub const APP_ID: &str = "io.github.TimeShallHalt.Harmony";
}
mod desktop_entry {
    pub const LEGACY_ID: &str = "io.github.yapfast.Yapfast";
    pub fn write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        let sandbox =
            std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("fixture sandbox"));
        assert!(
            path.starts_with(sandbox),
            "fixture must never write live integration files"
        );
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, bytes)
    }
    pub fn xdg_home(_variable: &str, _fallback: &str) -> Option<std::path::PathBuf> {
        None
    }
    // @LINUX_DESKTOP@
    // @ACTUAL_DESKTOP_TEMPLATE@
    #[test]
    fn actual_appimage_template_renders_harmony_mime_and_executable() {
        let text = entry("/home/a/Apps/Harmony 1.1.AppImage");
        assert!(text.contains("\nMimeType=x-scheme-handler/harmony;\n"));
        assert!(!text.contains("x-scheme-handler/yapfast"));
        assert!(text.contains("\nExec=\"/home/a/Apps/Harmony 1.1.AppImage\" --open-link %u\n"));
        assert!(!text.contains("Exec=harmony --open-link %u"));
    }
}

mod linux_autostart {
    fn command() -> std::io::Result<Vec<String>> {
        Ok(["/opt/Harmony/harmony", "--background"]
            .map(str::to_owned)
            .to_vec())
    }
    // @LINUX_AUTOSTART@
}

pub mod Win32 {
    pub mod Foundation {
        pub type WIN32_ERROR = u32;
        pub const ERROR_SUCCESS: u32 = 0;
        pub const ERROR_FILE_NOT_FOUND: u32 = 2;
        pub const ERROR_MORE_DATA: u32 = 234;
    }
    pub mod System {
        pub mod Registry {
            use super::super::Foundation::*;
            use std::{cell::RefCell, collections::BTreeMap, ffi::c_void};
            pub type HKEY = *mut c_void;
            pub type REG_VALUE_TYPE = u32;
            pub const HKEY_CURRENT_USER: HKEY = 1usize as HKEY;
            pub const KEY_QUERY_VALUE: u32 = 1;
            pub const KEY_SET_VALUE: u32 = 2;
            pub const REG_SZ: u32 = 1;
            pub const REG_EXPAND_SZ: u32 = 2;
            pub const REG_BINARY: u32 = 3;
            pub const RRF_RT_REG_SZ: u32 = 2;
            pub const RRF_RT_REG_BINARY: u32 = 8;
            #[derive(Clone, Debug, PartialEq, Eq)]
            pub struct Raw {
                pub bytes: Vec<u8>,
                pub kind: u32,
            }
            #[derive(Default)]
            struct Registry {
                values: BTreeMap<(String, String), Raw>,
                handles: BTreeMap<usize, String>,
                next: usize,
                writes: Vec<(String, String)>,
                delete_on_write_open: Option<String>,
            }
            thread_local! { static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default()); }
            unsafe fn text(pointer: *const u16) -> String {
                if pointer.is_null() {
                    return String::new();
                }
                let length = (0..32_768)
                    .find(|&at| *pointer.add(at) == 0)
                    .expect("bounded fixture string");
                String::from_utf16(std::slice::from_raw_parts(pointer, length))
                    .unwrap()
                    .to_ascii_lowercase()
            }
            pub fn reset() {
                REGISTRY.with(|r| *r.borrow_mut() = Registry::default());
            }
            pub fn string(text: &str, kind: u32) -> Raw {
                Raw {
                    bytes: text
                        .encode_utf16()
                        .chain([0])
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                    kind,
                }
            }
            pub fn seed(key: &str, name: &str, value: Raw) {
                REGISTRY.with(|r| {
                    r.borrow_mut()
                        .values
                        .insert((key.to_ascii_lowercase(), name.to_ascii_lowercase()), value);
                });
            }
            pub fn get(key: &str, name: &str) -> Option<Raw> {
                REGISTRY.with(|r| {
                    r.borrow()
                        .values
                        .get(&(key.to_ascii_lowercase(), name.to_ascii_lowercase()))
                        .cloned()
                })
            }
            pub fn snapshot() -> BTreeMap<(String, String), Raw> {
                REGISTRY.with(|r| r.borrow().values.clone())
            }
            pub fn writes() -> Vec<(String, String)> {
                REGISTRY.with(|r| r.borrow().writes.clone())
            }
            pub fn delete_on_next_write_open(key: &str) {
                REGISTRY
                    .with(|r| r.borrow_mut().delete_on_write_open = Some(key.to_ascii_lowercase()));
            }
            pub unsafe fn RegOpenKeyExW(
                _root: HKEY,
                key: *const u16,
                _options: u32,
                access: u32,
                out: *mut HKEY,
            ) -> u32 {
                let key = text(key);
                REGISTRY.with(|r| {
                    let mut r = r.borrow_mut();
                    if access & KEY_SET_VALUE != 0
                        && r.delete_on_write_open.as_deref() == Some(key.as_str())
                    {
                        r.delete_on_write_open = None;
                        r.values.retain(|(k, _), _| *k != key);
                    }
                    if !r
                        .values
                        .keys()
                        .any(|(k, _)| k == &key || k.starts_with(&format!("{key}\\")))
                    {
                        return ERROR_FILE_NOT_FOUND;
                    }
                    r.next += 1;
                    let handle = r.next + 1;
                    r.handles.insert(handle, key);
                    *out = handle as HKEY;
                    ERROR_SUCCESS
                })
            }
            pub unsafe fn RegCloseKey(handle: HKEY) -> u32 {
                REGISTRY.with(|r| {
                    r.borrow_mut().handles.remove(&(handle as usize));
                });
                ERROR_SUCCESS
            }
            pub unsafe fn RegQueryValueExW(
                handle: HKEY,
                name: *const u16,
                _reserved: *const u32,
                kind: *mut u32,
                data: *mut u8,
                size: *mut u32,
            ) -> u32 {
                REGISTRY.with(|r| {
                    let r = r.borrow();
                    let key = r.handles.get(&(handle as usize)).expect("fixture handle");
                    let Some(value) = r.values.get(&(key.clone(), text(name))) else {
                        return ERROR_FILE_NOT_FOUND;
                    };
                    let capacity = *size as usize;
                    *size = value.bytes.len() as u32;
                    if !kind.is_null() {
                        *kind = value.kind;
                    }
                    if !data.is_null() {
                        if capacity < value.bytes.len() {
                            return ERROR_MORE_DATA;
                        }
                        std::ptr::copy_nonoverlapping(
                            value.bytes.as_ptr(),
                            data,
                            value.bytes.len(),
                        );
                    }
                    ERROR_SUCCESS
                })
            }
            pub unsafe fn RegGetValueW(
                root: HKEY,
                key: *const u16,
                name: *const u16,
                _flags: u32,
                kind: *mut u32,
                data: *mut c_void,
                size: *mut u32,
            ) -> u32 {
                let mut handle = std::ptr::null_mut();
                let status = RegOpenKeyExW(root, key, 0, KEY_QUERY_VALUE, &mut handle);
                if status != ERROR_SUCCESS {
                    return status;
                }
                let status =
                    RegQueryValueExW(handle, name, std::ptr::null(), kind, data.cast(), size);
                RegCloseKey(handle);
                status
            }
            pub unsafe fn RegSetValueExW(
                handle: HKEY,
                name: *const u16,
                _reserved: u32,
                kind: u32,
                data: *const u8,
                size: u32,
            ) -> u32 {
                let name = text(name);
                let bytes = std::slice::from_raw_parts(data, size as usize).to_vec();
                REGISTRY.with(|r| {
                    let mut r = r.borrow_mut();
                    let key = r
                        .handles
                        .get(&(handle as usize))
                        .expect("open fixture key")
                        .clone();
                    if !r.values.keys().any(|(k, _)| k == &key) {
                        return 1018;
                    }
                    r.writes.push((key.clone(), name.clone()));
                    r.values.insert((key, name), Raw { bytes, kind });
                    ERROR_SUCCESS
                })
            }
            pub unsafe fn RegSetKeyValueW(
                _root: HKEY,
                key: *const u16,
                name: *const u16,
                kind: u32,
                data: *const c_void,
                size: u32,
            ) -> u32 {
                let (key, name) = (text(key), text(name));
                let bytes = std::slice::from_raw_parts(data.cast::<u8>(), size as usize).to_vec();
                REGISTRY.with(|r| {
                    let mut r = r.borrow_mut();
                    r.writes.push((key.clone(), name.clone()));
                    r.values.insert((key, name), Raw { bytes, kind });
                });
                ERROR_SUCCESS
            }
            pub unsafe fn RegDeleteKeyValueW(
                _root: HKEY,
                key: *const u16,
                name: *const u16,
            ) -> u32 {
                let pair = (text(key), text(name));
                REGISTRY.with(|r| {
                    let mut r = r.borrow_mut();
                    r.writes.push(pair.clone());
                    if r.values.remove(&pair).is_some() {
                        ERROR_SUCCESS
                    } else {
                        ERROR_FILE_NOT_FOUND
                    }
                })
            }
            pub unsafe fn RegDeleteKeyW(_root: HKEY, key: *const u16) -> u32 {
                let key = text(key);
                REGISTRY.with(|r| {
                    let mut r = r.borrow_mut();
                    let existed = r.values.keys().any(|(k, _)| *k == key);
                    r.writes.push((key.clone(), "<delete-key>".into()));
                    r.values.retain(|(k, _), _| *k != key);
                    if existed {
                        ERROR_SUCCESS
                    } else {
                        ERROR_FILE_NOT_FOUND
                    }
                })
            }
        }
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use Win32::System::Registry::*;
    use std::path::{Path, PathBuf};
    const DISCORD: &str = r"Software\Classes\discord\shell\open\command";
    const CUSTOM: &str = r"Software\Classes\harmony\shell\open\command";
    const LEGACY: &str = r"Software\Classes\yapfast\shell\open\command";
    const BACKUP: &str = r"Software\Yapfast\DiscordProtocol";
    fn program() -> String {
        std::env::current_exe()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }
    fn line(legacy: bool, root: &Path) -> String {
        let exe = if legacy {
            PathBuf::from(program())
                .with_file_name("yapfast.exe")
                .to_str()
                .unwrap()
                .to_owned()
        } else {
            program()
        };
        format!(
            "{}  --data-dir {}   --open-link \"%1\"",
            autostart::windows_program(&exe),
            autostart::windows_argument(root.to_str().unwrap())
        )
    }
    fn old_backup() -> Raw {
        let raw = Raw {
            bytes: vec![b'%', 0, b'X', 0, b'%', 0, 0, 0, 0, 0],
            kind: REG_EXPAND_SZ,
        };
        seed(BACKUP, "PreviousCommand", raw.clone());
        seed(BACKUP, "HadCommand", string("1", REG_SZ));
        seed(BACKUP, "HadUrlProtocol", string("1", REG_SZ));
        raw
    }

    #[test]
    fn actual_refresh_changes_only_existing_owned_commands_and_keeps_backup_bytes() {
        reset();
        let root = default_directory().unwrap();
        let old = line(true, &root);
        seed(DISCORD, "", string(&old, REG_EXPAND_SZ));
        seed(LEGACY, "", string(&old, REG_SZ));
        let backup = old_backup();
        let before = snapshot();
        protocol::refresh(&root);
        let new = line(false, &root);
        assert_eq!(get(DISCORD, ""), Some(string(&new, REG_EXPAND_SZ)));
        assert_eq!(get(LEGACY, ""), Some(string(&new, REG_SZ)));
        assert!(
            get(CUSTOM, "").is_none(),
            "normal launch is not consent for harmony:"
        );
        assert_eq!(get(BACKUP, "PreviousCommand"), Some(backup));
        assert_eq!(snapshot().len(), before.len());
        assert_eq!(
            writes(),
            vec![
                (DISCORD.to_ascii_lowercase(), "".into()),
                (LEGACY.to_ascii_lowercase(), "".into())
            ]
        );
        assert!(protocol::discord_enabled(&root));
    }

    #[test]
    fn actual_isolated_nonpreview_refresh_cannot_take_live_handler_or_backup() {
        reset();
        let root = default_directory().unwrap();
        seed(DISCORD, "", string(&line(false, &root), REG_SZ));
        seed(LEGACY, "", string(&line(true, &root), REG_SZ));
        old_backup();
        let before = snapshot();
        let isolated = Path::new(r"C:\fixture\isolated nonpreview");
        protocol::refresh(isolated);
        assert!(!protocol::discord_enabled(isolated));
        assert_eq!(snapshot(), before);
        assert!(writes().is_empty());
    }

    #[test]
    fn explicit_setting_restores_exact_previous_registry_bytes_and_type() {
        reset();
        // A deliberately noncanonical REG_EXPAND_SZ, including an extra NUL
        // and an unpaired surrogate: it must not be repaired/re-encoded.
        let previous = Raw {
            bytes: vec![0, 0xd8, b'%', 0, b'X', 0, b'%', 0, 0, 0, 0, 0],
            kind: REG_EXPAND_SZ,
        };
        seed(DISCORD, "", previous.clone());
        seed(
            r"Software\Classes\discord",
            "URL Protocol",
            string("", REG_SZ),
        );
        let root = default_directory().unwrap();
        protocol::set_discord_enabled(&root, true).unwrap();
        assert_eq!(get(BACKUP, "PreviousCommand"), Some(previous.clone()));
        assert!(protocol::discord_enabled(&root));
        protocol::set_discord_enabled(&root, false).unwrap();
        assert_eq!(get(DISCORD, ""), Some(previous));
        assert!(get(BACKUP, "PreviousCommand").is_none());
    }

    #[test]
    fn explicit_setting_cannot_erase_another_instances_or_unattributed_backup() {
        for owner in [false, true] {
            reset();
            let root = default_directory().unwrap();
            seed(DISCORD, "", string(&line(false, &root), REG_SZ));
            old_backup();
            if owner {
                seed(BACKUP, "OwnerCommand", string(&line(false, &root), REG_SZ));
            }
            let before = snapshot();
            let isolated = Path::new(r"C:\fixture\isolated");
            assert!(protocol::set_discord_enabled(isolated, false).is_err());
            assert!(protocol::set_discord_enabled(isolated, true).is_err());
            assert_eq!(snapshot(), before);
            assert!(writes().is_empty());
        }
    }

    #[test]
    fn actual_refresh_does_not_recreate_a_concurrently_deleted_custom_scheme() {
        reset();
        let root = default_directory().unwrap();
        seed(CUSTOM, "", string(&line(true, &root), REG_SZ));
        delete_on_next_write_open(CUSTOM);
        protocol::refresh(&root);
        assert!(get(CUSTOM, "").is_none());
        assert!(
            writes().is_empty(),
            "an existing-key open must not fall back to create-key writes"
        );
    }

    #[test]
    fn existing_custom_scheme_retargets_but_stock_yapfast_is_not_clobbered() {
        reset();
        let root = default_directory().unwrap();
        seed(CUSTOM, "", string(&line(true, &root), REG_SZ));
        let stock = string(
            r#""C:\Program Files\Yapfast\yapfast.exe" --open-link "%1""#,
            REG_SZ,
        );
        seed(LEGACY, "", stock.clone());
        protocol::refresh(&root);
        assert_eq!(get(CUSTOM, ""), Some(string(&line(false, &root), REG_SZ)));
        assert_eq!(get(LEGACY, ""), Some(stock));
        assert!(get(DISCORD, "").is_none());
        assert_eq!(writes(), vec![(CUSTOM.to_ascii_lowercase(), "".into())]);
    }
}

mod linux_mime {
    use crate::{desktop_entry, icon};
    use std::{io, path::Path};
    const DEFAULTS: &str = "[Default Applications]";
    const SCHEMES: [&str; 2] = ["x-scheme-handler/discord", "x-scheme-handler/harmony"];
    const LEGACY_SCHEME: &str = "x-scheme-handler/yapfast";
    fn desktop() -> String {
        format!("{}.desktop", icon::APP_ID)
    }
    fn write_list(path: &Path, text: &str) -> io::Result<()> {
        desktop_entry::write(path, text.as_bytes())
    }
    // @LINUX_MIME@

    #[test]
    fn actual_linux_legacy_mime_read_and_restore_requires_owned_proof() {
        let dir = crate::tempdir().unwrap();
        let list = dir.path().join("mimeapps.list");
        let previous = dir.path().join("discord-protocol-previous");
        let old = "[Default Applications]\nx-scheme-handler/discord=io.github.yapfast.Yapfast.desktop;\nx-scheme-handler/yapfast=io.github.yapfast.Yapfast.desktop;\ntext/html=browser.desktop;\n";
        let kept = "x-scheme-handler/discord = discord.desktop;vesktop.desktop;\nx-scheme-handler/yapfast=stock-other.desktop;\n";
        desktop_entry::write(&list, old.as_bytes()).unwrap();
        desktop_entry::write(&previous, kept.as_bytes()).unwrap();
        assert!(!enabled_with_legacy(&list, false));
        assert!(enabled_with_legacy(&list, true));
        apply_with_legacy(&list, &previous, false, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&list).unwrap(),
            old,
            "stock/unproven defaults remain byte-identical"
        );
        assert_eq!(
            std::fs::read_to_string(&previous).unwrap(),
            kept,
            "off/no-op keeps unattributable rollback data"
        );
        apply_with_legacy(&list, &previous, false, true).unwrap();
        let restored = std::fs::read_to_string(&list).unwrap();
        assert!(restored.contains("x-scheme-handler/discord = discord.desktop;vesktop.desktop;\n"));
        assert!(restored.contains("x-scheme-handler/yapfast=stock-other.desktop;\n"));
        assert!(restored.contains("text/html=browser.desktop;\n"));
        assert!(!previous.exists());
    }
    #[test]
    fn legacy_mime_default_needs_proven_ownership_and_other_choices_stay_intact() {
        let legacy = "x-scheme-handler/discord=io.github.yapfast.Yapfast.desktop;";
        let owned = |line: &str, proof: bool| names_harmony(line) || proof && names_legacy(line);
        assert!(owned(legacy, true));
        assert!(
            !owned(legacy, false),
            "stock Yapfast must never count as Harmony"
        );
        assert!(!owned("x-scheme-handler/discord=discord.desktop;", true));
        let before = format!(
            "# preserved\n[Default Applications]\n{legacy}\ntext/html=browser.desktop;\n[Added Associations]\nx-scheme-handler/discord=other.desktop;\n"
        );
        assert_eq!(
            default_line(&before, "x-scheme-handler/discord"),
            Some(legacy)
        );
        let restored = with_line(
            &before,
            "x-scheme-handler/discord",
            Some("x-scheme-handler/discord=discord.desktop;"),
        );
        assert!(restored.contains("text/html=browser.desktop;\n[Added Associations]"));
        assert!(restored.contains("[Added Associations]\nx-scheme-handler/discord=other.desktop;"));
        assert!(restored.contains("# preserved\n"));
    }
}
