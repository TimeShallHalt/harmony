# Ren's Yapfast workspace

## Layout

- Editable repository: `C:\Users\ren\Documents\Apps\yapfast`
- Local customization branch: `ren/custom-client`
- Upstream remote: `origin`, https://github.com/yapfast/yapfast.git
- Installed Windows portable deployment: `%LOCALAPPDATA%\Programs\Yapfast` (official v0.3.0), with a Start menu shortcut. Startup was not enabled. Launch verified: responding process and sign-in/local-preview controls present.
- Official baseline being reviewed for installation: v0.3.0.
- Working source starts at upstream main commit `6a6e3e51ce0449c81bb34081678ad0d9d32b39fb`, newer than the stable binary. Source edits do not update the installed executable until built and deployed.

Private GitHub source repository: https://github.com/TimeShallHalt/harmony. This is a private copy, not a public GitHub fork. The original upstream remote remains `origin`; the private repository is configured as remote `harmony`. Preserve upstream license and attribution. GitHub Actions are disabled on the private copy to avoid triggering upstream build/release jobs; the workflow files remain included for both supported platforms.

## Platform requirement

Windows and Linux are both first-class targets for all future changes. Keep shared application behavior in Rust and UI changes in the shared Slint interface. Isolate operating-system integrations behind existing platform modules/cfg gates; never hard-code this workstation's Windows paths into shared application behavior.

Retain Cargo.lock, rust-toolchain.toml, nix/, flake.nix, Linux packaging and the existing Linux and Windows jobs in .github/workflows/nightly.yml. Do not perform unrestricted dependency upgrades.

Before shipping custom versions, run checks and build for both platforms. A Windows launch alone is NOT Linux validation. This initial setup does not build or execute Linux artifacts.

## Linux development, from upstream docs

With Nix and a graphical Linux session:

```sh
nix develop path:./nix --command cargo run --locked --no-default-features --features desktop,discord -- --preview
nix develop path:./nix --command cargo build --locked --release --no-default-features --features desktop,discord
scripts/check --fast
```

Consult docs/building.md and docs/packaging.md for native dependencies and AppImage packaging. Stable upstream already publishes a Linux AppImage and Arch package.

## Harmony first-build notes

The native Windows build uses Rust 1.97.1, Visual Studio 2022 C++ tools/SDK, LLVM 23.1.5, CMake/Ninja, and portable NASM 3.02 in `.local/tools/nasm`. LLVM was installed from its official winget package with user approval; NASM's per-user winget installer was unavailable, so the official portable ZIP was used and Defender-scanned.

The isolated `nix/skia-windows-builder/Cargo.lock` originally paired bindgen 0.73.2 / Syn 3 with prettyplease 0.2.37 / Syn 2, causing a native compiler error. Only that builder's prettyplease was updated to 0.3.0, matching the root lockfile's existing Syn 3 pairing. The prepared Skia build then completed successfully. The root Cargo.lock was not updated.

Load the VS x64 developer environment and put LLVM/bin, NASM, CMake and Ninja on PATH. Set LIBCLANG_PATH to LLVM/bin and LLVM_HOME to the LLVM installation. Then use `python scripts/prepare-windows-skia.py --jobs 4` once and its `--print-env` output to select the prepared archive. Build Discord-only for local testing:

```sh
cargo test --locked --profile fast --no-default-features --features desktop,discord harmony_ -- --test-threads=1
cargo build --locked --profile fast --no-default-features --features desktop,discord
```

Do not distribute this native development build as a production release; upstream's packaging documentation requires its supported cross-build route for releases.

## Verified first milestone

Harmony is installed alongside the stock rollback app at `%LOCALAPPDATA%\Programs\Harmony\yapfast.exe`. Start menu shortcuts are `Harmony` (normal mode, isolated `%LOCALAPPDATA%\Harmony\data`) and `Harmony Preview` (account-free, isolated preview data). The installed development executable SHA-256 is `7bbe8b57ca09616626e08e629da52b906593d67505ef721d366175f02d37504f`; installed/source-build hashes matched and Defender reported no threats. It launched and rendered as Harmony.

- Six source-contract branding checks passed after the final notice wording fix.
- Two native Harmony tests passed; native UI subset: 184 passed, 1 benchmark ignored.
- Wider root app tests: 231 passed, 1 failed, 6 ignored. Failure: `emoji_text::tests::a_picture_keeps_its_round_edges_wherever_it_lands`, bottom-edge pixel assertion. Its source and renderer were not changed by this work; no claim that the full suite is green or that it has been proven to fail on pristine upstream.
- Real Windows preview: live switch Bonfire -> Tokyo Night -> Catppuccin Latte updated titlebar and content; custom maximize/restore/minimize and close-to-tray were exercised. Drag and resize callback directions were verified through native tests, not a complete multi-monitor OS gesture sweep.
- A leftover session-stopped notice seen during preview was rebranded, rebuilt, rescanned, and verified in the installed app. Preview currently emits that pre-existing stopped-session banner on opening; it is dismissible. No Discord account was linked.
- Linux native code and packaging remain intact, but no Linux build/runtime verification was performed here.
- Primary display branding is Harmony. Internal Yapfast identities, upstream icon, theme IDs, licenses, and some less-used backend strings remain intentionally unmigrated for now.

