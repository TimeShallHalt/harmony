# Harmony feature reconnaissance

## Scope

Ren's personal native Rust/Slint Discord client on Windows and Linux. Source is the existing Yapfast fork, not a new web client. Ren explicitly chose extensible native theme packs instead of actual browser CSS, to minimize CPU and memory. The current scope is ALL requested features, original-resolution image viewing, automatic updates and Settings app/build information. Work may be integrated in bounded verified slices, but a partial release or roadmap must never be represented as completion of the full request. See `architecture.md` for native ownership and acceptance boundaries.

## Reference sources

- Seven user-supplied Discord screenshots: conversation/profile sidebar; own profile Board; own profile Activity; emoji picker; composer attachment menu; full profile Activity; media overlay.
- Existing open-source implementation and docs in `docs/discord/`, `ui/`, `src/ui/`, and `crates/yapfast-*`.
- Timezones: https://github.com/zrodevkaan/BDPlugins/tree/main/Plugins/Timezones/Timezones.plugin.js
- Spotify inspiration: https://github.com/sewsho/BetterDiscordAddons/blob/main/Plugins/SpotifyListenAlong/SpotifyListenAlong.plugin.js
- Read all: https://github.com/mwittrien/BetterDiscordAddons/tree/master/Plugins/ReadAllNotificationsButton/
- Jump to top: https://github.com/snappycreeper/BetterDiscordPlugins/tree/main/JumpToTop
- Silent typing: https://github.com/Strencher/BetterDiscordStuff/blob/master/InvisibleTyping/InvisibleTyping.plugin.js
- Game toggle: https://github.com/mwittrien/BetterDiscordAddons/tree/master/Plugins/GameActivityToggle/
- BetterVolume: https://github.com/Zerthox/BetterDiscord-Plugins
- Platforms: https://github.com/Strencher/BetterDiscordStuff/blob/master/PlatformIndicators/APlatformIndicators.plugin.js
- Nitro-inspired local enhancements: https://github.com/riolubruh/YABDP4Nitro
- Pear Desktop public API plugin: https://github.com/pear-devs/pear-desktop/tree/master/src/plugins/api-server
- Compatibility investigation references supplied by Ren: https://github.com/morluto/rea and https://github.com/goodnight000/api-anything . REA tools are already available in this session; no additional installation has been performed. Use only when a concrete compatibility question needs artifact/runtime evidence, not as runtime dependencies of Harmony. If API Anything is later evaluated, default to local-only and never publish account tokens, cookies, personal messages or private captures to a shared registry. Better compatibility does not establish undetectability or eliminate Discord account-policy risks.
- Spotify playback API: https://developer.spotify.com/documentation/web-api/reference/start-a-users-playback

Do not execute third-party plugin files or copy code without checking licensing. BetterDiscord plugins depend on Discord's JavaScript modules and cannot load into this native client. YABDP4Nitro uses OSL-3.0, not MIT; do not copy it wholesale into the GPL project without resolving license compatibility. BetterVolume repository's MIT license has been inspected. No third-party plugin code has been imported by this milestone.

## Screen inventory from supplied references

| ID | Screen | Components / observed state |
| --- | --- | --- |
| S01 | Conversation and sidebar | Compact server rail with voice/live badges; direct-message navigation; shops/quests entries; composer actions; docked profile, current activity and wishlist cards |
| S02 | Own profile Board and appearance | Profile context selector; nameplate, avatar/decoration, display-name style, theme/banner, effects/frame controls; Board/Activity/Wishlist tabs; game-grid widget and Add Widget |
| S03 | Own profile Activity | Recent activity cards, icons, relative timestamps and achievement/status chips; overflow controls |
| S04 | Expression picker | GIF/Stickers/Emoji tabs; search; recent and per-server sections; Twemoji-style art, category rail, hover name/footer |
| S05 | Composer menu | Upload File, Share Clip, Create Poll, Use Apps, Schedule Message, Record Voice Message |
| S06 | Full profile Activity | Current rich activity card separate from recent history; mutual friends/servers and other profile tabs |
| S07 | Media viewer | Dimmed backdrop; centered media; author/time area; zoom, share/open and overflow controls; close |

