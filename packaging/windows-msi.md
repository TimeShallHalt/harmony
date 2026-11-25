# Harmony Windows MSI

This is an offline, per-user x64 Windows Installer package for the verified updater-free Harmony build. It is not an automatic updater, bootstrapper, download service, signing service, or public distribution change.

## Using it

1. Quit Harmony through its tray **Quit** menu. Closing the window may only hide it.
2. If a legacy Harmony MSI owns `yapfast.exe`, **stop here and follow Manual legacy MSI migration below**. The renamed MSI refuses that upgrade before removing the old product, including in silent `/qn` mode. It does not migrate launchers automatically.
3. Otherwise run the MSI normally, not as a different administrator account. It defaults to `%LOCALAPPDATA%\Programs\Harmony` and remembers an MSI-managed custom program folder.
4. Open `Harmony.exe` or an existing shortcut already targeting `Harmony.exe`, preserving custom `--data-dir` arguments. Setup does not replace shortcuts, claim/change protocol handlers, enable/change startup, start the app, resume calls/capture, or automatically terminate processes. In-use files require user action; no reboot is requested automatically.

Only enumerated program/notice/source files are owned by MSI. User settings/accounts/data are not included, reset, or removed on uninstall. Unowned files and rollback copies are left alone. Standard uninstall removes empty declared directories only, including their empty Programs ancestor, never recursive directory contents. The first MSI adopts an unmanaged manual installation; a later uninstall removes its package-owned program files, not restores the previous manual executable. Automatic Restart Manager relaunch is also explicitly disabled with MSIDISABLERMRESTART=1.

The package includes its corresponding GPL source archive and third-party notices. The installer is unsigned; Windows may show **Unknown Publisher**. Do not disable security controls.

## Manual legacy MSI migration

This is an explicit manual alternative, not a permission for setup to edit live integrations. Leaving the old installation intact is safe; simply cancel the MSI if you are not ready. The portable build is the preferred delivery path for this rebrand.

1. Quit through the tray menu. Retain the old MSI/portable rollback artifact and back up your existing settings/account/data directories privately. Record the current program path, chosen data directory, working directory, icon path and **all** launcher arguments. Do not put account data in the new package or source tree.
2. Extract the verified portable `Harmony.exe` and its accompanying notices/source into a **separate, unowned program folder**, not the old MSI-owned folder. Do not delete or rename the registered `yapfast.exe`; MSI repair/uninstall still owns it. Preserve the same data location (including each existing `--data-dir` argument); do not reset accounts or copy a default profile over a custom one.
3. Inspect each existing Harmony/Harmony Preview shortcut and existing taskbar pin. Change only entries whose exact executable target is your recorded legacy installation. Retarget executable and owned icon/working-directory paths to the new portable folder, retaining unrelated fields and arguments. A taskbar pin can retain a separate cached shortcut: verify that exact pin, or replace only that already-existing pin. Leave other applications and other installations untouched.
4. Inspect only already-selected protocol handlers (for example an existing Discord link handler) and already-present startup entries belonging to that **exact old executable path**. Retarget those commands while preserving quoting, arguments, placeholders such as `%1`, and their enabled/disabled state. **Do not register a new protocol, take over another application's handler, or enable startup.** If ownership or the command format is uncertain, leave it alone and keep the old MSI installed; ask the maintainer for an exact reviewed migration rather than running a generic registry/shortcut rewrite.
5. Read back every changed target/command and verify each existing entry resolves to the new executable with its original data selection. Test launching only when you are ready to resume the app. Do not uninstall the old MSI until every owned entry has been accounted for and no entry you use still targets its `yapfast.exe`. You can remain on the portable build without installing a new MSI.
6. Only after that verification, explicitly uninstall the **old Harmony MSI** through Windows Installed apps as the same user. Keep user data and rollback artifacts; do not delete registry keys to bypass the guard. A fresh renamed MSI is optional afterward; if you choose a different final program path, manually retarget the same verified owned entries again. Setup still does not do it for you.

