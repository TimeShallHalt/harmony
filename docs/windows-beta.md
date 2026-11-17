# Windows

Harmony targets Intel/AMD PCs running 64-bit Windows 10 or 11. The primary
fork package is a build-stamped, Discord-only portable ZIP, with an optional
offline [MSI builder](../packaging/windows-msi.md). The inherited Linux
cross-build also supports a Discord-only flavour and a full preview with Matrix,
whose homeserver defaults to editable `https://matrix.org`. These are build
options, not claims of published or validated fork releases.

Harmony derives from Yapfast 0.4.1 by wiredtenshi and contributors and retains
its original GPL license, dependency notices and technical implementation notes.
Wine observations below are inherited upstream evidence, not a new Windows or
Linux acceptance result for this integration. The stock updater is disconnected;
no fork package checks for or applies stock releases.

## Run the package

The primary producer, `scripts/package-harmony.py`, writes
`Harmony-VERSION-bBUILD_ID-windows-x64.zip`, its matching source ZIP, checksums
and producer manifest to the explicitly chosen empty output folder. See
[packaging](packaging.md#windows-portable-package) for exact inputs and clean-build
requirements. Do not confuse its build-stamped outputs with the optional
cross-build artifacts below.

1. Copy the verified portable ZIP and matching source archive to the tester's PC.
2. Extract the whole portable ZIP, then run `Harmony.exe` in the extracted folder.
3. Link Discord by QR code. Third-party personal-account clients carry account
   risk. For a separately built full preview, sign in to Matrix; voice needs a
   homeserver offering MatrixRTC (see [voice](voice.md)).
4. Open a channel for text or a voice channel for voice.

No Rust, Visual Studio or separate runtime installation is needed to run the
package. It includes native media code and Skia. An Explorer launch shows only
the window and logs to `yapfast.log` under the legacy data folder
`%LOCALAPPDATA%\Yapfast\Yapfast\data`, or the explicit `--data-dir` folder.
The legacy path and Credential Manager service IDs are preserved so existing
accounts remain usable; do not rename them as branding cleanup. The log grows
across starts and resets above 4 MiB. From a console, diagnostics go there instead:

```powershell
.\Harmony.exe --renderer software
.\Harmony.exe --renderer gpu
```

The optional cross-build ZIP includes `run-harmony.cmd` to keep errors visible;
the primary portable ZIP does not include that launcher. See
[Rendering on Windows](#rendering-on-windows).

Update manually: use the tray's **Quit**, not just the close button, before
replacing files. Packages do not launch or force-close the app, enable startup,
claim protocols or change existing shortcuts. Retarget an old `yapfast.exe`
shortcut manually, preserving `--data-dir`. Optional MSI installation is per-user
and defaults to `%LOCALAPPDATA%\Programs\Harmony`; it leaves data and unowned
files alone. The optional Inno recipe, `packaging/windows/harmony.iss`, can add
new Start-menu/optional desktop shortcuts but has no startup or app-launch task.
These installers use distinct fork identities, not upstream Yapfast registration.
No signing result is claimed here; do not disable Windows security controls to
run an unverified package.

Passwords, recovery keys and saved sessions are excluded from the package.
Session secrets use Windows Credential Manager. Encrypted SDK stores live under
the user's application-data directory. Keep the first verified session while
device verification and recovery are being tested.

## Rendering on Windows

Harmony renders with Skia Software unless "Render with the GPU" in Settings →
Advanced is on. On Windows that setting asks Slint for Skia with Direct3D
required, which draws through wgpu's Direct3D 12; Linux uses OpenGL. NVIDIA's
OpenGL driver on Windows kept about one core busy while it waited for the
screen, which is why Windows doesn't. If the GPU can't start, Harmony restarts
itself with Skia Software, says so in the window and turns the setting off
(`src/graphics.rs`, `src/ui.rs`).

## First two-machine check

With Alice signed in on Linux and Bob on Windows:

1. Exchange messages in general and confirm they survive restarting Harmony.
2. Alice joins Lounge, stays muted and waits. Bob joins later and should see Alice.
3. Choose microphone and speakers in the voice settings. Use headphones.
4. Unmute both people and confirm each can hear the other.
5. Check mute, deafen, leaving, and rejoining. Browse general while voice stays connected.
6. Choose a camera in the voice settings, try "Test video", then turn the camera
   on in Lounge. Each should see the other's camera. Restart Harmony and check the
   camera is still chosen.
7. Record Windows version, GPU/driver, audio devices, cameras, input/copy/paste behavior,
   window resizing, and any error text. Never include passwords or recovery keys.

## Discord voice on Windows

Discord voice, the input meter and the Discord input sensitivity meter,
ringing, Harmony's sounds, and a watched stream's or a video's sound use WASAPI
on Windows, in `crates/yapfast-core/src/audio_io/wasapi.rs`. Each stream is
shared-mode and event-driven: the audio engine converts between Harmony's 48 kHz
and the device's format, and reading or writing waits for its event on the
thread that already did the reading or writing on Linux, copying straight
between Harmony's buffers and the engine's. WebRTC's Windows device module names
each device by its endpoint id (`{0.0.0.00000000}.{…}` for speakers,
`{0.0.1.00000000}.{…}` for microphones), so the chosen device opens directly
by that id; none is the console default. Go Live's sound is in
[Go Live on Windows](#go-live-on-windows).

Microphones open in raw mode (`AUDCLNT_STREAMOPTIONS_RAW`, through
`IAudioClient2::SetClientProperties`), which skips the effects Windows and the
sound driver run on them: what Sound settings calls "device default effects"
or "audio enhancements", often the driver's own noise suppression or
beamforming. Under Harmony's suppressor they muffled a voice that Discord's
client left clear. A device without raw mode opens with its effects. Settings ›
Voice's "Let Windows process your microphone in Discord calls"
(`windows_microphone_effects`, off unless turned on, shown on Windows only)
keeps them, for a laptop microphone that needs its driver's beamforming; a
call's microphone opens again within 2 s of the switch. Matrix calls record
through LiveKit's WebRTC, which opens the microphone itself, with the effects.

A Discord call's threads join MMCSS tasks, the microphone's "Audio" and the
call's and speakers' "Pro Audio", and while the call goes on the process opts
out of power throttling and asks for a 1 ms timer; see
[Voice](discord/voice.md#playing-what-others-say). Whether that keeps the
microphone whole on a weak processor sharing its screen through x264, with
Harmony's window minimized, needs checking on real Windows.

Wine's capture starts 0.3–1 s after it's opened, so the first moments a
microphone hears are lost there. Real Windows needs checking for that, along
with Bluetooth headsets, a device unplugged during a call, a default device
changed during a call, exclusive-mode applications, and whether the
communications ducking setting turns other sound down during a call. Wine has
no microphone effects, so raw mode, and a device without it, need checking on
Windows too.

## Go Live on Windows

Sharing, watching and playing a video run in the stream worker, as on Linux:
Harmony starts its own executable with `--stream-worker`, which writes its logs
where Harmony does: Harmony's console, or Harmony's log file. The video frames reach
the window through a temporary file both processes map. Windows won't delete
a mapped file, so there Harmony creates it with `FILE_FLAG_DELETE_ON_CLOSE` and
`FILE_ATTRIBUTE_TEMPORARY`, which keeps it in memory while it fits, and
Windows removes it once both processes close it. On Linux it's still unlinked once the worker has it open.

Windows has no screen sharing dialog for Harmony to use, so the share menu
lists what there is to share above the resolution: the screens as pictures, two
to a row, named "Entire screen" (or "Screen 1", "Screen 2", … with several),
then each window with a title. Harmony's own windows aren't listed. libwebrtc's
capturers make the list, on a short-lived thread as the menu opens, and the
worker finds the picked one by its id. The menu opens with the screens' names;
their pictures come from a worker job (`Job::Glance`) that takes one frame of
each screen, scaled down to its card, and exits, so capturing them takes memory
only until they're in. A window closed after it was listed ends sharing with
"That screen or window is gone." Without a pick, Harmony shares the first
screen.

Go Live's sound comes from WASAPI process loopback: one stream of everything
Windows plays except Harmony's own process tree, the Harmony window and its
workers. It needs Windows 10's 2004 update or later. Unlike Linux, where
each application is heard on its own and Discord's own client is left out,
Windows mixes every other application into one stream and can leave out only
Harmony. Where Windows refuses process loopback, Harmony shares without sound and
logs why, as it does on Linux without a PulseAudio server.

The stream's H264 goes to NVIDIA's NVENC, then AMD's AMF, through FFmpeg in
`crates/yapfast-core/native/hwenc.c`, while "Encode streams with the GPU" is
on, and to x264 when neither opens or the setting is off. Intel graphics
encode with x264 on Windows. Where the card encodes AV1 or H265, the stream
offers those first, in the same way. The camera always uses x264. AMF has
run on AMD hardware only in a test of `native/hwenc.c` on its own, on an
RX 7900 XTX. [Encoding on the GPU](discord/go-live.md#encoding-on-the-gpu) has
the rest.

Real Windows needs checking for: the window list's contents and titles,
sharing a window that's minimized, moved to another screen or closed while
shared, screens on mixed-DPI setups, Windows Graphics Capture's yellow
border, process loopback hearing other applications and leaving out Harmony's
own voices, AMF on an AMD card in a whole Harmony (AV1 on an RX 7000 or
newer), and the worker's start-up time from a
packaged build.

## Cameras on Windows

Cameras film through libwebrtc's DirectShow capture; see
[Cameras](voice.md#cameras) for how, and how a camera's id is kept.

Only real Windows can check a real camera through DirectShow's kernel
streaming (MJPEG, YUY2 and NV12, and the capability chosen for 1280×720 at 30
frames a second), that a camera's id survives restarts and replugging, COM on
the threads that list and open cameras, a camera another application is
using, the camera privacy setting turned off, built-in laptop cameras that
only Media Foundation lists, and the memory and processor time a camera
takes.

## The tray on Windows

The notification-area icon is `Shell_NotifyIconW` on a hidden window of the
UI thread; see [the tray](tray.md#windows).

Under Wine, closing the window to the tray and opening it again from the
icon work. A right click reaches Harmony (`WM_RBUTTONDOWN`, `WM_RBUTTONUP`) and
`TrackPopupMenu` runs, but Wine's Wayland driver never shows the menu,
most likely because its owner is a window that was never shown. Until the
menu closes, its loop holds Harmony's UI thread, as every Win32 menu does; on
Windows a click anywhere else closes it. So Open Harmony, Disconnect and Quit
in the menu are untested on Windows.

[The tray](tray.md#windows) lists what to check on real Windows.

## Notifications on Windows

Desktop notifications are WinRT toasts, shown from a thread of their own
(`src/notify.rs`). Windows shows a toast only from an app ID it knows. Setting
one on the process would also have to go on the Start menu shortcut, or the
taskbar would group the window apart from it, and a portable copy has no
shortcut. So each start registers `io.github.TimeShallHalt.Harmony` under
`HKCU\Software\Classes\AppUserModelId` instead, with Harmony's name and the
256-pixel picture from its icon, written to `notifications\icon.png` in the
data folder. Fork installers do not clean runtime registrations on uninstall;
these are separate from their enumerated program-file ownership.
Each toast's author picture goes in
`notifications\0.png` to `19.png`, in turn. Toasts are silent, since Harmony
plays its own sounds.

Clicking a toast while Harmony runs opens its conversation, through the toast's
`Activated` event, for the last 20 shown: as many as the notification center
keeps of an app's. Harmony registers no COM activator, so a toast clicked after
it quit doesn't start it.

Wine has no toast service: Harmony logs `starting toasts: 0x80040154` (class
not registered) as it starts and goes on without notifications. The app ID's
registry key and `icon.png` are still written, so that much can be checked
there.

Real Windows needs checking for: a toast's name, icon and picture; a click
bringing the window forward from behind another and from the tray; Focus
assist and Do Not Disturb; and Harmony in Settings > System > Notifications.

## Dragging the window

From the press on the title bar or a border until the release, Windows moves or
sizes a window in a message loop of its own. winit still passes window messages
on, but its own loop doesn't run, and Slint advances its timers and animation
clock only when it does, so without help the window looks frozen while dragged:
animations stand still, and pictures that arrive wait. `src/ui/move_loop.rs`
starts a Win32 timer with the first move or resize winit reports while Windows
is moving or sizing the window (`GetGUIThreadInfo`, `GUI_INMOVESIZE`). Windows
calls it from its own loop every 16 ms, and it advances Slint's timers and
animations and redraws what animates, until the window stops moving.
`YAPFAST_LOG=debug` logs when it starts and stops.

Wine doesn't reproduce the freeze. So check on real Windows: while the window is
held by its title bar, still and moving, "… is typing" should keep pulsing
and its dots rising; the same while resizing by a border.

## The window's size and place

Harmony starts where it was left; see [Layout and
look](chat-ui.md#layout-and-look). Slint makes the native window only once its
event loop runs, and shows it straight away, so there's no window to give
`SetWindowPlacement` before it shows. Harmony gives Slint the size and place
before `show()` instead, and keeps the un-maximized ones itself while the
window is maximized. `MonitorFromRect` and `GetMonitorInfoW`
(`src/ui/placement.rs`) check them against the monitors: if the title bar would
show on no monitor, Harmony forgets the place and Windows puts the window on
the primary monitor, and a window too big for its monitor's work area shrinks
to it.

Check on real Windows: two monitors at different scales, starting on the
one that isn't the primary, maximized and not; a monitor unplugged while
Harmony is closed; a window snapped to half the screen, which comes back at
the snapped size; and the taskbar at the top or the left.

## Rebuild from Linux

On a Linux host with the matching Nix environment, from the project folder
(the merged fork's full Linux/cross-build route remains to be validated):

```sh
nix develop path:./nix#windows --command scripts/build-windows        # Discord-only, optional offline Inno
nix develop path:./nix#windows --command scripts/build-windows full --no-installer  # full preview ZIP
```

Both flavours build to the same `Harmony.exe`, so each run packages straight
away; the packaging script tells them apart by the Matrix SDK's source paths
and refuses the wrong one. This installs the pinned Rust toolchain's Windows
target, cross-compiles with cargo-xwin and Clang, then packages the executable.
The first run downloads pinned build dependencies, including the Microsoft
CRT/SDK, and compiles C++ media bindings. Optional Inno compilation is offline:
provide an existing 7.1.0 compiler in the default local prefix or set
`HARMONY_INNO_PREFIX`. No Inno compiler is downloaded or installed automatically;
pass `discord --no-installer` for a Discord ZIP without Inno.

The Windows Nix shell supplies the matching `webrtc-51ef663` archive, LLVM 21,
GN, Protobuf, NASM, Wine and checksum-pinned Skia 0.157.0 source with its
dependencies. `scripts/build-windows` builds Skia with `/MT` to match WebRTC;
the upstream Skia Windows archive uses an incompatible dynamic CRT. This source
build uses two native jobs and is cached. Preparation then gives Skia's JPEG
table a private symbol name. It checks runtime directives and symbols, and
preserves every native section's code/data and relocations through the rename.
The executable links the C runtime statically, so it needs no separate Visual
C++ runtime DLL.

`scripts/build-windows` sources `scripts/release-env x86_64-pc-windows-msvc`
before `cargo build --release`: it takes `cargo xwin env`'s environment and
remaps source paths, so a binary people download names no folder of this
machine ([Building](building.md#windows)). Source it the same way before any
Windows binary you hand to someone.

A [one-line WebRTC patch](../vendor/webrtc-sys/README.yapfast.md) also fixes an
MSVC class/struct declaration mismatch, matching the upstream resolution.

The cross-build's optional packaging route writes
`target/dist/discord/Harmony-VERSION-windows-x64.zip` and
`Harmony-VERSION-source.zip`; full outputs in `target/dist/full` carry the
`-full-preview` suffix. With offline Inno available it also compiles
`Harmony-VERSION[-full-preview]-windows-x64-setup.exe` from
`packaging/windows/harmony.iss`. Output folders must be empty.

The package contains dependency notices, original licenses, `THIRD-PARTY.json`,
`SOURCE.txt` and `BUILD.json` with source/executable hashes. The matching source
ZIP accompanies the package and is also included inside its
`corresponding-source/` folder. `SOURCE.txt` names this local source, **not an
upstream release tag**. Keep it available with the binaries. Full/Matrix source
also includes the exact upstream AGPL MatrixRTC sources; the default Discord
build does not use them. Shared source helpers copy only tracked, allowlisted
repository paths, excluding ignored `.local` material, credentials, sessions,
logs and Wine prefixes. Do not track private files in release source paths.
`SHA256SUMS.txt` covers the artifacts; a checksum is not a signature.

## Build for development

For work on Windows code, build with the `fast` profile in the Windows shell,
with cargo-xwin's environment. The app crate needs the prepared Skia archive,
as `scripts/build-windows` makes it, and thorvg-sys's bindgen the target that
`scripts/release-env` adds, which `cargo xwin build` would overwrite:

```sh
nix develop path:./nix#windows --command bash -c '
  export SKIA_BINARIES_URL="file://$(python3 nix/prepare-windows-skia.py)"
  eval "$(cargo xwin env --target x86_64-pc-windows-msvc)"
  export BINDGEN_EXTRA_CLANG_ARGS_x86_64_pc_windows_msvc="$BINDGEN_EXTRA_CLANG_ARGS_x86_64_pc_windows_msvc --target=x86_64-pc-windows-msvc"
  cargo build --profile fast --target x86_64-pc-windows-msvc \
    --no-default-features --features desktop,discord'
```

For the full build, replace the two feature flags with `--features matrix`;
simply dropping them now selects the Discord-only defaults. The executable is
`target/x86_64-pc-windows-msvc/fast/Harmony.exe`. `Harmony.exe --help` and
`--smoke-test` run under Wine without a display; `--audio-devices` lists the
microphones, speakers and cameras Wine sees.

## Test under Wine

Build the test executable without running it, then run it with Wine.
`yapfast-core` needs no Skia:

```sh
nix develop path:./nix#windows --command \
  cargo xwin test --no-run --profile fast --target x86_64-pc-windows-msvc \
  -p yapfast-core --lib
```

Cargo prints the path,
`target/x86_64-pc-windows-msvc/fast/deps/yapfast_core-HASH.exe`. Run it in its
own prefix, with the usual test filter and flags:

```sh
nix develop path:./nix#windows --command env WINEPREFIX=/tmp/wine-test \
  WINEDEBUG=-all DISPLAY= WAYLAND_DISPLAY= wine \
  target/x86_64-pc-windows-msvc/fast/deps/yapfast_core-HASH.exe frames
```

A new prefix takes `wineboot -i` first. The device tests are `#[ignore]`d;
run each by name with `--ignored`, and give it what its ignore reason asks
for:

- Sound (`audio_io` and `discord_audio` tests): `PULSE_SERVER` naming a private
  PulseAudio of null sinks, whose default input hears its default output, so
  nothing reaches real speakers. `a_call_sends_what_the_microphone_hears` also
  needs `bun testing/fake_discord.ts` and `YAPFAST_DISCORD_TEST_SERVER`.
- Screens and windows
  (`stream_worker::tests::shares_a_listed_screen_and_window`): an X display,
  such as `Xvfb`, with `wine notepad` open on it. Wine's GDI window capture
  gives black windows without a compositor; Windows uses Windows Graphics
  Capture.
- Cameras (`camera::tests::films_a_real_camera`, with `YAPFAST_TEST_CAMERA`
  naming it): Wine's DirectShow, built without libv4l2, reads only a V4L2
  device that supports `read()` and BGR24, such as a v4l2loopback device fed
  by `ffmpeg -re -f lavfi -i testsrc2=size=1280x720:rate=30 -pix_fmt bgr24 -f
  v4l2 /dev/videoN`. It lists a USB webcam too, which then fails to open.

`audio_io::tests::hears_other_applications` can only show the fallback under
Wine: Wine has no `VAD\Process_Loopback` device, so the capture fails with
"File not found" and sharing goes on without sound. AMF can't open either,
since Wine has no `amfrt64.dll`, so streams encode with x264 there.

## Run the app under Wine

`scripts/ui-session` runs a Windows build through `--bin`, a script that
starts Wine and waits on `wineserver -w`. Start the session from the Windows
shell, which has Wine, with `TMPDIR=/tmp`: the shell's own `TMPDIR` makes the
Wayland socket's path longer than its 108-byte limit.

```sh
nix develop path:./nix#windows --command env TMPDIR=/tmp \
  scripts/ui-session start --discord --desktop --bin /tmp/wine-harmony.sh
```

There are two ways to show the window, and each misses something:

- Wine's Wayland driver, on the session's Weston: the script unsets `DISPLAY`
  and runs plain `wine Harmony.exe "$@"`. This is enough for most UI checks.
  `wine explorer /desktop=…` fails here with "xdg_surface geometry … is
  larger than the configured fullscreen state", since the desktop's frame
  doesn't fit the kiosk window. Screen capture gets no frames, the preview of
  a share is black, Weston's kiosk shell ignores moves and resizes, and the
  tray's menu never shows. Start the session with `--desktop`: Harmony's own
  title bar leaves the window frameless, so the kiosk shell sizes it to the
  whole display. Wine then confines the pointer to it, as to any fullscreen
  window, and moving that lock trips an assertion in Weston 14
  (`weston_coord_surface_to_global`). Weston quits, and Wine's Wayland driver
  ends every process with exit code 1 ("Failed to read events from the
  compositor"), a few seconds after Harmony starts.
- Wine's X11 driver, on a rootful Xwayland inside the session: the script
  starts `Xwayland :58 -geometry 1120x760` (the session's size; not
  `-fullscreen`, which gives 640×480 stretched), then, with `DISPLAY=:58`,
  `WAYLAND_DISPLAY` unset and `SLINT_SCALE_FACTOR=1`, runs `wine explorer
  /desktop=harmony,1120x760 Harmony.exe "$@"`. GDI then captures the real
  screen, and the window can be moved, resized and maximized. Stop the extra
  Xwayland by its PID after `scripts/ui-session stop`.

Wine on a bare X server such as Xvfb can't show the window: Xvfb reports its
monitor at 0 Hz, and Slint's winit backend divides by the refresh rate
(`frame_throttle.rs`), so it panics before the window opens. A Windows
display that reports no refresh rate would hit the same.

`--renderer gpu` draws Direct3D 12 through vkd3d on Mesa's software Vulkan,
which shows the path works but nothing about a real card. To check the
fallback to Skia Software, hide Vulkan with `VK_DRIVER_FILES=/nonexistent.json
VK_ICD_FILENAMES=/nonexistent.json`. Don't use `WINEDLLOVERRIDES=d3d12=d`:
`Harmony.exe` imports `d3d12.dll`, so it then won't start at all.

## What's open

The Wine observations in this guide come from upstream Yapfast's development
notes, not a test receipt for the merged Harmony candidate. Each section lists
checks still needed on real Windows; full fork Linux/cross-build and installer
validation must be reported separately. Beyond those: text entry,
selection, copy/paste, keyboard focus, resizing, both renderers, restarts,
network interruptions and crashes on a tester's PC; whether Direct3D 12 keeps
an NVIDIA card's CPU use low; a display that reports no refresh rate, under
remote desktop or in a virtual machine; and Intel GPU encoding. The packages
must be treated as unsigned unless an actual signature is separately verified.
