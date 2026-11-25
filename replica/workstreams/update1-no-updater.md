# Harmony Update 1 — no updater

## Authorized scope

This candidate contains About/build information, native theme packs, native window
controls, and the image/GIF viewer's zoom, pan and navigation foundation. All
updater/auto-update and hosting/sharing implementation is deferred. No push,
release publication, installation or activation is performed by this worker.
Experimental updater branches remain preserved; they are not prerequisites for
this candidate and the rejected backend checkpoint is not merged.

The candidate removes the updater module, CLI/bootstrap/helper entrypoints,
startup/exit hooks, persisted policy and update controls, rather than merely
hiding a switch. About states `Updater not included in this build.` Old layout
update-consent fields are ignored by deserialization and are not serialized.
Normal protocol/autostart commands once again identify the running executable,
not an experimental stable update launcher.

## Packaging

Suggested first private release: Harmony 1.0.0, build 1. A clean-source build is
required for a positive `HARMONY_BUILD_ID`. Use `scripts/package-harmony.py` for
an offline portable Windows x64 ZIP and its matching tracked source ZIP, with
GPL-3.0-or-later, font/Twemoji and native/Rust dependency notices. This helper
never uploads, installs, registers startup, stops the app or launches it.

The existing upstream Inno Setup recipe targets `%LOCALAPPDATA%/Programs/Yapfast`
and forcibly closes applications. It is NOT used for this release. Historical
Harmony installation tooling targets `%LOCALAPPDATA%/Programs/Harmony/yapfast.exe`;
that installation and any running process remain untouched. Legacy Yapfast
storage/integration identifiers remain intentionally unchanged to preserve data.
For a manual update, the user must choose when to close the existing app and keep
the previous executable separately. No rollback/installation test is claimed for
a ZIP that is deliberately not installed.

## Verification boundaries

Acceptance is based on exact-source Windows integrated compilation and selected
registered tests using Slint's in-memory backend, plus package/hash/source
readback. Tests are run individually in fresh processes to avoid process-global
backend collisions. Physical input, OS capture, focus, GPU presentation, actual
Win32 move loops and Linux desktop runtime are outside the permitted boundary.
No Linux runtime claim is made. Final results and artifact hashes are recorded
outside the source commit at `.local/evidence/update1-no-updater/` in the parent
repository. An independent parent review remains required before publication.
