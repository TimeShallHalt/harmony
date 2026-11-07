//! Account-free native helper entry point; dispatched before TLS/runtime/UI.
use super::*;
use std::{ffi::OsString, path::PathBuf};
pub fn cli_main(args: &[OsString]) -> Option<Result<()>> {
    let flag = args.first()?.to_str()?;
    if flag == "--harmony-build-info" {
        return Some(if args.len() != 1 {
            Err(Error::Invalid("build-info arguments"))
        } else {
            serde_json::to_string_pretty(&BuildInfo::current())
                .map(|json| println!("{json}"))
                .map_err(|_| Error::Invalid("build metadata"))
        });
    }
    if flag != "--harmony-update-helper" {
        return None;
    }
    Some((|| {
        if args.len() != 4 {
            return Err(Error::Invalid("helper arguments"));
        }
        let install = PathBuf::from(&args[1]);
        let transaction = args[2]
            .to_str()
            .ok_or(Error::Invalid("helper transaction"))?;
        safe_fs::leaf(transaction)?;
        let parent: u32 = args[3]
            .to_str()
            .and_then(|s| s.parse().ok())
            .filter(|p| *p > 0 && *p != std::process::id())
            .ok_or(Error::Invalid("helper parent process"))?;
        let result = (|| {
            // Parent wait is an OS event, not PID polling. No termination API.
            wait_for_parent(parent)?;
            let pending =
                transaction::pending(&install)?.ok_or(Error::Invalid("helper pending update"))?;
            if pending.0 != transaction {
                return Err(Error::Conflict);
            }
            transaction::activate(&install)
        })();
        if result.is_err() {
            let _ = transaction::record_helper_failure(&install);
        }
        result
    })())
}
pub fn wait_for_parent(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        type Handle = *mut std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
            fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
            fn CloseHandle(handle: Handle) -> i32;
        }
        let process = unsafe { OpenProcess(0x0010_0000, 0, pid) };
        if process.is_null() {
            return if std::io::Error::last_os_error().raw_os_error() == Some(87) {
                Ok(())
            } else {
                Err(Error::Io("wait parent permission"))
            };
        }
        let status = unsafe { WaitForSingleObject(process, 120_000) };
        unsafe {
            CloseHandle(process);
        }
        if status == 0 {
            Ok(())
        } else if status == 258 {
            Err(Error::Timeout)
        } else {
            Err(Error::Io("wait parent exit"))
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                Ok(())
            } else {
                Err(Error::Io("pidfd parent wait (Linux 5.3+ required)"))
            };
        }
        let process = unsafe { std::fs::File::from_raw_fd(fd as i32) };
        let start = std::time::Instant::now();
        loop {
            let remaining = 120_000u128.saturating_sub(start.elapsed().as_millis());
            if remaining == 0 {
                return Err(Error::Timeout);
            }
            let mut poll = libc::pollfd {
                fd: process.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let status = unsafe { libc::poll(&mut poll, 1, remaining as i32) };
            if status > 0 && poll.revents & libc::POLLIN != 0 {
                return Ok(());
            }
            if status == 0 {
                return Err(Error::Timeout);
            }
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return Err(Error::Io("wait parent exit"));
            }
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = pid;
        Err(Error::Unsupported)
    }
}
