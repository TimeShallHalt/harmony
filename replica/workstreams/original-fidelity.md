# Original media fidelity: integration-owner slice

## Implemented source changes

- Core viewer bounds now match the decoder's existing 8192-per-side safety ceiling, rather than shrinking accepted originals into 1920 by 1200. Presentation still fits the image to the viewport independently.
- Supported PNG/APNG/JPEG/GIF/WebP attachments select the received signed original URL verbatim. No size/format parameters are appended to that original.
- Component galleries use original Discord attachment URLs; externally hosted gallery media stays on Discord's proxy.
- External embed pictures keep Discord's full proxy URL, without a new size/format transformation for supported formats. No new third-party contact is introduced.
- AVIF/HEIC/SVG/unknown source formats retain the existing animated WebP compatibility path, but request source dimensions rather than the old window-sized cap. This is a service conversion, not a promise of byte-identical unsupported source decoding.
- Timeline preview requests and their 550 by 400 bound remain separate and unchanged.

## Observed RED -> GREEN

`python3 testing/run_fullres_pixel_tests.py --test-threads=1` compiles a small native Windows test executable against the built production core/Discord libraries. Its runner resolves dependent serde_json using actual Cargo fingerprints, rather than selecting incompatible newest copies. Rebuild production libraries before using this helper to claim GREEN.

- Before the core fix: the generated 3840 by 1200 PNG decoded as 1920 by 600. The resolution assertion failed.
- After the core fix and a successful native desktop build: it decoded as 3840 by 1200, and every RGBA byte matched the generated source. A downscaled-then-upscaled substitute cannot pass the byte comparison.
- Before URL selection changes: attachment/gallery/embed tests all observed resized WebP/proxy URLs. The unsupported animated-format test also saw the old size cap.
- After URL changes and a second successful native desktop build: **9 native fidelity/URL/safety tests passed**, zero failed. This includes original URL selection without preview metadata for PNG/APNG/JPEG/GIF/WebP; rejection of direct third-party and lookalike gallery hosts; source-pixel equality; original signed URL selection; external proxy retention; gallery originals; full-size animated compatibility requests; two GIF frames with unchanged pixels and 80/120 ms delays; and refusal above decoder side bounds plus malformed input.
- Existing production Slint viewer harness: **11 passed**, covering pan/zoom/navigation/resize/animation behavior without physical input.
- Branding/theme/viewer source contracts: **18 passed**.
- `rustfmt` on changed Rust files and `git diff --check` passed. Existing renderer unused-Result and settings-nav geometry warnings remain.
- Root application suite after the original-fidelity slice, before the later follow-ups: **269 passed, 1 failed, 6 ignored, 1 filtered out**. The failure is the existing `emoji_text::tests::a_picture_keeps_its_round_edges_wherever_it_lands`, with `the bottom is flat, 9 of 22 pixels, at 16 pixel text, shifted 0` at `src/emoji_text.rs:520`. The expressions workstream owns its fix; it was not hidden or relabelled proven-upstream. Capture-mutating chrome input remained explicitly filtered out.

The additional registered root UI adapter regression **passed**: `cargo test --locked --profile fast --no-default-features --features desktop,discord an_original_reaches_the_native_viewer_without_an_adapter_resize -- --nocapture --test-threads=1`. It feeds a generated original through the real core decoder, `Adapter::refresh` and Slint image-buffer conversion, verifies the full dimensions and every RGBA byte, then closes and asserts release of the original Picture allocation. Native result: **1 passed**, zero failed; 276 other tests were filtered out. This test uses the headless native backend, not physical desktop input.

## Related transport follow-up

`media-transport.md` records a separately reproduced and fixed inherited downloader redirect/size-limit weakness. Its seven real loopback HTTP regressions passed against rebuilt production libraries, and the nine original-fidelity regressions passed again afterward. That follow-up leaves original/preview byte budgets unchanged. The whole-root suite result above predates the transport follow-up; final all-branch integrated testing remains required.

## Deliberate limits and remaining work

The original-fetch byte budget is still the baseline 20 MiB and decoder allocation budget is still 256 MiB. An image beyond these safeguards must offer save/open-original rather than be silently represented as a blurry success. The `fullres.md` lifecycle follow-up is now applied for parent integration: typed limit/access errors, active-only original retention, cancellation/stale-result rejection and the single fetch/decode/queued-completion slot passed its Windows checks. Parent's combined root suite compiled and ran with **271 passed, 1 known emoji-edge failure, 6 ignored, 1 capture-mutating test filtered**; the nine fidelity and seven HTTP regressions passed again. Windows desktop builds passed; the independent-review default-filter/zero-test runner bug was fixed by84f99436 and verified with13 Python/native checks. Fresh-context re-review passed with no concrete security/logic findings; parent confirmed all27 source sections match and reran13 runner checks plus9 fidelity and7 HTTP regressions successfully. The media source review gate is cleared, not the whole-app/profile/release gates. Additional provider-renderer/error and filtered parser unit checks passed; see `media-review.md`. The separate profile-media workstream still owns avatar/banner entry points. No automatic signed-URL refresh is implemented; an access/expired-link failure is explicit rather than silently showing a thumbnail.

Cache inspection found that `cache::unsigned` removes only signature parameters, preserving transformation fields, so existing thumbnail keys do not alias full proxy keys. Direct original CDN attachments are not persisted as public picture-cache entries; `keep_picture` still returns their bytes normally. Their encoded animations may therefore remain in bounded memory while open. Do not add unencrypted private attachment caching to optimize this.

No live/private account image, microphone, desktop, game, call or music device was used as a fixture. A public CDN compatibility probe returned the default avatar PNG, while both media-proxy variants returned HTTP 401; that probe does **not** establish live proxy conversion behavior. The successful acceptance evidence above is native production-code execution with generated/offline fixtures. Linux full desktop build/runtime and live-account media verification remain unverified.

Nothing from this slice has been published, installed or activated at this checkpoint. The previously installed candidate remains untouched. Overall feature completion still requires the other branches and integrated regression/release gates.
