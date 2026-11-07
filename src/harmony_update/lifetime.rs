//! Parent lifetime capability created by the parent BEFORE launch, inherited as
//! stdin. No PID lookup in the helper. A readiness acknowledgement completes
//! handoff only after the helper has validated/held the capability.
use super::*;
use std::{
    fs::File,
    io::{Read, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
pub struct Parent {
    file: File,
    pub pid: u32,
    pub birth: u64,
}
impl Parent {
    pub fn for_self() -> Result<Self> {
        let pid = std::process::id();
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle;
            let h = unsafe { win::OpenProcess(0x0010_1000, 0, pid) };
            if h.is_null() {
                return Err(Error::Io("hold spawning process"));
            }
            let file = unsafe { File::from_raw_handle(h) };
            let birth = win::birth(&file)?;
            return Ok(Self { file, pid, birth });
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::FromRawFd;
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            if fd < 0 {
                return Err(Error::Unsupported);
            }
            let file = unsafe { File::from_raw_fd(fd as i32) };
            Ok(Self {
                file,
                pid,
                birth: linux_birth(pid)?,
            })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Err(Error::Unsupported)
        }
    }
    pub fn into_stdio(self) -> Stdio {
        Stdio::from(self.file)
    }
    pub fn receive(pid: u32, birth: u64) -> Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle;
            let mut duplicate = std::ptr::null_mut();
            let process = unsafe { win::GetCurrentProcess() };
            let input = unsafe { win::GetStdHandle(-10i32 as u32) };
            if unsafe { win::DuplicateHandle(process, input, process, &mut duplicate, 0, 0, 2) }
                == 0
            {
                return Err(Error::Invalid("inherited parent capability"));
            }
            let file = unsafe { File::from_raw_handle(duplicate) };
            if unsafe { win::GetProcessId(duplicate) } != pid || win::birth(&file)? != birth {
                return Err(Error::Conflict);
            }
            return Ok(Self { file, pid, birth });
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::FromRawFd;
            require_procfs()?;
            let fd = unsafe { libc::fcntl(0, libc::F_DUPFD_CLOEXEC, 3) };
            if fd < 0 {
                return Err(Error::Invalid("inherited pidfd"));
            }
            let file = unsafe { File::from_raw_fd(fd) };
            let kind = std::fs::read_link(format!("/proc/self/fd/{fd}"))
                .map_err(|_| Error::Unsupported)?;
            if kind.as_os_str() != "anon_inode:[pidfd]" {
                return Err(Error::Invalid("inherited handle is not a pidfd"));
            }
            let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}"))
                .map_err(|_| Error::Unsupported)?;
            let held = info
                .lines()
                .find_map(|line| line.strip_prefix("Pid:\t"))
                .and_then(|s| s.parse::<u32>().ok());
            // A reaped pidfd may report -1. Fail closed rather than inferring a
            // start identity from a missing PID. Normal launch waits for ACK.
            if held != Some(pid) || linux_birth(pid)? != birth {
                return Err(Error::Conflict);
            }
            Ok(Self { file, pid, birth })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Err(Error::Unsupported)
        }
    }
    pub fn wait(self) -> Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            match unsafe { win::WaitForSingleObject(self.file.as_raw_handle(), 120_000) } {
                0 => Ok(()),
                258 => Err(Error::Timeout),
                _ => Err(Error::Io("wait held parent exit")),
            }
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let start = Instant::now();
            loop {
                let remaining = 120_000u128.saturating_sub(start.elapsed().as_millis());
                if remaining == 0 {
                    return Err(Error::Timeout);
                }
                let mut poll = libc::pollfd {
                    fd: self.file.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let status = unsafe { libc::poll(&mut poll, 1, remaining as i32) };
                if status > 0 && poll.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                    return Ok(());
                }
                if status == 0 {
                    return Err(Error::Timeout);
                }
                if status < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
                {
                    continue;
                }
                return Err(Error::Io("wait held pidfd"));
            }
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Err(Error::Unsupported)
        }
    }
}
pub fn acknowledge() -> Result<()> {
    std::io::stdout()
        .write_all(b"HUP")
        .and_then(|_| std::io::stdout().flush())
        .map_err(|_| Error::Io("acknowledge parent capability"))
}
pub fn launch(mut cmd: Command, parent: Parent) -> Result<Child> {
    cmd.arg(parent.pid.to_string())
        .arg(parent.birth.to_string())
        .arg("parent-stdin-v1")
        .stdin(parent.into_stdio())
        .stdout(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_4000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|_| Error::Io("launch identity-bound helper"))?;
    let mut output = child
        .stdout
        .take()
        .ok_or(Error::Io("helper readiness pipe"))?;
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut ack = [0u8; 3];
        let result = output.read_exact(&mut ack).map(|_| ack);
        let _ = send.send(result);
    });
    let ready = matches!(receive.recv_timeout(Duration::from_secs(5)),Ok(Ok(ack)) if ack==*b"HUP");
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
    } // ONLY this owned helper child.
    let _ = reader.join();
    if ready {
        Ok(child)
    } else {
        Err(Error::Invalid("helper parent capability acknowledgement"))
    }
}
#[cfg(target_os = "linux")]
pub(super) fn require_procfs() -> Result<()> {
    use std::os::fd::AsRawFd;
    let proc = File::open("/proc/self").map_err(|_| Error::Unsupported)?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(proc.as_raw_fd(), stat.as_mut_ptr()) } != 0
        || unsafe { stat.assume_init() }.f_type != libc::PROC_SUPER_MAGIC
    {
        return Err(Error::Unsupported);
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn linux_birth(pid: u32) -> Result<u64> {
    require_procfs()?;
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|_| Error::Conflict)?;
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(19))
        .and_then(|s| s.parse().ok())
        .ok_or(Error::Invalid("parent kernel start identity"))
}
#[cfg(windows)]
mod win {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    type H = *mut std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Time {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> H;
        pub fn GetCurrentProcess() -> H;
        pub fn GetStdHandle(which: u32) -> H;
        pub fn DuplicateHandle(
            source: H,
            h: H,
            target: H,
            out: *mut H,
            access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
        pub fn GetProcessId(h: H) -> u32;
        pub fn WaitForSingleObject(h: H, millis: u32) -> u32;
        fn GetProcessTimes(
            h: H,
            birth: *mut Time,
            exit: *mut Time,
            kernel: *mut Time,
            user: *mut Time,
        ) -> i32;
    }
    pub fn birth(file: &File) -> Result<u64> {
        let (mut b, mut e, mut k, mut u) = (
            Time::default(),
            Time::default(),
            Time::default(),
            Time::default(),
        );
        if unsafe { GetProcessTimes(file.as_raw_handle(), &mut b, &mut e, &mut k, &mut u) } == 0 {
            return Err(Error::Invalid("inherited process handle start identity"));
        }
        Ok((u64::from(b.high) << 32) | u64::from(b.low))
    }
}
