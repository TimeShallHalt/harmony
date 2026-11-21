//! Generation transactions: ciphertext/files first, atomic manifest commit last.
//! A sealed journal survives every incomplete transition; only explicit recovery
//! authenticates the winning manifest and its files before deleting losing data.
use super::*;
#[derive(Serialize, Deserialize)]
struct Transaction {
    old: String,
    next: String,
    #[serde(default)]
    legacy_ids: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    before: Vec<u8>,
    after: Vec<u8>,
}
fn write_saved(
    directory: &Path,
    saved: &Saved,
    id: &str,
    key: &storage::Key,
) -> Result<(), String> {
    let plain = zeroize::Zeroizing::new(serde_json::to_vec(saved).map_err(|e| e.to_string())?);
    storage::atomic(
        &directory.join("queue.json"),
        &storage::seal(&plain, id, key, "queue")?,
    )
}
fn erase_generation(directory: &Path, id: &str, store: &dyn SecretStore) -> Result<(), String> {
    if !storage::valid_id(id) {
        return Err("Invalid retired scheduler generation.".into());
    }
    let path = directory.join("attachments").join(id);
    match std::fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Scheduler retired ciphertext cleanup failed: {e}")),
    }
    storage::sync_directory(&directory.join("attachments"))?;
    store.remove(id)
}
fn verify_files(
    directory: &Path,
    saved: &Saved,
    id: &str,
    key: &Arc<storage::Key>,
) -> Result<(), String> {
    for item in &saved.items {
        for (index, file) in item.files.iter().enumerate() {
            let mut file = file.clone();
            file.path = directory
                .join("attachments")
                .join(id)
                .join(&item.id)
                .join(format!("{index}.sealed"));
            file.protection = Some(Protection {
                id: id.into(),
                key: key.clone(),
                context: format!("attachment:{}:{index}", item.id),
            });
            let _bytes = zeroize::Zeroizing::new(read_attachments(&[file])?.remove(0).1);
        }
    }
    Ok(())
}
fn legacy_layout(
    directory: &Path,
    saved: &Saved,
    allowed_generation: Option<&str>,
) -> Result<(), String> {
    let problem = || {
        "Legacy scheduler contains unreferenced or linked attachment paths. Nothing was removed. Restore the manifest or move the orphan data to a backup before migrating.".to_owned()
    };
    let base = directory.join("attachments");
    let meta = match std::fs::symlink_metadata(&base) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(problem());
    }
    for (count, entry) in std::fs::read_dir(&base)
        .map_err(|e| e.to_string())?
        .take(1025)
        .enumerate()
    {
        if count >= 1024 {
            return Err(problem());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().into_string().map_err(|_| problem())?;
        if allowed_generation == Some(name.as_str()) {
            continue;
        }
        let item = saved
            .items
            .iter()
            .find(|i| i.id == name)
            .ok_or_else(problem)?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if !kind.is_dir() || kind.is_symlink() {
            return Err(problem());
        }
        let expected: std::collections::HashSet<_> = item
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| format!("{index}-{}", file.name))
            .collect();
        for (count, file) in std::fs::read_dir(entry.path())
            .map_err(|e| e.to_string())?
            .take(12)
            .enumerate()
        {
            if count >= 10 {
                return Err(problem());
            }
            let file = file.map_err(|e| e.to_string())?;
            let kind = file.file_type().map_err(|e| e.to_string())?;
            let name = file.file_name().into_string().map_err(|_| problem())?;
            if !expected.contains(&name) || !kind.is_file() || kind.is_symlink() {
                return Err(problem());
            }
        }
    }
    Ok(())
}
fn legacy_saved(
    directory: &Path,
    data: &[u8],
    allowed_generation: Option<&str>,
) -> Result<Saved, String> {
    let mut saved: Saved = serde_json::from_slice(data)
        .map_err(|_| "Invalid legacy scheduler manifest; data untouched.".to_owned())?;
    validate_saved(&saved)?;
    legacy_layout(directory, &saved, allowed_generation)?;
    for item in &mut saved.items {
        for (index, file) in item.files.iter_mut().enumerate() {
            file.path = directory
                .join("attachments")
                .join(&item.id)
                .join(format!("{index}-{}", file.name));
            file.protection = None;
        }
        let _ = read_attachments(&item.files)?;
    }
    Ok(saved)
}
fn erase_legacy(directory: &Path, tx: &Transaction) -> Result<(), String> {
    for id in &tx.legacy_ids {
        if id.len() != 24 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid legacy cleanup identifier.".into());
        }
        match std::fs::remove_dir_all(directory.join("attachments").join(id)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Legacy scheduler cleanup failed: {e}")),
        }
    }
    storage::sync_directory(&directory.join("attachments"))
}
impl Queue {
    /// Never runs implicitly: legacy pending authorizations require review again.
    pub fn migrate(root: &Path, store: Arc<dyn SecretStore>) -> Result<(), String> {
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
            .map_err(|_| "Scheduler queue is still open.".to_owned())?;
        if directory
            .join("maintenance.json")
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            return Err("Recover interrupted scheduler storage before migrating.".into());
        }
        let data = storage::read(&directory.join("queue.json"), 8 * 1024 * 1024)?;
        let mut saved = legacy_saved(&directory, &data, None)?;
        for item in &mut saved.items {
            item.state = match &item.state {
                State::Pending => State::Blocked(
                    "Legacy schedule encrypted. Review and explicitly reschedule before sending."
                        .into(),
                ),
                State::InFlight => State::Uncertain(
                    "Legacy process stopped after dispatch. Check the channel; no automatic retry."
                        .into(),
                ),
                state => state.clone(),
            };
        }
        let (id, key) = storage::new_secret(&*store)?;
        let tx = Transaction {
            old: "legacy".into(),
            next: id.clone(),
            legacy_ids: saved.items.iter().map(|i| i.id.clone()).collect(),
        };
        let journal = Journal {
            before: vec![],
            after: storage::seal(
                &serde_json::to_vec(&tx).map_err(|e| e.to_string())?,
                &id,
                &key,
                "maintenance",
            )?,
        };
        storage::atomic(
            &directory.join("maintenance.json"),
            &serde_json::to_vec(&journal).map_err(|e| e.to_string())?,
        )?;
        let helper = Self {
            committed: saved.clone(),
            saved: saved.clone(),
            fault: None,
            directory: directory.clone(),
            _lock: lock,
            store: store.clone(),
            key_id: id.clone(),
            key: key.clone(),
            claims: Default::default(),
            initialized: false,
        };
        helper.stage_files(&mut saved, &id, &key)?;
        verify_files(&directory, &saved, &id, &key)?;
        write_saved(&directory, &saved, &id, &key)?;
        erase_legacy(&directory, &tx)?;
        std::fs::remove_file(directory.join("maintenance.json")).map_err(|e| e.to_string())?;
        storage::sync_directory(&directory)
    }
    pub fn purge(&mut self, account: &str) -> Result<(), String> {
        self.revoke_claims(Some(account));
        if let Some(error) = &self.fault {
            return Err(error.clone());
        }
        let mut saved = self.saved.clone();
        saved.items.retain(|item| item.account != account);
        let result = self.reseal(saved);
        if let Err(error) = result {
            self.fault = Some(format!(
                "Account scheduler purge incomplete; dispatch paused. Recover storage, then retry purge/unlink: {error}"
            ));
            return Err(self.fault.clone().unwrap());
        }
        Ok(())
    }
    pub fn rotate(&mut self) -> Result<(), String> {
        self.revoke_claims(None);
        if let Some(error) = &self.fault {
            return Err(error.clone());
        }
        let result = self.reseal(self.saved.clone());
        if let Err(error) = result {
            self.fault = Some(format!(
                "Scheduler key transaction incomplete; dispatch paused. Recover storage before continuing: {error}"
            ));
            return Err(self.fault.clone().unwrap());
        }
        Ok(())
    }
    fn reseal(&mut self, mut next_saved: Saved) -> Result<(), String> {
        // In-flight sends never become pending as a consequence of maintenance.
        for item in &mut next_saved.items {
            if item.state == State::InFlight {
                item.state=State::Uncertain("Storage maintenance interrupted an accepted send. Check the channel; no automatic retry.".into());
            }
        }
        self.revoke_claims(None);
        self.verify_manifest()?;
        let next = uuid::Uuid::new_v4().simple().to_string();
        let tx = Transaction {
            old: self.key_id.clone(),
            next: next.clone(),
            legacy_ids: vec![],
        };
        let plain = serde_json::to_vec(&tx).map_err(|e| e.to_string())?;
        // Authenticate/persist the pause with the loaded key BEFORE consulting
        // an unavailable facility or registering the next key. A failed unlink
        // must not resurrect its old send authorizations after restart.
        let mut journal = Journal {
            before: storage::seal(&plain, &tx.old, &self.key, "maintenance")?,
            after: vec![],
        };
        storage::atomic(
            &self.directory.join("maintenance.json"),
            &serde_json::to_vec(&journal).map_err(|e| e.to_string())?,
        )?;
        self.verify_committed()?;
        let key = storage::new_secret_for(&*self.store, &next)?;
        journal.after = storage::seal(&plain, &next, &key, "maintenance")?;
        storage::atomic(
            &self.directory.join("maintenance.json"),
            &serde_json::to_vec(&journal).map_err(|e| e.to_string())?,
        )?;
        self.stage_files(&mut next_saved, &next, &key)?;
        verify_files(&self.directory, &next_saved, &next, &key)?;
        write_saved(&self.directory, &next_saved, &next, &key)?; // sole commit point
        self.saved = next_saved;
        self.committed = self.saved.clone();
        self.key_id = next;
        self.key = key;
        erase_generation(&self.directory, &tx.old, &*self.store)?;
        std::fs::remove_file(self.directory.join("maintenance.json")).map_err(|e| e.to_string())?;
        storage::sync_directory(&self.directory)
    }
    fn stage_files(
        &self,
        saved: &mut Saved,
        next: &str,
        key: &Arc<storage::Key>,
    ) -> Result<(), String> {
        let base = self.directory.join("attachments");
        private_directory(&base)?;
        let generation = base.join(next);
        private_directory(&generation)?;
        for item in &mut saved.items {
            let folder = generation.join(&item.id);
            if !item.files.is_empty() {
                private_directory(&folder)?;
            }
            for (index, file) in item.files.iter_mut().enumerate() {
                let bytes = zeroize::Zeroizing::new(
                    read_attachments(std::slice::from_ref(file))?.remove(0).1,
                );
                let context = format!("attachment:{}:{index}", item.id);
                let path = folder.join(format!("{index}.sealed"));
                storage::atomic(&path, &storage::seal(&bytes, next, key, &context)?)?;
                file.path = path;
                file.protection = Some(Protection {
                    id: next.into(),
                    key: key.clone(),
                    context,
                });
            }
            if folder.exists() {
                storage::sync_directory(&folder)?;
            }
        }
        storage::sync_directory(&generation)?;
        storage::sync_directory(&base)
    }
    /// Explicit action only. A damaged journal or winning generation is never
    /// guessed around, deleted, overwritten, or dispatched.
    pub fn recover(root: &Path, store: Arc<dyn SecretStore>) -> Result<(), String> {
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
            .map_err(|_| "Scheduler queue is still open.".to_owned())?;
        if !directory
            .join("maintenance.json")
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            return Ok(());
        }
        let data = storage::read(&directory.join("queue.json"), 8 * 1024 * 1024 + 256)?;
        let journal: Journal = serde_json::from_slice(&storage::read(
            &directory.join("maintenance.json"),
            64 * 1024,
        )?)
        .map_err(|_| "Scheduler maintenance journal is corrupt; no files removed.".to_owned())?;
        let legacy = journal.before.is_empty() && !data.starts_with(b"YFS2");
        let id = if legacy {
            storage::key_id(&journal.after)?.to_owned()
        } else {
            storage::key_id(&data)?.to_owned()
        };
        let key = Arc::new(store.get(&id)?);
        let mut saved = if legacy {
            legacy_saved(&directory, &data, Some(&id))?
        } else {
            let saved: Saved = serde_json::from_slice(&storage::open(&data, &id, &key, "queue")?)
                .map_err(|_| "Invalid committed scheduler manifest.".to_owned())?;
            validate_saved(&saved)?;
            verify_files(&directory, &saved, &id, &key)?;
            saved
        };
        let sealed = if storage::key_id(&journal.before).ok() == Some(id.as_str()) {
            &journal.before
        } else {
            &journal.after
        };
        let tx: Transaction =
            serde_json::from_slice(&storage::open(sealed, &id, &key, "maintenance")?)
                .map_err(|_| "Invalid scheduler transaction.".to_owned())?;
        if !storage::valid_id(&tx.next)
            || tx.old == tx.next
            || (tx.old != "legacy" && !storage::valid_id(&tx.old))
            || (id != tx.old && id != tx.next)
        {
            return Err("Scheduler journal does not match committed generation.".into());
        }
        if legacy {
            if tx.old != "legacy"
                || tx.legacy_ids != saved.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>()
            {
                return Err("Legacy scheduler journal mismatch.".into());
            }
            erase_generation(&directory, &tx.next, &*store)?;
        } else if tx.old == "legacy" {
            erase_legacy(&directory, &tx)?;
        } else {
            let losing = if id == tx.old { &tx.next } else { &tx.old };
            erase_generation(&directory, losing, &*store)?;
        }
        if !legacy {
            for item in &mut saved.items {
                item.state=match &item.state {
                State::Pending=>State::Blocked("Recovered interrupted scheduler storage. Review/reschedule or retry purge/unlink; earlier pending authorization is paused.".into()),
                State::InFlight=>State::Uncertain("Recovered after an accepted send. Check the channel; no automatic retry.".into()),
                state=>state.clone(),
            };
            }
            write_saved(&directory, &saved, &id, &key)?;
        }
        std::fs::remove_file(directory.join("maintenance.json")).map_err(|e| e.to_string())?;
        storage::sync_directory(&directory)
    }
}
