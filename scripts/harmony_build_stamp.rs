//! Shared by build.rs and a small standalone test harness; no crate dependencies.
use std::{
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Debug)]
pub struct Stamp {
    pub commit: String,
    pub dirty: bool,
    pub build_id: u64,
}
pub fn build_id(value: Option<&str>) -> Result<u64, &'static str> {
    let Some(value) = value else {
        return Ok(0);
    };
    let n: u64 = value
        .parse()
        .map_err(|_| "HARMONY_BUILD_ID must be a canonical unsigned integer")?;
    if n.to_string() != value {
        return Err("HARMONY_BUILD_ID must be canonical");
    }
    Ok(n)
}
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
pub fn inspect(root: &Path, id: u64) -> Result<Stamp, &'static str> {
    let commit = git(root, &["rev-parse", "--verify", "HEAD"]).filter(|s| {
        s.len() == 40
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    });
    let dirty = git(root, &["status", "--porcelain", "--untracked-files=normal"])
        .is_none_or(|s| !s.is_empty());
    if id > 0 && (commit.is_none() || dirty) {
        return Err(
            "Release build IDs require a clean committed Git tree. Commit integration before building; do not stamp dirty binaries as a release.",
        );
    }
    Ok(Stamp {
        commit: commit.unwrap_or_else(|| "unknown (source archive)".into()),
        dirty,
        build_id: id,
    })
}
pub fn emit() {
    for name in ["HARMONY_BUILD_ID"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for path in [
        "scripts/harmony_build_stamp.rs",
        "src",
        "ui",
        "crates",
        "Cargo.toml",
        "Cargo.lock",
        "packaging",
        "nix",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let root =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    for name in ["HEAD", "index"] {
        if let Some(path) = git(&root, &["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    if let Some(reference) = git(&root, &["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&root, &["rev-parse", "--git-path", &reference])
    {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let id = std::env::var("HARMONY_BUILD_ID").ok();
    let stamp = inspect(
        &root,
        build_id(id.as_deref()).expect("valid Harmony build ID"),
    )
    .expect("truthful Harmony build identity");
    println!("cargo:rustc-env=HARMONY_BUILD_COMMIT={}", stamp.commit);
    println!("cargo:rustc-env=HARMONY_BUILD_DIRTY={}", stamp.dirty);
    println!("cargo:rustc-env=HARMONY_BUILD_ID={}", stamp.build_id);
    // This allowlisted compile stamp can accompany each platform's packaging.
    // No environment dump, paths, accounts, tokens, or provider output.
    let version = std::env::var("CARGO_PKG_VERSION").expect("package version");
    let platform = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").expect("target architecture");
    let json = format!(
        "{{\"version\":\"{version}\",\"build_id\":{},\"commit\":\"{}\",\"dirty\":{},\"platform\":\"{platform}\",\"architecture\":\"{arch}\",\"upstream\":\"Yapfast 0.3.0\",\"license\":\"GPL-3.0-or-later\"}}\n",
        stamp.build_id, stamp.commit, stamp.dirty
    );
    std::fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output"))
            .join("harmony-build-info.json"),
        json,
    )
    .expect("write Harmony compile stamp");
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_build_ids_do_not_normalize_bad_tokens() {
        assert_eq!(build_id(None), Ok(0));
        assert_eq!(build_id(Some("42")), Ok(42));
        for s in ["01", "+1", " 1", "-1", "1x", "18446744073709551616"] {
            assert!(build_id(Some(s)).is_err());
        }
    }
    #[test]
    fn a_non_git_archive_cannot_claim_a_release_commit() {
        let root = std::env::temp_dir();
        assert!(inspect(&root, 42).is_err());
        assert!(inspect(&root, 0).unwrap().commit.contains("unknown"));
    }
}
