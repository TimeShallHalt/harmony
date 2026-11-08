//! GPL-3.0-or-later. Optional stable native launcher; no desktop/runtime/network.
//! Package separately after review. Never self-replace or install automatically.
#![allow(dead_code, unused_imports)]
#![cfg_attr(windows, windows_subsystem = "windows")]
#[path = "../src/harmony_update/mod.rs"]
mod harmony_update;
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = harmony_update::bootstrap::install_cli(&args)
        .unwrap_or_else(|| harmony_update::bootstrap::launch(&args));
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