If setup reports a leftover exact legacy component marker after an explicit uninstall, stop and retain the uninstall log for the maintainer. Do not force `RemoveExistingProducts`, erase installer bookkeeping, or pass invented bypass properties. The guard deliberately prefers a refusal over orphaned launchers. Unmanaged portable `yapfast.exe` files alone do not trigger it and are not silently removed.

## Version and identity rules

- MSI compares only three version fields. Encode app `A.B.P` build `N` as MSI `A.B.(P*1000+N)`, with `1 <= N <= 999`, `A,B <= 255`, and the final field at most 65535. App `1.0.0` build 1 is therefore MSI `1.0.1`; this is not a different application build.
- UpgradeCode remains `B85CFFDA-896F-5BBC-8863-BC5F4DD0E810` for the per-user x64 product; ProductCode changes with significant MSI version. Same-release rebuilds use the same ProductCode. Component GUIDs and HKCU key paths are stable by relative file path. Production `Harmony.exe` packages require app 1.1.0 build 1 (MSI `1.1.1`) or later, avoiding reuse of a legacy ProductCode.
- A detect-only Upgrade row searches that exact UpgradeCode for registered versions **below `1.1.1`**. A separate 64-bit HKCU AppSearch reads only `Software\Harmony\Installer\Components`, value `C_bb51d6588f1a73f021a9113e`, the original `yapfast.exe` component key path. Either signal blocks a new install; the version range also catches damaged old component registration, and the exact marker catches unexpectedly versioned legacy payloads. No generic executable/shortcut/registry scan or migration custom action is used. A stale marker fails closed too.
- Both UI and execute sequences explicitly run FindRelatedProducts (25), AppSearch (50), then LaunchConditions (100), before InstallInitialize (1500) and MajorUpgrade's RemoveExistingProducts after it. Thus the legacy refusal precedes the removal transaction in wizard and silent modes. `Installed` allows the already-installed renamed product's repair/uninstall; this is not a migration bypass for a different ProductCode. Fresh installs have neither signal. Compatible later Harmony.exe major upgrades retain the normal transactional removal and newer-version downgrade guard.
- The production payload requires the exact name `Harmony.exe` and rejects legacy `yapfast.exe` bundles. `packaging/windows/harmony.rc` gives the new binary Harmony icon/version fields; MSI upgrade ordering still follows registered MSI product versions, not arbitrary portable executables. `REINSTALLMODE=amus` deliberately replaces only the enumerated program files. A newer **unregistered manual/portable** build is not protected by MSI product registration; do not run an older MSI over one. This is not authenticated artifact transport; the builder takes a trusted local producer manifest and rechecks its byte identities.
- Native ICE40 (explicit overwrite mode) and ICE91 (per-user-only destination that cannot become all-users) warnings are intentional. Other validation errors/warnings must not be silently suppressed.

## Build

Use Python with `Path.is_junction()`, an existing .NET runtime capable of running the pinned WiX v4 CLI, and its matching UI extension. The build-only tool preparer pins **WiX 4.0.6** and **WixToolset.UI.wixext 4.0.6** to their NuGet-catalog SHA-512 values; it does not execute downloads or install a .NET SDK. Scan the downloaded tool directory with your security software before executing it. Installer users do not need Python, Git, WiX or .NET.

Set `HARMONY_TEST_SCRATCH`, `TMPDIR`, `TMP` and `TEMP` explicitly to your approved scratch directory when building/testing; do not assume the shell's default temporary path is the intended profile scratch.

```
python3 scripts/prepare-windows-wix.py
python3 scripts/package_harmony_msi.py --manifest PATH/TO/release-artifacts.json --wix-dll .local/tools/wix-4.0.6/wix/tools/net6.0/any/wix.dll --ui-extension .local/tools/wix-4.0.6/wixtoolset.ui.wixext/wixext4/WixToolset.UI.wixext.dll --output PATH/TO/Harmony.msi
```

