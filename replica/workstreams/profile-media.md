# profile-media — native received profile assets

## Delivered slice

- Profile card, full profile, and docked DM profile avatar/banner targets open the existing `ImageViewer`. Ordinary member/message avatars retain their original open-profile actions. Editor previews and absent/unsupported assets have no media target.
- Native accessible buttons provide distinct avatar/banner labels and default actions; pointer clicks, Enter and Space activate the target. A press begun on a different profile cannot retarget its release to the replacement person. Escape closes the media overlay first and restores the existing profile keyboard boundary. No OS window activation/cursor/capture API was introduced.
- `ViewerSource::{Message, Profile(Session)}` discriminates the source. Existing item/index/picture-map fields are explicitly message-only for compatibility with the parallel message-viewer lifecycle work. Profile callbacks never manufacture a message or pass a profile identity through attachment indexes. Profile sessions have process-unique request ids, a profile/panel slot, account identity and the exact received asset identity.
- Resolve only from the currently displayed received profile; validate exact HTTPS CDN/media hosts, endpoint shape, user id, guild member path, hash and format. There is no arbitrary-URL viewer action. Initial/loading shells are not asset authorities. Account transitions discard old Discord profile authorities, rather than silently rebinding them to the new account.
- Request Discord's documented maximum `size=4096`. Preserve the received global/member hash and path. Animated `a_` hashes request `.webp?size=4096&animated=true`, including when the received thumbnail used PNG; Discord now documents animated WebP as compatible with WebP/AVIF uploads that do not always convert cleanly to GIF. Static assets request PNG; default avatars remain their real default-avatar CDN resource. Source: https://docs.discord.com/developers/reference#image-formatting . No claim that lost upload detail can be recovered; overlay reports actual decoded dimensions and explicitly notes that a maximum-size request may contain less detail.
- Full-size fetch occurs only on click, through existing `Discord::download_now` and its existing allowlisted/cache-backed path. Compressed bytes remain limited to 20 MiB and profile work to 25 seconds. Profile originals now share `original::admission()` with both Discord and Matrix message originals: one process-wide slot spans fetch, queued/running blocking decode and the decoded event awaiting Application consumption. Cancellation drops the real fetch future; noninterruptible decoders retain their slot until exit.
- Loading, provider unavailable, network/timeout/size/decode errors are surfaced in the same overlay. Open original is available while loading/error. Save is disabled until original downloaded bytes exist, then saves those exact bytes (including after decode failure) through the native file dialog. Save-dialog completion is request/account/source checked. Profile Copy Image is intentionally not advertised; Copy Link/Open Link remain available in its existing menu.
- Close, source change, stale profile/hash and account changes release session bytes/pictures and cancel the profile request. Existing Slint still/animation frame conversion and cache lifetimes are reused.

## Executed TDD / verification

Observed behavioral REDs before fixes: existing card avatar enlarged the profile instead of media (1 vs 0); maximum-asset resolution absent; stale request/account acceptance; cancel did not drop pending fetch; absent provider left loading unfinished; a new account retained the previous account's received profile authority. All corresponding regressions pass now.

All compilation/test outputs are under distinct Hermes scratch directories, not the root target. Native compiler/test processes use BELOW_NORMAL_PRIORITY_CLASS, rustc codegen-units=2 and test-threads=1; compiler invocations are serialized. No Cargo invocation, running-app restart, live account operation, credential read, screenshot or physical input was used.

From the assigned worktree:

```sh
python testing/run_profile_media_tests.py --core --deps-root C:/Users/ren/Documents/Apps/yapfast --output-dir C:/Users/ren/AppData/Local/hermes/cache/scratch/harmony-profile-media-core
```

Exact final result:

```text
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 448 filtered out; finished in 0.03s
```

This directly compiles the current production core against exact cached Cargo dependency fingerprints and runs the production Application dispatch/event/command tests, resolver/session tests and actual bounded image decoder tests (including the existing animated WebP fixture). It does not rebuild native dependencies or run a desktop.

The same current core test executable, filtered to `application::`, run below-normal priority:

```text
test result: ok. 252 passed; 0 failed; 0 ignored; 0 measured; 219 filtered out; finished in 0.13s
```

