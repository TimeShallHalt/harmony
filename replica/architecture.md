# Harmony native feature-completion architecture

## Scope and baseline

The requested scope is every requirement in `features.csv`, plus original-resolution media, automatic updates and app/build information. This document is an integration contract, not an implementation or completion report. A menu, model, fixture or roadmap alone does not complete a feature.

Baseline: commit `79ccfcd252cc69e56fc8082fe960f85cf5b806af`; previously installed candidate SHA-256 `3f6c512cf86088d452f3d3f11a4dac49a11c99254ccf9429ac0db652d0adf96e`. The installed baseline adds native branding/chrome, data-only themes and image zoom/pan. Its viewer still caps image fidelity. Most remaining requirements are absent or partial. The version of the installed binary, the current source revision, and the active process are separate facts.

## Stack and boundaries

| Layer | Existing choice | Integration rule |
| --- | --- | --- |
| Presentation | Slint native components and shared semantic Theme/Style globals | No webview, Electron, executable CSS or JS plugin engine. Keep new controls themeable. |
| Interaction and state | Rust `yapfast-core::application::Application`, Action, Effect, chat Command/Event | A reachable UI must dispatch the actual feature; don't create a parallel network client. |
| Service adapter | Existing `yapfast-discord` parsing/service/API and core `discord_text` | Retain authentication, host allowlists, permissions, rate limits and account-scoped state. No fabricated service success. |
| Local preferences | Existing Layout/settings snapshots and owned app data | Add bounded, versioned defaults. Contact timezone/appearance annotations stay local. |
| Durable jobs | Account-owned local queue, UTC due dates and one-shot wakeups | Persist before enabling delivery. Document app-running requirement, ambiguity after crashes and retry policy. |
| Media | Bounded original fetch/decode; cheap timeline previews; existing shared animation buffers | Preserve source pixels at actual size. Show explicit refusal/save-original fallback at safety limits; don't silently upscale a thumbnail. |
| Platforms | Shared Rust/Slint with existing Windows/Linux adapters | Keep native platform policy. A Windows test is not Linux GUI proof. |
| Updates | Explicit private Harmony release provider, verified staging and rollback | No embedded credentials, arbitrary update hosts or automatic call-ending restart. |

No new hosted database, payment processor or account system is required for a client fork. Local schemas belong in typed Rust persistence beside their existing account/preferences owners, not a speculative web SQL migration.

## Ownership and vertical slices

Each implementation branch starts from the baseline. The integration owner alone merges shared declarations, runs integrated Cargo builds, publishes releases and installs binaries. Worktree builds must not invalidate the shared native target. Helpers/harness outputs remain distinct. Branch names are assignments, not evidence of implementation.

| Workstream | Requirement IDs | End-to-end acceptance boundary |
| --- | --- | --- |
| Original media (integration owner) | fullres, media-viewer, profile-media | Source URL -> bounded fetch -> original decode -> shared image/frame buffer -> true decoded dimensions/100% display -> close/release. Profile avatar/banner entry points use the same safe viewer. |
| Updates and information | auto-update, app-info | Settings/version/build -> authenticated release lookup -> compare -> validate manifest/asset -> owned staging -> rollback-safe user-controlled activation. |
| Presence and QoL | read-all, jump-top, game-toggle, better-volume, platform-indicators, server-voice-badge, server-stream-badge, timezones, game-detection, silent-typing, call-timer | Real received state/command -> visible native control -> persistence/permission/error handling; no inferred unseen devices or remote upload of timezone annotations. |
| Profiles | avatar-decorations, nameplates, name-fonts, profile-widgets, activity-tab, wishlists | Actual optional profile fields -> safe asset loading -> native layers/cards -> permitted edits only. Label local overrides; don't invent ownership or historical activity. |
| Expressions | stickers, emoji-art, animated-emoji, animated-stickers | Native picker/search/permissions -> outgoing supported payload -> real result; incoming frame/motion behavior stays consistent. Fix raster regression without weakened thresholds. |
| Scheduling and polls | scheduled-messages, polls | Compose/review/schedule -> account-scoped durable job -> next-due dispatch -> cancel/edit/failure/restart behavior. Existing poll controls get behavior tests. |
| Recording | voice-messages, clips | Explicit opt-in source/start/stop/cancel -> bounded real encoding -> review -> explicit authorized upload -> playback/failure. Tests use generated samples, never the user's microphone/screen. |
| Music | spotify-listen-along, pear-listen-along | Explicit provider authorization -> selected device/player -> invitation/session -> bounded drift correction -> leave/disconnect. Respect provider eligibility; Pear is authenticated loopback only. |
| Service-backed features | quests, orbs-shop, shop, apps, nitro-inspired | Real catalogue/capabilities/eligibility -> review/authorized action -> real receipt/state. No simulated quest progress, entitlements or payment success. Unsupported service/runtime portions remain specifically blocked. |
| Regression preservation | theme-packs, window-controls | Existing data-only pack safety and platform tests stay valid. Physical window gestures remain unverified until renewed permission, not silently treated as passed. |

