# fullres — bounded original-view lifecycle

## Scope and implementation

- Native viewer state retains only its current decoded original/pending entry. Navigation drops inactive pixels and pending markers; returning to an attachment issues a fresh request rather than remaining in Loading.
- Each `Command::ViewImage` / `Event::Image` carries an opaque request identity/cancellation handle. The Application owns its lifetime separately from clonable presentation state. Closed/reopened views, attachment navigation, room/aside/account changes, logout/unlink, stopped transport and rejected backend commands invalidate old requests/results.
- Discord `OpenChannel` and Matrix `OpenRoom` own an original-only task, separate from preview/history/message tasks. Discord shelves cancel this task immediately; close/navigation also drops an in-flight transport future through request cancellation. No `abort_all` on unrelated work.
- A process-wide one-slot admission semaphore covers original download, blocking decode and the completed result while queued for the Application. This bounds compressed downloads, cross-room/account/provider decoders and queued full decoded results, not merely futures waiting for a decoder.
- The permit is moved **inside `spawn_blocking`** and then into the completed result. Tokio cannot stop an already running blocking decoder; cancelling its async awaiter cannot release this slot. A running old decoder must finish and discard its result before another original is admitted. A queued not-yet-started decoder checks cancellation before allocating. The UI never accepts cancelled results.
- The adapter's single converted viewer-image cache also clears when the next attachment has neither an original nor a preview. Its previous None branch otherwise pinned an inactive full decoded image even after Application eviction.
- Typed Discord `TooLarge` now explains the safe download limit and Save-original fallback; HTTP 403 explains expired/inaccessible links and asks to reopen the conversation. No blind retry, no new account API operations. Automatic signed-URL refresh is not implemented in this slice.
- Original and preview transport budgets remain 20 MiB. No URL functions, VIEW constants, decoder allocation limits or profile-media paths were changed; parent owns original URL / 8192-dimension quality changes.

## Executed acceptance evidence (Windows)

Runner uses cached integration-root dependencies/native artifacts with exact Cargo fingerprint matching, direct rustc, private worker scratch outputs, below-normal-priority compiler/test children, two codegen units and serial tests. It never invokes Cargo, starts the app, operates an account, captures input or accesses real media.

```sh
python3 testing/run_viewer_lifecycle_tests.py --cache-root C:/Users/ren/Documents/Apps/yapfast application:: discord_text::open_channel::original_tests
# 256 Application tests passed; 3 provider/task-isolation tests passed.
python3 testing/run_viewer_lifecycle_tests.py --cache-root C:/Users/ren/Documents/Apps/yapfast --ui-cache viewer_picture
# 2 imported production viewer cache tests passed.
git diff --check
# passed
```

RED -> GREEN was observed for actual inactive pixel retention (Weak<Picture> ownership), stale same-attachment results after close/reopen, detached blocking-decoder overlap, logout/unlink/stopped cancellation, backend rejection stuck Loading, typed TooLarge fallback, HTTP403 actionability and adapter-cache retention without a preview.

Production Application action/event tests use fixture messages and generated 2x2 PNGs, with injected delayed fetches/decoder barriers and the real request worker/sink. They verify early fetch drop on explicit close, 32 fast supersessions followed by a successful earlier-attachment reload with exact decoded pixels, stale queued results, cross-owner decode serialization after cancellation, source/account/room transitions, closed source threads, Application drop, and reload after backend rejection. No 256 MiB test buffers are allocated.

## Verification boundaries / integration notes

- The direct-rustc test executable compiles the real Discord-enabled core source, not a model or enum-only lifecycle simulation. The UI cache harness imports the actual production module against cached Slint/core types.
- Parent applied feature commit `527171a9da22318950004c78a76381abdc3e4128` onto the original-fidelity/HTTP-hardening source and reran both commands above: **256 Application, 3 provider/task-isolation and 2 production-cache tests passed**.
- Parent's combined Windows root-app test executable compiled and ran: **271 passed, 1 failed, 6 ignored, 1 filtered out**. Only the existing `emoji_text::tests::a_picture_keeps_its_round_edges_wherever_it_lands` failed (`the bottom is flat, 9 of 22 pixels, at 16 pixel text, shifted 0`). The real original-pixels-to-Slint adapter test and both new cache tests passed. Capture-mutating input remained filtered.
- Against those rebuilt production libraries, **9 original-fidelity/URL tests and 7 real loopback HTTP tests passed again**; forbidden target contacts remained zero.
- The separate Windows desktop build passed, including the final formatting-only UI module-order cleanup. Independent review found one P2 default-filter/zero-test bug in the cached test runner. Parent applied fix `84f99436` and independently passed all 13 Python/native runner regressions: default cache executes 2 tests; default lifecycle executes 14; zero-match runs are rejected. Fresh-context follow-up review `deleg_87ee8648` passed with no concrete security/logic findings. Parent confirmed all27 reviewed source sections match and reran13 runner checks plus9 fidelity and7 loopback HTTP tests successfully. The media review gate is cleared; shared profile integration and final all-branch acceptance remain pending. The explicit-filter cache runs above really executed both tests. Four added provider-renderer/missing-metadata validations passed (7 provider/error tests total); see `media-review.md`. This evidence does not establish live-account media behavior or Linux/Matrix desktop validation. Matrix source has matching request/task/result ownership plumbing but its SDK-enabled feature path has not been compiled here.
- Parent's existing emoji raster failure and filtered capture-mutating native test remain outside this slice. Nothing was launched, installed, merged or pushed from this worktree.
- `Event::Image.picture` and Discord `Done::Image.picture` now carry an `original::Decoded` envelope; Application unwraps it to the unchanged `Viewer.pictures` Arc type. Any new parent test constructing successful Image events should either use the real original Task sink or (in core tests) `Decoded::fixture` (available with core's existing `test-fixtures` feature).