Native production ProfileCard/ProfileModal/ImageViewer/accessibility tests:

```sh
python testing/run_profile_media_tests.py --deps-root C:/Users/ren/Documents/Apps/yapfast --output-dir C:/Users/ren/AppData/Local/hermes/cache/scratch/harmony-profile-media-native
```

```text
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

Existing message ImageViewer regressions:

```sh
python testing/run_profile_media_tests.py --viewer-regression --deps-root C:/Users/ren/Documents/Apps/yapfast --output-dir C:/Users/ren/AppData/Local/hermes/cache/scratch/harmony-profile-media-viewer-regression
```

```text
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.58s
```

The pre-existing message-viewer harness emits its existing Slint edition deprecation warning; tests remain unchanged and pass. The new targeted tests compile without warnings. No claim that these overlapping subsets sum to an independently verified whole-app test total.

## Merge seams

- New core modules: `src/profile_media.rs`, `src/profile_media_io.rs`, `src/application/profile_media.rs` and their fixture tests. New enum/Action/Event/DiscordCommand wiring in `application.rs`/`chat.rs`; owned loader in `discord_text.rs`. Missing-provider fallback in `discord_text/commands.rs` and backend-send fallback in `application/session.rs`.
- `Viewer.source` is additive: retain both source-specific coordinators and their cancellation/retention fields. Message reconciliation cancels only its own obsolete request and clears only the viewer it still owns; it must not clear a newly selected profile. Profile reconciliation likewise preserves a newly selected message. Both workers share admission, not ownership. Retain the Message-source checks in stepping/loading and `Event::Image`.
- `src/ui/refresh.rs::show_viewer` has only an early `show_profile_media` branch and message-mode property reset. Preserve the parent's original-URL/8192 decoder/pixel-fidelity path underneath it. New profile presentation and bindings live in `src/ui/profile_media.rs`; `callbacks.rs` only calls that binding helper.
- Shared `ui/people.slint`: one import; two ProfileData availability booleans; `view-media(int)` on ProfileCard/ProfileModal; avatar/banner targets and modal propagation. Preserve profile-decoration/widget child changes around these small hunks. `src/ui/profile.rs` supplies only those availability booleans.
- `ui/messages.slint` adds a profile mode, validated link/source key/save readiness, profile save routing and Open original toolbar action. Message defaults preserve behavior. `ui/yapfast.slint` connects all three profile surfaces and existing overlay, plus shared return-key helper.
- `src/ui/tests.rs` existing Viewer literal now explicitly names `ViewerSource::Message`.

## Explicit verification boundaries / remaining parent work

- Parent owns integrated desktop compilation/build. Added **unexecuted here** integration regression `ui::tests::profile_media_adapter::profile_media_adapter_uses_loaded_pixels_and_releases_them_on_close` in `src/ui/profile_media_adapter_tests.rs`; it dispatches the pure production model, injects fixture pixels, calls real Adapter.refresh and checks Slint pixels/close-release without sending backend effects. Run it after integration. The current complete UI adapter/callback compilation has not been claimed from the component harness.
- Native Save dialog and external browser launch were not exercised on the user's desktop. Native component callbacks and model payload identity are tested; real file-dialog/provider/account interaction is not claimed.
- No live CDN/account request was used. Provider fetch uses the existing production safe path; decoder tests use deterministic local fixtures. The parent's existing allowlist/redirect policy remains in force; this slice does not broaden it.
- Windows targeted execution only; no Linux build/runtime claim. Shared Rust/Slint code remains portable. Matrix profile media is deliberately not added: unsupported/non-Discord profile URLs have no enabled target, rather than being sent to Discord's authenticated adapter.
- This is a coherent implemented profile-media slice pending the parent's integrated validation, not a claim that every feature or the known whole-app baseline is green.

## Integrated original dependencies and cross-source follow-up

Worktree/branch: `.local/worktrees/profile-media`, `feat/profile-media`. The completed profile slice was `ddbe9d05ed18753a2b121dcf2302d8d4346df76f`. Replayed the exact requested dependency commits, in order:

| Parent dependency | Local cherry-pick |
| --- | --- |
| `18b5c5cc30b74763959cbfa2951f78150edce350` original fidelity | `6ab3a70c6b03b5ae53f594ff7f85a7372716334a` |
| `875ee56d4b9c4980a3fe6c8fa03ce4e1f60f9239` HTTP policy | `01acf36e370b211108c18e48a928f1247b04a996` |
| `527171a9da22318950004c78a76381abdc3e4128` original lifecycle | `3ee378f7e4bbc9927f12118b1540911aad8a7954` |
| `84f994369aefe77d7fc86aa6da2e1c7c05a586a9` independent runner fix, subsequently authorized | `d3b886abf1ed93d6b26478ffa7283f59b2c4a59b` |

The integration-fix commit is based on `d3b886abf1ed93d6b26478ffa7283f59b2c4a59b`. Its diff contains only this follow-up; the original profile slice is still `ddbe9d05`. Do not replay equivalent dependency patches onto a parent tree that already has them. The lifecycle conflict in `Application::apply` retained BOTH account-authority invalidation and `sync_original`. Parent-owned feature matrix/LOCAL-DEVELOPMENT changes were retained only by cherry-pick, never hand-edited.

### Ownership and production behavior

- `original::Completion<T>` is non-Clone and owns the queued lease. `Decoded` remains its message-picture specialization, and `Task::start` keeps its existing signature. `Event::ProfileMedia` now carries `Completion<Loaded>`; mutable animation-cache binding happens while that envelope still owns admission. A stale envelope drops its buffers before its lease; only a validated current event is converted into cloneable `Loaded` presentation state.
- `Application::profile_original` owns a non-clone RAII guard. The profile command carries a cancellation observer, not ownership in `Viewer`, `Session`, or `Loaded`. Close, replacement profile, message selection, account/source invalidation, unlink/logout/reset/stopped and Application drop cancel actual work even if cleanup effects are never delivered or a Viewer snapshot survives. Loader replacement/drop also cancels its own request only. A cancelled current loader is surfaced as an error on the next Application transition, rather than an immortal Loading state.
- `Loader` no longer declares a semaphore. It acquires the same process-wide `original::admission()` before polling the fetch. The permit and original raw bytes move inside the blocking closure, then into the queued completion, including a legitimate decode error with saveable bytes. Profile/account/request/asset validation precedes completion acceptance.
- Preserve explicit-click authority validation, exact member/global/default asset identities, `size=4096`, animated WebP identity/delays, the existing native overlay and raw-byte Save. No original URL/fidelity/8192-bound behavior, HTTP allowlist, limit, or unrelated preview/message task policy was relaxed. Profile errors now preserve typed `TooLarge`, access-refused and stalled-transport distinctions instead of flattening them into blind-retry text.

### Executed RED -> GREEN evidence

Behavioral REDs were observed before their respective production fixes:

1. Message -> profile cleared the just-opened profile (`message reconciliation must not erase the newly selected profile`).
2. A profile fetch entered while a cancelled message decoder was still blocked (independent gate / admission after fetch).
3. A queued profile result freed admission early, allowing a message fetch before Application consumption.
4. Dropping Application left its actual profile fetch running despite retaining only a cloned presentation.
5. `stopped()` omitted the new profile guard; the transition table specifically failed its pending-fetch drop assertion.
6. Typed safe-limit failures reached the profile sink as generic retry text.
7. Dropping a provider Loader stranded the current profile in Loading; explicit reopen now finishes with new real fixture pixels.

New cross-source regressions are in `crates/yapfast-core/src/application/profile_media_lifecycle_tests.rs`, registered under the existing lifecycle module and gated to Discord-enabled test builds. They use the real production Application dispatch/events/commands, `original::Task` and profile `Loader`; only fetch timing/decoder timing is injected, with tiny generated PNGs, blocking barriers, real completion envelopes and Weak ownership:

```text
profile_media_cross_source_message_profile_message_preserves_new_selection
profile_media_cross_source_delayed_message_profile_message_returns_to_real_pixels
profile_media_cross_source_profile_fetch_waits_for_cancelled_message_decoder
profile_media_cross_source_running_profile_decoder_retains_slot_when_message_opens
profile_media_cross_source_cancelled_queued_blocking_decode_never_runs_or_releases_early
profile_media_cross_source_queued_profile_completion_bounds_message_fetch_and_drops_stale_buffers
profile_media_cross_source_queued_message_blocks_profile_then_is_dropped_without_filling_it
profile_media_cross_source_close_reopen_rejects_queued_profile_then_loads_new_request
profile_media_cross_source_application_owns_cancellation_not_cloned_presentations
profile_media_cross_source_transitions_drop_pending_fetch_without_cleanup_delivery
profile_media_cross_source_dropped_loader_finishes_loading_with_an_error_and_reopen_recovers
profile_media_cross_source_real_decode_error_keeps_exact_save_bytes_until_close
```

Additional actual Loader sink test: `profile_media_io::io_tests::typed_safe_limit_and_access_failures_reach_the_profile_loader_sink`. The transition test covers close, message selection, replacement profile, asset invalidation, account event, unlink, logout, logged-out event and stopped without forwarding cleanup commands.

### Final targeted execution in this worktree

All compiler/test children remained below-normal priority with two codegen units and serialized heavy invocations. Scratch outputs are worker-private; the root target was only read. No Cargo, running-app/OS-window operation, live provider/account, credential, recording, browser or native dialog operation was performed.

The common runner is `python testing/run_profile_media_tests.py --deps-root C:/Users/ren/Documents/Apps/yapfast --output-dir <private-scratch> [mode/filter]`. It selects exact cached dependency fingerprints, imports real modules for `--model`, updates fixture event envelopes, and refuses zero-test success. New `--fidelity-regression` compiles this worktree's actual core library into scratch before linking the existing checked-in pixel/URL tests; it does not substitute a duplicate model or stale cached core.

| Executed mode/filter | Exact result | Scratch suffix below `C:/Users/ren/AppData/Local/hermes/cache/scratch/` |
| --- | --- | --- |
| `--core` (default `profile_media`) | 36 passed; 0 failed; 463 filtered | `harmony-profile-integration-core` |
| Same core executable, `application:: --test-threads=1 --quiet` | 275 passed; 0 failed; 224 filtered | same core output |
| Same core executable, `viewer_lifecycle --test-threads=1 --quiet` | 26 passed; 0 failed; 473 filtered | same core output |
| Same core executable, `chat::tests:: --test-threads=1 --quiet` | 10 passed; 0 failed; 489 filtered | same core output |
| Same core executable, `discord_text::open_channel::original_tests --test-threads=1 --quiet` | 3 passed; 0 failed; 496 filtered | same core output |
| No mode: native ProfileCard/ProfileModal/ImageViewer/accessibility | 9 passed; 0 failed | `harmony-profile-integration-native` |
| `--viewer-regression` | 11 passed; 0 failed | `harmony-profile-integration-viewer` |
| `--viewer-cache` | 2 passed; 0 failed | `harmony-profile-integration-viewer-cache` |
| `--model` | 16 passed; 0 failed | `harmony-profile-integration-model` |
| `--fidelity-regression` | 9 passed; 0 failed | `harmony-profile-integration-fidelity` |
| Loopback HTTP fixture with `--regression-source testing/media_download_regression.rs` | 7 passed; 0 failed; unapproved target contacted 0 times; redirect-loop count 5 | `harmony-profile-integration-http` |

HTTP command executed: `python C:/Users/ren/AppData/Local/hermes/cache/scratch/harmony-profile-http-fixture.py`. This scratch-only launcher reuses the committed `testing/run_media_download_tests.py` fixture and unchanged Rust tests, substituting only the worker-safe cached compilation command. The ordinary HTTP runner was not edited. Native message viewer still emits its existing Slint edition warning; current core/new targeted tests compile without warnings. These suites overlap and are not a summed whole-app test total.

The independent runner commit was retained unmodified. `python -m unittest testing.test_viewer_lifecycle_runner -v` reported 13 discovered, OK with 5 native opt-in tests skipped (not a claim of 13 executed native checks). Its actual default cache and lifecycle commands were independently executed here: 2 and then 25 passed at that revision, with `TMPDIR=C:/Users/ren/AppData/Local/hermes/cache/scratch/harmony-profile-integration-runner` to avoid shared scratch. The later Loader-drop test makes the final lifecycle filter 26, as shown above. The profile runner's own unmatched-filter execution reported zero tests and correctly returned exit 2 rather than success.

### Parent integrated-source validation checkpoint

Parent applied the combined scoped profile delta from `ddbe9d05ed18753a2b121dcf2302d8d4346df76f` and `ae7cc151c6af9c1d74808340c78ac3af040fc256` onto `b849c27130cac16a970379c41aaee63c3dc45400`, without replaying equivalent dependencies. Parent-only four provider-renderer/missing-metadata tests and original-URL documentation remained unchanged. Rustfmt changes are included in the frozen review candidate.

Executed against the actual integrated root source using `testing/run_profile_media_tests.py` and private `harmony-parent-profile-integrated-*` scratch:

- `--core`: **36 passed**, then that compiled executable explicitly filtered to `application::`: **275 passed**, `viewer_lifecycle`: **26 passed**, and `discord_text::open_channel::original_tests`: **7 passed**. These overlap; the7 include all four parent additions.
- Native profile components/accessibility: **9 passed**.
- Existing message ImageViewer: **11 passed**, with its existing Slint edition warning.
- Conversion cache: **2 passed**.
- `--fidelity-regression`, which rebuilds the current production core in scratch rather than using an old cached core: **9 passed**.

Independent review `deleg_12ac3f11` is reviewing the immutable `harmony-profile-review.diff` (**148631 bytes/31 source sections**) and static scan. No credential/eval/pickle/SQL matches; one fixed Visual Studio discovery shell invocation in the test helper. Verdict pending.

The actual parent Cargo sequence `proc_af21283b38d9` completed **exit0**. Its full log confirms the exact registered `ui::tests::profile_media_adapter::profile_media_adapter_uses_loaded_pixels_and_releases_them_on_close` executed **1 passed**, the existing `an_original_reaches_the_native_viewer_without_an_adapter_resize` executed **1 passed**, and the Windows desktop build succeeded. Neither was a zero-match run. These are the real integrated Adapter and desktop target, not the small component harness.

Fresh-context review `deleg_12ac3f11` returned **passed: true**, with no concrete security/logic findings. Parent confirmed the current Rust/Python/Slint diff exactly matches all31 immutable reviewed sections. A subsequent full root suite returned **281 passed, 1 failed, 6 ignored, 1 filtered out**, exit101. The only failure is the unchanged known `emoji_text::tests::a_picture_keeps_its_round_edges_wherever_it_lands`, same9-of22 flat-bottom assertion at16px; no full-suite-green claim. Capture-mutating tests remain excluded. Full log: `harmony-profile-parent-root-suite.log` under agent scratch.

The profile/message shared-source integration gate is cleared for parent commit. The review's nonblocking suggestion to improve blank-frame animation fixtures remains noted; the actual colored integrated profile pixel/release and detailed message-pixel tests passed, but they do not make every animation assertion a rendered-color test. Live CDN/dialog/account, Matrix profile support and Linux desktop/runtime remain outside this Windows checkpoint. No artifact installed, published or app restarted.

### Parent boundaries and genuine limitation

- Parent-only additions in `discord_text/original_tests.rs` were NOT copied or edited. Keep those four provider-renderer/unsupported-metadata regressions when integrating; this worktree's provider-only result is the original three, not the parent's seven. `Task::start` remains source-compatible with them.
- Registered real Adapter test remains `ui::tests::profile_media_adapter::profile_media_adapter_uses_loaded_pixels_and_releases_them_on_close`; its fixture now uses the real completion envelope. Full Adapter/Cargo, Linux and Matrix SDK builds remain parent-owned and were not run here. The common production Task is used by both message providers; targeted Windows execution is not Matrix SDK/Linux validation.
- Running native/blocking decoders cannot be forcibly interrupted. Cancellation detaches the awaiter but retains the one slot until the closure exits. Cancelled queued blocking work likewise retains its lease until it can run its cancellation check (and then skips decoding). The profile deadline cannot kill such native work. If a decoder never exits, later originals must wait or time out rather than start an unsafe concurrent decoder. Barrier tests prove recovery when it does exit, not immunity to a hung codec.
- No new build/install/push/release or overall feature-completion claim. Real native dialogs/browser/account behavior and the pre-existing whole-app baseline still require the parent's authorized integration validation.
