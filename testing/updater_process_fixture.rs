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
        "auth" | "api" => {
            // Official-gh stand-in ONLY in the owned PATH of the UI runner.
            // Logs subprocess execution, never contacts a service or keyring.
            use std::io::Write;
            let log = std::env::var_os("HARMONY_FIXTURE_GH_LOG").expect("owned gh fixture log");
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(log)
                    .unwrap(),
                "{mode}"
            )
            .unwrap();
            if mode == "api" {
                println!("[]");
            }
        }
        "fail" => std::process::exit(7),
        "identify-current" | "identify-substituted" => {
            if mode == "identify-substituted" {
                let running = std::env::current_exe().unwrap();
                std::fs::rename(&running, running.with_file_name("original-image")).unwrap();
                // Same bytes, different inode: repeated path hashes cannot
                // distinguish this substitution. This new file is NEVER run.
                std::fs::copy(&args[1], &running).unwrap();
            }
            std::fs::write(
                &args[2],
                if harmony_update::controller::current_install().is_ok() {
                    b"accepted".as_slice()
                } else {
                    b"rejected".as_slice()
                },
            )
            .unwrap();
        }
        "lease-parent" => {
            use harmony_update::lifetime::{self, Parent};
            let parent = Parent::for_self().unwrap();
            let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
            cmd.arg("lease-child")
                .arg(&args[1])
                .arg(&args[2])
                .stderr(std::process::Stdio::null());
            if args[2] == "early" {
                cmd.arg(parent.pid.to_string())
                    .arg(parent.birth.to_string())
                    .arg("parent-stdin-v1")
                    .stdin(parent.into_stdio())
                    .stdout(std::process::Stdio::null());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0800_4000);
                }
                cmd.spawn().unwrap(); // Deliberately exit before child validation.
            } else {
                let result = lifetime::launch(cmd, parent);
                if args[2] == "wrong-birth" || args[2] == "wrong-pid" {
                    assert!(result.is_err());
                } else {
                    assert!(result.is_ok());
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            }
        }
        "lease-child" => {
            use harmony_update::lifetime::{self, Parent};
            if args[2] == "early" {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            let mut pid = args[3].to_str().unwrap().parse::<u32>().unwrap();
            let mut birth = args[4].to_str().unwrap().parse::<u64>().unwrap();
            if args[2] == "wrong-birth" {
                birth += 1;
            }
            if args[2] == "wrong-pid" {
                pid += 1;
            }
            match Parent::receive(pid, birth) {
                Ok(parent) => {
                    lifetime::acknowledge().unwrap();
                    parent.wait().unwrap();
                    std::fs::write(&args[1], b"exited").unwrap();
                }
                Err(_) => std::fs::write(&args[1], b"rejected").unwrap(),
            }
        }
        "handoff" => {
            let install = std::path::Path::new(&args[1]);
            let expected = harmony_update::transaction::pending(install)
                .unwrap()
                .unwrap()
                .0;
            harmony_update::transaction::spawn_helper(install, &expected).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        _ => panic!("unknown fixture mode"),
    }
}
