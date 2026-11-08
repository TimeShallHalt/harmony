//! Standalone, account-free updater process fixture. Never links/opens the app.
#![allow(dead_code, unused_imports)]
#[path = "../scripts/harmony_build_stamp.rs"]
mod build_stamp;
#[path = "../src/harmony_update/mod.rs"]
mod harmony_update;
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(result) = harmony_update::bootstrap::install_cli(&args) {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = harmony_update::cli_main(&args) {
        // Owned-fixture wrapper records the REAL production helper result only
        // after it returns. This is not an activation/identity bypass.
        if let Some(path) = std::env::var_os("HARMONY_FIXTURE_HELPER_RESULT") {
            std::fs::write(path, format!("{result:?}")).unwrap();
        }
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
        "normal-startup" | "normal-startup-status" | "normal-authorize-hold" => {
            let install =
                harmony_update::controller::current_install().expect("normal current_install");
            let controller = harmony_update::controller::Controller::new(Ok(install.clone()));
            let loaded = controller.load_pending();
            if mode != "normal-startup-status" {
                loaded.clone().expect("normal Controller startup");
            }
            controller.handoff_on_full_exit().unwrap(); // No authorization -> no publication.
            if mode == "normal-authorize-hold" {
                assert!(!controller.snapshot().staged.as_ref().unwrap().1.armed);
                controller
                    .arm(true)
                    .expect("explicit recovery authorization");
                controller
                    .handoff_on_full_exit()
                    .expect("normal natural-exit handoff");
                std::fs::write(&args[2], b"authorized helper ready; parent alive").unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
                while !std::path::Path::new(&args[3]).exists() {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "owned parent release timeout"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            let staged = controller.snapshot().staged;
            std::fs::write(
                &args[1],
                serde_json::to_vec(&serde_json::json!({
                    "install": install,
                    "entry": harmony_update::controller::normal_entry().unwrap(),
                    "load_error": loaded.err().map(|e| e.to_string()),
                    "phase": staged.as_ref().map(|(_, p)| &p.phase),
                    "armed": staged.as_ref().is_some_and(|(_,p)| p.armed)
                }))
                .unwrap(),
            )
            .unwrap();
        }
        "identify-current" | "identify-substituted" | "identify-legacy-layout" => {
            let proposed = std::env::current_exe().unwrap();
            if mode == "identify-substituted" {
                let running = &proposed;
                std::fs::rename(&running, running.with_file_name("original-image")).unwrap();
                // Same bytes, different inode: repeated path hashes cannot
                // distinguish this substitution. This new file is NEVER run.
                std::fs::copy(&args[1], &running).unwrap();
            }
            std::fs::write(
                &args[2],
                if (if mode == "identify-legacy-layout" {
                    harmony_update::controller::current_install().map(|_| ())
                } else {
                    harmony_update::image_identity::running(&proposed).map(|_| ())
                })
                .is_ok()
                {
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
        "recover-handoff" => {
            let controller =
                harmony_update::controller::Controller::new(Ok(std::path::PathBuf::from(&args[1])));
            controller.load_pending().unwrap();
            assert!(!controller.snapshot().staged.as_ref().unwrap().1.armed);
            controller.arm(true).unwrap();
            controller.handoff_on_full_exit().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        "handoff" | "handoff-hold" => {
            let install = std::path::Path::new(&args[1]);
            let expected = harmony_update::transaction::pending(install)
                .unwrap()
                .unwrap()
                .0;
            harmony_update::transaction::spawn_helper(install, &expected).unwrap();
            if mode == "handoff-hold" {
                std::fs::write(&args[2], b"ready").unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !std::path::Path::new(&args[3]).exists() {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "owned handoff release timed out"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        _ => panic!("unknown fixture mode"),
    }
}
