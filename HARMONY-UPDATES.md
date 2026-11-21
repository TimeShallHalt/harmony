# Harmony Update Roadmap

Planning revision: 2026-11-21

## Current stage

Harmony 1.1.0 incorporates Yapfast 0.4.1 and the fork’s first four updates. The upstream fork provides the expressions and profiles listed under Update 5. The 1.2.0 Windows build adds the public-release updater, Discord Rich Presence, profile activity history, local scheduled messages and voice recording.

## How these lists work

- Work in updates of up to five features; the final listed update has four.
- Agree on each update's exact scope and acceptance criteria before implementation.
- Complete one feature end to end at a time with focused checks, then verify the integrated update.
- Existing working or partially implemented features are reused, not rewritten to fill a batch.
- Checkboxes below mean acceptance/delivery in this new roadmap, not that all existing code is absent.
- Repairs are part of finishing a feature, not additional features to pad a release.
- Add future ideas to the backlog without silently enlarging the active update.
- The order after Update 1 remains provisional and can change for dependencies or agreed priorities.
- No feature is labelled delivered because only a screen, mock, source module or cached test exists.

## Planned updates

### Update 1: The foundation

- [x] **Public-release updater in Harmony 1.2.0** (`auto-update`)
  - Manual **Check for updates** action.
  - Optional checks on startup and at a configurable interval while the app is running.
  - Check published GitHub Releases rather than ordinary repository commits.
  - Show version, release notes and an **Update available / Download update** button.
  - Optional background download, with an explicit user setting.
  - Verify the correct platform/version and signed update metadata/artifact before staging.
  - Run verified Setup on the next full quit when the user chooses it. A registered Windows installation retains its scope and location.
  - Interrupted downloads can be retried. Previous installers remain available for manual rollback. Recovery from a failed scope migration can require rerunning Setup.
  - Keep notify-only and automatic-download behaviour separately configurable.
  - Resolve release hosting, access and licence compliance before distributing to friends; never embed a shared GitHub token.

- [ ] **Settings About panel with app version and build** (`app-info`)

- [ ] **Native theme packs: retain and verify the existing foundation** (`theme-packs`)

- [ ] **Native window dragging and resizing: finish verification** (`window-controls`)

- [ ] **Image/GIF viewer: zoom, pan and navigation** (`media-viewer`)

### Update 2: Media, privacy and server badges

- [ ] **Original/full-resolution media viewing** (`fullres`)

- [ ] **Open profile avatars and banners in the media viewer** (`profile-media`)

- [ ] **Silent typing** (`silent-typing`)

- [ ] **Voice-channel badges on servers** (`server-voice-badge`)

- [ ] **Streaming badges on servers** (`server-stream-badge`)

### Update 3: Presence and personal controls

- [ ] **Desktop/web/mobile platform indicators** (`platform-indicators`)

- [ ] **Call timer** (`call-timer`)

- [ ] **Precise extended per-user voice and stream volume** (`better-volume`)

- [ ] **Locally assigned contact timezones** (`timezones`)

- [ ] **Consistent emoji artwork and improved picker** (`emoji-art`)

### Update 4: Names, navigation and profile appearance

- [ ] **Name fonts and styles** (`name-fonts`)

- [ ] **Jump-to-top and Home/End history navigation** (`jump-top`)

- [ ] **Read-all notification buttons with confirmation** (`read-all`)

- [ ] **Profile decorations** (`avatar-decorations`)

- [ ] **Animated emoji** (`animated-emoji`)

### Update 5: Expressions and richer interactions (provided by Yapfast)

- [x] **Stickers** (`stickers`)

- [x] **Animated stickers** (`animated-stickers`)

- [x] **Nameplates** (`nameplates`)

- [x] **Polls** (`polls`)

- [x] **Profile widgets** (`profile-widgets`)

### Update 6: Activity and composer workflows

- [ ] **Quick game-activity sharing toggle** (`game-toggle`)

- [x] **Activity/game detection and Discord desktop Rich Presence** (`game-detection`)

- [x] **Detailed activity/history tab** (`activity-tab`)

- [x] **Local scheduled messages with account-scoped delivery** (`scheduled-messages`)

- [x] **Record, review and send voice messages** (`voice-messages`)

### Update 7: Music and service integrations

- [ ] **Pear Desktop / YouTube Music Listen Along** (`pear-listen-along`)

- [ ] **Spotify Listen Along** (`spotify-listen-along`)

- [ ] **Wishlists** (`wishlists`)

- [ ] **Orbs Shop** (`orbs-shop`)

- [ ] **App support: supported commands, forms and interactions** (`apps`)

### Update 8: Largest remaining features

- [ ] **Regular Shop** (`shop`)

- [ ] **Local Nitro-inspired enhancements, without pretending to unlock paid entitlements** (`nitro-inspired`)

- [ ] **Clips** (`clips`)

- [ ] **Quests with legitimate participation and reward state** (`quests`)

## Release acceptance for each update

- [ ] The agreed features are reachable through the actual app and have meaningful error/cancel paths.
- [ ] Relevant focused tests and integrated regressions pass against the exact candidate.
- [ ] Security/privacy findings are resolved or explicitly block the affected release.
- [ ] Windows and Linux build/test coverage is stated accurately; untested runtime behaviour is not labelled verified.
- [ ] Version/build information matches the packaged artifact.
- [ ] Distribution includes required licences, notices and compliant source availability.
- [ ] Install/update integrity, app-data preservation and rollback are verified.
- [ ] Installation and activation are reported separately; no forced restart during active sessions.

## Distribution

Harmony publishes stable releases in the public TimeShallHalt/harmony repository. Include matching GPL source, installer source and dependency notices with each binary release. Publish the signed update manifest and its detached signature beside the Windows installer.

## Future ideas

- Additional features: to discuss after Ren's remaining instruction.
- No additional feature has been added to the 39-item roadmap yet.