## Original-image defect and contract

Root investigation identifies separate lossy boundaries:

1. Core `chat::decode_view` uses the old `VIEW_WIDTH = 1920` / `VIEW_HEIGHT = 1200` and downsamples larger files.
2. Discord attachment/embed/component view URLs request sized WebP through the proxy, potentially discarding source detail before decode.
3. Presentation `pictures::fit` historically bounds reported source metadata, but the existing Slint `ImageViewer` already measures actual decoded still/frame dimensions for 100% and labels previews. That hypothesis is not a separate current pixel-loss source; retain its native behavior regressions rather than changing zoom to disguise lower resolution.

The first regression encodes a generated detailed PNG wider than the old cap and compares both dimensions and every decoded byte with the source. A merely enlarged preview must fail. URL regressions must preserve signed original attachment URLs; external embeds remain on Discord's proxy to avoid third-party contact. Decoder allocation/side/download limits are independent safety limits, not a justification for silent reduced quality. Preview caching and original-view state must remain separate, with bounded retention and stale-result handling.

## Persistence and concurrency rules

- Scope jobs, provider sessions and account-visible edits to the owning account. Account switches must not send another account's queued work.
- Save validated preferences/jobs before applying visible committed state. Preserve existing snapshots on write failure.
- Store timestamps as UTC plus explicit presentation timezone. Test DST invalid/ambiguous local times, clock jumps and restart behavior.
- Prefer idempotent service identifiers and explicit ambiguous-send states over blind retries after a crash.
- Keep large media buffers bounded and release them when a viewer closes or navigates. Don't preload every original in the timeline.
- Local annotations, rendered profile data, purchased ownership and service eligibility are distinct concepts.
- No new background capture, frequent global polling or silently controlled music devices.
- Keep filenames, archive members and update manifests confined to owned staging; no traversal, reparse/symlink escapes or unverified executables.

## Verification and release gates

Every requirement needs a specific implemented/partial/blocked result and its executed acceptance evidence in the feature matrix/workstream notes. Record separately:

1. Source/static checks.
2. Production-module native tests, with observed RED -> GREEN where behavior changed.
3. Integrated application/model/provider regression tests.
4. Windows and Linux build/runtime coverage, separately.
5. Live-account validation, only with appropriate permission. Local fake-server payload tests do not prove live service acceptance.
6. Exact source revision and binary hash, scanner completion, release manifest/asset read-back and installed hash.
7. Installation versus activation. Never stop the user's call/game or assume a stale PID still identifies his process.

The baseline root suite has one known emoji round-edge assertion failure; its fix belongs to expressions and must not be hidden or relabelled proven-upstream. Full Linux desktop runtime and physical Windows movement remain explicit gaps until actually exercised. No private message/profile screenshot, credential, runtime account data or generated recording belongs in the source repository.
