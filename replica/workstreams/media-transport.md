# Media HTTP transport hardening

## Reproduction and scope

The existing downloader approved the first URL but its ordinary HTTP client followed redirects without applying that allowlist to the destination. A real, anonymous production `Api::download_now` invocation against two generated loopback fixtures followed the redirect: the forbidden fixture recorded **1 request**. A redirect loop made **11 requests** before the library default stopped it.

The in-memory downloader also extended its result before checking the byte budget and returned an untyped error. Its streaming-to-file counterpart already had a typed `TooLarge`; a full-resolution image's UI cannot reliably identify a limit failure when the in-memory path returns only a string.

These were local test observations, not exfiltration of real account data or claims about an attack against a live service. Tests use the app's existing test-server hook in the native child's environment only. Cookie persistence is never initialized, no saved credentials/cookies are loaded, and neither Discord nor a real player is contacted.

## Source changes

- Keep account REST's HTTP client/cookie behavior unchanged.
- Give media a separate HTTP client without the account cookie jar.
- Validate each redirect destination with the existing file/provider allowlist before following it. Bound the chain explicitly, including same-origin/relative redirects.
- Remove signed URLs from media request errors.
- Reject excessive declared Content-Length before reading the body; reject a chunk exceeding the remaining budget before copying it into the result. Use the existing typed `TooLarge` consistently and accept a body exactly at the limit.
- Do not enlarge timeline/original byte budgets or relax decoder/host limits in this change.

## Observed verification

Commands:

```text
python testing/run_media_download_tests.py
python testing/run_fullres_pixel_tests.py --test-threads=1
cargo build --locked --profile fast --no-default-features --features desktop,discord
```

The native transport regression exercises the actual rebuilt production `Api` through real local HTTP, not a replacement HTTP implementation. Its generated declared/chunked bodies use tiny 16/64-byte test bounds; the huge declared-length case sends only headers, never an actual huge allocation/body. The fixture was refined from a protocol-level unrepresentable length to a valid large Content-Length so the test isolates the application's pre-body limit check.

Before the fix: **2 passed, 5 failed**, including the unexpected destination contact, excessive redirect chain and untyped size errors. After the fix: **7 passed, zero failed**. Fixture counters after GREEN: forbidden target requests **0**, redirect-loop requests **5**. Approved relative redirects still return the exact fixture body; error output excludes the fixture's signed-query marker.

The existing original-image suite was rerun against the same rebuilt libraries: **9 passed**, including pixel-identical originals, GIF frame/delay fidelity, signed URLs, proxy isolation and refusal above image safety bounds. The Windows desktop binary built successfully. Rust formatting, Python syntax and diff whitespace checks passed; existing unrelated renderer/settings warnings remain.

The last whole root-app suite was run before this transport change: 269 passed, one known emoji-edge failure, six ignored, one capture-mutating test filtered. A final integrated suite after all feature branches is still required; this document does not claim that full suite is green. Physical input, live-account media behavior and Linux full desktop runtime remain unverified. This source slice has not been published, installed or activated.
