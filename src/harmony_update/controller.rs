use super::*;
use release::{Candidate, Transport};
use std::{path::PathBuf, sync::Mutex};
use transaction::{Phase, Plan};
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub check_on_startup: bool,
    pub download_automatically: bool,
    pub last_check_unix: Option<u64>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            download_automatically: false,
            last_check_unix: None,
        }
    }
}
pub fn startup_due(enabled: bool, now: u64, last: Option<u64>) -> bool {
    enabled
        && last.is_none_or(|last| {
            now.checked_sub(last)
                .is_some_and(|elapsed| elapsed >= 86_400)
        })
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub status: String,
    pub candidate: Option<Candidate>,
    pub staged: Option<(String, Plan)>,
}
pub struct Controller {
    install: Result<PathBuf>,
    pub current: BuildInfo,
    state: Mutex<Snapshot>,
    busy: AtomicBool,
    cancel: Cancel,
    observer: Mutex<Option<Box<dyn Fn() + Send>>>,
}
struct Operation<'a>(&'a Controller);
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
        self.0.notify();
    }
}
impl Controller {
    pub fn new(install: Result<PathBuf>) -> Self {
        Self { install, current: BuildInfo::current(), state: Mutex::new(Snapshot { status: "Not checked. Private releases require the official GitHub CLI signed in to github.com.".into(), candidate: None, staged: None }), busy: AtomicBool::new(false), cancel: Cancel::default(), observer: Mutex::new(None) }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.cancel.0.store(true, Ordering::Release);
    }
    pub fn observe(&self, observer: Box<dyn Fn() + Send>) {
        *self.observer.lock().unwrap() = Some(observer);
    }
    fn notify(&self) {
        if let Some(observer) = self.observer.lock().unwrap().as_ref() {
            observer();
        }
    }
    pub fn status(&self, text: impl Into<String>) {
        self.state.lock().unwrap().status = text.into();
        self.notify();
    }
    fn begin(&self, text: &str) -> Result<Operation<'_>> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::Busy)?;
        self.cancel.0.store(false, Ordering::Release);
        self.status(text);
        Ok(Operation(self))
    }
    fn finish<T>(&self, result: Result<T>) -> Result<T> {
        if let Err(ref error) = result {
            self.status(error.to_string());
        }
        result
    }
    pub fn load_pending(&self) -> Result<()> {
        let _operation = self.begin("Reading saved update state…")?;
        let result = (|| {
            let pending = transaction::pending(self.install.as_ref().map_err(Clone::clone)?)?;
            if let Some((_, plan)) = &pending {
                self.status(match plan.phase {
                Phase::Staged if plan.armed => "Verified update is staged and authorized for your next full quit. Closing to tray does not install it.",
                Phase::Staged => "Verified update is staged; choose Install on full exit when convenient.",
                Phase::Installed => "Completed update journal; rollback retained. Running build information above is authoritative, even if another executable was installed later.",
                Phase::Applying => "Interrupted update journal; rollback retained. Do not discard until recovery is reviewed.",
                Phase::Failed => "Previous update failed; rollback retained. Inspect installation before retrying.",
            });
            } else {
                self.status("Not checked. No staged update.");
            }
            self.state.lock().unwrap().staged =
                pending.filter(|(_, p)| p.phase != Phase::Installed);
            Ok(())
        })();
        self.finish(result)
    }
    pub fn check(&self, transport: &dyn Transport, auto_download: bool) -> Result<()> {
        let _operation = self.begin("Checking private Harmony release metadata…")?;
        let result = (|| {
            if self.snapshot().staged.is_some() {
                return Err(Error::Conflict);
            }
            let candidate = release::check_release(transport, &self.current, &self.cancel)?;
            self.state.lock().unwrap().candidate = candidate.clone();
            match candidate {
                Some(c) if auto_download => self.download_inner(&c, transport),
                Some(c) => {
                    self.status(format!("Harmony {} (build {}) is available. Downloading never restarts the app or ends a call.", c.manifest.version, c.manifest.build_id));
                    Ok(())
                }
                None => {
                    self.status("No newer compatible stable Harmony release found (a release may not have been published yet).");
                    Ok(())
                }
            }
        })();
        self.finish(result)
    }
    fn download_inner(&self, c: &Candidate, transport: &dyn Transport) -> Result<()> {
        self.status("Downloading and verifying SHA-256 into owned staging…");
        let install = self.install.as_ref().map_err(Clone::clone)?;
        transaction::stage(install, c, transport, &self.cancel)?;
        let staged =
            transaction::pending(install)?.ok_or(Error::Invalid("staging read-back missing"))?;
        self.state.lock().unwrap().staged = Some(staged);
        self.status("Verified update staged. Install on full exit is opt-in; Harmony will not end your call or restart itself.");
        Ok(())
    }
    pub fn download(&self, transport: &dyn Transport) -> Result<()> {
        let _operation = self.begin("Preparing update download…")?;
        let result = (|| {
            let c = self
                .snapshot()
                .candidate
                .ok_or(Error::Invalid("check for an update first"))?;
            self.download_inner(&c, transport)
        })();
        self.finish(result)
    }
    pub fn arm(&self, enabled: bool) -> Result<()> {
        let _operation = self.begin("Saving install authorization…")?;
        let result = (|| {
            let install = self.install.as_ref().map_err(Clone::clone)?;
            transaction::arm(install, enabled)?;
            self.state.lock().unwrap().staged = transaction::pending(install)?;
            self.status(if enabled { "Installation authorized for your next natural full quit. No automatic quit, restart, or call termination. Close-to-tray is not a full quit." } else { "Installation authorization removed; update remains staged." });
            Ok(())
        })();
        self.finish(result)
    }
    pub fn discard(&self) -> Result<()> {
        let _operation = self.begin("Removing staged payload…")?;
        let result = (|| {
            transaction::discard(self.install.as_ref().map_err(Clone::clone)?)?;
            self.state.lock().unwrap().staged = None;
            self.status("Staged update removed. Any rollback and journal were retained.");
            Ok(())
        })();
        self.finish(result)
    }
    pub fn handoff_on_full_exit(&self) -> Result<()> {
        // Called only after a clean full event-loop exit, never on close-to-tray
        // or renderer restarts. A busy download is canceled, never activated.
        if self.busy() {
            self.cancel();
            return Ok(());
        }
        let state = self.snapshot();
        if !state
            .staged
            .as_ref()
            .is_some_and(|(_, p)| p.phase == Phase::Staged && p.armed)
        {
            return Ok(());
        }
        self.finish(transaction::spawn_helper(
            self.install.as_ref().map_err(Clone::clone)?,
        ))
    }
}
pub fn current_install() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let p = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .ok_or(Error::Unsupported)?;
        if !p.is_absolute() {
            return Err(Error::Invalid("absolute APPIMAGE required"));
        }
        Ok(p)
    }
    #[cfg(windows)]
    {
        std::env::current_exe().map_err(|_| Error::Io("find running executable"))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Err(Error::Unsupported)
    }
}
