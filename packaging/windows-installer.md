# Harmony Windows GUI installer

Inno Setup builds the installer from `packaging/windows/harmony-simple.iss`.
The Windows maintenance helper in `packaging/windows/repair` inventories programs and repairs launchers for the initiating account.

## Build

```sh
python3 scripts/build-harmony-installer.py --manifest PATH/TO/release-artifacts.json --output-dir dist/harmony-simple
```

The builder verifies the portable and source archives from the supplied release manifest
and pinned Inno 7.1.1 compiler, then produces an unsigned EXE. The application
is not rebuilt. Application source, legal notices and installer source are included.

## Sign an update

On the publisher's Windows machine, run this from the release checkout after building and checking the installer. The key name is `Harmony release updates v1` in the current user's Windows CNG store. `packaging/keys/harmony-update.pub` contains its public P-256 key.

```powershell
./scripts/sign-harmony-update.ps1 -Tag harmony-v1.2.0-b1 -Installer ./dist/harmony-simple/Harmony-1.2.0-Setup.exe -OutputDirectory ./dist/update-metadata
```

Attach both `harmony-update.json` and `harmony-update.json.sig` alongside that installer on the matching stable GitHub release. Harmony verifies the signature, tag, platform, size and SHA-256 before offering to run Setup on full quit. Registered Inno installs keep their scope and folder. Other Windows copies open the installation wizard. On Linux the release page is available for manual installation.

The private signing key is non-exportable. Moving signing to another Windows account or PC requires shipping a trusted public-key change while the current signer is available, or distributing a new installer manually.

## Options

- **Install / update for all users** (default folder `%ProgramFiles%\Harmony`).
- **Install / move to me only** (default `%LOCALAPPDATA%\Programs\Harmony`).
- **Repair installed copies and keep the newest version** lists the discovered programs and the folder it will keep. Run `Harmony-1.2.0-Setup.exe /CURRENTUSER /REPAIR` to select this mode directly.

Setup compares numeric major, minor, patch and build numbers. A registered installation wins a tie with a portable deployment, then the shared installation wins. Repair can keep a newer version than the one bundled in Setup. The surviving program can stay open. Close any older copy that Repair needs to remove.

Install and update also clean up superseded copies after verifying the replacement executable. Same-scope updates default to the existing folder. Start Setup from your Windows account. Removing a shared installation can prompt for administrator access and ends shared access for other users. A relocation that reuses the same Windows installer registration must retire the previous registered program before installing. Rerun Setup if copying is interrupted.

Setup recognizes Harmony's registered Inno identity, its Windows Installer product family, and the personal programs folder and versioned releases beneath it. It verifies the executable against its build record. The pre-rebrand MSI migration checks the original cached package. Registered programs use their own uninstallers. Versioned portable releases use a file ownership catalog. Changed and unrecognized files stay in place. For a release without a catalog, Repair removes the verified executable and its build record.

Repair removes redundant Start entries and the old Harmony Preview shortcut. It retargets existing account shortcuts, pins, protocol handlers and startup commands that point to a discovered executable. Custom arguments, including `--data-dir`, stay attached to those launchers. Profile directories, sign-ins and rollback archives stay in place.

## Native smoke test

```sh
python3 testing/test_simple_installer_native.py --allow-private-install --manifest PATH/TO/release-artifacts.json
python3 testing/test_installer_repair_native.py --allow-private-install
```

For a separate release worktree, pass `--tooling-receipt PATH/TO/tooling-provenance.json` to either command to use the prepared compiler.

The first test installs the full package under a private identity and exercises the updater handoff, Unicode paths, relocation, uninstall and reinstall. The second uses synthetic executables to exercise a running keeper, a running older copy, duplicate and foreign shortcuts, Unicode profile arguments, numeric version ordering, downgrade refusal, normal-update cleanup and repeated Repair.
