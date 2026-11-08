# Harmony Update2 — scoped five-feature candidate

Target app version: **1.0.1**. Development tests use build ID **0**. A clean,
reviewed commit and the parent's production build are required before build ID1.
Internal executable, data-directory and protocol identities remain Yapfast's.

## Included behavior

- `fullres`: reuse the integrated original-source URL/decode path, including
  supported original pixels, bounded transport/decode, explicit errors and
  cancellation/stale-completion ownership. This does not promise unlimited
  resolution, arbitrary source probes or signed-link refresh.
- `profile-media`: reuse avatar/banner actions and the shared native viewer,
  original handling, errors, zoom/pan/navigation and lifetime release.
- `silent-typing`: reuse the persisted **Let others see when you're typing**
  switch. Off suppresses outbound typing start/stop at the Adapter's backend
  dispatch boundary, but does not suppress draft saving or received indicators.
  This is a device-local preference retained across window/account contexts.
- `server-voice-badge` / `server-stream-badge`: native server icons and closed
  folders show distinct received voice/Go Live evidence. Ordinary voice does not
  imply streaming. Counts deduplicate people per server, exclude hidden/locked,
  DM and other-network rooms, and clear when unknown, offline or unavailable.
  They are **received state, not complete server totals**. They introduce no
  polling, joins, capture, game/RPC dependencies or service constructors.

## Integration boundaries

Only scoped activity hunks were reused from the old QoL source. About,
updater exclusion and all newer original/profile-media code are retained. No
whole-branch merge, unrelated QoL features, updater, installer execution or
public release is included. GPL, upstream history, artwork and font attribution
are unchanged.

`src/ui/update2_tests.rs` is registered in the actual application's test target.
It drives real received snapshots through Application/Adapter/native rail and
uses the real settings control/composer callbacks with a store-free anonymous
Backend transport. `Backend::anonymous_fixture` exists only with the already
available `test-fixtures` feature and never starts chat/service/device actors.
The core aggregation test and inherited media regressions use production code.
All native selections must have successful nonempty summaries.

Execution receipts, failures, per-ID results, exact commands, source and artifact
hashes and explicit compiler-slot release are in the controlling handoff:
`C:/Users/ren/Documents/Apps/yapfast/.local/evidence/update2/implementation.json`.
Cached exact-graph rustc tests are not a clean Cargo production build, a Linux
app build, live Discord/CDN acceptance, GPU/physical-input acceptance or MSI
acceptance. Those remain parent-owned gates.
