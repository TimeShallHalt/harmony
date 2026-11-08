//! Account-free native helper entry point; dispatched before TLS/runtime/UI.
use super::*;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
#[cfg(test)]
thread_local! {
    // Scheduling seam only; activation/failure both recheck ID under one lock.
    pub(super) static BEFORE_ACTIVATE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}
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
        if args.len() != 6 || args[5] != "parent-stdin-v1" {
            return Err(Error::Invalid("identity-bound helper arguments"));
        }
        let install = PathBuf::from(&args[1]);
        let expected = args[2]
            .to_str()
            .ok_or(Error::Invalid("helper transaction"))?;
        safe_fs::leaf(expected)?;
        let pid = args[3]
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|p| *p > 0 && *p != std::process::id())
            .ok_or(Error::Invalid("helper parent process"))?;
        let birth = args[4]
            .to_str()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(Error::Invalid("helper parent start identity"))?;
        let result = (|| {
            let parent = lifetime::Parent::receive(pid, birth)?;
            lifetime::acknowledge()?;
            parent.wait()?;
            after_parent(&install, expected)
        })();
        if result.is_err() {
            let _ = transaction::record_helper_failure(&install, expected);
        }
        result
    })())
}
pub(super) fn after_parent(install: &Path, expected: &str) -> Result<()> {
    #[cfg(test)]
    BEFORE_ACTIVATE.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    let result = transaction::activate(install, expected);
    if result.is_err() {
        let _ = transaction::record_helper_failure(install, expected);
    }
    result
}