Screenshots are private design references, not shipping assets. Do not publish the messages or personal profile data they contain.

## Flows

- F01 Personalize app: Appearance -> import or create theme -> live preview -> save/reload/reset; reject invalid input without damaging active theme.
- F02 Inspect media: attachment -> viewer -> zoom / pan / actual size / fit -> navigate or close. Profile avatar/banner should eventually enter the same viewer.
- F03 Expressions: open picker -> GIF/sticker/emoji tab -> search/category -> select -> send supported content. Animation follows reduced-motion/user settings.
- F04 Profile: open user -> Board/Activity/Wishlist -> inspect media or edit permitted local/account properties. Distinguish local overlays from account-visible changes.
- F05 Compose extras: choose attachment/poll/app/schedule/voice/clip flow -> review -> send only on explicit user action.
- F06 Privacy/navigation: configure typing/activity behavior -> quick toggle -> read-all confirmation or jump-to-first-history without flooding requests.
- F07 Listening session: opt in -> authorize local player -> share explicit session invitation -> guests play using their own authorized players -> pause/seek/resync -> leave. No music rebroadcast or silent account/device control.
- F08 Commerce/quests: view actual service data -> satisfy actual eligibility -> open secure authorized checkout/reward flow; never simulate ownership or task completion.

## Entities and constraints

- ThemePack: version, name, base, semantic colors, bounded layout/typography/motion roles, optional local assets. Native configuration, never executable code. No remote resource fetch or per-frame CSS evaluation.
- LocalUserPreferences: user ID -> IANA timezone, optional client-only appearance overlay. Keep local by default; do not copy the reference timezone plugin's bulk friend-ID upload.
- Profile/Activity/Collectible: only data the current account legitimately receives. Layout can be recreated; historical activity cannot be inferred from a live-presence event.
- ScheduledMessage: owning account/channel, UTC due time, content/attachments, state, cancellation and retry/idempotency policy. If local scheduling is used, explicitly require the app to be running; do not imply server-side delivery when offline.
- ListenSession: provider/track identity, host playback timestamp and monotonic timing, consenting guests, transport/auth state. Catalog matching and service eligibility differ across providers.
- Clips/VoiceMessage: explicit user recording, bounded storage, encoder/waveform/metadata, confirmation before upload. Never capture voice/screen automatically.

## Known external limits

Spotify's referenced plugin changes a local `isPremium` flag. That is not evidence that the supported playback API works without Premium. Official playback API requirements must be respected and failure surfaced. Pear's API plugin exists but defaults to listening on all interfaces with first-client authorization; recommend loopback-only binding and retain authorization. No Pear settings have been changed.

Quests/Orbs, commerce, premium collectibles and complete activity history require service integration and account permissions, not merely a new screen. No fabricated balances, purchases, rewards, quest progress or premium entitlements. Local-only profile styles must be labeled local. Platform indicators are only as complete as the presence data received, not a device-surveillance capability.

## Build order

1. Audit baseline coverage and preserve every requested item in `replica/features.csv`.
2. Native theme-pack foundation and working media-viewer improvement, with real unit and GUI checks.
3. Privacy/navigation quick controls and local timezone annotations.
4. Expressions, profile media and richer presence/profile presentation.
5. Composer workflows: scheduling, voice notes, clips and app interactions, in separately verified slices.
6. Opt-in music sync with Spotify/Pear capability and authorization checks.
7. Account-backed commerce, collectibles, widgets/history, quests/rewards where documented and testable.

No placeholder menu item counts as a completed feature. Existing capability counts as baseline, not newly implemented work. Windows and Linux verification must be tracked separately.