## Current native extension verification

- Theme-pack production module: 18 native tests passed on Windows and 18 passed on native Linux in Ubuntu WSL, including FIFO rejection, held-handle directory/leaf replacement, symlink/junction rejection, bounded PNG validation and export-failure ordering. This is module-level Linux verification, not a Linux desktop build or runtime claim.
- Production media-viewer harness: all 11 native Windows tests passed, covering bounded zoom, decoded-pixel actual size, pointer/wheel pan and zoom, resize clamping, navigation, reset and animation-frame identity. The resizable harness uses preferred dimensions and asserts propagated view size.
- Branding/theme/media source contracts: all 18 checks passed. They supplement native tests and do not prove OS window movement.
- User-reported Windows drag/resize regression reproduced with native input and unchanged window rectangle. A Windows-only native bridge fixes a proven winit 0.30.13 message-packing defect; Linux retains winit. Two native queue/packing tests and two imported production-chrome hit-region tests passed, but queue/callback success is not proof of physical window movement. The first patched preview did not pass CUA movement checks.
- Actual GUI theme workflow in isolated Windows preview passed import, native-dialog export, reset logo and explicit reload. Read-back confirmed the saved Orchid layout/typography/logo snapshot and matching exported PNG bytes. The user's live app/data were not changed.
- Physical drag/all-eight-directions/caption regression is prepared in `testing/test_window_chrome_native.py`, but deferred at the user's explicit request while gaming. Do not move the cursor, steal focus or restart his active call. The hidden Win32 queue test also manipulates OS capture, so it is ignored by default and requires the explicit `testing/run_window_chrome_tests.py --native` opt-in on an idle desktop.
- Final background-only integrated checks: 268 passed, 1 failed, 6 ignored, 1 filtered out (the capture-mutating native queue test). The only failure is `emoji_text::tests::a_picture_keeps_its_round_edges_wherever_it_lands`; do not label the full suite green. The Windows application build succeeded and Defender reported no threats.
- Installed development artifact: `%LOCALAPPDATA%\Programs\Harmony\yapfast.exe`, SHA-256 `3f6c512cf86088d452f3d3f11a4dac49a11c99254ccf9429ac0db652d0adf96e`, matching `target/fast/yapfast.exe`. The staged copy was Defender-scanned and the installed file was independently rehashed. Previous executable is retained alongside it as `yapfast.rollback-7bbe8b57ca09.exe`, with its original hash verified.
- Deployment preserved the existing responding process (PID 74192), user data, mouse and game focus: the running application remains on the prior build until the user fully quits and relaunches Harmony. No restart or physical gesture test was performed. `BUILD.json` in the install folder records the installed artifact and verification limits. Actual window movement remains unverified.

## Deployment policy

Keep installed binaries separate from source. Scan new build artifacts, retain a rollback copy, and preserve the user's data directory. Prefer a closed application for replacement. If Windows permits renaming the running executable, retain it as rollback, install the verified staged copy at the unchanged shortcut target, independently verify both hashes and the surviving process, and leave activation until the user's next full quit/relaunch. If rename is blocked, leave the copy staged and request a convenient exit; never kill an active call. Do not overwrite the custom app with upstream updates without reviewing the changes. Do not enable startup, change Discord link handlers, log in, or import account tokens automatically.

## Initial safety checks

Official v0.3.0 Windows portable ZIP SHA-256:
`f720b6910e05a632fad64c8419f7af6733028a7453ec693fa3ecc64c68044a6e`

Verified against both GitHub's release asset digest and the release's SHA256SUMS.txt. Microsoft Defender custom scans of the download/unpacked release and the cloned repository completed with exit code 0 and reported no threats. The executable is unsigned. Hash verification establishes download integrity, not absence of malware. Bounded first-party source triage of main and v0.3.0 found no clear credential stealing, malicious exfiltration, or covert persistence. Review noted a media redirect allowlist hardening gap, an environment-selectable stream worker executable, and mutable GitHub Actions version tags. Normal launch had no YAPFAST_STREAM_WORKER override. These findings are not established exploits, but should be reviewed before custom releases. No exhaustive dependency audit, runtime traffic audit, or reproducible-build comparison was performed. A bounded source review and antivirus scan cannot prove safety or binary/source equivalence. Unofficial Discord client account-policy and compatibility risks remain.
