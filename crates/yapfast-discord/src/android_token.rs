//! Android keeps the session and cache key in app-private Keystore-backed storage.
use anyhow::{Context, Result, bail};
use std::sync::OnceLock;

pub trait SecretStore: Send + Sync {
    fn read(&self, key: &str) -> Result<Option<String>>;
    fn write(&self, key: &str, value: Option<&str>) -> Result<()>;
}

static STORE: OnceLock<Box<dyn SecretStore>> = OnceLock::new();
pub fn install(store: Box<dyn SecretStore>) {
    let _ = STORE.set(store);
}
fn secrets() -> Result<&'static dyn SecretStore> {
    STORE
        .get()
        .map(Box::as_ref)
        .context("Android secure storage is unavailable")
}

#[derive(Debug)]
pub struct NotLinked;
impl std::fmt::Display for NotLinked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("No linked account")
    }
}
impl std::error::Error for NotLinked {}

pub fn clean(value: &str) -> Result<String> {
    let value = value.trim().trim_matches(['"', '\'']).trim();
    if value.is_empty() || value.contains(char::is_whitespace) {
        bail!("Enter a valid account token.");
    }
    Ok(value.to_owned())
}
pub fn load() -> Result<String> {
    secrets()?.read("session")?.ok_or_else(|| NotLinked.into())
}
pub fn store(token: &str) -> Result<()> {
    secrets()?.write("session", Some(&clean(token)?))
}
pub(crate) fn cache_key() -> Result<crate::cache::Key> {
    let store = secrets()?;
    if let Some(encoded) = store.read("cache-key")? {
        return crate::cache::read_key(&encoded).context("Invalid encrypted cache key");
    }
    let key = crate::cache::new_key();
    store.write("cache-key", Some(&crate::cache::write_key(&key)))?;
    Ok(key)
}
pub fn forget() -> Result<()> {
    secrets()?.write("session", None)?;
    crate::cache::forget();
    secrets()?.write("cache-key", None)
}
