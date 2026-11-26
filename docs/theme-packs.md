# Native Harmony theme packs (v1)

Theme packs are **JSON data interpreted by native Rust/Slint**, not CSS, HTML,
JavaScript, a webview, or executable Slint. They do not install fonts, fetch URLs,
run scripts, poll files, or add idle timers.

## Use and author

Settings → Appearance exposes **Import pack**, **Export pack**, **Reload pack**,
**Reset pack**, **Edit pack colors**, and **Reset logo** using the existing native
open/save dialogs. Start with Export pack or
[`examples/themes/orchid.harmony.json`](../examples/themes/orchid.harmony.json)
and its `logos/orchid.png` sibling. Edit JSON in any text editor; Reload explicitly
reads the imported source again. There is no automatic reload.

A minimal pack is:

```json
{"version":1,"name":"My theme","base":"bonfire","colors":{"accent":"#c4a1f5"}}
```

The [JSON Schema](theme-pack-v1.schema.json) is an editor aid; Rust validation is
authoritative for byte lengths, local-file containment, and PNG limits. Do not
put `$schema` in the pack itself: unknown keys are rejected.

## Exact v1 schema and limits

- UTF-8 JSON input: **at most 65,536 bytes**, no comments or trailing commas.
- Required: `version` (integer **1**), `name` (nonblank, **1–80 UTF-8 bytes**, no
  Unicode control characters), and `base` (one of the IDs below).
- Optional: `dark` (boolean), `colors` (object), `layout` (object), `typography`
  (object), `motion` (object), `logo` (relative PNG path).
- Optional scalar overrides accept `null` to inherit, except `reduced-motion`,
  which is a boolean. Omitted objects inherit defaults; objects cannot be null.
- All objects reject unknown keys. Unsupported versions, unknown bases/colors,
  malformed colors, nonfinite numbers, and values outside the ranges below fail
  the whole import. Missing values inherit the selected base/native defaults;
  importing a second pack does not retain overrides from the first.

Base IDs match the built-in themes, with `harmony` added as the new default:
`harmony`, `yap`, `bonfire`, `catppuccin-mocha`, `dracula`, `nord`, `gruvbox-dark`,
`tokyo-night`, `system24-v2`, `catppuccin-latte`, `rose-pine-dawn`, `sakura`.
`yap` is the preserved Harmony Classic ID for the original Yapfast palette.
Existing packs keep that base and any imported logo; no migration is required.
The base supplies unspecified colors and `dark`. A pack based on
`system24-v2` also inherits its native panel styling and monospace font.
Existing palette credits remain in [themes.md](themes.md).

### Semantic colors

`colors` values are exactly `#rgb`, `#rrggbb`, or `#rrggbbaa` (alpha last),
case-insensitive hex, **without surrounding whitespace**. Supported keys:

```text
rail sidebar panel chat stage raised field sunken hover active row-hover
line edge outline outline-chat outline-field text secondary muted faint
accent accent-hover on-accent accent-soft danger danger-hover on-danger
live loud link button-default button-default-hover overlay shimmer
reply-spine tooltip avatar mentioned mentioned-hover
```

These are all mutable native `Theme` colors. Derived menu/composer/placeholder
colors still follow the shared native bindings. Per-person/server role colors
are not overridden. The existing color editor edits the same semantic palette,
including `mentioned` and `mentioned-hover`.

### Layout

Lengths are **logical pixels before window zoom**; fractional finite numbers
are allowed. Only these `layout` properties are supported:

| Key | Inclusive range | What changes |
| --- | --- | --- |
| `title-height` | 24–64 | Custom titlebar and matching content inset |
| `title-inset` | 0–40 | Custom titlebar wordmark/logo left inset |
| `rail-tile` | 32–48 | Server tile height; rail width/folder tiles stay native |
| `sidebar-width` | 180–380 | Default sidebar width, unless personally resized |
| `header-height` | 36–80 | Native channel/server header rows |
| `account-height` | 48–96 | Account panel height |
| `profile-width` | 240–420 | Conversation profile panel width |
| `members-width` | 180–360 | Conversation member panel width |
| `composer-height` | 40–96 | Default/minimum composer height; content can grow |
| `composer-padding` | 4–32 | Composer horizontal padding |
| `composer-gutter` | 0–32 | Composer outer gutter |
| `message-gap` | 0–16 | Gap between grouped message rows |
| `message-group-gap` | 0–40 | Gap between message groups |
| `channel-row-height` | 24–48 | Channel list row height |

This is token-based layout customization, not arbitrary reordering, positioning,
selectors, component injection, or CSS compatibility. Some presentation-specific
surfaces still use fixed native metrics.

### Typography

`typography` supports:

- `font-family`: installed or bundled family name, **1–80 UTF-8 bytes**, nonblank,
  Unicode letters/numbers plus ASCII spaces, `-`, `_` only. No files or URLs are
  loaded. An unavailable family follows Slint's native fallback. `DM Sans` is
  bundled; external font files are not imported.
- `text-size`: **10–24** logical px, the native default text/message size.
- `title-text-size`: **10–20** logical px, custom titlebar text.

