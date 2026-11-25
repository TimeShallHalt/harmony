//! Bounded, data-only native Harmony theme packs.

use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

pub const MAX_PACK_BYTES: usize = 65_536;
pub const MAX_LOGO_BYTES: usize = 262_144;
pub const MAX_LOGO_EDGE: u32 = 512;
pub const COLOR_NAMES: &[&str] = &[
    "rail",
    "sidebar",
    "panel",
    "chat",
    "stage",
    "raised",
    "field",
    "sunken",
    "hover",
    "active",
    "row-hover",
    "line",
    "edge",
    "outline",
    "outline-chat",
    "outline-field",
    "text",
    "secondary",
    "muted",
    "faint",
    "accent",
    "accent-hover",
    "on-accent",
    "accent-soft",
    "danger",
    "danger-hover",
    "on-danger",
    "live",
    "loud",
    "link",
    "button-default",
    "button-default-hover",
    "mentioned",
    "mentioned-hover",
    "overlay",
    "shimmer",
    "reply-spine",
    "tooltip",
    "avatar",
];
pub const BASES: &[&str] = &[
    "harmony",
    "yap",
    "bonfire",
    "catppuccin-mocha",
    "dracula",
    "nord",
    "gruvbox-dark",
    "tokyo-night",
    "catppuccin-latte",
    "rose-pine-dawn",
    "sakura",
];

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct LayoutMetrics {
    pub title_height: Option<f32>,
    pub title_inset: Option<f32>,
    pub rail_tile: Option<f32>,
    pub sidebar_width: Option<f32>,
    pub header_height: Option<f32>,
    pub account_height: Option<f32>,
    pub profile_width: Option<f32>,
    pub members_width: Option<f32>,
    pub composer_height: Option<f32>,
    pub composer_padding: Option<f32>,
    pub composer_gutter: Option<f32>,
    pub message_gap: Option<f32>,
    pub message_group_gap: Option<f32>,
    pub channel_row_height: Option<f32>,
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Typography {
    pub font_family: Option<String>,
    pub text_size: Option<f32>,
    pub title_text_size: Option<f32>,
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Motion {
    pub transition_ms: Option<u32>,
    pub popup_ms: Option<u32>,
    pub reduced_motion: bool,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ThemePack {
    pub version: u32,
    pub name: String,
    pub base: String,
    pub dark: Option<bool>,
    #[serde(default)]
    pub colors: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub layout: LayoutMetrics,
    #[serde(default)]
    pub typography: Typography,
    #[serde(default)]
    pub motion: Motion,
    pub logo: Option<String>,
}
impl ThemePack {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_PACK_BYTES {
            return Err("Theme pack exceeds 65,536 bytes".into());
        }
        let pack: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid theme JSON: {e}"))?;
        pack.validate()?;
        Ok(pack)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Only theme pack version 1 is supported".into());
        }
        if !BASES.contains(&self.base.as_str()) {
            return Err("Unknown base theme".into());
        }
        if self.name.trim().is_empty()
            || self.name.len() > 80
            || self.name.chars().any(char::is_control)
        {
            return Err("Theme name must be 1–80 UTF-8 bytes, without control characters".into());
        }
        for (name, color) in &self.colors {
            let valid = color.strip_prefix('#').is_some_and(|hex| {
                [3, 6, 8].contains(&hex.len()) && hex.bytes().all(|c| c.is_ascii_hexdigit())
            });
            if !COLOR_NAMES.contains(&name.as_str()) || !valid {
                return Err(format!("Invalid semantic color: {name}"));
            }
        }
        bounded("title-height", self.layout.title_height, 24.0, 64.0)?;
        bounded("title-inset", self.layout.title_inset, 0.0, 40.0)?;
        bounded("rail-tile", self.layout.rail_tile, 32.0, 48.0)?;
        bounded("sidebar-width", self.layout.sidebar_width, 180.0, 380.0)?;
        bounded("header-height", self.layout.header_height, 36.0, 80.0)?;
        bounded("account-height", self.layout.account_height, 48.0, 96.0)?;
        bounded("profile-width", self.layout.profile_width, 240.0, 420.0)?;
        bounded("members-width", self.layout.members_width, 180.0, 360.0)?;
        bounded("composer-height", self.layout.composer_height, 40.0, 96.0)?;
        bounded("composer-padding", self.layout.composer_padding, 4.0, 32.0)?;
        bounded("composer-gutter", self.layout.composer_gutter, 0.0, 32.0)?;
        bounded("message-gap", self.layout.message_gap, 0.0, 16.0)?;
        bounded(
            "message-group-gap",
            self.layout.message_group_gap,
            0.0,
            40.0,
        )?;
        bounded(
            "channel-row-height",
            self.layout.channel_row_height,
            24.0,
            48.0,
        )?;
        bounded("text-size", self.typography.text_size, 10.0, 24.0)?;
        bounded(
            "title-text-size",
            self.typography.title_text_size,
            10.0,
            20.0,
        )?;
        if let Some(font) = &self.typography.font_family {
            if font.trim().is_empty()
                || font.len() > 80
                || !font
                    .chars()
                    .all(|c| c.is_alphanumeric() || [' ', '-', '_'].contains(&c))
            {
                return Err("Font family must be a local family name (1–80 bytes, letters/numbers/space/-/_)".into());
            }
        }
        if self.motion.transition_ms.is_some_and(|ms| ms > 1000)
            || self.motion.popup_ms.is_some_and(|ms| ms > 1000)
        {
            return Err("Motion durations must be 0–1000 ms".into());
        }
        if let Some(logo) = &self.logo {
            validate_logo_path(logo)?;
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_PACK_BYTES {
            return Err("Theme pack exceeds 65,536 bytes".into());
        }
        Ok(bytes)
    }
}

fn bounded(name: &str, value: Option<f32>, min: f32, max: f32) -> Result<(), String> {
    if value.is_some_and(|v| !v.is_finite() || !(min..=max).contains(&v)) {
        return Err(format!("{name} must be finite and between {min} and {max}"));
    }
    Ok(())
}

fn windows_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
    ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

fn validate_logo_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.len() > 160
        || path.contains(['\\', ':'])
        || !path.ends_with(".png")
        || path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || windows_device_name(part)
                || part.starts_with('.')
                || part.ends_with(['.', ' '])
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._- ".contains(&b))
        })
        || Path::new(path).is_absolute()
    {
        return Err("Logo must be a safe relative .png path inside the pack folder".into());
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Applied {
    pub pack: ThemePack,
    pub source: Option<PathBuf>,
    pub logo_png: Vec<u8>,
}
impl Applied {
    pub fn validate(&self) -> Result<(), String> {
        self.pack.validate()?;
        if self.pack.logo.is_some() != !self.logo_png.is_empty() {
            return Err("Saved theme logo is missing or unexpected".into());
        }
        if !self.logo_png.is_empty() {
            decode_logo(&self.logo_png)?;
        }
        Ok(())
    }

    /// Export the snapshot, not the mutable source. New assets never overwrite other files.
    pub fn export(&self, path: &Path) -> Result<(), String> {
        self.export_with_writer(path, write_atomic)
    }

    fn export_with_writer(
        &self,
        path: &Path,
        write_manifest: impl FnOnce(&Path, &[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        use std::io::Write;
        self.validate()?;
        let folder = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut pack = self.pack.clone();
        let mut asset = None;
        if !self.logo_png.is_empty() {
            let mut file = tempfile::Builder::new()
                .prefix("harmony-logo-")
                .suffix(".png")
                .tempfile_in(folder)
                .map_err(|e| e.to_string())?;
            file.write_all(&self.logo_png).map_err(|e| e.to_string())?;
            file.as_file().sync_all().map_err(|e| e.to_string())?;
            pack.logo = Some(
                file.path()
                    .file_name()
                    .ok_or("Missing export logo name")?
                    .to_string_lossy()
                    .into_owned(),
            );
            asset = Some(file);
        }
        // Serialize while NamedTempFile still owns cleanup. In particular, an
        // export-size error must not leave an unreferenced permanent asset.
        let manifest = pack.to_bytes()?;
        let finalized = if let Some(asset) = asset {
            #[cfg(test)]
            tests::before_asset_finalize(asset.path());
            // keep() can fail on Windows while clearing TEMPORARY attributes.
            // Finish it BEFORE committing JSON; no fallible logo operation may
            // occur after the manifest points at this unique permanent file.
            let (file, path) = asset.keep().map_err(|e| e.to_string())?;
            drop(file);
            Some(path)
        } else {
            None
        };
        if let Err(error) = write_manifest(path, &manifest) {
            if let Some(asset) = finalized {
                if let Err(cleanup) = std::fs::remove_file(&asset) {
                    if cleanup.kind() != std::io::ErrorKind::NotFound {
                        return Err(format!(
                            "{error}; cannot remove new logo {}: {cleanup}",
                            asset.display()
                        ));
                    }
                }
            }
            return Err(error);
        }
        Ok(())
    }

    /// Read only the explicitly chosen pack and its declared relative PNG. No polling.
    pub fn load(path: &Path) -> Result<Self, String> {
        reject_network_path(path)?;
        let (mut folder, manifest, source) = pack_fs::open_pack(path)?;
        let pack = ThemePack::parse(&read_bounded(manifest, MAX_PACK_BYTES)?)?;
        let mut logo_png = Vec::new();
        if let Some(logo) = &pack.logo {
            let file = folder.open_logo(logo)?;
            #[cfg(test)]
            tests::before_logo_read();
            logo_png = read_bounded(file, MAX_LOGO_BYTES)?;
            decode_logo(&logo_png)?;
        }
        Ok(Self {
            pack,
            source: Some(source),
            logo_png,
        })
    }
}

pub fn deserialize_saved<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Applied>, D::Error> {
    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
    // A damaged optional appearance must not discard unrelated preferences.
    Ok(serde_json::from_value::<Applied>(value)
        .ok()
        .filter(|saved| saved.validate().is_ok()))
}

fn reject_network_path(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if matches!(path.components().next(), Some(Component::Prefix(p)) if !matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        {
            return Err("Network and device theme paths are not supported".into());
        }
        if path.components().any(|c| matches!(c, Component::Normal(name) if name.to_string_lossy().contains(':') || windows_device_name(&name.to_string_lossy()))) {
            return Err("Device names and alternate data streams are not supported".into());
        }
    }
    #[cfg(not(windows))]
    if path.to_string_lossy().contains(['\\', ':']) {
        return Err("Windows/network theme paths are not supported here".into());
    }
    if path.to_string_lossy().starts_with("//") {
        return Err("Network theme paths are not supported".into());
    }
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let folder = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(folder).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

fn read_bounded(file: std::fs::File, max: usize) -> Result<Vec<u8>, String> {
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > max as u64 {
        return Err(format!("Theme file is not regular or exceeds {max} bytes"));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err(format!("Theme file exceeds {max} bytes"));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
mod pack_fs {
    use super::*;
    use std::{
        ffi::CString,
        fs::{File, OpenOptions},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, OpenOptionsExt},
            },
        },
    };

    pub(super) struct PackFolder {
        directory: File,
        proc_fds: File,
    }

    fn open_path(
        directory: i32,
        path: &Path,
        beneath: bool,
        is_directory: bool,
    ) -> Result<File, String> {
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // O_PATH acquires only an inode reference: it cannot wait for a FIFO
        // writer or invoke a device's data-open operation. No untrusted symlink
        // is followed, even if intermediate directories are swapped mid-open.
        // SAFETY: open_how contains only integer fields; zero initializes every
        // field (and any future extension) as required by the kernel ABI.
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (libc::O_PATH
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | if is_directory { libc::O_DIRECTORY } else { 0 }) as u64;
        how.resolve = libc::RESOLVE_NO_SYMLINKS | libc::RESOLVE_NO_MAGICLINKS;
        if beneath {
            how.resolve |= libc::RESOLVE_BENEATH | libc::RESOLVE_NO_XDEV;
        }
        // SAFETY: path/how are initialized and valid for the syscall; directory
        // is either a live owned descriptor or AT_FDCWD. Fail closed on old
        // kernels/seccomp policies, including ENOSYS, EAGAIN and ELOOP.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                directory,
                path.as_ptr(),
                &how,
                std::mem::size_of::<libc::open_how>(),
            )
        };
        if fd < 0 {
            return Err(format!(
                "Cannot safely open theme path (openat2 required): {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: successful openat2 returns a new owned file descriptor.
        Ok(unsafe { File::from_raw_fd(fd as i32) })
    }

    fn open_proc_fds() -> Result<File, String> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open("/proc/self/fd")
            .map_err(|e| format!("Theme loading requires procfs: {e}"))?;
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: file is live and stat points at writable storage.
        if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: successful fstatfs initialized stat.
        if unsafe { stat.assume_init() }.f_type != libc::PROC_SUPER_MAGIC {
            return Err("Theme loading requires genuine procfs handles".into());
        }
        Ok(file)
    }

    impl PackFolder {
        fn open_regular(&self, path: &Path) -> Result<File, String> {
            let pinned = open_path(self.directory.as_raw_fd(), path, true, false)?;
            let meta = pinned.metadata().map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err("Theme files must be regular, not symlinks, FIFOs or devices".into());
            }
            // Linux cannot upgrade O_PATH directly. Reopen ONLY this verified
            // regular inode through its held procfs descriptor, never its name.
            // This deliberate kernel fd-link is the sole no-follow exception;
            // O_NOFOLLOW here would reject procfs itself. It is NOT a pathname
            // fallback, and replacing/renaming any pack entry cannot redirect it.
            let name = CString::new(pinned.as_raw_fd().to_string()).unwrap();
            // SAFETY: proc_fds and pinned stay live; name is NUL-terminated.
            let fd = unsafe {
                libc::openat(
                    self.proc_fds.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            // SAFETY: successful openat returned a new owned descriptor.
            let file = unsafe { File::from_raw_fd(fd) };
            let opened = file.metadata().map_err(|e| e.to_string())?;
            if !opened.is_file() || opened.dev() != meta.dev() || opened.ino() != meta.ino() {
                return Err("Theme file identity changed while opening".into());
            }
            Ok(file)
        }

        pub(super) fn open_logo(&mut self, logo: &str) -> Result<File, String> {
            self.open_regular(Path::new(logo))
        }
    }

    pub(super) fn open_pack(path: &Path) -> Result<(PackFolder, File, PathBuf), String> {
        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
        let parent = path.parent().ok_or("Theme pack has no parent folder")?;
        let folder = PackFolder {
            directory: open_path(libc::AT_FDCWD, parent, false, true)?,
            proc_fds: open_proc_fds()?,
        };
        let file = folder.open_regular(Path::new(
            path.file_name().ok_or("Missing theme pack name")?,
        ))?;
        // A source/reload hint from the actual handle, never a containment test.
        let source = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
            .map_err(|e| e.to_string())?;
        Ok((folder, file, source))
    }
}

#[cfg(windows)]
mod pack_fs {
    use super::*;
    use std::{
        fs::{File, OpenOptions},
        os::windows::fs::OpenOptionsExt,
        path::Component,
    };

    // Win32 flags used through std's OpenOptionsExt (no extra FFI/dependency).
    const FILE_SHARE_READ: u32 = 1;
    const NO_REPARSE: u32 = 0x0020_0000; // FILE_FLAG_OPEN_REPARSE_POINT
    const DIRECTORY: u32 = 0x0200_0000; // FILE_FLAG_BACKUP_SEMANTICS

    pub(super) struct PackFolder {
        path: PathBuf,
        // Retain EVERY ancestor, not just the leaf. Deny WRITE (including setting
        // reparse data) and DELETE (rename/replacement) while names are reused.
        guards: Vec<File>,
    }

    fn open_guard(path: &Path) -> Result<File, String> {
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(NO_REPARSE | DIRECTORY)
            .open(path)
            .map_err(|e| format!("Cannot safely open theme path: {e}"))
    }

    fn open_directory(path: &Path) -> Result<File, String> {
        let file = open_guard(path)?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.is_dir() || is_reparse(&meta) {
            return Err("Theme directories cannot contain symlinks or reparse points".into());
        }
        Ok(file)
    }

    fn open_regular(path: &Path) -> Result<File, String> {
        // All ancestors are already guarded; reject special entries before any
        // data open. The no-follow handle check also covers a final-entry swap.
        let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() || is_reparse(&meta) {
            return Err("Theme files must be regular, not symlinks or devices".into());
        }
        let file = open_guard(path)?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() || is_reparse(&meta) {
            return Err("Theme files must be regular, not symlinks or devices".into());
        }
        Ok(file)
    }

    pub(super) fn open_pack(path: &Path) -> Result<(PackFolder, File, PathBuf), String> {
        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
        reject_network_path(&path)?;
        let parent = path.parent().ok_or("Theme pack has no parent folder")?;
        let mut folder = PackFolder {
            path: PathBuf::new(),
            guards: Vec::new(),
        };
        for component in parent.components() {
            if !matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            ) {
                return Err("Choose an absolute local theme path without dot segments".into());
            }
            folder.path.push(component.as_os_str());
            if !matches!(component, Component::Prefix(_)) {
                folder.guards.push(open_directory(&folder.path)?);
            }
        }
        let file = open_regular(&path)?;
        // Canonical name is only a source/reload hint and path-syntax check,
        // never reopened for data or relied on for containment. Every relevant
        // entry is already pinned by handles.
        let source = path.canonicalize().map_err(|e| e.to_string())?;
        reject_network_path(&source)?;
        Ok((folder, file, source))
    }

    impl PackFolder {
        pub(super) fn open_logo(&mut self, logo: &str) -> Result<File, String> {
            let mut path = self.path.clone();
            let mut parts = logo.split('/').peekable();
            while let Some(part) = parts.next() {
                path.push(part);
                if parts.peek().is_none() {
                    return open_regular(&path);
                }
                self.guards.push(open_directory(&path)?);
            }
            Err("Missing logo name".into())
        }
    }
}

#[cfg(windows)]
fn is_reparse(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0
}
#[cfg(not(any(windows, target_os = "linux")))]
mod pack_fs {
    use super::*;
    use std::fs::File;
    pub(super) struct PackFolder;
    pub(super) fn open_pack(_: &Path) -> Result<(PackFolder, File, PathBuf), String> {
        Err("Safe theme file loading is supported only on Windows and Linux".into())
    }
    impl PackFolder {
        pub(super) fn open_logo(&mut self, _: &str) -> Result<File, String> {
            Err("Safe theme file loading is supported only on Windows and Linux".into())
        }
    }
}

pub fn decode_logo(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    if bytes.is_empty() || bytes.len() > MAX_LOGO_BYTES {
        return Err("Logo must contain 1–262,144 PNG bytes".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_LOGO_EDGE);
    limits.max_image_height = Some(MAX_LOGO_EDGE);
    limits.max_alloc = Some(4 * 1024 * 1024);
    // PngDecoder checks dimensions while reading IHDR, before allocating pixels.
    let decoder = image::codecs::png::PngDecoder::with_limits(Cursor::new(bytes), limits)
        .map_err(|e| format!("Logo must be a valid PNG, at most 512×512: {e}"))?;
    if decoder.is_apng().map_err(|e| e.to_string())? {
        return Err("Only static PNG logos are supported, not APNG".into());
    }
    let image = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("Invalid PNG logo: {e}"))?
        .into_rgba8();
    if image.width() == 0 || image.height() == 0 {
        return Err("Logo must have nonzero dimensions".into());
    }
    Ok(image)
}

#[cfg(test)]
#[path = "theme_pack_tests.rs"]
mod tests;
