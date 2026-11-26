# Packaging and releases

Harmony is independently versioned, based on
[Yapfast 0.4.1](https://github.com/yapfast/yapfast) by wiredtenshi and contributors.
The Cargo package remains `yapfast`; its explicit binary target is `Harmony`.
Windows produces `Harmony.exe`; native Linux builds produce
`target/<profile>/Harmony`, and Linux packages install the command as `harmony`.
Development instructions are in [building](building.md).

Harmony checks its stable GitHub releases in **Settings → About**. Publish
the signed update files and their matching packages on the same release. Keep
the corresponding source, GPL license, upstream credits and dependency notices
with distributed builds.

## Windows portable package

The primary Windows producer is `scripts/package-harmony.py`. It requires:

- A clean committed candidate, including all build and packaging inputs.
- Its Discord-only AMD64 Windows executable, built with a positive canonical
  `HARMONY_BUILD_ID`, and the matching `harmony-build-info.json` written under
  Cargo's build `OUT_DIR`. An unset build ID is `0`, for development.
- Cargo metadata for that candidate's Discord-only Windows target, the actual
  prepared Skia archive, pinned Skia source and extracted WebRTC tree. Do not
  pass full/Matrix metadata to this producer.
- A new or empty output directory; it refuses to overwrite artifacts.

From the repository root, set these variables to the actual build inputs:

```powershell
python scripts/package-harmony.py --exe "$Exe" --stamp "$Stamp" --metadata "$Metadata" --skia-archive "$SkiaArchive" --skia-source "$SkiaSource" --webrtc "$WebRtc" --output "$Output"
```

It produces `Harmony-VERSION-bBUILD_ID-windows-x64.zip`, the matching
`Harmony-VERSION-bBUILD_ID-source.zip`, `SHA256SUMS.txt` and
`release-artifacts.json`. The portable ZIP contains `Harmony.exe`,
`harmony-build-info.json`, `BUILD.json`, `SOURCE.txt`, `THIRD-PARTY.json`, the
original `LICENSE.txt` and dependency licenses. Its source ZIP accompanies it,
not inside it; preserve the pair. Matching metadata and an AMD64 PE header are
not executable attestation: retain actual clean-build and test evidence.

The optional offline [MSI builder](../packaging/windows-msi.md) consumes the
manifest and includes corresponding source. It needs explicit local WiX CLI and
UI-extension paths; it never downloads tools. WiX is currently unavailable in
the local integration environment, so do not claim MSI compilation or installation.
The optional cross-build/Inno route is separate; see
[the Windows guide](windows-beta.md#rebuild-from-linux).

Quit through the tray's **Quit** menu before manually replacing files; closing
the window may leave the app running. Packaging does not enable startup, change
existing shortcuts, migrate accounts or claim protocol handlers. Retarget an old
`yapfast.exe` shortcut to `Harmony.exe` manually, preserving `--data-dir`.
Existing data and credential identifiers keep legacy Yapfast names for compatibility.

## Linux AppImage

`scripts/build-appimage` writes to an empty `target/dist/linux` (or `--output`):
`Harmony-VERSION-x86_64.AppImage`, `Harmony-VERSION-source.zip` and
`SHA256SUMS.txt`. There is **no `.zsync` artifact or AppImageUpdate channel**.
`nix/appimage.nix` leaves update information empty and clears the runtime's
`.upd_info` section. `scripts/package-linux.py` rejects nonempty update metadata
before packing and from `--appimage-updateinformation` on the finished file.

For a build-stamped release, build the clean checkout with a positive
`HARMONY_BUILD_ID` and `YAPFAST_RELEASE=1` in the native Nix development
shell. Source `scripts/release-env x86_64-unknown-linux-gnu` for the path
remaps, then build with `cargo build --locked --release --no-default-features
--features desktop,discord --bin Harmony`. Pass `--exe PATH`, `--stamp PATH`
and `--runtime PATH` to `scripts/package-linux.py` in that environment with
patchelf, readelf, rsvg-convert and mksquashfs available. Use the build's
`harmony-build-info.json` and the runtime pinned in
`packaging/linux/appimage-policy.json`. Both producers enforce the same ABI
floors and host-library allowlist.

Nix builds `nix/package.nix`'s Discord-only release configuration.
`nix/appimage.nix` puts the unwrapped binary in an AppDir with the desktop file,
icons and `packaging/linux/io.github.TimeShallHalt.Harmony.metainfo.xml`.
Nix installs the wrapper as `harmony` with backing binary `.harmony-wrapped`;
the AppDir binary and AppRun command are `harmony`. The derivation sets the
interpreter to `/lib64/ld-linux-x86-64.so.2` and removes Nix's RUNPATH, so the
binary uses the host loader, glibc and libstdc++. That lets host GPU drivers
load: Mesa needs compatible host libraries, and NVIDIA's userspace must match
its kernel module, so neither driver belongs in the AppImage.

The derivation rejects unexpected linked libraries, a glibc requirement newer
than 2.39 or a libstdc++ requirement newer than GCC 14's. These floors target
Ubuntu 24.04, Fedora 40, Debian 13 and newer; they are compatibility policy, not
recorded Harmony tests on those distributions. A nixpkgs update can raise the
requirements. Raise floors only knowing which distributions that drops; Ubuntu
22.04 needs an older build baseline. Besides C/C++ runtimes, the binary links
GLib, PulseAudio's client, fontconfig and FreeType, and loads EGL, Vulkan,
Wayland, X11 and PipeWire at runtime.

`scripts/package-linux.py` runs in the development shell with squashfs-tools
added from its nixpkgs. Shared helpers in `scripts/release_package.py` add the
original GPL `LICENSE`, licenses/notices, `THIRD-PARTY.json`, `SOURCE.txt` and
`BUILD.json` under `usr/share/doc/harmony`. The source ZIP is also included there
under `corresponding-source/`, in addition to the adjacent copy. Skia's prebuilt
archive has only its own license; third-party notices come from the pinned
source, `nix build path:./nix#skia-source`.

Packing uses mksquashfs (zstd level 19, 1 MiB blocks, fixed owners and times)
behind the hash-pinned official AppImage type-2 runtime. This static runtime uses
FUSE 3's `fusermount3`, not libfuse; `--appimage-extract-and-run` avoids FUSE.
Nix flakes see only Git-tracked files, so new build inputs must be tracked.
Full fork Linux builds, AppImage desktop behavior and screen sharing remain to
be tested; packaging fixture checks do not substitute for them.

## Arch Linux packages

`packaging/arch/PKGBUILD` builds the Discord-only fork from a **local verified
corresponding-source ZIP**, never by cloning a Yapfast version tag. Supply:

- `HARMONY_SOURCE_ARCHIVE`: the local source ZIP available to makepkg.
- `HARMONY_SOURCE_ROOT`: its actual one top-level directory, such as
  `Harmony-VERSION-bBUILD_ID-source` for a build-stamped source ZIP.
- `HARMONY_SOURCE_SHA256`: the verified 64-character lowercase SHA-256 from its
  matching producer's checksums/manifest. `prepare()` refuses an unset/invalid hash.

Build with `makepkg` from `packaging/arch` in an isolated Arch environment.
`makepkg -si` also installs on that environment's host; do not use it on the live
system merely to check the recipe. WebRTC and Skia archives keep their pinned
checksums. Arch's LLVM 21 can be newer than Nix's, which is why
`nix/prepare-skia-native.py` accepts LLVM 21 or later. Validate this recipe in a
clean `archlinux:base-devel` container before distribution.

To package the AppImage's existing binary, run `scripts/package-arch [APPIMAGE]`
after `scripts/build-appimage`. `packaging/arch/release.PKGBUILD` makes
`harmony-VERSION-1-x86_64.pkg.tar.zst` beside the AppImage and adds its hash to
`SHA256SUMS.txt`. It carries that AppImage's desktop entry, icon, metainfo,
license, notices and corresponding source. The script needs Docker: it builds
with makepkg, checks with namcap, installs **inside the disposable container**,
and runs `harmony --smoke-test`. This is script behavior, not a recorded passing
Linux test for this integration.

For prereleases the script removes the first version hyphen for pacman's
ordering (`A.B.P-nightly.DATE.RUN` becomes `A.B.Pnightly.DATE.RUN`), rather than
adding a dot before `nightly`. `sudo pacman -U FILE.pkg.tar.zst` installs an
approved package; no Harmony package repository is configured, so `pacman -Syu`
does not automatically fetch a fork release.

## Nightly releases

`.github/workflows/nightly.yml` is retained upstream machinery, not the supported
Harmony publishing path. It still contains Yapfast artifact globs, release titles,
signing identities and AppImage-updater prose. Do not dispatch it as a fork
release workflow without a separate audit and explicit release authorization.
The packaging scripts operate locally and do not publish.

The inherited convention is `A.B.0-nightly.YYYYMMDD.RUN`: the UTC day and run
number order same-day builds. `scripts/set-version` edits version fields; that
neither builds nor publishes anything. A future fork pipeline must validate its
own artifacts, source pairing, keys and destinations, not substitute upstream
tags or infer an update channel from SemVer.

## Which build is running

Settings and `harmony --version` on installed Linux (or `Harmony.exe --version`
on Windows) identify Harmony. About diagnostics retain the full commit, app
version, numeric build ID, dirty/development state, platform, architecture,
`Upstream: Yapfast 0.4.1`, GPL-3.0-or-later and both source links. The footer also
carries the channel and short commit/date when available (`src/build_info.rs`).
Report actual output, not a sample release string as publication evidence.

`build.rs` uses `scripts/harmony_build_stamp.rs` to write
`harmony-build-info.json`. A positive `HARMONY_BUILD_ID` requires a clean committed
Git tree; development uses `0`. `scripts/package-harmony.py` checks stamp version,
commit, clean state, platform, architecture, upstream baseline and license.
Do not edit a sidecar to bless an old executable.

The separate channel footer keeps internal inputs `YAPFAST_RELEASE`,
`YAPFAST_COMMIT` and `YAPFAST_COMMIT_DATE`. Packaging sets the release flag; a
plain `cargo run` is a development build. Nix supplies the commit and UTC date
from `self.rev`/`dirtyRev` and `lastModifiedDate` because its sandbox has no Git.
This footer is not the clean-tree stamp: a channel label alone proves neither
a clean candidate nor a validated release.

## Newer releases

The feed reads the latest stable release of `TimeShallHalt/harmony` and
orders canonical `harmony-vMAJOR.MINOR.PATCH-bBUILD` tags by version and build
number. Use a positive, increasing build ID when releasing a new build at the
same version. Drafts, prereleases and unrelated tags stay outside the feed.
The About page provides release notes and the release page link.

On Windows x64, Harmony verifies the publisher's manifest and installer
before offering to run Setup on full quit. On Linux x86_64, a running
AppImage in a writable user-owned folder can download and verify its
replacement. Full quit applies the replacement atomically when enabled, and
the next launch runs the new version. Other Linux installation types can
open the release page to get the update.

### New tags

A setting can show a NEW tag until its page has been visited and left
(`src/ui/new_tags.rs`). Tagged settings carry introduction versions;
`first_version` and `seen_new` in `layout.json` track which were met. These are
local presentation markers inherited from upstream, not evidence of an updater
or published Harmony release. A fresh layout counts existing settings as met.

To tag a setting, add its key, section and version to the list and give its row
`new-key: "<key>"`; tests check agreement with the `.slint` files. Preserve legacy
markers for existing layouts and review Harmony's independent version history
before assigning new introduction versions.

## Signing releases

Use `scripts/sign-harmony-update.ps1` on the Windows publisher account with
the existing Harmony CNG key. The desktop updater pins
`packaging/keys/harmony-update.pub`. The signature authenticates the platform,
tag, filename, SHA-256 and length in the manifest. It is used by the Harmony
updater and is separate from Windows Authenticode.

For Windows run the [Setup signing command](../packaging/windows-installer.md)
and attach `harmony-update.json` and `harmony-update.json.sig`. For Linux
pass `-Platform linux-x64 -AppImage PATH` together with the same tag and
output directory. Attach `harmony-update-linux-x64.json` and
`harmony-update-linux-x64.json.sig`. Upload both packages, the source
archives and checksums before publishing the release as latest stable.

## The app icon

`packaging/icon.svg` is Harmony's colored application icon;
`ui/icons/logo.svg` is the monochrome, theme-tinted UI mark. Imported theme-pack
logos remain local overrides; product artwork changes must not erase them.

The Linux app ID is `io.github.TimeShallHalt.Harmony` (`src/icon.rs`), matching
`packaging/linux/io.github.TimeShallHalt.Harmony.desktop`, the icon key and
AppStream ID. `scripts/install-desktop-entry [EXECUTABLE]` installs for the current
user, defaulting to native Cargo output `target/release/Harmony`; packaged entries
use `harmony --open-link %u`. An AppImage installs nothing at packaging time;
on launch `src/desktop_entry.rs` can write the entry/icon under `~/.local/share`,
naming the AppImage in Exec and TryExec. It refreshes a moved AppImage and respects
existing package/manager entries. X11 also receives the icon directly, and
notifications name the desktop entry.

On Windows, `build.rs` compiles `packaging/windows/harmony.rc` with `llvm-rc`,
or the SDK's `rc.exe` if necessary, and links `packaging/windows/harmony.ico`
into `Harmony.exe`. Explorer, shortcuts and the window use it. After changing
`packaging/icon.svg`, `scripts/make-windows-icon` regenerates the ICO with
ImageMagick; validate its actual multiresolution contents and compiled resource,
not just its filename.
