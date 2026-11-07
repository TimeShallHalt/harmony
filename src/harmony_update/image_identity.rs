//! Kernel-backed running image pin. A pathname (including APPIMAGE) is only a
//! proposed installation location, never evidence of what is executing.
use super::*;
use std::{fs::File, path::Path};
pub(super) fn running(install: &Path) -> Result<File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        lifetime::require_procfs()?;
        // proc_pid_exe(5): dereferencing this kernel link opens the EXECUTED
        // inode, even if its old pathname has been renamed/replaced.
        let image = File::open("/proc/self/exe").map_err(|_| Error::UnsupportedImageIdentity)?;
        let parent = install.parent().ok_or(Error::UnsupportedImageIdentity)?;
        let name = install
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(Error::UnsupportedImageIdentity)?;
        let target = safe_fs::Dir::open(parent)?.file(name, false)?;
        let a = image
            .metadata()
            .map_err(|_| Error::UnsupportedImageIdentity)?;
        let b = target
            .metadata()
            .map_err(|_| Error::UnsupportedImageIdentity)?;
        if !a.is_file()
            || a.nlink() != 1
            || a.dev() != b.dev()
            || a.ino() != b.ino()
            || b.uid() != unsafe { libc::geteuid() }
            || b.mode() & 0o022 != 0
        {
            return Err(Error::UnsupportedImageIdentity);
        }
        // Keep this actual kernel image open through copy and launch. Linux's
        // executable write exclusion pins its bytes, not just the old filename.
        return Ok(image);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = install;
        // GetModuleFileName/GetMappedFileName are names, not a supported handle
        // to the image section's backing file. Do NOT reinterpret these names
        // as an independent file ID, compare only selected PE sections, or run a
        // purported helper to ask it for identity. Portable Windows activation
        // remains unsupported until a kernel-backed image binding is designed.
        Err(Error::UnsupportedImageIdentity)
    }
}
