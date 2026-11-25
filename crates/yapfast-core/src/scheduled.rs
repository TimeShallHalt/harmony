//! Durable device-local scheduled messages. No network client lives here.
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[path = "scheduled_lifecycle.rs"]
mod lifecycle;
#[path = "scheduled_storage.rs"]
mod storage;
#[cfg(any(test, feature = "test-fixtures"))]
pub use storage::fixtures;
pub use storage::{OsSecrets, SecretStore};
#[derive(Clone)]
struct Protection {
    id: String,
    key: Arc<storage::Key>,
    context: String,
}
impl std::fmt::Debug for Protection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[sealed scheduler snapshot]")
    }
}
use std::path::{Path, PathBuf};
#[derive(Clone, Debug)]
pub struct Claim(Arc<tokio::sync::watch::Sender<bool>>);
impl Default for Claim {
    fn default() -> Self {
        Self(Arc::new(tokio::sync::watch::channel(false).0))
    }
}
impl Claim {
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    pub fn revoke(&self) {
        self.0.send_replace(true);
    }
    pub async fn run<T>(&self, future: impl std::future::Future<Output = T>) -> Result<T, String> {
        let mut cancelled = self.0.subscribe();
        tokio::select! { biased;
            _ = cancelled.wait_for(|value| *value) => Err("Scheduled claim cancelled by account/storage lifecycle; delivery may be uncertain, never retry automatically.".into()),
            value = future => Ok(value),
        }
    }
}
pub trait Clock {
    fn now_ms(&self) -> i64;
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Pending,
    InFlight,
    Sent,
    Blocked(String),
    Uncertain(String),
    Cancelled,
}
#[derive(Clone, Debug)]
pub struct Dispatch {
    pub id: String,
    pub account: String,
    pub room: String,
    pub body: String,
    pub due_ms: i64,
    pub files: Vec<Snapshot>,
    pub claim: Claim,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub name: String,
    #[serde(skip)]
    pub path: PathBuf,
    pub bytes: u64,
    pub checksum: u64,
    pub spoiler: bool,
    #[serde(skip)]
    protection: Option<Protection>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub account: String,
    pub room: String,
    pub body: String,
    pub due_ms: i64,
    pub state: State,
    pub files: Vec<Snapshot>,
}
use std::io::{Read, Write};
#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    version: u32,
    namespace: String,
    next: u32,
    items: Vec<Item>,
}
pub struct Queue {
    saved: Saved,
    committed: Saved,
    fault: Option<String>,
    directory: PathBuf,
    _lock: std::fs::File,
    store: Arc<dyn SecretStore>,
    key_id: String,
    key: Arc<storage::Key>,
    claims: std::collections::HashMap<String, (String, Claim)>,
    initialized: bool,
}
impl Queue {
    pub fn open(root: &Path) -> Result<Self, String> {
        Self::open_with(root, Arc::new(OsSecrets))
    }
    pub fn open_with(root: &Path, store: Arc<dyn SecretStore>) -> Result<Self, String> {
        let directory = root.join("scheduled");
        private_directory(&directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("queue.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|_| "Scheduled messages are open in another app window.".to_string())?;
        let path = directory.join("queue.json");
        if directory
            .join("maintenance.json")
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            return Err("Scheduler storage transaction was interrupted. Dispatch paused. Use Recover storage to validate the committed generation and finish cleanup; no message is retried.".into());
        }
        let initialized = path.try_exists().map_err(|e| e.to_string())?;
        if !initialized
            && (lock.metadata().map_err(|e| e.to_string())?.len() > 0
                || directory
                    .join("attachments")
                    .try_exists()
                    .map_err(|e| e.to_string())?)
        {
            return Err("Scheduler manifest is missing from an existing store. Dispatch paused; restore the manifest/key backup. An empty replacement was not created.".into());
        }
        let (saved, key_id, key) = if initialized {
            let data = storage::read(&path, 8 * 1024 * 1024 + 256)?;
            let id = storage::key_id(&data)?.to_owned();
            let key = Arc::new(store.get(&id)?);
            let plain = storage::open(&data, &id, &key, "queue")?;
            let saved: Saved = serde_json::from_slice(&plain)
                .map_err(|_| "Invalid authenticated scheduled queue.".to_owned())?;
            if saved.version != 1 || saved.items.len() > 100 {
                return Err("Unsupported scheduled queue.".into());
            }
            (saved, id, key)
        } else {
            let (id, key) = storage::new_secret(&*store)?;
            (
                Saved {
                    version: 1,
                    namespace: uuid::Uuid::new_v4().simple().to_string()[..16].to_owned(),
                    next: 1,
                    items: vec![],
                },
                id,
                key,
            )
        };
        validate_saved(&saved)?;
        let mut queue = Self {
            committed: saved.clone(),
            saved,
            fault: None,
            directory,
            _lock: lock,
            store,
            key_id,
            key,
            claims: Default::default(),
            initialized,
        };
        let mut recovered = false;
        for item in &mut queue.saved.items {
            for (index, file) in item.files.iter_mut().enumerate() {
                if !safe_name(&file.name) {
                    return Err("Invalid scheduled attachment name.".into());
                }
                file.path = queue
                    .directory
                    .join("attachments")
                    .join(&queue.key_id)
                    .join(&item.id)
                    .join(format!("{index}.sealed"));
                file.protection = Some(Protection {
                    id: queue.key_id.clone(),
                    key: queue.key.clone(),
                    context: format!("attachment:{}:{index}", item.id),
                });
            }
            if item.state == State::InFlight {
                item.state = State::Uncertain("App stopped after dispatch. Check the channel; this message will not be retried.".into());
                recovered = true;
            }
        }
        if recovered || !queue.initialized {
            queue.persist()?;
        }
        // Resolved owned paths are deliberately absent from the manifest, but
        // rollback must retain them as part of the current committed snapshot.
        queue.committed = queue.saved.clone();
        queue
            ._lock
            .write_all(b"YFS2")
            .and_then(|_| queue._lock.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(queue)
    }
    fn persist(&mut self) -> Result<(), String> {
        if let Some(error) = &self.fault {
            self.saved = self.committed.clone();
            return Err(error.clone());
        }
        match self.write_snapshot() {
            Ok(()) => {
                self.committed = self.saved.clone();
                self.initialized = true;
                Ok(())
            }
            Err(error) => {
                self.saved = self.committed.clone();
                self.fault = Some(format!(
                    "Local queue could not be saved; dispatch paused until storage is recovered/reopened. Staged encrypted copies may remain until purge/key rotation: {error}"
                ));
                Err(self.fault.clone().unwrap())
            }
        }
    }
    fn verify_committed(&self) -> Result<(), String> {
        if self.store.get(&self.key_id)?.as_ref() != self.key.as_ref().as_ref() {
            return Err(
                "Scheduler OS key changed unexpectedly; dispatch paused without replacing data."
                    .into(),
            );
        }
        self.verify_manifest()
    }
    fn verify_manifest(&self) -> Result<(), String> {
        if self.initialized {
            let bytes = storage::read(&self.directory.join("queue.json"), 8 * 1024 * 1024 + 256)?;
            let plain = storage::open(&bytes, &self.key_id, &self.key, "queue")?;
            let committed = zeroize::Zeroizing::new(
                serde_json::to_vec(&self.committed).map_err(|e| e.to_string())?,
            );
            if *plain != *committed {
                return Err("Scheduler manifest changed outside its transaction; dispatch paused without replacing data.".into());
            }
        }
        Ok(())
    }
    fn write_snapshot(&self) -> Result<(), String> {
        self.verify_committed()?;
        let plain =
            zeroize::Zeroizing::new(serde_json::to_vec(&self.saved).map_err(|e| e.to_string())?);
        let data = storage::seal(&plain, &self.key_id, &self.key, "queue")?;
        let mut file =
            tempfile::NamedTempFile::new_in(&self.directory).map_err(|e| e.to_string())?;
        file.write_all(&data)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        file.persist(self.directory.join("queue.json"))
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        std::fs::File::open(&self.directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn dispatch_due(
        &mut self,
        account: &str,
        online: bool,
        clock: &(impl Clock + ?Sized),
        mut sink: impl FnMut(Dispatch) -> Result<(), String>,
    ) -> Result<usize, String> {
        if let Some(error) = &self.fault {
            return Err(error.clone());
        }
        if !online || self.saved.items.iter().any(|i| i.state == State::InFlight) {
            return Ok(0);
        }
        let mut due: Vec<_> = self
            .saved
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                i.account == account && i.state == State::Pending && i.due_ms <= clock.now_ms()
            })
            .map(|(index, i)| (i.due_ms, i.id.clone(), index))
            .collect();
        due.sort();
        let mut count = 0;
        // One bounded preflight/claim per wake, and only one scheduled upload
        // in flight. Its completion event wakes the next job; no due-now polling.
        for (_, _, index) in due.into_iter().take(1) {
            if clock
                .now_ms()
                .saturating_sub(self.saved.items[index].due_ms)
                > MAX_LATENESS_MS
            {
                self.saved.items[index].state = State::Blocked(
                    "More than 24 hours late. Review and explicitly reschedule.".into(),
                );
                self.persist()?;
                continue;
            }
            if let Err(error) = read_attachments(&self.saved.items[index].files) {
                self.saved.items[index].state = State::Blocked(format!(
                    "Scheduled attachment cannot be authenticated/read: {}",
                    error.chars().take(1000).collect::<String>()
                ));
                self.persist()?;
                continue;
            }
            self.saved.items[index].state = State::InFlight;
            // The durable claim must precede command acceptance. No auto retries.
            self.persist()?;
            let item = &self.saved.items[index];
            let claim = Claim::default();
            self.claims
                .insert(item.id.clone(), (item.account.clone(), claim.clone()));
            let command = Dispatch {
                id: item.id.clone(),
                account: item.account.clone(),
                room: item.room.clone(),
                body: item.body.clone(),
                due_ms: item.due_ms,
                files: item.files.clone(),
                claim,
            };
            match sink(command) {
                Ok(()) => count += 1,
                Err(error) => {
                    self.saved.items[index].state =
                        State::Blocked(error.chars().take(1000).collect());
                    self.persist()?;
                }
            }
        }
        Ok(count)
    }
    fn revoke_claims(&mut self, account: Option<&str>) {
        self.claims.retain(|_, (owner, claim)| {
            let revoke = account.is_none_or(|a| a == owner);
            if revoke {
                claim.revoke();
            }
            !revoke
        });
    }
    fn owned_index(&self, account: &str, id: &str) -> Result<usize, String> {
        self.saved
            .items
            .iter()
            .position(|i| i.id == id && i.account == account)
            .ok_or_else(|| "Scheduled message not found for this account.".into())
    }
    pub fn cancel(&mut self, account: &str, id: &str) -> Result<(), String> {
        let index = self.owned_index(account, id)?;
        if !matches!(
            self.saved.items[index].state,
            State::Pending | State::Blocked(_) | State::Cancelled
        ) {
            return Err(
                "Already dispatched; cancellation cannot unsend it. Check the channel.".into(),
            );
        }
        self.saved.items[index].state = State::Cancelled;
        self.persist()
    }
    pub fn edit(
        &mut self,
        account: &str,
        id: &str,
        body: &str,
        due_ms: i64,
        clock: &(impl Clock + ?Sized),
    ) -> Result<(), String> {
        let index = self.owned_index(account, id)?;
        let item = &self.saved.items[index];
        if !matches!(item.state, State::Pending | State::Blocked(_)) {
            return Err("Only undispatched messages can be edited or rescheduled.".into());
        }
        validate(account, &item.room, body, due_ms, item.files.len(), clock)?;
        let item = &mut self.saved.items[index];
        item.body = body.to_owned();
        item.due_ms = due_ms;
        item.state = State::Pending;
        self.persist()
    }
    pub fn complete(
        &mut self,
        account: &str,
        id: &str,
        result: Result<(), String>,
    ) -> Result<(), String> {
        let index = self.owned_index(account, id)?;
        if self.saved.items[index].state != State::InFlight {
            return Ok(());
        }
        if let Some((_, claim)) = self.claims.remove(id) {
            claim.revoke();
        }
        self.saved.items[index].state = match result {
            Ok(()) => State::Sent,
            Err(error) => State::Uncertain(format!(
                "{}. No automatic retry; check the channel before scheduling again.",
                error.chars().take(1000).collect::<String>()
            )),
        };
        self.persist()
    }
    /// One active timer, none while disconnected/empty. Reconcile wall time at
    /// most every five minutes only while pending, plus on account/network events.
    /// A monotonic timer firing is never itself permission to send early.
    pub fn next_delay(
        &self,
        account: &str,
        online: bool,
        clock: &(impl Clock + ?Sized),
    ) -> Option<std::time::Duration> {
        if !online
            || self.fault.is_some()
            || self.saved.items.iter().any(|i| i.state == State::InFlight)
        {
            return None;
        }
        let due = self
            .saved
            .items
            .iter()
            .filter(|i| i.account == account && i.state == State::Pending)
            .map(|i| i.due_ms)
            .min()?;
        Some(std::time::Duration::from_millis(
            due.saturating_sub(clock.now_ms()).clamp(0, 300_000) as u64,
        ))
    }
    pub fn interrupt(&mut self, account: &str, reason: &str) -> Result<usize, String> {
        self.revoke_claims(Some(account));
        let mut count = 0;
        for item in &mut self.saved.items {
            if item.account == account && item.state == State::InFlight {
                item.state = State::Uncertain(format!(
                    "{}. Delivery may have occurred; check the channel. No automatic retry.",
                    reason.chars().take(1000).collect::<String>()
                ));
                count += 1;
            }
        }
        if count > 0 {
            self.persist()?;
        }
        Ok(count)
    }
    pub fn dismiss(&mut self, account: &str, id: &str) -> Result<(), String> {
        let index = self.owned_index(account, id)?;
        if !matches!(
            self.saved.items[index].state,
            State::Sent | State::Cancelled | State::Uncertain(_)
        ) {
            return Err("Cancel a pending message before dismissing its record.".into());
        }
        self.saved.items.remove(index);
        self.persist()?;
        let directory = self
            .directory
            .join("attachments")
            .join(&self.key_id)
            .join(id);
        if directory.exists() {
            std::fs::remove_dir_all(directory).map_err(|e| {
                format!("Record dismissed, but its local attachment copy could not be removed: {e}")
            })?;
        }
        Ok(())
    }
    pub fn problem(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    pub fn items(&self) -> &[Item] {
        &self.saved.items
    }
    /// Freeze exactly the reviewed attachment bytes: checksum validation and
    /// writing the owned copy use the same bounded read, closing a review/save race.
    pub fn schedule_reviewed(
        &mut self,
        account: &str,
        room: &str,
        body: &str,
        due_ms: i64,
        files: &[Snapshot],
        clock: &(impl Clock + ?Sized),
    ) -> Result<String, String> {
        let sources: Vec<_> = files.iter().map(|f| (f.path.clone(), f.spoiler)).collect();
        self.schedule_checked(account, room, body, due_ms, &sources, Some(files), clock)
    }
    pub fn schedule(
        &mut self,
        account: &str,
        room: &str,
        body: &str,
        due_ms: i64,
        files: &[(PathBuf, bool)],
        clock: &(impl Clock + ?Sized),
    ) -> Result<String, String> {
        self.schedule_checked(account, room, body, due_ms, files, None, clock)
    }
    fn schedule_checked(
        &mut self,
        account: &str,
        room: &str,
        body: &str,
        due_ms: i64,
        files: &[(PathBuf, bool)],
        reviewed: Option<&[Snapshot]>,
        clock: &(impl Clock + ?Sized),
    ) -> Result<String, String> {
        validate(account, room, body, due_ms, files.len(), clock)?;
        if self.saved.items.len() >= 100 {
            return Err(
                "The local queue holds at most 100 messages; dismiss old records first.".into(),
            );
        }
        let id = format!("{}{:08x}", self.saved.namespace, self.saved.next);
        self.saved.next = self
            .saved
            .next
            .checked_add(1)
            .ok_or("Scheduled ID limit reached.")?;
        // Reserve the deterministic ID durably before copying, so a crash
        // cannot reuse an attachment directory from an uncommitted message.
        self.persist()?;
        let snapshots = self.snapshot(&id, files, reviewed)?;
        self.saved.items.push(Item {
            id: id.clone(),
            account: account.into(),
            room: room.into(),
            body: body.into(),
            due_ms,
            state: State::Pending,
            files: snapshots,
        });
        if let Err(error) = self.persist() {
            // Rename may already have committed before a directory sync error.
            // Keep ciphertext until authenticated generation cleanup, never
            // delete bytes that the on-disk manifest could now reference.
            return Err(error);
        }
        Ok(id)
    }
    fn snapshot(
        &self,
        id: &str,
        files: &[(PathBuf, bool)],
        reviewed: Option<&[Snapshot]>,
    ) -> Result<Vec<Snapshot>, String> {
        let directory = self
            .directory
            .join("attachments")
            .join(&self.key_id)
            .join(id);
        if files.is_empty() {
            return Ok(vec![]);
        }
        private_directory(&self.directory.join("attachments"))?;
        private_directory(&self.directory.join("attachments").join(&self.key_id))?;
        private_directory(&directory)?;
        let result = (|| {
            let mut snapshots = Vec::new();
            let mut total = 0;
            for (index, (source, spoiler)) in files.iter().enumerate() {
                let name = source
                    .file_name()
                    .and_then(|n| n.to_str())
                    .filter(|n| safe_name(n))
                    .ok_or("Choose an attachment with a short filename.")?
                    .to_owned();
                let data = read_bounded(source)?;
                if let Some(expected) = reviewed.and_then(|files| files.get(index))
                    && (expected.name != name
                        || expected.bytes != data.len() as u64
                        || expected.checksum != checksum(&data))
                {
                    return Err(
                        "Scheduled attachment changed after review; review again before saving."
                            .into(),
                    );
                }
                total += data.len() as u64;
                if total > MAX_TOTAL_BYTES {
                    return Err("Scheduled attachments total at most 25 MiB.".into());
                }
                let path = directory.join(format!("{index}.sealed"));
                let protection = Protection {
                    id: self.key_id.clone(),
                    key: self.key.clone(),
                    context: format!("attachment:{id}:{index}"),
                };
                let ciphertext =
                    storage::seal(&data, &protection.id, &protection.key, &protection.context)?;
                let mut output = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|e| e.to_string())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    output
                        .set_permissions(std::fs::Permissions::from_mode(0o600))
                        .map_err(|e| e.to_string())?;
                }
                output
                    .write_all(&ciphertext)
                    .and_then(|_| output.sync_all())
                    .map_err(|e| e.to_string())?;
                snapshots.push(Snapshot {
                    name,
                    path,
                    bytes: data.len() as u64,
                    checksum: checksum(&data),
                    spoiler: *spoiler,
                    protection: Some(protection),
                });
            }
            #[cfg(unix)]
            {
                for folder in [&directory, &self.directory.join("attachments")] {
                    std::fs::File::open(folder)
                        .and_then(|f| f.sync_all())
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(snapshots)
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&directory);
        }
        result
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        self.revoke_claims(None);
    }
}
fn private_directory(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err("Scheduled storage must be an owned local directory, not a link.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn validate_saved(saved: &Saved) -> Result<(), String> {
    if saved.version != 1 || saved.items.len() > 100 {
        return Err("Unsupported scheduled queue.".into());
    }
    let hex =
        |text: &str, len: usize| text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit());
    if !hex(&saved.namespace, 16) || saved.next == 0 {
        return Err("Invalid scheduled ID namespace.".into());
    }
    let mut seen = std::collections::HashSet::new();
    for item in &saved.items {
        if !hex(&item.id, 24)
            || !item.id.starts_with(&saved.namespace)
            || !seen.insert(&item.id)
            || u32::from_str_radix(&item.id[16..], 16).map_or(true, |n| n == 0 || n >= saved.next)
            || item.files.len() > 10
            || item
                .files
                .iter()
                .any(|f| f.bytes > MAX_FILE_BYTES || !safe_name(&f.name))
            || item.files.iter().map(|f| f.bytes).sum::<u64>() > MAX_TOTAL_BYTES
        {
            return Err("Invalid or duplicate scheduled message ID/attachments.".into());
        }
        // Validate payload shape independently of current wall time: overdue
        // messages remain visible and are handled by the lateness policy.
        validate(
            &item.account,
            &item.room,
            &item.body,
            60_000,
            item.files.len(),
            &FixedClock(0),
        )?;
        if item.due_ms < 0 || item.due_ms > 253_402_300_799_000 {
            return Err("Invalid scheduled timestamp.".into());
        }
    }
    Ok(())
}
struct FixedClock(i64);
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 25 * 1024 * 1024;
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 120
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
}
fn checksum(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let problem =
        || "Scheduled attachment is unavailable, not a regular file, or exceeds 10 MiB.".to_owned();
    let meta = std::fs::symlink_metadata(path).map_err(|_| problem())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_FILE_BYTES {
        return Err(problem());
    }
    let file = std::fs::File::open(path).map_err(|_| problem())?;
    if !file.metadata().map_err(|_| problem())?.is_file() {
        return Err(problem());
    }
    let mut data = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|_| problem())?;
    if data.len() as u64 > MAX_FILE_BYTES {
        return Err(problem());
    }
    Ok(data)
}
/// Rechecked by the authorized backend immediately before uploading; no path is
/// accepted from the manifest, only paths rebuilt under the queue's own root.
pub fn inspect_sources(files: &[(PathBuf, bool)]) -> Result<Vec<Snapshot>, String> {
    if files.len() > 10 {
        return Err("Attach at most 10 files.".into());
    }
    let mut total = 0;
    let mut inspected = Vec::new();
    for (path, spoiler) in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| safe_name(n))
            .ok_or("Invalid scheduled attachment name.")?
            .to_owned();
        let data = read_bounded(path)?;
        total += data.len() as u64;
        if total > MAX_TOTAL_BYTES {
            return Err("Scheduled attachments total at most 25 MiB.".into());
        }
        inspected.push(Snapshot {
            name,
            path: path.clone(),
            bytes: data.len() as u64,
            checksum: checksum(&data),
            spoiler: *spoiler,
            protection: None,
        });
    }
    Ok(inspected)
}
pub fn read_attachments(files: &[Snapshot]) -> Result<Vec<(String, Vec<u8>)>, String> {
    if files.len() > 10 {
        return Err("Too many scheduled attachments.".into());
    }
    let mut total = 0;
    let mut result = Vec::new();
    for file in files {
        let data = if let Some(p) = &file.protection {
            storage::open(
                &storage::read(&file.path, MAX_FILE_BYTES + 256)?,
                &p.id,
                &p.key,
                &p.context,
            )?
            .to_vec()
        } else {
            read_bounded(&file.path)?
        };
        total += data.len() as u64;
        if total > MAX_TOTAL_BYTES
            || data.len() as u64 != file.bytes
            || checksum(&data) != file.checksum
        {
            return Err("Scheduled attachment changed; cancel and attach a new copy.".into());
        }
        let name = if file.spoiler && !file.name.starts_with("SPOILER_") {
            format!("SPOILER_{}", file.name)
        } else {
            file.name.clone()
        };
        result.push((name, data));
    }
    Ok(result)
}
/// Discord's existing @silent prefix is control text, not message content.
pub fn sending_text(body: &str) -> (&str, bool) {
    let silent = body
        .strip_prefix("@silent")
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
    (silent.map_or(body, str::trim_start), silent.is_some())
}
pub const MAX_LATENESS_MS: i64 = 86_400_000;

