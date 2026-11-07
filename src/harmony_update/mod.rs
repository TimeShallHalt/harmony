//! Harmony build identity and private release updates. No account credentials.
mod build_info;
pub use build_info::BuildInfo;
pub mod controller;
pub mod gh;
mod helper;
mod image_identity;
pub(crate) mod lifetime;
pub mod release;
pub use helper::cli_main;
mod safe_fs;
pub mod transaction;
#[cfg(windows)]
mod windows_fs;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Clone, Default)]
pub struct Cancel(pub Arc<AtomicBool>);
impl Cancel {
    pub fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Acquire) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    NotFound,
    Cancelled,
    Invalid(&'static str),
    Io(&'static str),
    Durability,
    Busy,
    MissingGh,
    Auth,
    Network,
    Timeout,
    Size,
    Hash,
    Conflict,
    Unsupported,
    UnsupportedImageIdentity,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingGh => f.write_str("Updates unavailable: install the official GitHub CLI (gh). No embedded credentials are used."),
            Self::Auth => f.write_str("GitHub CLI authentication unavailable (not signed in, expired, or offline). Sign in separately with gh auth login --hostname github.com; never paste a token here."),
            Self::Network => f.write_str("GitHub request failed: offline, repository access denied, rate limit, or asset unavailable. Retry manually when connected."),
            Self::Cancelled => f.write_str("Update operation canceled; the installed executable was not changed."),
            Self::Busy => f.write_str("An update operation is already in progress."),
            Self::Hash => f.write_str("SHA-256 verification failed. Update was not accepted; keep the verified rollback."),
            Self::Size => f.write_str("Update size mismatch or safety limit exceeded. Update was not accepted."),
            Self::Conflict => f.write_str("Update identity/storage conflict: same version/build with another commit, a changed installation, or an existing staged update. Inspect/discard staging before retrying."),
            Self::Timeout => f.write_str("Update provider/helper timed out; no automatic restart was attempted."),
            Self::UnsupportedImageIdentity => f.write_str("Automatic installation is disabled: independent kernel-backed running-image identity is unavailable for this layout. Windows portable and mounted/inner AppImage activation are not currently supported. Use a separately verified manual installation."),
            Self::Unsupported => f.write_str("Self-update unsupported here: Windows portable executable or Linux AppImage required; platform artifact may be absent. Manual installation remains available."),
            Self::Invalid(context) => write!(f, "Update rejected: {context}."),
            Self::Io(context) => write!(f, "Update filesystem/provider failure: {context}. Check permissions/free space; no automatic restart."),
            Self::Durability => f.write_str("Update state was published, but directory durability could not be confirmed. Recoverable files were retained; inspect saved state before retrying."),
            Self::NotFound => f.write_str("Update storage entry not found."),
        }
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[cfg(test)]
mod tests;
