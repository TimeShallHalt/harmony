# Harmony Windows GUI installer

One standard Inno Setup script: `packaging/windows/harmony-simple.iss`.
The custom coordinator, IPC, bridges and receipt framework were removed.

## Build

```sh
python3 scripts/build-harmony-installer.py --output-dir dist/harmony-simple
```

The builder verifies the existing Harmony 1.1.0 build 1 portable/source archives
and pinned Inno 7.1.1 compiler, then produces an unsigned EXE. The application
is not rebuilt. Application source, legal notices and installer source are included.

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
python3 testing/test_simple_installer_native.py --allow-private-install
```

This private-identity test covers current-user install, custom Unicode directory,
update, relocation, uninstall, reinstall and final cleanup. Every payload hash and
registration is read back; synthetic data and a foreign file must survive. Harmony
is never run. Real machine-scope/UAC migration and real account restoration require
separate interactive validation. Evidence and Defender results live under
`.local/evidence/simple-installer`.