pub fn validate(
    account: &str,
    room: &str,
    body: &str,
    due_ms: i64,
    files: usize,
    clock: &(impl Clock + ?Sized),
) -> Result<(), String> {
    validate_contents(account, room, body, files)?;
    let delay = due_ms
        .checked_sub(clock.now_ms())
        .ok_or("Invalid due time.")?;
    if !(1_000..=365 * 86_400_000).contains(&delay) {
        return Err("Choose a time at least a second and at most 365 days ahead.".into());
    }
    Ok(())
}

pub(crate) fn validate_contents(
    account: &str,
    room: &str,
    body: &str,
    files: usize,
) -> Result<(), String> {
    let numeric =
        |id: &str| !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit());
    let parts: Vec<_> = room.split(':').collect();
    if !numeric(account)
        || parts.len() != 3
        || parts[0] != "discord"
        || !(parts[1] == "@me" || numeric(parts[1]))
        || !numeric(parts[2])
    {
        return Err("Choose an authorized Discord account and channel.".into());
    }
    let (content, _) = sending_text(body);
    if (content.trim().is_empty() && files == 0)
        || content.chars().count() > 2000
        || body.len() > 16_000
    {
        return Err("Use 1–2,000 characters, or attach a file.".into());
    }
    if files > 10 {
        return Err("Attach at most 10 files.".into());
    }
    Ok(())
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}
/// Local resolves the operating system timezone at review time, not at send
/// time. Reject DST folds/gaps rather than guessing; offsets never follow DST.
pub fn parse_due(local: &str, zone: &str) -> Result<i64, String> {
    use chrono::TimeZone;
    if local.len() != 16 {
        return Err("Enter YYYY-MM-DD HH:MM.".into());
    }
    let date = chrono::NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M")
        .map_err(|_| "Enter a valid date as YYYY-MM-DD HH:MM.".to_owned())?;
    if zone == "Local" {
        return chrono::Local.from_local_datetime(&date).single().map(|t| t.timestamp_millis()).ok_or_else(|| "That local time is ambiguous or nonexistent (daylight saving). Choose an explicit UTC offset.".into());
    }
    if zone.contains('/') {
        let tz: chrono_tz::Tz = zone.parse().map_err(|_| {
            "Unknown IANA timezone. Choose a region/city from the zone list.".to_owned()
        })?;
        return tz.from_local_datetime(&date).single().map(|t|t.timestamp_millis()).ok_or_else(||"That IANA time is ambiguous or nonexistent (daylight saving). Choose another time or an explicit UTC offset; no offset is guessed.".into());
    }
    let offset = if zone == "UTC" {
        0
    } else {
        let b = zone.as_bytes();
        if b.len() != 6
            || !matches!(b[0], b'+' | b'-')
            || b[3] != b':'
            || ![b[1], b[2], b[4], b[5]].iter().all(u8::is_ascii_digit)
        {
            return Err(
                "Timezone must be Local, UTC, an IANA region/city, or an explicit +HH:MM / -HH:MM offset.".into(),
            );
        }
        let hours = i32::from(b[1] - b'0') * 10 + i32::from(b[2] - b'0');
        let minutes = i32::from(b[4] - b'0') * 10 + i32::from(b[5] - b'0');
        if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
            return Err("UTC offset must be between -14:00 and +14:00.".into());
        }
        (hours * 3600 + minutes * 60) * if b[0] == b'-' { -1 } else { 1 }
    };
    chrono::FixedOffset::east_opt(offset)
        .and_then(|z| z.from_local_datetime(&date).single())
        .map(|t| t.timestamp_millis())
        .ok_or_else(|| "Invalid due time.".into())
}
pub fn timezone_choices() -> Vec<String> {
    let mut zones: Vec<_> = chrono_tz::TZ_VARIANTS
        .iter()
        .map(|z| z.name())
        .filter(|z| z.contains('/'))
        .map(str::to_owned)
        .collect();
    zones.sort();
    zones.dedup();
    zones.insert(0, "UTC".into());
    zones.insert(0, "Local".into());
    zones
}
pub fn utc_label(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Invalid date".into())
}
pub fn local_input(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_manifest(root: &Path) -> Vec<u8> {
        let data = std::fs::read(root.join("scheduled/queue.json")).unwrap();
        let id = storage::key_id(&data).unwrap();
        storage::open(&data, id, &fixtures::keys().get(id).unwrap(), "queue")
            .unwrap()
            .to_vec()
    }
    struct FakeClock(i64);
    impl Clock for FakeClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }
    #[test]
    fn queue_and_owned_attachment_are_authenticated_ciphertext_at_rest() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("private-fixture-name.txt");
        let secret = b"generated fixture attachment never saved in plaintext";
        std::fs::write(&source, secret).unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "generated fixture secret body",
            1_060_000,
            &[(source, false)],
            &FakeClock(1_000_000),
        )
        .unwrap();
        let manifest = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
        let body = b"generated fixture secret body";
        assert!(
            !manifest.windows(body.len()).any(|v| v == body),
            "queue body must be encrypted"
        );
        let owned = std::fs::read(&q.items()[0].files[0].path).unwrap();
        assert!(
            !owned.windows(secret.len()).any(|v| v == secret),
            "owned bytes must be encrypted"
        );
        assert_eq!(read_attachments(&q.items()[0].files).unwrap()[0].1, secret);
    }
    #[test]
    fn unavailable_key_stops_live_dispatch_without_replacing_ciphertext_or_claiming() {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(fixtures::Keys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "fixture",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        let before = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
        keys.0.lock().unwrap().clear();
        let mut called = false;
        assert!(
            q.dispatch_due("123", true, &FakeClock(1_060_000), |_| {
                called = true;
                Ok(())
            })
            .is_err()
        );
        assert!(!called);
        assert_eq!(q.items()[0].state, State::Pending);
        assert_eq!(
            std::fs::read(root.path().join("scheduled/queue.json")).unwrap(),
            before
        );
        assert!(q.next_delay("123", true, &FakeClock(1_060_000)).is_none());
        drop(q);
        assert!(Queue::open_with(root.path(), keys).is_err());
    }
    #[test]
    fn interrupted_rotation_marker_blocks_reopen_without_touching_committed_data() {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "retained",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        let before = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
        drop(q);
        std::fs::write(
            root.path().join("scheduled/maintenance.json"),
            b"interrupted fixture transaction",
        )
        .unwrap();
        assert!(
            Queue::open_with(root.path(), fixtures::keys()).is_err(),
            "incomplete key transaction must not silently dispatch"
        );
        assert_eq!(
            std::fs::read(root.path().join("scheduled/queue.json")).unwrap(),
            before
        );
    }
    #[test]
    fn rotation_commits_new_ciphertext_before_retiring_old_key_and_preserves_ids_and_bytes() {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(fixtures::Keys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        let source = root.path().join("fixture.txt");
        std::fs::write(&source, b"generated rotation fixture").unwrap();
        let id = q
            .schedule(
                "123",
                "discord:10:11",
                "keep",
                1_060_000,
                &[(source, false)],
                &FakeClock(1_000_000),
            )
            .unwrap();
        let old = q.key_id.clone();
        let old_path = q.items()[0].files[0].path.clone();
        q.rotate().unwrap();
        assert_ne!(q.key_id, old);
        assert!(keys.get(&old).is_err());
        assert!(!old_path.exists());
        assert!(!root.path().join("scheduled/maintenance.json").exists());
        assert_eq!(q.items()[0].id, id);
        assert_eq!(
            read_attachments(&q.items()[0].files).unwrap()[0].1,
            b"generated rotation fixture"
        );
        drop(q);
        let q = Queue::open_with(root.path(), keys).unwrap();
        assert_eq!(q.items()[0].id, id);
        assert_eq!(q.items()[0].state, State::Pending);
    }
    #[test]
    fn purge_revokes_claims_deletes_only_owned_content_and_never_replays_ambiguous_send() {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(fixtures::Keys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        let source = root.path().join("fixture.txt");
        std::fs::write(&source, b"owned sensitive fixture").unwrap();
        let id = q
            .schedule(
                "123",
                "discord:10:11",
                "purge",
                1_060_000,
                &[(source.clone(), false)],
                &FakeClock(1_000_000),
            )
            .unwrap();
        let other = q
            .schedule(
                "456",
                "discord:20:21",
                "preserve",
                1_060_000,
                &[(source, false)],
                &FakeClock(1_000_000),
            )
            .unwrap();
        let mut accepted = None;
        q.dispatch_due("123", true, &FakeClock(1_060_000), |payload| {
            accepted = Some(payload);
            Ok(())
        })
        .unwrap();
        let accepted = accepted.unwrap();
        assert!(!accepted.claim.is_cancelled());
        let old_path = accepted.files[0].path.clone();
        q.purge("123").unwrap();
        assert!(
            accepted.claim.is_cancelled(),
            "buffered or running backend must release this sensitive payload"
        );
        assert!(!old_path.exists());
        assert_eq!(q.items().len(), 1);
        assert_eq!(q.items()[0].id, other);
        assert_eq!(q.items()[0].body, "preserve");
        assert!(
            q.complete("123", &id, Ok(())).is_err(),
            "late result cannot restore purged records"
        );
        q.dispatch_due("123", true, &FakeClock(1_060_000), |_| {
            panic!("purged claim replayed")
        })
        .unwrap();
        drop(q);
        let q = Queue::open_with(root.path(), keys).unwrap();
        assert_eq!(q.items().len(), 1);
        assert_eq!(q.items()[0].account, "456");
        assert_eq!(
            read_attachments(&q.items()[0].files).unwrap()[0].1,
            b"owned sensitive fixture"
        );
    }
    #[test]
    fn explicit_legacy_migration_preserves_ids_and_claim_uncertainty_but_requires_review() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("scheduled");
        std::fs::create_dir_all(&directory).unwrap();
        let id = "0123456789abcdef00000001";
        let folder = directory.join("attachments").join(id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("0-fixture.txt"), b"generated legacy fixture").unwrap();
        let saved = Saved {
            version: 1,
            namespace: "0123456789abcdef".into(),
            next: 2,
            items: vec![Item {
                id: id.into(),
                account: "123".into(),
                room: "discord:10:11".into(),
                body: "generated legacy body".into(),
                due_ms: 1_060_000,
                state: State::InFlight,
                files: vec![Snapshot {
                    name: "fixture.txt".into(),
                    path: PathBuf::new(),
                    bytes: b"generated legacy fixture".len() as u64,
                    checksum: checksum(b"generated legacy fixture"),
                    spoiler: false,
                    protection: None,
                }],
            }],
        };
        let legacy = serde_json::to_vec(&saved).unwrap();
        std::fs::write(directory.join("queue.json"), &legacy).unwrap();
        assert!(
            Queue::open_with(root.path(), fixtures::keys()).is_err(),
            "legacy must not dispatch or silently downgrade"
        );
        assert_eq!(std::fs::read(directory.join("queue.json")).unwrap(), legacy);
        Queue::migrate(root.path(), fixtures::keys()).unwrap();
        let q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        assert_eq!(q.items()[0].id, id);
        assert!(matches!(q.items()[0].state, State::Uncertain(_)));
        assert_eq!(
            read_attachments(&q.items()[0].files).unwrap()[0].1,
            b"generated legacy fixture"
        );
        assert!(!folder.exists());
        assert_ne!(std::fs::read(directory.join("queue.json")).unwrap(), legacy);
    }
    #[test]
    fn iana_zones_convert_winter_summer_and_non_hour_offsets_and_reject_dst_folds_gaps() {
        assert_eq!(
            parse_due("2026-01-15 12:00", "America/New_York").unwrap(),
            parse_due("2026-01-15 17:00", "UTC").unwrap()
        );
        assert_eq!(
            parse_due("2026-07-15 12:00", "America/New_York").unwrap(),
            parse_due("2026-07-15 16:00", "UTC").unwrap()
        );
        assert_eq!(
            parse_due("2026-07-15 12:00", "Asia/Kathmandu").unwrap(),
            parse_due("2026-07-15 06:15", "UTC").unwrap()
        );
        for (date, zone) in [
            ("2026-03-08 02:30", "America/New_York"),
            ("2026-11-01 01:30", "America/New_York"),
            ("2026-03-29 01:30", "Europe/London"),
            ("2026-10-25 01:30", "Europe/London"),
            ("2026-07-15 12:00", "Mars/Olympus"),
            ("2026-07-15 12:00", "EST"),
        ] {
            assert!(
                parse_due(date, zone).is_err(),
                "must not guess: {date} {zone}"
            );
        }
    }
    #[test]
    fn corrupted_live_manifest_is_not_overwritten_by_claim_or_dispatched() {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "retained",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        let path = root.path().join("scheduled/queue.json");
        let mut bytes = std::fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        let mut called = false;
        assert!(
            q.dispatch_due("123", true, &FakeClock(1_060_000), |_| {
                called = true;
                Ok(())
            })
            .is_err()
        );
        assert!(!called);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert_eq!(q.items()[0].state, State::Pending);
    }
    #[derive(Default)]
    struct FaultKeys {
        keys: fixtures::Keys,
        deny_put: std::sync::atomic::AtomicBool,
        deny_remove: std::sync::atomic::AtomicBool,
    }
    impl SecretStore for FaultKeys {
        fn get(&self, id: &str) -> Result<storage::Key, String> {
            self.keys.get(id)
        }
        fn put(&self, id: &str, key: &storage::Key) -> Result<(), String> {
            if self.deny_put.load(std::sync::atomic::Ordering::SeqCst) {
                Err("Fixture secure facility unavailable".into())
            } else {
                self.keys.put(id, key)
            }
        }
        fn remove(&self, id: &str) -> Result<(), String> {
            if self.deny_remove.load(std::sync::atomic::Ordering::SeqCst) {
                Err("Fixture interrupted key retirement".into())
            } else {
                self.keys.remove(id)
            }
        }
    }
    #[test]
    fn failed_rotation_before_commit_keeps_old_generation_and_recovery_does_not_guess() {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(FaultKeys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        let source = root.path().join("fixture.txt");
        std::fs::write(&source, b"transaction fixture").unwrap();
        let id = q
            .schedule(
                "123",
                "discord:10:11",
                "retain",
                1_060_000,
                &[(source, false)],
                &FakeClock(1_000_000),
            )
            .unwrap();
        let path = q.items()[0].files[0].path.clone();
        let original = std::fs::read(&path).unwrap();
        let manifest = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
        let old = q.key_id.clone();
        std::fs::write(&path, b"corrupt fixture ciphertext").unwrap();
        assert!(q.rotate().is_err());
        assert!(q.problem().is_some());
        assert!(keys.get(&old).is_ok());
        assert_eq!(
            std::fs::read(root.path().join("scheduled/queue.json")).unwrap(),
            manifest
        );
        assert!(
            q.dispatch_due("123", true, &FakeClock(1_060_000), |_| panic!(
                "rotation failed"
            ))
            .is_err()
        );
        drop(q);
        assert!(
            Queue::recover(root.path(), keys.clone()).is_err(),
            "cannot delete losing generation until committed files authenticate"
        );
        std::fs::write(&path, original).unwrap();
        Queue::recover(root.path(), keys.clone()).unwrap();
        let q = Queue::open_with(root.path(), keys).unwrap();
        assert_eq!(q.key_id, old);
        assert_eq!(q.items()[0].id, id);
        assert_eq!(
            read_attachments(&q.items()[0].files).unwrap()[0].1,
            b"transaction fixture"
        );
    }
    #[test]
    fn interrupted_postcommit_key_retirement_is_visible_and_explicit_recovery_preserves_uncertain_claim()
     {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(FaultKeys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        let id = q
            .schedule(
                "123",
                "discord:10:11",
                "claim",
                1_060_000,
                &[],
                &FakeClock(1_000_000),
            )
            .unwrap();
        let old = q.key_id.clone();
        let mut accepted = None;
        q.dispatch_due("123", true, &FakeClock(1_060_000), |p| {
            accepted = Some(p);
            Ok(())
        })
        .unwrap();
        keys.deny_remove
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(q.rotate().is_err());
        assert!(accepted.unwrap().claim.is_cancelled());
        assert!(q.problem().is_some());
        assert_ne!(q.key_id, old);
        assert!(matches!(q.items()[0].state, State::Uncertain(_)));
        assert_eq!(q.next_delay("123", true, &FakeClock(1_060_000)), None);
        let new = q.key_id.clone();
        drop(q);
        assert!(Queue::open_with(root.path(), keys.clone()).is_err());
        assert!(Queue::recover(root.path(), keys.clone()).is_err());
        assert!(keys.get(&old).is_ok());
        assert!(keys.get(&new).is_ok());
        keys.deny_remove
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Queue::recover(root.path(), keys.clone()).unwrap();
        assert!(keys.get(&old).is_err());
        let mut q = Queue::open_with(root.path(), keys).unwrap();
        assert_eq!(q.items()[0].id, id);
        assert!(matches!(q.items()[0].state, State::Uncertain(_)));
        q.dispatch_due("123", true, &FakeClock(1_060_000), |_| {
            panic!("ambiguous send replay")
        })
        .unwrap();
    }
    #[test]
    fn unavailable_migration_key_preserves_generated_legacy_manifest_and_files() {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(FaultKeys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "legacy fixture",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        let legacy = serde_json::to_vec(&q.saved).unwrap();
        drop(q);
        let path = root.path().join("scheduled/queue.json");
        std::fs::write(&path, &legacy).unwrap();
        keys.deny_put
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(Queue::migrate(root.path(), keys.clone()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), legacy);
        keys.deny_put
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Queue::migrate(root.path(), keys.clone()).unwrap();
        let q = Queue::open_with(root.path(), keys).unwrap();
        assert!(
            matches!(&q.items()[0].state,State::Blocked(reason) if reason.contains("review")||reason.contains("Review"))
        );
    }
    #[test]
    fn authentication_rejects_wrong_keys_and_swapped_attachment_contexts_without_replacement() {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(fixtures::Keys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        let a = root.path().join("a.txt");
        let b = root.path().join("b.txt");
        std::fs::write(&a, b"same fixture bytes").unwrap();
        std::fs::write(&b, b"same fixture bytes").unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "fixture",
            1_060_000,
            &[(a, false), (b, false)],
            &FakeClock(1_000_000),
        )
        .unwrap();
        let f0 = q.items()[0].files[0].path.clone();
        let f1 = q.items()[0].files[1].path.clone();
        let bytes = std::fs::read(&f0).unwrap();
        std::fs::write(f1, bytes).unwrap();
        q.dispatch_due("123", true, &FakeClock(1_060_000), |_| {
            panic!("swapped authenticated context")
        })
        .unwrap();
        assert!(matches!(q.items()[0].state, State::Blocked(_)));
        let id = q.key_id.clone();
        let manifest = std::fs::read(root.path().join("scheduled/queue.json")).unwrap();
        drop(q);
        keys.put(&id, &zeroize::Zeroizing::new([99; 32])).unwrap();
        assert!(Queue::open_with(root.path(), keys).is_err());
        assert_eq!(
            std::fs::read(root.path().join("scheduled/queue.json")).unwrap(),
            manifest
        );
    }
    #[test]
    fn revoking_running_claim_drops_the_sensitive_provider_future_without_waiting_or_retry() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Sensitive(Arc<AtomicBool>);
        impl Drop for Sensitive {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        runtime.block_on(async {
            let claim = Claim::default();
            let running = claim.clone();
            let release = dropped.clone();
            let (tx, rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                running
                    .run(async move {
                        let _sensitive = Sensitive(release);
                        tx.send(()).unwrap();
                        std::future::pending::<()>().await;
                    })
                    .await
            });
            rx.await.unwrap();
            claim.revoke();
            assert!(task.await.unwrap().is_err());
        });
        assert!(dropped.load(Ordering::SeqCst));
    }
    #[test]
    fn claims_are_serial_even_if_a_caller_switches_accounts_without_interrupting() {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "first",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        q.schedule(
            "456",
            "discord:20:21",
            "second",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        q.dispatch_due("123", true, &FakeClock(1_060_000), |_| Ok(()))
            .unwrap();
        let mut called = false;
        assert_eq!(
            q.dispatch_due("456", true, &FakeClock(1_060_000), |_| {
                called = true;
                Ok(())
            })
            .unwrap(),
            0
        );
        assert!(!called);
        assert!(q.next_delay("456", true, &FakeClock(1_060_000)).is_none());
    }
    #[test]
    fn missing_durable_manifest_never_initializes_an_empty_replacement_queue() {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(fixtures::Keys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "retained",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        drop(q);
        let path = root.path().join("scheduled/queue.json");
        let saved = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(
            Queue::open_with(root.path(), keys.clone()).is_err(),
            "missing is corruption, not an empty queue"
        );
        assert!(!path.exists());
        std::fs::write(&path, saved).unwrap();
        let q = Queue::open_with(root.path(), keys).unwrap();
        assert_eq!(q.items()[0].body, "retained");
    }
    #[test]
    fn key_creation_failure_leaves_a_durable_pause_and_recovery_requires_pending_review() {
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(FaultKeys::default());
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "retain",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        keys.deny_put
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(q.purge("123").is_err());
        drop(q);
        assert!(
            root.path().join("scheduled/maintenance.json").exists(),
            "failed purge/rotation must remain paused across restart"
        );
        assert!(Queue::open_with(root.path(), keys.clone()).is_err());
        keys.deny_put
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Queue::recover(root.path(), keys.clone()).unwrap();
        let mut q = Queue::open_with(root.path(), keys).unwrap();
        assert_eq!(q.items()[0].body, "retain");
        assert!(
            matches!(q.items()[0].state, State::Blocked(_)),
            "a failed purge must never resume earlier send authorization on recovery"
        );
        q.dispatch_due("123", true, &FakeClock(1_060_000), |_| {
            panic!("purge intent resurrected a send")
        })
        .unwrap();
    }
    #[test]
    fn failed_final_save_keeps_staged_ciphertext_until_a_verified_generation_cleanup() {
        struct LateDenied {
            keys: fixtures::Keys,
            root: PathBuf,
            deny: std::sync::atomic::AtomicBool,
        }
        impl SecretStore for LateDenied {
            fn get(&self, id: &str) -> Result<storage::Key, String> {
                if self.deny.load(std::sync::atomic::Ordering::SeqCst)
                    && self.root.join("scheduled/attachments").join(id).is_dir()
                {
                    return Err("Fixture final-write key unavailable".into());
                }
                self.keys.get(id)
            }
            fn put(&self, id: &str, key: &storage::Key) -> Result<(), String> {
                self.keys.put(id, key)
            }
            fn remove(&self, id: &str) -> Result<(), String> {
                self.keys.remove(id)
            }
        }
        let root = tempfile::tempdir().unwrap();
        let keys = Arc::new(LateDenied {
            keys: Default::default(),
            root: root.path().into(),
            deny: std::sync::atomic::AtomicBool::new(true),
        });
        let mut q = Queue::open_with(root.path(), keys.clone()).unwrap();
        let source = root.path().join("fixture.txt");
        std::fs::write(&source, b"staged fixture").unwrap();
        let path = root
            .path()
            .join("scheduled/attachments")
            .join(&q.key_id)
            .join(format!("{}00000001", q.saved.namespace))
            .join("0.sealed");
        assert!(
            q.schedule(
                "123",
                "discord:10:11",
                "retain",
                1_060_000,
                &[(source, false)],
                &FakeClock(1_000_000)
            )
            .is_err()
        );
        assert!(q.items().is_empty());
        assert!(q.problem().is_some());
        assert!(
            path.is_file(),
            "an uncertain final commit must not delete snapshot bytes it may already reference"
        );
        assert!(std::fs::read(&path).unwrap().starts_with(b"YFS2"));
        drop(q);
        keys.deny.store(false, std::sync::atomic::Ordering::SeqCst);
        let mut q = Queue::open_with(root.path(), keys).unwrap();
        q.purge("123").unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn legacy_migration_rejects_unreferenced_plaintext_instead_of_deleting_or_leaving_it_unsealed()
    {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "legacy fixture",
            1_060_000,
            &[],
            &FakeClock(1_000_000),
        )
        .unwrap();
        let legacy = serde_json::to_vec(&q.saved).unwrap();
        let id = q.items()[0].id.clone();
        drop(q);
        let path = root.path().join("scheduled/queue.json");
        std::fs::write(&path, &legacy).unwrap();
        let folder = root.path().join("scheduled/attachments").join(&id);
        std::fs::create_dir_all(&folder).unwrap();
        let orphan = folder.join("unreferenced.txt");
        std::fs::write(&orphan, b"generated unknown legacy bytes").unwrap();
        assert!(
            Queue::migrate(root.path(), fixtures::keys()).is_err(),
            "never delete unknown plaintext as if it had been migrated"
        );
        assert_eq!(std::fs::read(path).unwrap(), legacy);
        assert_eq!(
            std::fs::read(orphan).unwrap(),
            b"generated unknown legacy bytes"
        );
    }
    #[test]
    fn due_jobs_dispatch_serially_and_wait_for_completion_without_a_busy_timer() {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let clock = FakeClock(1_000_000);
        let first = q
            .schedule("123", "discord:10:11", "first", 1_060_000, &[], &clock)
            .unwrap();
        let second = q
            .schedule("123", "discord:10:12", "second", 1_060_000, &[], &clock)
            .unwrap();
        let clock = FakeClock(1_060_000);
        let mut sent = Vec::new();
        q.dispatch_due("123", true, &clock, |p| {
            sent.push((p.id, p.room));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            sent.len(),
            1,
            "bound upload memory and provider concurrency"
        );
        assert_eq!(
            q.next_delay("123", true, &clock),
            None,
            "completion event, not a repeating due-now timer, wakes the next job"
        );
        q.dispatch_due("123", true, &clock, |_| panic!("already in flight"))
            .unwrap();
        q.complete("123", &first, Ok(())).unwrap();
        q.dispatch_due("123", true, &clock, |p| {
            sent.push((p.id, p.room));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            sent,
            [
                (first, "discord:10:11".into()),
                (second, "discord:10:12".into())
            ]
        );
    }
    #[test]
    fn backend_diagnostic_text_is_bounded_before_persistence() {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let id = q
            .schedule(
                "123",
                "discord:10:11",
                "hi",
                1_060_000,
                &[],
                &FakeClock(1_000_000),
            )
            .unwrap();
        q.dispatch_due("123", true, &FakeClock(1_060_000), |_| Ok(()))
            .unwrap();
        q.complete("123", &id, Err("💥".repeat(5000))).unwrap();
        let State::Uncertain(error) = &q.items()[0].state else {
            panic!("expected visible uncertainty");
        };
        assert!(error.chars().count() <= 1200);
    }
    #[test]
    fn pending_restart_claim_is_durable_before_sink_and_success_never_retries() {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let id = q
            .schedule(
                "123",
                "discord:10:11",
                "hi",
                1_060_000,
                &[],
                &FakeClock(1_000_000),
            )
            .unwrap();
        drop(q);
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let clock = FakeClock(1_080_000);
        q.dispatch_due("123", true, &clock, |payload| {
            let saved: Saved = serde_json::from_slice(&fixture_manifest(root.path())).unwrap();
            assert_eq!(saved.items[0].id, payload.id);
            assert_eq!(saved.items[0].state, State::InFlight);
            Ok(())
        })
        .unwrap();
        q.complete("123", &id, Ok(())).unwrap();
        drop(q);
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        assert_eq!(q.items()[0].state, State::Sent);
        q.dispatch_due("123", true, &clock, |_| panic!("already confirmed"))
            .unwrap();
    }
    #[test]
    fn reload_then_failed_edit_preserves_the_last_committed_attachment_paths() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("file.txt");
        std::fs::write(&source, b"fixture").unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let clock = FakeClock(1_000_000);
        let id = q
            .schedule(
                "123",
                "discord:10:11",
                "hi",
                1_060_000,
                &[(source, false)],
                &clock,
            )
            .unwrap();
        drop(q);
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let owned = q.items()[0].files[0].path.clone();
        let path = root.path().join("scheduled/queue.json");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(path).unwrap();
        assert!(q.edit("123", &id, "not saved", 1_070_000, &clock).is_err());
        assert_eq!(q.items()[0].files[0].path, owned);
    }
    #[test]
    fn reviewed_attachment_bytes_are_verified_in_the_same_read_that_is_snapshotted() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("reviewed.txt");
        std::fs::write(&source, b"reviewed bytes").unwrap();
        let inspected = inspect_sources(&[(source.clone(), true)]).unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let clock = FakeClock(1_000_000);
        q.schedule_reviewed("123", "discord:10:11", "hi", 1_060_000, &inspected, &clock)
            .unwrap();
        assert_eq!(
            read_attachments(&q.items()[0].files).unwrap()[0].1,
            b"reviewed bytes"
        );
        std::fs::write(&source, b"replaced bytes").unwrap();
        assert!(
            q.schedule_reviewed("123", "discord:10:11", "hi", 1_060_000, &inspected, &clock)
                .is_err()
        );
        assert_eq!(q.items().len(), 1);
    }
    #[test]
    fn interrupted_accepted_commands_are_uncertain_not_pending_or_silently_sending() {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        q.schedule("123", "discord:10:11", "hi", 1_060_000, &[], &clock)
            .unwrap();
        clock.0 = 1_060_000;
        q.dispatch_due("123", true, &clock, |_| Ok(())).unwrap();
        assert_eq!(q.interrupt("456", "other account disconnected").unwrap(), 0);
        assert_eq!(q.interrupt("123", "backend stopped").unwrap(), 1);
        assert!(
            matches!(&q.items()[0].state, State::Uncertain(e) if e.contains("backend stopped"))
        );
        q.dispatch_due("123", true, &clock, |_| panic!("duplicate"))
            .unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn private_queue_and_attachment_copies_are_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("private.txt");
        std::fs::write(&source, b"private fixture").unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        q.schedule(
            "123",
            "discord:10:11",
            "private fixture",
            1_060_000,
            &[(source, false)],
            &FakeClock(1_000_000),
        )
        .unwrap();
        for path in [
            root.path().join("scheduled"),
            q.items()[0].files[0].path.clone(),
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o077,
                0,
                "only the local OS user may access scheduled content"
            );
        }
    }
    #[test]
    fn direct_messages_use_the_existing_at_me_room_scope() {
        let root = tempfile::tempdir().unwrap();
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        assert!(
            q.schedule(
                "123",
                "discord:@me:456",
                "hello",
                1_060_000,
                &[],
                &FakeClock(1_000_000)
            )
            .is_ok()
        );
    }
    #[test]
    fn dismiss_only_terminal_records_and_never_reuse_their_deterministic_id() {
        let root = tempfile::tempdir().unwrap();
        let clock = FakeClock(1_000_000);
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let id = q
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        assert!(q.dismiss("123", &id).is_err());
        q.cancel("123", &id).unwrap();
        q.dismiss("123", &id).unwrap();
        assert!(q.items().is_empty());
        drop(q);
        let mut q = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        assert_ne!(
            id,
            q.schedule("123", "discord:10:11", "next", 1_060_000, &[], &clock)
                .unwrap()
        );
    }
    #[test]
    fn failed_durable_claim_disables_dispatch_and_failed_edits_do_not_appear_saved() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        let id = queue
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        let path = root.path().join("scheduled/queue.json");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(
            queue
                .edit("123", &id, "not saved", 1_070_000, &clock)
                .is_err()
        );
        assert_eq!(queue.items()[0].body, "hello");
        clock.0 = 1_080_000;
        assert!(
            queue
                .dispatch_due("123", true, &clock, |_| panic!("not durable"))
                .is_err()
        );
        assert!(queue.next_delay("123", true, &clock).is_none());
    }
    #[test]
    fn corrupt_manifest_cannot_redirect_attachments_or_duplicate_ids() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let clock = FakeClock(1_000_000);
        queue
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        let path = root.path().join("scheduled/queue.json");
        let mut saved: serde_json::Value =
            serde_json::from_slice(&fixture_manifest(root.path())).unwrap();
        saved["items"][0]["id"] = "../../outside".into();
        drop(queue);
        let data = std::fs::read(&path).unwrap();
        let id = storage::key_id(&data).unwrap();
        let ciphertext = storage::seal(
            &serde_json::to_vec(&saved).unwrap(),
            id,
            &fixtures::keys().get(id).unwrap(),
            "queue",
        )
        .unwrap();
        std::fs::write(&path, ciphertext).unwrap();
        assert!(Queue::open_with(root.path(), fixtures::keys()).is_err());
    }
    #[test]
    fn timezone_conversion_is_explicit_utc_equivalent_and_invalid_input_is_rejected() {
        let utc = parse_due("2026-11-07 17:45", "UTC").unwrap();
        assert!(utc > 0);
        assert_eq!(parse_due("2026-11-07 12:45", "-05:00").unwrap(), utc);
        assert_eq!(parse_due("2026-11-07 23:30", "+05:45").unwrap(), utc);
        for (date, zone) in [
            ("2026-02-30 12:00", "UTC"),
            ("tomorrow", "Local"),
            ("2026-11-07 12:00", "+25:00"),
            ("2026-11-07 12:00", "+05:99"),
            ("2026-11-07 12:00", "EST"),
        ] {
            assert!(parse_due(date, zone).is_err(), "{date}, {zone}");
        }
    }
    #[test]
    fn next_due_is_one_shot_and_wall_clock_changes_never_send_early() {
        use std::time::Duration;
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        queue
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        assert_eq!(
            queue.next_delay("123", true, &clock),
            Some(Duration::from_secs(60))
        );
        assert_eq!(queue.next_delay("123", false, &clock), None);
        assert_eq!(queue.next_delay("456", true, &clock), None);
        clock.0 -= 86_400_000;
        queue
            .dispatch_due("123", true, &clock, |_| panic!("clock moved backward"))
            .unwrap();
        assert_eq!(
            queue.next_delay("123", true, &clock),
            Some(Duration::from_secs(300))
        );
        clock.0 = 1_060_000;
        queue.dispatch_due("123", true, &clock, |_| Ok(())).unwrap();
        assert_eq!(queue.next_delay("123", true, &clock), None);
    }
    #[test]
    fn a_day_late_is_blocked_instead_of_surprise_delivery_after_a_long_shutdown() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        queue
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        clock.0 = 1_060_000 + 86_400_001;
        queue
            .dispatch_due("123", true, &clock, |_| panic!("stale"))
            .unwrap();
        assert!(matches!(queue.items()[0].state, State::Blocked(_)));
    }
    #[test]
    fn attachments_are_owned_snapshots_and_missing_or_changed_copies_block_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("photo.png");
        std::fs::write(&original, b"original bytes").unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        queue
            .schedule(
                "123",
                "discord:10:11",
                "photo",
                1_060_000,
                &[(original.clone(), true)],
                &clock,
            )
            .unwrap();
        assert_eq!(queue.items()[0].files.len(), 1);
        let snapshot = queue.items()[0].files[0].clone();
        std::fs::remove_file(&original).unwrap();
        assert_eq!(
            read_attachments(std::slice::from_ref(&snapshot)).unwrap()[0].1,
            b"original bytes"
        );
        assert!(snapshot.spoiler);
        drop(queue);
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        std::fs::write(&snapshot.path, b"modified bytes").unwrap();
        clock.0 = 1_060_000;
        queue
            .dispatch_due("123", true, &clock, |_| panic!("changed snapshot"))
            .unwrap();
        assert!(matches!(&queue.items()[0].state, State::Blocked(e) if e.contains("attachment")));
    }
    #[test]
    fn restart_of_claimed_message_is_uncertain_and_never_automatically_retried() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        let id = queue
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        clock.0 = 1_060_000;
        queue.dispatch_due("123", true, &clock, |_| Ok(())).unwrap();
        drop(queue);
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        assert!(matches!(queue.items()[0].state, State::Uncertain(_)));
        queue
            .dispatch_due("123", true, &clock, |_| panic!("duplicate"))
            .unwrap();
        assert!(queue.edit("123", &id, "retry", 1_070_000, &clock).is_err());
    }
    #[test]
    fn completion_is_correlated_and_errors_are_visible_without_retry() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        let id = queue
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        clock.0 = 1_060_000;
        queue.dispatch_due("123", true, &clock, |_| Ok(())).unwrap();
        assert!(queue.complete("456", &id, Ok(())).is_err());
        queue.complete("123", &id, Err("timeout".into())).unwrap();
        assert!(matches!(&queue.items()[0].state, State::Uncertain(e) if e.contains("timeout")));
        queue.complete("123", &id, Ok(())).unwrap();
        assert!(
            matches!(queue.items()[0].state, State::Uncertain(_)),
            "a stale result cannot overwrite a terminal state"
        );
        queue
            .dispatch_due("123", true, &clock, |_| panic!("duplicate"))
            .unwrap();
    }
    #[test]
    fn cancel_is_durable_account_scoped_and_wins_before_claim_but_not_after() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        let id = queue
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        assert!(queue.cancel("456", &id).is_err());
        queue.edit("123", &id, "edited", 1_070_000, &clock).unwrap();
        assert_eq!(queue.items()[0].body, "edited");
        queue.cancel("123", &id).unwrap();
        drop(queue);
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        clock.0 = 1_070_000;
        queue
            .dispatch_due("123", true, &clock, |_| panic!("cancelled"))
            .unwrap();
        assert_eq!(queue.items()[0].state, State::Cancelled);
        let id = queue
            .schedule("123", "discord:10:11", "next", 1_080_000, &[], &clock)
            .unwrap();
        clock.0 = 1_080_000;
        queue.dispatch_due("123", true, &clock, |_| Ok(())).unwrap();
        assert!(
            queue.cancel("123", &id).is_err(),
            "cannot unsend an accepted command"
        );
        assert!(
            queue
                .edit("123", &id, "too late", 1_090_000, &clock)
                .is_err()
        );
    }
    #[test]
    fn production_sink_is_called_only_once_at_or_after_due_for_own_online_account() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let mut clock = FakeClock(1_000_000);
        let id = queue
            .schedule("123", "discord:10:11", "hello", 1_060_000, &[], &clock)
            .unwrap();
        let mut commands = vec![];
        queue
            .dispatch_due("123", true, &clock, |c| {
                commands.push(c);
                Ok(())
            })
            .unwrap();
        clock.0 = 1_060_000;
        queue
            .dispatch_due("456", true, &clock, |c| {
                commands.push(c);
                Ok(())
            })
            .unwrap();
        queue
            .dispatch_due("123", false, &clock, |c| {
                commands.push(c);
                Ok(())
            })
            .unwrap();
        assert!(commands.is_empty());
        queue
            .dispatch_due("123", true, &clock, |c| {
                commands.push(c);
                Ok(())
            })
            .unwrap();
        queue
            .dispatch_due("123", true, &clock, |c| {
                commands.push(c);
                Ok(())
            })
            .unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].id, id);
        assert_eq!(queue.items()[0].state, State::InFlight);
        let persisted: Saved = serde_json::from_slice(&fixture_manifest(root.path())).unwrap();
        assert_eq!(persisted.items[0].state, State::InFlight);
    }
    #[test]
    fn validation_rejects_empty_overlong_past_and_unbounded_schedule() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let clock = FakeClock(1_000_000);
        for (account, room, body, due) in [
            (
                "123",
                "discord:10:11",
                "@silent".to_owned(),
                clock.0 + 60_000,
            ),
            (
                "123",
                "discord:10:11",
                "@silent  \n".to_owned(),
                clock.0 + 60_000,
            ),
            ("", "discord:10:11", "hi".to_owned(), clock.0 + 60_000),
            ("123", "matrix:11", "hi".to_owned(), clock.0 + 60_000),
            ("123", "discord:10:11", " ".to_owned(), clock.0 + 60_000),
            ("123", "discord:10:11", "a".repeat(2001), clock.0 + 60_000),
            ("123", "discord:10:11", "hi".to_owned(), clock.0),
            (
                "123",
                "discord:10:11",
                "hi".to_owned(),
                clock.0 + 366 * 86_400_000,
            ),
        ] {
            assert!(
                queue
                    .schedule(account, room, &body, due, &[], &clock)
                    .is_err()
            );
        }
        assert!(queue.items().is_empty());
    }
    #[test]
    fn saves_and_reloads_the_account_channel_and_exact_due_time() {
        let root = tempfile::tempdir().unwrap();
        let clock = FakeClock(1_000_000);
        let mut queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let id = queue
            .schedule(
                "123",
                "discord:10:11",
                "hello",
                clock.0 + 60_000,
                &[],
                &clock,
            )
            .unwrap();
        drop(queue);
        let queue = Queue::open_with(root.path(), fixtures::keys()).unwrap();
        let item = &queue.items()[0];
        assert_eq!(item.id, id);
        assert_eq!(item.account, "123");
        assert_eq!(item.room, "discord:10:11");
        assert_eq!(item.due_ms, 1_060_000);
        assert_eq!(item.body, "hello");
        assert_eq!(item.state, State::Pending);
    }
}
