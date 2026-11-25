# Harmony Android 1.2.0

Harmony Android is a native Rust and Slint client with its own Gradle project and Android application ID, `io.github.timeshallhalt.harmony.android`. It runs on Android 9 (API 28) and later. The APK targets Android 15 (API 35) and includes ARM64 and x86-64. The default 1.2.0 desktop configuration ships Discord. That is the account service in this Android checkpoint.

The independent repository began at commit `8b766906`. It contains the working sources of Harmony 1.2.0 from desktop HEAD `e4968032`, including its local source changes. The original checkout, its Git directory, the desktop installer project and the installed application are independent of this build.

## Link Discord on this phone

In Harmony, choose **Link Discord**, then **Open in Discord**. Harmony requests a fresh code and opens its link in the official Discord app. Review the login there, then return to Harmony. To use another device, tap **Show QR code** and scan it with Discord on that device. You can also tap **Open in Discord** on the code screen. If Discord is unavailable on this phone, the same code stays on screen. If it expires, either button starts a fresh attempt. [Phone linking screen](testing/emulator-link-discord.png) · [Discord’s QR login guide](https://support.discord.com/hc/en-us/articles/360039213771-QR-Code-Login-FAQ).

## Build a debug APK

Use the Rust toolchain in the root `rust-toolchain.toml`, JDK 17, Python 3, Git, CMake, Ninja, Android SDK Platform 35, Build Tools 35.0.0 and Android NDK r30 (`30.0.16248370`). Set `JAVA_HOME`, `ANDROID_HOME` and `ANDROID_NDK_HOME`. Put Cargo, Python, CMake and Ninja on the build shell’s PATH. Gradle and Cargo fetch the wrapper, locked dependencies and matching Skia binary on a fresh machine. On Windows a short Cargo home path helps CMake stay within host path limits.

Run these commands from the root of the Android copy. On Windows use `android\gradlew.bat` in place of `./android/gradlew`.

```sh
rustup target add aarch64-linux-android x86_64-linux-android
./android/gradlew -p android :app:assembleLiveDebug
python3 android/verify-apk.py android/app/build/outputs/apk/live/debug/app-live-debug.apk
```

The installable APK is `android/app/build/outputs/apk/live/debug/app-live-debug.apk`. Gradle uses a local debug key. The Android shell and Discord networking live in `android/native`. The Android UI uses Harmony’s logo, the shared Discord linking panel, a touch-specific Slint shell and the system Roboto font. Shared Rust modules handle linking, gateway events, REST, Markdown links, voice, Opus and Go Live. Android APIs provide the keyboard, back navigation, document picker, Keystore, notification channel, media devices, player and projection consent.

## Build a release candidate

```sh
./android/gradlew -p android :app:assembleLiveRelease
python3 android/verify-apk.py android/app/build/outputs/apk/live/release/app-live-release-unsigned.apk
```

The release task generates an unsigned universal APK. The manual **Android APK** workflow in `.github/workflows/android.yml` builds either a debug APK or an unsigned release candidate and stores the chosen artifact on the workflow run. Distribution signing uses a separately supplied Android release identity. There is no store upload or GitHub Release step. This checkpoint exercised the debug build and resolved the release task graph locally. A full release build and distribution signing still need validation.

## Run the isolated conversation test

The separate `fixture` package adds `.fixture` to the application ID. It has its own Android storage and connects to the repository’s fake Discord at loopback port 18473. It uses the same Slint controls, gateway, account linking and message transport as the installable debug APK. **Explore local preview** is an offline sample inside either package.

1. Start a disposable x86-64 API 35 Android emulator. The automated keyboard check uses a 720 × 1560 portrait display at 280 dpi and the system Latin keyboard. Substitute its actual `emulator-…` serial below.
2. In one terminal start the fixture. Keep it running while using the fixture APK.
3. In a second terminal build, install and open the fixture APK on that emulator.

```sh
bun testing/fake_discord.ts --host 127.0.0.1 --port 18473 --udp-host 10.0.2.2 --extra 0
```

```sh
./android/gradlew -p android :app:assembleFixtureDebug -Pabis=x86_64
adb -s "$SERIAL" reverse tcp:18473 tcp:18473
adb -s "$SERIAL" install -r android/app/build/outputs/apk/fixture/debug/app-fixture-debug.apk
adb -s "$SERIAL" shell am start -n io.github.timeshallhalt.harmony.android.fixture/io.github.timeshallhalt.harmony.android.HarmonyActivity
```

In **Harmony Fixture**, choose **Link Discord** and request a QR code. The fixture impersonates the second device in the scan and approval steps. The local fixture’s documented test token also works in the manual-link field. Use synthetic credentials only in this package.

```sh
curl -X POST http://127.0.0.1:18473/_control/qr-scan
curl -X POST http://127.0.0.1:18473/_control/qr-confirm
python3 android/testing/smoke.py --serial "$SERIAL"
python3 android/testing/smoke.py --serial "$SERIAL" --restart-draft
```

The smoke check opens Bob’s DM, types a unique message through the Android keyboard, presses Send while the final word is still composing and verifies the exact text in the fixture. The `--restart-draft` check confirms encrypted storage, force-stops the fixture with the composer open and sends the restored message. Both checks accept an emulator serial and an optional `--adb-port` for a private ADB server.

## Android mobile redesign

Android build 10201 uses Discord Android 354.12 Stable as its visual reference. The signed package supplied the avatar sizes, message-column inset, attachment spacing and corner treatments. Discord’s published mobile examples supplied the server rail, gestures and floating profile bar. The APK uses Harmony’s own UI, assets and Rust Discord service. [Reference build and evidence](testing/discord-reference.md).

On the Messages screen, tap a server to browse its channel categories. Tap the profile bar to open your profile, hold the bar to change status, hold the avatar to open Settings, or swipe the bar to switch between DMs and the last server. In a conversation, tap its name for details, swipe a message left to reply, hold for message actions, and double-tap to react. The composer accepts the Android keyboard, attachments, voice messages, emoji, GIFs, stickers and polls. It saves unfinished text and reply targets per account and room in the Android Keystore-backed vault. Tap an individual covered phrase or attachment to reveal it.

Choose **Explore local preview** to try the interface without an account. Its four servers, seven direct/group conversations and media are local sample data. The live APK uses the Discord service when linked. The separately named fixture APK connects to the local synthetic Discord server described above.

## Emulator results

### Same-phone QR linking in build 10202

The previous build reported “Can’t reach Discord’s login service” when the Android certificate verifier could not fetch the issuer’s revocation list. Build 10202 uses rustls-platform-verifier 0.7.1 and its matching Android library. The live app keeps certificate validation enabled and allows the issuer’s revocation check. [Verifier release notes](https://github.com/rustls/rustls-platform-verifier/releases/tag/v%2F0.7.1).

The live debug APK received fresh QR codes from Discord on Android 11 and 15. On Android 15 without Discord installed, tapping the button kept the same scannable code on screen. On Android 11, tapping the button opened the signed Discord app. Its ARM library then failed to load under x86-64 translation, so its approval screen still needs a test on ARM64. The separate local test account covered expiry, requesting a fresh code, cancellation, approval while Harmony was in the background, and returning to the linked inbox.

To repeat the live QR check, open the signed-out live APK on an emulator and choose **Link Discord** and **Show QR code**. With Python’s `opencv-python` and `numpy` packages installed, run `python3 android/testing/qr_code.py --serial "$SERIAL"`. The check validates the displayed link in memory and cancels the attempt before approval.

The universal debug APK for build 10202 is 166,185,955 bytes. Its SHA-256 is `1c6c7cd90185ed50b0bb10e9233b360f7c6d3b6f8f06a383aa5546db6f5baa24`. Android Build Tools verified its v2 debug signature and 16 KiB ZIP alignment. The repository’s verifier checked both ABIs for 16 KiB ELF alignment.

### Mobile redesign in build 10201

The redesigned app was exercised on an Android 15 / API 35 x86-64 emulator. The universal live debug APK contains both ARM64 and x86-64 libraries. Android Build Tools verified the APK’s v2 debug signature and 16 KiB ZIP and ELF alignment. The 165,196,886-byte build 10201 had SHA-256 `9e1d912268d02aadcb5b163692632e4ff984c95d9c692f64b8232fd295ff21a7`. ARM64 execution still needs a compatible runtime. [Side-by-side screenshots](testing/mobile-overview.webp) · [Messages](testing/emulator-messages.png) · [Server conversation](testing/emulator-chat.png).

| Redesign coverage | What was verified |
| --- | --- |
| Navigation and appearance | Opened DMs, a server and a channel. Used the profile bar’s status sheet and avatar shortcut. Changed between Light and Dark and checked the appearance value in app-private storage. Android Back returned from chat to its browser. |
| Composer and gestures | Sent with the real on-screen keyboard. The keyboard test delivered exact mixed-case, multiword text 12 times consecutively and after three additional cold starts. Each appeared once on the server. Held a message to open its action sheet, edited its original ID, and swiped another message to send a reply with its original channel and message ID. |
| Drafts and spoilers | Force-stopped during Gboard’s underlined final word, opened another channel, then returned to the original DM and sent the exact restored draft once. Sending cleared its encrypted entry. A second restored draft kept its original reply target. Tapped two independently covered phrases in succession and revealed a covered photo in the chat. |
| Search, pins and members | Searched Bob’s DM for “snacks”, jumped to the exact history result, opened a pinned message list, viewed the server’s reader roster, opened Bob’s profile and used its Message action. Tapped a reaction and checked the confirmed emoji/count on the fixture. |
| Expressions and polls | Sent a trending GIF and the server’s Wave sticker from their respective pickers. Verified the posted URL and sticker ID in channel history. Voted for Ramen in a received poll and saw the fixture record the answer and the result change to 75%. |
| Attachments | Selected an 8,417-byte animated GIF through the Android media picker, verified the uploaded filename, MIME type and size in the DM, and opened it in the native viewer. Six captures of the visible playback region contained four distinct animation frames. Double-tapping the station artwork zoomed it in the Android viewer. The system Back button returned to the same conversation. |
| Groups, servers and gateway | Created a named group from Bob and Carol, sent its first message, created a server, and opened its channel browser. Opened an archived thread and used Android Back to return to its archived thread list. The separate loopback contract test covers invite review/accept, invalid invites, channel-scoped read receipts, reconnect, archived-thread pagination and hidden thread access. Run `bun android/testing/conversation_fixture.ts` from the repository root. |
| Voice call | Joined Bob’s fixture DM call, enabled the microphone and disconnected. The local voice gateway received 423 packets, including 272 with voice, with zero decryption failures. |
| Reference coverage | The downloaded Discord release supplies ARM native libraries. The test emulator supplies x86-64. The reference report identifies the signed resources and documented behavior used for each comparison. |

### Original port checkpoint

The preceding Android checkpoint at `05c04d8` established the following account and platform behavior on the same API level.

| Area | Observed result |
| --- | --- |
| Shipped debug APK | Installed and cold-launched. Opened the offline preview, entered a DM and sent a complete multiword message with the system keyboard. Verified the APK v2 debug signature, ZIP alignment and both native libraries for each ABI. |
| Account | Completed synthetic QR and manual-token linking, restored the account after a process restart, changed presence to Idle, saved a profile bio and verified it on the server. Confirmed the session entry was encrypted at rest. Signed out and cold-started back at the welcome screen. |
| Conversations | Opened DMs and server channels, read history, accepted an incoming friend request and a message request, and sent to Bob and to the server’s general channel. Rotating to landscape and back preserved the open DM. |
| Message actions | Verified the full send and edited text server-side, including the final IME word. The reply contained the selected message ID. Adding a reaction updated the message from the gateway. Tapping it again removed it. |
| Files and playback | Selected a 39-byte document in Android Downloads, uploaded it without a caption and compared the retrieved bytes to the original. Opened a PNG and played a six-second MP4. Recorded, reviewed and delivered a 2.08-second Ogg/Opus voice message with waveform metadata. |
| Notifications | Granted the runtime permission, sent the app to the background and generated an incoming DM. Android displayed Dave’s notification. Tapping it opened Dave’s conversation. |
| Voice | Joined a fixture DM call muted, enabled the microphone, and left. The fixture counted 112 outgoing voice packets with zero decryption failures in the sampled interval. Audio capture and output used emulator devices. Acoustic quality and a conversation between independent peers still require hardware testing. |
| Screen sharing | Accepted Android’s MediaProjection prompt. Tested the entire emulator display and a single external emulator app. The fixture assembled 576 × 1280 H.264 frames and keyframes with zero decryption failures. Stopping the share and leaving the call returned to chat. |

## Remaining parity work

| Area | Remaining implementation or verification |
| --- | --- |
| Discord account and server features | Live login, MFA/challenges, membership screening, server-specific profile editing, moderation, roles, server settings, and entitlement-dependent features still need work or live validation. The local test account establishes the tested HTTP and gateway behavior. |
| Messaging | Search/pins, polls, emoji, GIFs, stickers, embeds, members and thread browsing are implemented. Scheduled messages, interactive bot components, full forum authoring, favorites management, app commands, monetized gifts and Super Reactions remain partial or absent. Lottie stickers can be sent and still need a renderer. |
| Media and history | Inline previews show stills. Supported animated images play in the Android viewer. Inbound Android share intents, resumable uploads after process death, large-history virtualization and recovery of staged files or recordings after process death remain open. The mobile file staging/download limit is 32 MiB. |
| Calls and notifications | Voice messages, foreground voice and outbound H.264 screen sharing are implemented. Camera video, inbound stream viewing, system-audio capture, incoming-call UI and a background call service need platform work. Delivery after process eviction or prolonged Doze requires a push transport. |
| Device and release coverage | Physical ARM64, additional keyboards, real two-device calls, release signing and store distribution remain unverified. The emulator’s accessibility hierarchy exposed the native surface without the individual Slint controls. TalkBack navigation needs platform work. Theme-pack import, optional Matrix support and desktop Rich Presence are outside the implemented Android feature set. |
