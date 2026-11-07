# Expressions independent-review gate

Candidate: `e1f0a1ce37bb29bf4c38ac3bb1264acd7d69fb1d` on `feat/expressions`. This candidate is not integrated into root or installed.

## First independent verdict

Review `deleg_7456f926` returned **passed: false**, with **no concrete security concern** and two concrete logic defects, after reviewing all 31 sections of the immutable 127946-byte source diff and its scan. This was static review only, not a command/test/live-service run.

1. **Sticker response identity:** `crates/yapfast-discord/src/service.rs::Discord::send_sticker` validated channel, message ID and sticker, but not the returned nonce or authenticated author. Core completion echoed the requested nonce, so a mismatched response could finish a pending selection, update recents and consume its reply. Validation must happen before name-cache/timeline/success mutation, using actual returned identity.
2. **Empty viewport pins:** `src/ui/expressions.rs::show_stickers` cleared its last requested IDs while rebuilding. A no-match search then computed the same empty set, skipped `ExpressionInput::Shown([])`, and left old `sticker_picker.shown` URLs pinned by `pictures_on_screen`. Unknown/dirty viewport state must not be conflated with an already-reported empty viewport.

Nonblocking follow-ups: the executable-mode runner needs zero/missing-summary rejection; successful synthetic Application events do not prove service-response validation; stale account/session completion and root original-media interaction require explicit integration checks. Metadata-only adapter mode must remain labelled metadata-only rather than being forced to emit libtest output.

## Fix assignment and boundaries

A separate fix context, `deleg_3b557b53`, is assigned the clean expressions worktree and only these bugs plus narrowly related zero-test hardening. It must observe production-boundary RED/GREEN, valid and mismatched acknowledgements through safe injected/anonymous fixtures, actual populated-to-empty viewport pin release, real no-match failure and executed matching tests. No real Discord service constructor/cache/keyring, live account send, root/shared-target build, desktop input/capture, publication or installation is authorized.

Parent independently executed 17 raster tests, including the unchanged round-edge assertion; that evidence remains valid, but it does not clear these sticker defects or make the whole root suite green. Font generation/art/assertions are outside the requested fix. A new exact commit and independent follow-up review are required before integrating the expressions candidate. The current root profile-media build/review is a separate gate.