Explicit native widget sizes remain explicit. A person's Messages → Small or
Large setting takes precedence over pack message size; Default uses the pack.
A person's saved sidebar drag width takes precedence over pack sidebar width.
Import/reset never erases either personal preference.

### Motion

`motion.transition-ms` and `motion.popup-ms` are integers **0–1000** inclusive;
defaults are **120** and **100**. `motion.reduced-motion` defaults to false. When
true, the shared `Theme.quick` hover/color transitions and `Popup.duration`
open/close animations become zero-duration. This does **not** disable every
hardcoded animation in the application or change media playback. GIF/emoji/
sticker playback remains controlled by the separate Messages setting.
No custom animation code, keyframes, timers, or expressions are supported.

### Local titlebar logo

`logo` is a **relative forward-slash `.png` path, 1–160 ASCII bytes**, resolved
inside the explicitly selected pack's folder. Example: `logos/orchid.png`.
Each segment may contain ASCII letters/numbers, spaces, `.`, `_`, `-`, but cannot
be empty, `.`, `..`, hidden (start with `.`), end in dot/space, or name a Windows
reserved device (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, even
before another extension). Absolute paths, backslashes, colons/ADS, URLs, UNC,
symlinks and junctions are rejected, including ancestors of the selected pack.
The selected pack and logo must both be regular files, never devices, FIFOs or
sockets; the pack cannot use UNC/device path syntax.

Reads are pinned to verified filesystem handles, not a canonical-path preflight:
Linux uses `openat2` beneath a held pack-directory descriptor with no symlink or
mount traversal for relative files. It acquires an `O_PATH` inode reference,
checks that it is regular, then reopens **that inode**, not the mutable pathname,
through its held genuine procfs fd directory with `O_NONBLOCK`. The only followed
link is this application-owned kernel fd-link, never a pack-supplied symlink.
This requires **Linux 5.6+**, permitted `openat2`, and `/proc/self/fd`; otherwise
loading fails closed. Windows opens each directory without following reparse
points and holds every ancestor without write/delete sharing until the bounded
reads finish, preventing rename/reparse swaps. Files being actively written may
therefore fail with a sharing error. Bounded reads use the verified file handle.

This is pathname confinement, not origin/authenticity verification: regular hard
links are allowed, and Linux files can be modified in place by another writer.
The bytes actually read must still pass all JSON/PNG limits before application.
Regular filesystem/storage operations can still wait on I/O (including remotely
backed mounts); `O_NONBLOCK` does not make disk I/O real-time or certify privacy.

Logo limits: **1–262,144 encoded bytes**, **1–512 pixels on each axis**, decoder
allocation limit **4 MiB**. Dimensions are checked from PNG headers before pixel
allocation. APNG, GIF, SVG, JPEG, WebP, fonts, and executable assets are rejected.
The static pixels are contained in the existing custom titlebar brand area;
caption buttons, drag/double-click behavior, resize targets and close-to-tray
callbacks remain native. A system titlebar is not replaced. Reset logo restores
the Harmony wordmark without changing other pack values. A later Reload reads
the source again and may restore its logo.

## Saving, editing, reset and sharing

Import validates the entire chosen file and relative asset, then saves a pack
and PNG **snapshot in `layout.json`** before applying it. Restart/recreated windows
use that snapshot without reading source paths. Missing or changed source files
do not break saved appearance. An invalid saved pack is ignored without losing
unrelated preferences. Imported files and their source assets are never edited.

- **Edit pack colors** uses the shared live color editor. Save changes the saved
  pack snapshot (colors/dark/name only), preserving layout, typography, motion,
  logo and reload source. Cancel/leave Appearance restores the saved pack,
  including the Harmony default when no theme was explicitly selected.
- **New theme** copies colors into the existing standalone custom palette system.
  Saving it clears pack style overrides; cancel preserves the pack. Existing
  custom IDs/storage remain unchanged.
- **Reload** replaces the snapshot with the source's current contents, discarding
  snapshot-only editor/logo changes after successful validation and persistence.
- **Reset pack** returns to that pack's base palette and native style defaults.
  Choosing any built-in or saved custom palette also clears all pack overrides.
- **Export** writes the current saved pack, or a color-only pack for the selected
  built-in/custom palette. Personal sidebar/message-size preferences are not
  included. With a logo, export creates a new `harmony-logo-*.png` sibling and
  references it relatively. The new PNG is flushed and finalized (including
  clearing Windows temporary attributes) **before** atomic JSON replacement.
  A finalization failure leaves the old export untouched; a manifest-write
  failure removes the new logo and preserves the old JSON/logo. If filesystem
  errors also prevent cleanup, the error names the leftover new logo. Share the
  JSON **and its PNG** together. Export never overwrites another logo; repeated
  successful exports or a crash can leave older/unused logo siblings.
  Export does not change the source used by Reload.

Validation/read/save errors are shown in Appearance and preserve the current
visible palette, style and saved selection. Saves use a flushed same-folder
temporary file and atomic JSON replacement; a persistence error is not reported
as success. This is not a guarantee against filesystem/device failure or power
loss. No additional background work runs after an action completes.
