# Harmony Update Roadmap

Planning revision: 2026-11-07T18:41:43-05:00

## Current stage

**Update 1 is authorized and in progress: About, theme-pack, window-control and media-viewer acceptance. Ren explicitly deferred ALL updater/auto-update and sharing/hosting work. Publish the verified remaining update only to the existing private Harmony repository.**

Implementation uses isolated worktrees and bounded workers. Earlier unrelated repair checkpoints remain preserved and paused; they are not being folded into this release. No physical input, forced app restart or automatic checks/downloads are authorized by starting development.

**Updater and hosting deferred by Ren.** They are not acceptance gates for this release. Experimental updater code must not be activated or shipped as part of Update 1.

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

### Update 1: The foundation (updater deferred)

- [ ] **DEFERRED, outside Update 1: update / auto-update system** (`auto-update`)
  - Manual **Check for updates** action.
  - Optional checks on startup and at a configurable interval while the app is running.
  - Check published GitHub Releases rather than ordinary repository commits.
  - Show version, release notes and an **Update available / Download update** button.
  - Optional background download, with an explicit user setting.
  - Verify the correct platform/version and signed update metadata/artifact before staging.
  - Apply the verified update on the next normal app restart; do not force-close a call or session.
  - Preserve app data, retain rollback and recover safely from interrupted downloads/installations.
  - Keep notify-only and automatic-download behaviour separately configurable.
  - Resolve release hosting, access and licence compliance before distributing to friends; never embed a shared GitHub token.

- [ ] **Settings About panel: app version and build; updater explicitly unavailable in this release** (`app-info`)

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

### Update 5: Expressions and richer interactions

- [ ] **Stickers** (`stickers`)

- [ ] **Animated stickers** (`animated-stickers`)

- [ ] **Nameplates** (`nameplates`)

- [ ] **Polls** (`polls`)

- [ ] **Profile widgets** (`profile-widgets`)

### Update 6: Activity and composer workflows

- [ ] **Quick game-activity sharing toggle** (`game-toggle`)

- [ ] **Activity/game detection and Rich Presence** (`game-detection`)

- [ ] **Detailed activity/history tab** (`activity-tab`)

- [ ] **Scheduled messages** (`scheduled-messages`)

- [ ] **Voice messages** (`voice-messages`)

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

## Distribution and licensing decision: still to resolve

- [ ] Agree on the friends-only/public distribution and update-hosting arrangement.
- [ ] Keep the development repository and unfinished history private where desired.
- [ ] Resolve licence requirements before offering downloadable binaries. The current codebase is declared **GPL-3.0-or-later**. Heavy modifications, renaming, or becoming visually different do not by themselves permit replacing that licence for the derived combined work.
- [ ] If the GPL-based application is distributed, provide compliant access to the matching Corresponding Source and retain required notices. Recipients do not have to download or browse that source, and the private working repository/history need not be made public.
- [ ] If a different distribution licence is essential, obtain the necessary permissions or agree on a genuinely independent implementation/foundation before proceeding; do not simply change the existing licence file.

No licence or repository visibility has been changed. Reference: [GPLv3 sections 5 and 6](https://www.gnu.org/licenses/gpl-3.0.html#section5).

## Future ideas

- Additional features: to discuss after Ren's remaining instruction.
- No additional feature has been added to the 39-item roadmap yet.

## Next agreed action

**Finish the four remaining Update 1 features, verify and package the candidate, then publish to the existing private Harmony repository. Auto-update and sharing/hosting are deferred. Keep other updates paused and do not force an app restart.**
