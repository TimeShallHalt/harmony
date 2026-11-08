# Harmony Windows MSI

This is an offline, per-user x64 Windows Installer package for the verified updater-free Harmony build. It is not an automatic updater, bootstrapper, download service, signing service, or public distribution change.

## Using it

1. Quit Harmony through its tray **Quit** menu. Closing the window may only hide it.
2. Run the MSI normally, not as a different administrator account. It defaults to `%LOCALAPPDATA%\Programs\Harmony` and remembers an MSI-managed custom program folder.
3. Open your existing shortcut afterward. Setup does not replace shortcuts or their custom `--data-dir` arguments, start the app, resume calls/capture, or automatically terminate processes. In-use files require user action; no reboot is requested automatically.

Only enumerated program/notice/source files are owned by MSI. User settings/accounts/data are not included, reset, or removed on uninstall. Unowned files and rollback copies are left alone. Standard uninstall removes empty declared directories only, including their empty Programs ancestor, never recursive directory contents. The first MSI adopts an unmanaged manual installation; a later uninstall removes its package-owned program files, not restores the previous manual executable. Automatic Restart Manager relaunch is also explicitly disabled with MSIDISABLERMRESTART=1.

The package includes its corresponding GPL source archive and third-party notices. The installer is unsigned; Windows may show **Unknown Publisher**. Do not disable security controls.

## Version and identity rules

- MSI compares only three version fields. Encode app `A.B.P` build `N` as MSI `A.B.(P*1000+N)`, with `1 <= N <= 999`, `A,B <= 255`, and the final field at most 65535. App `1.0.0` build 1 is therefore MSI `1.0.1`; this is not a different application build.
- UpgradeCode is stable for the per-user x64 product; ProductCode changes with significant MSI version. Same-release rebuilds use the same ProductCode. Component GUIDs and HKCU key paths are stable by relative file path.
- Major upgrades remove the old MSI after InstallInitialize, inside the rollback transaction. Newer MSI-registered versions block downgrades.
- The current executable has no PE version resource. `REINSTALLMODE=amus` deliberately replaces only the enumerated program files so legacy manually copied binaries are actually updated. A newer **unregistered manual/portable** build cannot be version-detected; do not run an older MSI over one. This is not authenticated artifact transport; the builder takes a trusted local producer manifest and rechecks its byte identities.
- Native ICE40 (explicit overwrite mode) and ICE91 (per-user-only destination that cannot become all-users) warnings are intentional. Other validation errors/warnings must not be silently suppressed.

## Build

Use Python on Windows, an existing .NET runtime capable of running the pinned WiX v4 CLI, and matching UI extension. WiX is build-only; installer users do not need Python, Git, WiX or .NET.

```
python scripts/package_harmony_msi.py --manifest PATH/TO/release-artifacts.json --wix-dll PATH/TO/wix.dll --ui-extension PATH/TO/WixToolset.UI.wixext.dll --output PATH/TO/Harmony.msi
```

The builder does not download tools, rebuild Rust, install/launch the app, change repository visibility, or upload anything. Retain a pinned tooling provenance record and the original application production-build evidence. A matching sidecar or AMD64 header alone is not executable attestation.

## Verification boundaries

Unit tests cover version limits, per-user file/registry ownership, unchanged shortcuts, archive/hash pairing and safe staging, disjoint fixture identities, and nonrecursive empty-directory cleanup. A real small WiX compilation test uses explicit `HARMONY_WIX_DLL` and `HARMONY_WIX_UI` environment paths.

`testing/msi_database.py` reads MSI tables without WMI Win32_Product (which can trigger unrelated repairs). `testing/run_msi_lifecycle.py` is an explicit opt-in test driver: separate random product/upgrade/component GUIDs, a hidden fixture name, fixture-only HKCU keys and a scratch program directory. It uses the real previous production executable as a legacy-file preimage, checks full payload hashes after installation/upgrade/repair, checks the exact downgrade error, then uninstalls only generated fixture IDs and verifies data/unowned-file canaries and live process handles. It never installs the production MSI. Fixture MSI version changes do not represent new app builds.

Full interactive wizard presentation and deliberate crash/fault rollback injection are separate, unclaimed checks. Keep actual test/scan/source-review receipts; do not infer passing lifecycle tests from successful compilation or a nonzero downgrade exit alone.
