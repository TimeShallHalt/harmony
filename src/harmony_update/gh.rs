//! Optional official GitHub CLI credential boundary. Harmony never asks gh for
//! tokens. Only literal github.com API routes are used; release URLs are data,
//! not destinations. gh owns TLS, GitHub asset redirects and OS-keyring auth.
use super::*;
use release::{MAX_METADATA, REPOSITORY, Release, Transport};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct Gh;
pub fn command() -> Command {
    let mut cmd = Command::new(if cfg!(windows) { "gh.exe" } else { "gh" });
    // Do not inherit token overrides or HTTP debug output from the app process.
    // Existing official CLI configuration + secure OS keyring remain CLI-owned.
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_DEBUG",
        "GH_HOST",
        "GH_REPO",
        "GH_CONFIG_DIR",
    ] {
        cmd.env_remove(key);
    }
    cmd.env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1")
        .env("GH_PAGER", "cat");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    cmd
}
pub fn api_command(endpoint: &str, binary: bool) -> Result<Command> {
    let prefix = format!("repos/{REPOSITORY}/releases");
    let valid = endpoint == format!("{prefix}?per_page=100")
        || endpoint
            .strip_prefix(&format!("{prefix}/assets/"))
            .is_some_and(|n| {
                n.parse::<u64>()
                    .is_ok_and(|id| id > 0 && id.to_string() == n)
            });
    if !valid {
        return Err(Error::Invalid("untrusted API route"));
    }
    let mut cmd = command();
    cmd.args([
        "api",
        "--hostname",
        "github.com",
        "--method",
        "GET",
        endpoint,
        "--header",
        if binary {
            "Accept: application/octet-stream"
        } else {
            "Accept: application/vnd.github+json"
        },
        "--header",
        "X-GitHub-Api-Version: 2022-11-28",
    ]);
    if !binary {
        cmd.args([
            "--jq",
            "[.[] | {tag_name,html_url,draft,prerelease,assets:[.assets[] | {id,name,size,url}]}]",
        ]);
    }
    Ok(cmd)
}
#[cfg(test)]
thread_local! {
    // Injection seam: schedule cancellation after the runner's check, before
    // delivering the chunk to the real staging writer. No synthetic I/O.
    pub(super) static BEFORE_WRITE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}
pub fn run_command(
    mut cmd: Command,
    output: &mut dyn Write,
    limit: u64,
    cancel: &Cancel,
    timeout: Duration,
    failure: Error,
) -> Result<()> {
    cancel.check()?;
    let mut child = cmd.stdout(Stdio::piped()).spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::MissingGh
        } else {
            Error::Io("start update provider")
        }
    })?;
    let mut stdout = child.stdout.take().ok_or(Error::Io("provider output"))?;
    let (send, receive) = mpsc::sync_channel(2);
    let reader = std::thread::spawn(move || {
        loop {
            let mut b = vec![0; 64 * 1024];
            match stdout.read(&mut b) {
                Ok(0) => {
                    let _ = send.send(Ok(Vec::new()));
                    break;
                }
                Ok(n) => {
                    b.truncate(n);
                    if send.send(Ok(b)).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = send.send(Err(()));
                    break;
                }
            }
        }
    });
    let started = Instant::now();
    let mut count = 0u64;
    let result = (|| {
        loop {
            cancel.check()?;
            if started.elapsed() > timeout {
                return Err(Error::Timeout);
            }
            match receive.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(b)) if b.is_empty() => break,
                Ok(Ok(b)) => {
                    count += b.len() as u64;
                    if count > limit {
                        return Err(Error::Size);
                    }
                    #[cfg(test)]
                    BEFORE_WRITE.with(|hook| {
                        if let Some(hook) = hook.borrow_mut().take() {
                            hook();
                        }
                    });
                    output.write_all(&b).map_err(|_| {
                        if cancel.check().is_err() {
                            Error::Cancelled
                        } else {
                            Error::Io("write update download")
                        }
                    })?;
                }
                Ok(Err(())) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Error::Network);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
        }
        loop {
            cancel.check()?;
            if started.elapsed() > timeout {
                return Err(Error::Timeout);
            }
            if let Some(status) = child
                .try_wait()
                .map_err(|_| Error::Io("wait update provider"))?
            {
                return if status.success() {
                    Ok(())
                } else {
                    Err(failure)
                };
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })();
    drop(receive);
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    // The only killed process is the gh child we just created, never Harmony,
    // a call, or another gh process. Dropping the receiver unblocks its reader.
    let _ = reader.join();
    result
}
impl Transport for Gh {
    fn releases(&self, cancel: &Cancel) -> Result<Vec<Release>> {
        let mut auth = command();
        auth.args(["auth", "status", "--active", "--hostname", "github.com"]);
        run_command(
            auth,
            &mut std::io::sink(),
            MAX_METADATA,
            cancel,
            Duration::from_secs(40),
            Error::Auth,
        )?;
        let mut bytes = Vec::new();
        run_command(
            api_command(&format!("repos/{REPOSITORY}/releases?per_page=100"), false)?,
            &mut bytes,
            MAX_METADATA,
            cancel,
            Duration::from_secs(45),
            Error::Network,
        )?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("GitHub release response"))
    }
    fn asset(&self, id: u64, out: &mut dyn Write, limit: u64, cancel: &Cancel) -> Result<()> {
        run_command(
            api_command(&format!("repos/{REPOSITORY}/releases/assets/{id}"), true)?,
            out,
            limit,
            cancel,
            Duration::from_secs(20 * 60),
            Error::Network,
        )
    }
}
