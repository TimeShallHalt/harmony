# Harmony desktop update and Android handoff

This is the implementation brief prepared before the desktop changes. Use the screenshots for placement and interaction. Keep Harmony's own theme, components, and terminology.

## Screen sharing

- Keep desktop screen thumbnails visible in the share picker. Move Windows, Cameras, Resolution, Frame rate, and Bitrate into secondary menus. Support pointer hover, click, keyboard navigation, and returning to the parent menu. On Android, adapt secondary menus into tappable rows that open a sheet or a second step.
- Offer Source and named resolution presets, FPS presets, and Automatic bitrate. Put a custom field at the bottom of each quality menu. Resolution takes width and height, frame rate takes FPS, and bitrate takes kbps. Preserve each choice when reopening the picker. Validate input before starting and explain source, encoder, or session limits beside the affected setting. Show the resolution and frame rate actually being sent. Include presets above 1080p where the session supports them. Treat rates above 60 as a capture and service capability, with no promise that a Discord session will accept them.
- Add a persisted **Show cursor** option for screen and window sharing. Use the capture backend's cursor control. Android should expose the option only when its capture source provides cursor control.
- Check every source type, a custom value at each supported boundary, an out-of-range value, quality persistence, and visible cursor behavior in a second client. [Discord stream quality](https://support.discord.com/hc/en-us/articles/360040816151-Go-Live-and-Screen-Share)

## Quests and Orbs

- Add **Quests** beside the Friends and DM navigation. Give it a dedicated workspace with a responsive grid, an available view, a claimed view, and access to the existing Orbs balance and shop. Each card shows the quest name, status, reward, progress, and expiry when supplied. Use one column on narrow phones and add columns when space permits.
- Route opt-in, playback, play progress, and claims through Discord's supported flow. Refresh service-reported progress when Harmony regains focus. Never change a reward balance locally. Video picture-in-picture or muted playback can be used when the supported player offers it. Discord video quests currently pause on window switches and resume with saved progress. Game quests require the actual game or other stated task. [Discord Quests FAQ](https://support.discord.com/hc/en-us/articles/22225719947543-Discord-Quests-FAQ)
- Check available, active, completed, expired, and claimed states, plus missing or ineligible data. Verify that a claim changes the balance only after a refreshed service response.

## Calls and screen shares

- Add **Pop out call** to the active call and stream controls on desktop. Keep the existing call, streams, and device state. The separate window stays usable while navigating chat. Closing it returns the call view without hanging up. Leave still ends the call. Prevent duplicate call windows and close the separate window when the session ends.
- Use Android's system picture-in-picture where supported for the active video or share. Preserve the call and its foreground service while the activity changes state. Provide a clear way back to the full call. Test mute, deafen, participant video, switching conversations, closing the secondary view, and remote disconnect.

## Profiles and local time

- Keep all profile actions reachable in both a compact card and a full profile. Use viewport-aware sizing and a scrollable content region when needed. Move time-zone editing to the profile's **More options** menu. Show the saved local time as a compact line only when configured.
- Offer a named time-zone dropdown, city and country lookup, and entry of the person's current time or known offset from the viewer. Persist a named IANA zone when the person explicitly selects one or the location resolves unambiguously. Ask the user to pick when several locations match. A clock reading alone yields a relative offset, which should stay labeled as such. Store preferences locally per profile and keep named zones correct across daylight-saving changes.
- Test a short viewport, a long bio, an ambiguous city, a fractional UTC offset, a date rollover, and clearing a saved value.

## Activities

- Make a message's Activity entry controls actionable and retain the conversation and application context. Route platform-hosted activities such as Wordle to the supported Discord experience. In-app play depends on an authorized embedded Activity host and the application's launch context. An open in-app Activity must block an automatic idle restart. [Discord Activity entry points](https://docs.discord.com/developers/docs/interactions/slash-commands)

## Automatic updates

- Download verified, compatible desktop updates as soon as discovery finds one. Default automatic downloads to on and let users turn them off. Preserve Harmony's signature, platform, build, and hash checks. Apply the staged update on a full quit or restart according to the existing platform installer flow.
- Make **Restart when idle** a separate opt-in. Use whole minutes and interpret `0` as an indefinite wait for a manual restart. Restart only when Harmony has stayed unfocused and untouched for the configured time, the last sent message is at least twice that old, and there is no active call, open activity, or unfinished draft. Include secondary windows in focus and input checks. Recheck immediately before restart.
- On Android, use the authorized store update flow. Respect platform confirmation and installation restrictions. Share the idle-policy logic only where the platform allows an app-initiated restart. Test disabled downloads, a staged update at normal restart, zero minutes, focus and interaction resets, recent sends, calls, activities, and drafts in other conversations.

## Release verification

- Use one clean, identified source revision for Windows and Linux. Build and smoke-test both packages. Verify release checksums, both platform-specific updater manifests, and signatures before publishing the stable desktop release. Record the release version and the behavior actually delivered here before using this brief as the Android implementation checklist.

## Desktop behavior in 1.4.0 build 1

- The share picker keeps screen previews visible, opens secondary menus by hover on wide windows, and uses a second view at narrow widths. The capture worker keeps the selected cursor setting when switching sources. A 1440p or 2160p bounding box is available when the call supports Source. Read the current call's ceiling before setting a custom frame rate or bitrate.
- The Quests workspace has Available, Claimed, and Orbs Shop tabs. Cards show reported progress and use Discord for opt-in, viewing, and claiming. Outside playback follows Discord's window-focus rules. Game rewards use Discord's real play flow. The desktop release has no auto-completion queue.
- The call window can be opened from the call stage, voice dock, or floating stream. It shares the active call's state and has its own share picker. Closing the call window leaves the session connected. The separate window closes when the call ends.
- Compact and full profiles expose **More actions → Local time**. City lookup uses the bundled IANA representatives and ISO country names. If a city isn't listed, use a named time zone or the observed-clock mode. A clock reading uses the nearest local day and rounds to a civil quarter-hour offset. An explicit signed difference preserves minutes. Fixed offsets remain fixed across daylight-saving transitions.
- Recognized hosted Activity buttons open the original message in Discord's browser client, with the channel and message intact. Discord hosts the actual game.
- Automatic signed downloads and release checks default to on when the user has no saved preference. A verified package is ready for a full quit. Idle restart is off by default and supports a writable Linux AppImage or a registered Windows per-user installation. It waits for zero active calls, unfinished drafts, and open in-app work, then relaunches the same data profile after applying the update.
