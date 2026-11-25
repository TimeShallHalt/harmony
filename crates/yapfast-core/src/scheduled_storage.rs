//! Scheduler-only authenticated encryption. Keys never share the token/cache
//! credential, so a token refresh cannot strand scheduled work. No plaintext fallback.
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305,
    aead::{Aead, OsRng, Payload, rand_core::RngCore},
};
use std::{path::Path, sync::Arc};
use zeroize::Zeroizing;
pub type Key = Zeroizing<[u8; 32]>;
pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Key, String>;
    fn put(&self, id: &str, key: &Key) -> Result<(), String>;
    fn remove(&self, id: &str) -> Result<(), String>;
}
pub struct OsSecrets;
const SERVICE: &str = "io.github.yapfast.Yapfast.scheduler.v2";
fn entry(id: &str) -> Result<keyring::Entry, String> {
    if !valid_id(id) {
        return Err("Invalid scheduler key identifier.".into());
    }
    keyring::Entry::new(SERVICE, id)
        .map_err(|_| "Scheduler OS secure key facility unavailable.".into())
}
impl SecretStore for OsSecrets {
    fn get(&self, id: &str) -> Result<Key, String> {
        let bytes = Zeroizing::new(entry(id)?.get_secret().map_err(|_| "Scheduler key unavailable in OS secure storage. No messages will dispatch; restore/unlock the key facility, then reopen the queue.".to_owned())?);
        let key: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Scheduler key is corrupt. Existing data was not replaced.".to_owned())?;
        Ok(Zeroizing::new(key))
    }
    fn put(&self, id: &str, key: &Key) -> Result<(), String> {
        entry(id)?
            .set_secret(key.as_ref())
            .map_err(|_| "Could not save scheduler key in OS secure storage.".to_owned())?;
        if self.get(id)?.as_ref() != key.as_ref() {
            return Err("Scheduler secure key read-back failed.".into());
        }
        Ok(())
    }
    fn remove(&self, id: &str) -> Result<(), String> {
        match entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Could not remove retired scheduler key from OS secure storage.".into()),
        }
    }
}
pub fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn new_secret(store: &dyn SecretStore) -> Result<(String, Arc<Key>), String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let key = new_secret_for(store, &id)?;
    Ok((id, key))
}
pub fn new_secret_for(store: &dyn SecretStore, id: &str) -> Result<Arc<Key>, String> {
    let mut key = Zeroizing::new([0; 32]);
    OsRng
        .try_fill_bytes(key.as_mut())
        .map_err(|_| "Scheduler secure randomness unavailable.".to_owned())?;
    store.put(&id, &key)?;
    if store.get(&id)?.as_ref() != key.as_ref() {
        return Err("Scheduler secure key read-back failed.".into());
    }
    Ok(Arc::new(key))
}
const MAGIC: &[u8; 4] = b"YFS2";
const HEADER: usize = 4 + 32 + 24;
pub fn key_id(data: &[u8]) -> Result<&str, String> {
    if data.len() < HEADER + 16 || &data[..4] != MAGIC {
        return Err("Scheduler data is not authenticated v2 ciphertext. Legacy queues require explicit encryption/migration.".into());
    }
    let id = std::str::from_utf8(&data[4..36])
        .map_err(|_| "Invalid scheduler ciphertext header.".to_owned())?;
    if !valid_id(id) {
        return Err("Invalid scheduler key identifier.".into());
    }
    Ok(id)
}
pub fn seal(data: &[u8], id: &str, key: &Key, context: &str) -> Result<Vec<u8>, String> {
    let mut nonce = [0; 24];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| "Scheduler secure randomness unavailable.".to_owned())?;
    let mut out = Vec::from(MAGIC.as_slice());
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(&nonce);
    let mut aad = out.clone();
    aad.extend_from_slice(context.as_bytes());
    let encrypted = XChaCha20Poly1305::new(key.as_ref().into())
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: data,
                aad: &aad,
            },
        )
        .map_err(|_| "Could not encrypt scheduler data.".to_owned())?;
    out.extend(encrypted);
    Ok(out)
}
pub fn open(data: &[u8], id: &str, key: &Key, context: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    if key_id(data)? != id {
        return Err("Scheduler ciphertext belongs to another key generation.".into());
    }
    let mut aad = data[..HEADER].to_vec();
    aad.extend_from_slice(context.as_bytes());
    XChaCha20Poly1305::new(key.as_ref().into()).decrypt((&data[36..HEADER]).into(), Payload { msg: &data[HEADER..], aad: &aad })
        .map(Zeroizing::new).map_err(|_| "Scheduler ciphertext is corrupt, truncated, or has the wrong key. Dispatch paused; data was not replaced.".into())
}
pub fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
        return Err("Invalid or oversized scheduler file.".into());
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Scheduler storage is not a regular file.".into());
    }
    let mut data = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > limit {
        return Err("Scheduler file grew beyond its limit.".into());
    }
    Ok(data)
}
pub fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    if path.exists() {
        std::fs::File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub fn atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("No scheduler parent directory.")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(data)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    sync_directory(parent)
}
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures {
    //! In-memory injected secrets only. Never calls a real credential facility.
    use super::*;
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
    };
    #[derive(Default)]
    pub struct Keys(pub Mutex<HashMap<String, Key>>);
    impl SecretStore for Keys {
        fn get(&self, id: &str) -> Result<Key, String> {
            self.0
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .ok_or("Fixture key unavailable".into())
        }
        fn put(&self, id: &str, key: &Key) -> Result<(), String> {
            self.0.lock().unwrap().insert(id.into(), key.clone());
            Ok(())
        }
        fn remove(&self, id: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }
    pub fn keys() -> Arc<Keys> {
        static KEYS: OnceLock<Arc<Keys>> = OnceLock::new();
        KEYS.get_or_init(|| Arc::new(Keys::default())).clone()
    }
}
