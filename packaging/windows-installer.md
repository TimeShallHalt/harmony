# Harmony Windows GUI installer

One standard Inno Setup script: `packaging/windows/harmony-simple.iss`.
The custom coordinator, IPC, bridges and receipt framework were removed.

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
- A previous-install information page and an editable destination folder.

Same-scope updates default to the prior location. The known legacy MSI 1.0.3001
is identified using native context/assignment, not its registry hive. This simple
Inno recipe is also detected. Unknown recipes/multiple registrations stop with an
explanation rather than running arbitrary commands.

Scope/location changes invoke the supported previous program's normal uninstaller
before installing. Shared-to-personal removal requests UAC for that removal only;
the personal install continues under the initiating user. Start Setup normally,
not through another person's already-elevated shell. Shared relocation installs
an ordinary, uninstall-owned executable copy at the former path for old shortcuts.

Profiles, credentials, settings and custom data directories are not read, copied,
reset or deleted. Program scope is separate from each Windows user's saved data.
Moving to personal removes shared app availability, not other users' data.
Setup asks you to quit affected Harmony processes; it never kills, restarts or
launches the app. Existing shortcuts are not overwritten; absent startup/protocol
integrations are not enabled. Uninstall removes logged program files only.

Previous-program removal and new installation are not atomic. If new copying fails
after removal, rerun Setup; there is no claimed custom rollback framework.

## Native smoke test

```sh
python3 testing/test_simple_installer_native.py --allow-private-install --manifest PATH/TO/release-artifacts.json
```

For a separate release worktree, pass `--tooling-receipt PATH/TO/tooling-provenance.json` to either command to use the prepared compiler.

The private-identity test covers current-user installation, a custom Unicode directory, the full-quit update handoff, relocation, uninstall, reinstall and cleanup. Every payload hash and
registration is read back; synthetic data and a foreign file must survive. Harmony
is never run. Real machine-scope/UAC migration and real account restoration require
separate interactive validation. Evidence and Defender results live under
`.local/evidence/simple-installer`.
