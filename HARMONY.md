# Harmony

Harmony is Ren's personal Yapfast-based Discord client. Name chosen by Ren as the counterpart to discord. This is a local development brand, not a claim of trademark clearance, domain availability, or Discord affiliation. Public name, trademark, domain and store screening remains to be done before distribution.

## Identity

Harmony 1.3.0 adds device-local profile customization, measured streaming quality, Discord Quests and the Orbs Shop. It builds on Harmony 1.2.0 and Yapfast 0.4.1, including signed Windows updates, desktop Rich Presence, profile activity history, local message scheduling and voice recording.
Windows builds produce `Harmony.exe`. Linux
packages install `harmony`. Internal crate names and legacy credential-store keys
remain stable compatibility identifiers, not product branding.

The original mark is a linked H: two balanced vertical voices connected by one
flowing stroke. The application icon uses lavender and mint on deep indigo; the
in-app monochrome version follows the selected theme's semantic color. Explorer,
window, tray, notification and installer icons use the same source artwork.
See [branding](docs/branding.md).

## Preserved foundation

- A custom native titlebar integrated into the existing Rust/Slint desktop app.
- Harmony user-facing branding without renaming internal crates or invalidating saved credentials.
- Retain upstream license, dependency notices and attribution.
- Both Windows and Linux remain supported targets. Platform-specific behavior belongs in existing OS adapters, not shared UI.

## Theme contract

Use the existing live `Theme` semantic color tokens and appearance editor. Do not hard-code brand colors into individual components. Window chrome must follow the same theme as the content, including light themes and inactive state. Centralize spacing, control dimensions and typography in the shared client style rather than duplicating constants. Close/destructive actions use semantic danger colors.

Preserve all existing built-in themes and custom theme persistence. No new permanent palette is selected for this first milestone. Future changes should extend semantic roles rather than tie component names to hex colors.

## Voice and artwork

Calm, clear, friendly. Prefer short action labels and specific error messages.
The Harmony mark replaces upstream application artwork, not user avatars or
third-party service logos. Keep credits and legally required notices accessible.

## Acceptance checks

- Native build and account-free preview run.
- Harmony is shown in the window/titlebar and primary app-facing branding.
- Window can be moved, resized, minimized, maximized/restored and closed.
- Titlebar follows live theme switching without restart.
- No account login or credentials are needed for preview testing.
- Linux native build/test results must be reported separately from Windows results; neither platform's launch validates the other.

Editable source and installed application are separate. An edit is not deployed until a new executable is built and verified. Preserve the stock installation as a rollback baseline until the custom build is ready.
