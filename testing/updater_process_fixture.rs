//! Standalone, account-free updater process fixture. Never links/opens the app.
#![allow(dead_code, unused_imports)]
#[path = "../scripts/harmony_build_stamp.rs"]
mod build_stamp;
#[path = "../src/harmony_update/mod.rs"]
mod harmony_update;
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(result) = harmony_update::cli_main(&args) {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let mode = args.first().and_then(|s| s.to_str()).unwrap_or("");
    match mode {
        "emit-stamp" => build_stamp::emit(),
        "sleep" => std::thread::sleep(std::time::Duration::from_millis(
            args[1].to_str().unwrap().parse().unwrap(),
        )),
        "bytes" => {
            use std::io::Write;
            std::io::stdout()
                .write_all(&vec![b'x'; args[1].to_str().unwrap().parse().unwrap()])
                .unwrap();
        }
        "chunk-and-sleep" => {
            use std::io::Write;
            std::io::stdout().write_all(b"MZ").unwrap();
            std::io::stdout().flush().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
        "fail" => std::process::exit(7),
        "handoff" => {
            harmony_update::transaction::spawn_helper(std::path::Path::new(&args[1])).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        _ => panic!("unknown fixture mode"),
    }
}
