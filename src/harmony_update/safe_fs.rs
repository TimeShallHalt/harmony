//! Handle-relative local updater storage. No archive extractor exists: releases
//! contain one raw executable, never paths to unpack. Fail closed on links,
//! junctions, devices/FIFOs and unsupported platforms. Linux operations remain
//! relative to held directory descriptors even when a directory is renamed.
use super::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub fn leaf(name: &str) -> Result<()> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 180
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(Error::Invalid("unsafe local filename"));
    }
    Ok(())
}
#[cfg(test)]
thread_local! {
    // Fault injection only: a real successful rename followed by a failed sync.
    pub(super) static FAIL_SYNC_FOR: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
}
pub struct Dir {
    pub path: PathBuf,
    file: File,
    #[cfg(windows)]
    _ancestors: Vec<File>,
}
impl Dir {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(Error::Invalid("absolute local path required"));
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let root = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open("/")
                .map_err(|_| Error::Io("open root"))?;
            let mut dir = Self {
                path: PathBuf::from("/"),
                file: root,
            };
            for part in path.components() {
                if let Component::Normal(n) = part {
                    dir = dir.open_child_os(n)?;
                }
            }
            Ok(dir)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let mut parts = path.components();
            let Some(Component::Prefix(prefix)) = parts.next() else {
                return Err(Error::Invalid("local disk path required"));
            };
            if !matches!(prefix.kind(), std::path::Prefix::Disk(_)) {
                return Err(Error::Invalid("network/device paths refused"));
            }
            if parts.next() != Some(Component::RootDir) {
                return Err(Error::Invalid("absolute disk root required"));
            }
            let drive = PathBuf::from(format!("{}\\", prefix.as_os_str().to_string_lossy()));
            let file = OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x0020_0000 | 0x0200_0000)
                .open(&drive)
                .map_err(|_| Error::Io("pin local drive"))?;
            let mut dir = Self {
                path: drive,
                file,
                _ancestors: Vec::new(),
            };
            for part in parts {
                if let Component::Normal(n) = part {
                    let file = super::windows_fs::open(&dir.file, n, true, false, false)?;
                    dir._ancestors.push(dir.file);
                    dir.file = file;
                    dir.path.push(n);
                } else {
                    return Err(Error::Invalid("disk path component"));
                }
            }
            Ok(dir)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = path;
            Err(Error::Unsupported)
        }
    }
    #[cfg(target_os = "linux")]
    fn open_child_os(&self, name: &std::ffi::OsStr) -> Result<Self> {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        };
        let n = std::ffi::CString::new(name.as_bytes()).map_err(|_| Error::Invalid("NUL path"))?;
        // SAFETY: the parent descriptor and NUL-terminated single component live
        // through openat; O_NOFOLLOW applies to the only component being opened.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                n.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(Error::Io("pin local directory (links refused)"));
        }
        Ok(Self {
            path: self.path.join(name),
            file: unsafe { File::from_raw_fd(fd) },
        })
    }
    pub fn child(&self, name: &str, create: bool) -> Result<Self> {
        leaf(name)?;
        if create {
            #[cfg(target_os = "linux")]
            {
                use std::os::fd::AsRawFd;
                let n = std::ffi::CString::new(name).unwrap();
                let result = unsafe { libc::mkdirat(self.file.as_raw_fd(), n.as_ptr(), 0o700) };
                if result != 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(Error::Io("create update directory"));
                }
            }
            #[cfg(windows)]
            {
                if !self.exists(name)? {
                    drop(super::windows_fs::open(
                        &self.file,
                        std::ffi::OsStr::new(name),
                        true,
                        true,
                        false,
                    )?);
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            let child = self.open_child_os(std::ffi::OsStr::new(name))?;
            child.require_private()?;
            Ok(child)
        }
        #[cfg(windows)]
        {
            let file = super::windows_fs::open(
                &self.file,
                std::ffi::OsStr::new(name),
                true,
                false,
                false,
            )?;
            let mut ancestors: Vec<_> = self
                ._ancestors
                .iter()
                .map(|f| {
                    f.try_clone()
                        .map_err(|_| Error::Io("clone directory guard"))
                })
                .collect::<Result<_>>()?;
            ancestors.push(
                self.file
                    .try_clone()
                    .map_err(|_| Error::Io("clone directory guard"))?,
            );
            let child = Self {
                path: self.path.join(name),
                file,
                _ancestors: ancestors,
            };
            child.require_private()?;
            Ok(child)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Err(Error::Unsupported)
        }
    }
    fn require_private(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let m = self
                .file
                .metadata()
                .map_err(|_| Error::Io("inspect staging ownership"))?;
            if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 {
                return Err(Error::Invalid(
                    "staging must be user-owned and not writable by other users",
                ));
            }
        }
        #[cfg(windows)]
        super::windows_fs::private(&self.file)?;
        Ok(())
    }
    pub fn file(&self, name: &str, create: bool) -> Result<File> {
        leaf(name)?;
        #[cfg(target_os = "linux")]
        {
            use std::os::{
                fd::{AsRawFd, FromRawFd},
                unix::fs::MetadataExt,
            };
            let n = std::ffi::CString::new(name).unwrap();
            let fd = if create {
                unsafe {
                    libc::openat(
                        self.file.as_raw_fd(),
                        n.as_ptr(),
                        libc::O_RDWR
                            | libc::O_CREAT
                            | libc::O_EXCL
                            | libc::O_NOFOLLOW
                            | libc::O_NONBLOCK
                            | libc::O_CLOEXEC,
                        0o600,
                    )
                }
            } else {
                // Pin without a data-open: a device/FIFO must be rejected
                // before its driver's open operation could have side effects.
                let fd = unsafe {
                    libc::openat(
                        self.file.as_raw_fd(),
                        n.as_ptr(),
                        libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return Err(Error::Io("pin regular update file"));
                }
                let pinned = unsafe { File::from_raw_fd(fd) };
                let meta = pinned
                    .metadata()
                    .map_err(|_| Error::Io("pinned file metadata"))?;
                if !meta.is_file() || meta.nlink() != 1 {
                    return Err(Error::Invalid("regular single-link update file required"));
                }
                // The only deliberate symlink following is a kernel fd link
                // inside verified procfs, referring to this exact held inode.
                use std::os::unix::fs::OpenOptionsExt;
                let proc = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
                    .open("/proc/self/fd")
                    .map_err(|_| Error::Io("updater requires procfs"))?;
                let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
                if unsafe { libc::fstatfs(proc.as_raw_fd(), stat.as_mut_ptr()) } != 0
                    || unsafe { stat.assume_init() }.f_type != libc::PROC_SUPER_MAGIC
                {
                    return Err(Error::Io("updater requires genuine procfs"));
                }
                let handle = std::ffi::CString::new(pinned.as_raw_fd().to_string()).unwrap();
                let data = unsafe {
                    libc::openat(
                        proc.as_raw_fd(),
                        handle.as_ptr(),
                        libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC,
                    )
                };
                if data < 0 {
                    return Err(Error::Io("reopen pinned regular file"));
                }
                let data_file = unsafe { File::from_raw_fd(data) };
                let opened = data_file
                    .metadata()
                    .map_err(|_| Error::Io("data file identity"))?;
                if !opened.is_file() || opened.ino() != meta.ino() || opened.dev() != meta.dev() {
                    return Err(Error::Conflict);
                }
                use std::os::fd::IntoRawFd;
                data_file.into_raw_fd()
            };
            if fd < 0 {
                return Err(Error::Io("open regular update file (links refused)"));
            }
            let file = unsafe { File::from_raw_fd(fd) };
            let m = file.metadata().map_err(|_| Error::Io("file metadata"))?;
            if !m.is_file() || m.nlink() != 1 {
                return Err(Error::Invalid(
                    "regular unlinked-to-other-files storage required",
                ));
            }
            Ok(file)
        }
        #[cfg(windows)]
        {
            super::windows_fs::open(&self.file, std::ffi::OsStr::new(name), false, create, false)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Err(Error::Unsupported)
        }
    }
    pub fn exists(&self, name: &str) -> Result<bool> {
        leaf(name)?;
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let n = std::ffi::CString::new(name).unwrap();
            let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
            let r = unsafe {
                libc::fstatat(
                    self.file.as_raw_fd(),
                    n.as_ptr(),
                    st.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if r == 0 {
                return Ok(true);
            }
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                return Ok(false);
            }
            Err(Error::Io("inspect local entry"))
        }
        #[cfg(windows)]
        {
            // An entry may be a directory; try both without following links.
            match super::windows_fs::open(
                &self.file,
                std::ffi::OsStr::new(name),
                false,
                false,
                false,
            ) {
                Ok(_) => Ok(true),
                Err(Error::NotFound) => Ok(false),
                Err(_) => super::windows_fs::open(
                    &self.file,
                    std::ffi::OsStr::new(name),
                    true,
                    false,
                    false,
                )
                .map(|_| true),
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Err(Error::Unsupported)
        }
    }
    pub fn read(&self, name: &str, limit: u64) -> Result<Vec<u8>> {
        let mut f = self.file(name, false)?;
        if f.metadata().map_err(|_| Error::Io("file size"))?.len() > limit {
            return Err(Error::Size);
        }
        let mut bytes = Vec::new();
        (&mut f)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Io("read local file"))?;
        if bytes.len() as u64 > limit {
            return Err(Error::Size);
        }
        Ok(bytes)
    }
    pub fn atomic(&self, name: &str, bytes: &[u8]) -> Result<()> {
        static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp = format!(
            ".write-{}-{}",
            std::process::id(),
            ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let result = (|| {
            let mut f = self.file(&tmp, true)?;
            f.write_all(bytes)
                .and_then(|_| f.sync_all())
                .map_err(|_| Error::Io("persist update state"))?;
            drop(f);
            self.rename(&tmp, self, name, true)
        })();
        if result.is_err() {
            let _ = self.remove(&tmp);
        }
        result
    }
    pub fn rename(&self, from: &str, dest: &Dir, to: &str, replace: bool) -> Result<()> {
        leaf(from)?;
        leaf(to)?;
        if !replace && dest.exists(to)? {
            return Err(Error::Conflict);
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let from = std::ffi::CString::new(from).unwrap();
            let to = std::ffi::CString::new(to).unwrap();
            let flags = if replace { 0 } else { libc::RENAME_NOREPLACE };
            if unsafe {
                libc::renameat2(
                    self.file.as_raw_fd(),
                    from.as_ptr(),
                    dest.file.as_raw_fd(),
                    to.as_ptr(),
                    flags,
                )
            } != 0
            {
                return Err(Error::Io("rename update file"));
            }
            #[cfg(test)]
            if FAIL_SYNC_FOR.with(|slot| {
                let mut slot = slot.borrow_mut();
                if slot.as_deref() == to.to_str().ok() {
                    slot.take();
                    true
                } else {
                    false
                }
            }) {
                return Err(Error::Durability);
            }
            self.file
                .sync_all()
                .and_then(|_| dest.file.sync_all())
                .map_err(|_| Error::Durability)?;
        }
        #[cfg(windows)]
        {
            let file = super::windows_fs::open(
                &self.file,
                std::ffi::OsStr::new(from),
                false,
                false,
                true,
            )?;
            super::windows_fs::rename(&file, &dest.file, to, replace)?;
        }
        Ok(())
    }
    pub fn remove(&self, name: &str) -> Result<()> {
        leaf(name)?;
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let n = std::ffi::CString::new(name).unwrap();
            if unsafe { libc::unlinkat(self.file.as_raw_fd(), n.as_ptr(), 0) } != 0 {
                return Err(Error::Io("remove owned update file"));
            }
        }
        #[cfg(windows)]
        {
            let file = super::windows_fs::open(
                &self.file,
                std::ffi::OsStr::new(name),
                false,
                false,
                true,
            )?;
            super::windows_fs::remove(&file)?;
        }
        Ok(())
    }
    pub fn lock(&self) -> Result<File> {
        if !self.exists("lock")? {
            match self.file("lock", true) {
                Ok(f) => drop(f),
                Err(_) if self.exists("lock")? => (),
                Err(e) => return Err(e),
            }
        }
        let file = self.file("lock", false)?;
        file.try_lock().map_err(|_| Error::Busy)?;
        Ok(file)
    }
}