The production builder requires a clean, committed installer source tree. The MSI receipt records the **application commit** from the verified portable payload separately from the **packaging commit**, plus builder/tool hashes. Packaging-only changes do not relabel or rebuild the application. The original Harmony icon is embedded as the Installed Apps icon. The preparer refuses an existing tool directory rather than overwriting it; retain its `tooling-provenance.json` alongside build evidence.

`scripts/package-harmony.py` produces the portable `Harmony.exe` ZIP, matching source ZIP, checksums and `release-artifacts.json` consumed here. It requires the clean committed candidate and matching `harmony-build-info.json`. Its stamp credits upstream Yapfast 0.4.1 while retaining the internal Cargo package name `yapfast`.

The builder does not download tools, rebuild Rust, install/launch the app, change repository visibility, or upload anything. Retain a pinned tooling provenance record and the original application production-build evidence. A matching sidecar or AMD64 header alone is not executable attestation.

## Verification boundaries

Unit tests cover version limits/floor, stable upgrade identity, exact legacy marker matching, detect-only legacy version range, generated WXS pre-removal sequencing in both modes, fresh/future payload authoring, unchanged launcher ownership, archive/hash pairing and safe staging, disjoint fixture identities, and nonrecursive empty-directory cleanup. A real small WiX compilation test uses explicit `HARMONY_WIX_DLL` and `HARMONY_WIX_UI` environment paths and additionally checks compiled Upgrade/RegLocator/LaunchCondition and sequence tables. Source-recipe tests do not establish native MSI execution; no installer build, installation or live integration edit is required for the offline unit suite.

`testing/msi_database.py` reads MSI tables, bounded binary streams, and exact product/UpgradeCode state without WMI `Win32_Product` (which can trigger unrelated repairs). The opt-in lifecycle runner uses separate random namespaces for version-only legacy detection, component-marker-only detection, normal renamed upgrades, and rollback. Every compiled MSI is checked against its complete expected tables and byte hashes before registration. Pre-authored upgrade action properties are forbidden, so product-removal lists can come only from the scoped `FindRelatedProducts` search. An explicit reviewed-table allowance rejects additional locators/custom schemas; the generated `Signature` table must be inspected and empty. Full source/installed paths are checked against the native backend's UTF-16 `MAX_PATH` limit without dropping notices. Fixtures receive both `INSTALLFOLDER` and secure `HARMONYFIXTUREPROGRAMS` scratch paths; the latter is remembered in the isolated fixture registry for maintenance and nested predecessor uninstall, so empty-directory cleanup cannot fall back to real `%LOCALAPPDATA%\Programs`.

Prepare a fresh, single-use plan first; this step builds/inspects fixtures but never invokes `msiexec`:

```
python3 -B testing/run_msi_lifecycle.py --release-receipt PATH/TO/Harmony.msi.json --wix-dll PATH/TO/wix.dll --ui-extension PATH/TO/WixToolset.UI.wixext.dll --scratch-root PATH/TO/HEX/PROFILE/cache/scratch --include-fault-rollback --prepare-only
python3 -B testing/run_msi_lifecycle.py --run-prepared PATH/TO/SCRATCH/msi-lifecycle-UUID/plan.json --allow-isolated-msi-registration
```

The second command explicitly permits **fixture-only** registration, never production installation. It rechecks the complete plan, packages, payloads, namespaces, remembered paths, and compiled tables before the first native operation. It exercises independent legacy refusals before removal, then fresh renamed install, upgrade, missing-file repair, downgrade refusal, and uninstall. Controlled rollback uses a fixture-only Type 19 failure after `InstallExecute` has flushed deferred file writes; the production MSI cannot request that hook. Exact installed bytes, differing owned markers, new-only rollback files, fake user-data/unowned-file canaries, registration, and cleanup are checked. All exact fixture identities are independently considered for cleanup even after a failure. Fixture MSI versions are not new application builds.

Offline safety tests, successful compilation, or a nonzero refusal exit alone are not native lifecycle evidence. Retain each actual `plan.json`, MSI table snapshot, native step log, `cleanup.json`, and `result.json`. The runner's real registration tests use `/qn`; full interactive wizard presentation and deliberate process-crash recovery remain separate, unclaimed checks.
