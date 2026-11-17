# Harmony branding

## Product identity

- Product: **Harmony**; independently versioned from its Yapfast upstream.
- Windows application: `Harmony.exe`; Linux installed command: `harmony`.
- Desktop/application ID: `io.github.TimeShallHalt.Harmony`.
- Own link scheme: `harmony:`. Legacy `yapfast:` links remain readable, but the
  fork does not register over the stock app's own scheme.
- Source: <https://github.com/TimeShallHalt/harmony>.
- Upstream: <https://github.com/yapfast/yapfast>, baseline `v0.4.1`.

## Artwork

The linked H is original geometric artwork: two balanced voices connected by one
flowing stroke. `ui/icons/logo.svg` is its theme-colored monochrome version;
`packaging/icon.svg` is the full application icon. Its lavender (`#A79BFF`), mist
blue (`#A5C8F5`) and mint (`#79E5CE`) sit on indigo (`#15192D`).

Keep the UI on semantic theme roles. Do not force these colors over someone's
saved theme, replace account avatars, or remove a custom theme-pack logo.

Regenerate the ten native Windows ICO sizes and large PNG from the SVG with:

```sh
uv run --with resvg-py==0.5.0 --with pillow python scripts/generate-harmony-art.py
```

The tray, window, Explorer, Windows toast, installer and Linux app menu must share
this artwork. Unread notification dots are separate overlays, not part of the mark.
The artwork is part of this GPL-3.0-or-later source tree.

## Compatibility is not branding

Keep internal `yapfast-*` crate names, Slint type names and existing saved theme
IDs stable. Keep the legacy OS credential-store service identifiers to avoid
invalidating login credentials. The previous Discord-handler rollback record
also remains readable, so turning the setting off can restore the user's prior
handler. None of these identifiers is a reason to show the upstream brand as
this fork's product name.

Do not remove or relabel upstream authorship, third-party service logos, font
notices or licenses. Required legal/source information stays accessible from
About/diagnostics and accompanies distributed artifacts.

## Updates

The stock Yapfast updater is not wired into Harmony. It must never offer a stock
upstream binary as a Harmony update. A fork-specific signed release and hosting
policy must be agreed before implementing automatic updates.
