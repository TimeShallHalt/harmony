# Harmony Roadmap

| Update | Focus |
| --- | --- |
| **7 · Orbs** | Quests, the Orbs Shop, and free local enhancements for streaming and profiles |
| **8 · Customization Heaven** | Custom wallpapers, visual theme editors, and shareable theme packs |
| **9 · Plugins** | Optional built-in plugins and a path for community-created extensions |

## Update 7 · Orbs

Give users a place to discover Quests, track rewards, and browse the Orbs Shop. Add free streaming and profile customization that works within Harmony’s existing capabilities.

- **Quests.** Show available, active, and completed Quests with eligibility, progress, expiry, and reward status. Let users opt in and claim rewards through supported flows. Use Discord’s reported state for progress and balances.
- **Orbs Shop.** Show the current balance, browse eligible rewards, preview items, and confirm redemptions. Keep the displayed balance and purchase state in sync after success, rejection, or an interrupted request.
- **Free streaming enhancements.** Expose HD quality settings where the capture pipeline and session support them. Let users choose resolution and frame rate, and show the quality actually in use. Honor service limits.
- **Local profiles.** Give users a profile editor and preview for custom fonts, banners, decorations, and colors. Store those overrides on the device. Nitro-exclusive appearance changes are visible only to the user who sets them. Other people continue to see the profile served by Discord.

**Ready when** users can follow an eligible Quest through to its reported reward, redeem an eligible Shop item with confirmation, see the active stream quality, and verify that local profile changes stay on their device.

### Implementation on the 1.2.0 baseline

**1.3.0 build 1 is the Windows test candidate.** Local profile acceptance is verified on Windows. Final acceptance covers a hardware stream observed by a second client, a live Quest reward and a confirmed Shop redemption on an eligible account.

| Slice | Where to use it | Acceptance |
| --- | --- | --- |
| Local profiles | **Settings → Local Profile** previews the same card used by the account popover and profile views. Save stores the font, colors and still PNG artwork under the current account in Harmony’s data directory. | **Verified in native UI tests.** The saved appearance survives reopening, stays with its owner and emits zero backend commands. A subsequent Discord display-name edit carries only that change. |
| Streaming | Choose resolution and frame rate in the existing **Share your screen** menu. The **Stream** tab in the call details shows encoded dimensions and complete frames sent per second. | **Verified in core, UI and loopback tests.** The tier limits apply to remembered choices. Five video frames and one retransmitted packet counted as five frames. The readout clears on stop. A hardware capture and remote viewer still need verification. |
| Quests | **Settings → Quests & Orbs** reads available, excluded, active, completed and claimed Quests. Discord handles opt-in, task tracking and reward claims. Returning to Harmony refreshes the account’s reported state. | **Verified against loopback Discord.** Discovery, task counts, completion, claim history, claimed Orbs and an expired claim window matched the service responses. End-to-end acceptance needs an eligible account and Quest. |
| Orbs Shop | The same page reads the wallet, catalog and collectible ownership. Item details lead to Discord’s item preview and final confirmation. Harmony reads the balance and collection again on return. | **Verified in native UI and loopback tests.** Opening checkout leaves the balance intact. A refresh reflects confirmed ownership, a declined redemption or an unavailable wallet. A real redemption requires an eligible offer and explicit approval. |

Quest and Shop data depend on the consumer service making each read available to the signed-in account. Missing sections show as unavailable. Discord controls regional eligibility, available rewards, the final price and fulfillment. [Discord Quests FAQ](https://support.discord.com/hc/en-us/articles/22225719947543-Discord-Quests-FAQ) · [Discord Orbs FAQ](https://support.discord.com/hc/en-us/articles/30593690165783-Discord-Orbs-FAQ)

## Update 8 · Customization Heaven

Make the visible Harmony interface editable through the app. A user should be able to change the whole look in a minute or spend an evening tuning a single panel.

- **Custom wallpapers.** Import still images and animated wallpapers. Control fit, position, dimming, blur, and where the wallpaper appears. Offer a pause control and a still fallback for reduced motion.
- **Simple theme editor.** Start with a preset and change the wallpaper, accent, background, text, and overall corner roundness. Preview changes live and apply them together.
- **Detailed theme editor.** Edit visible surfaces and components individually, including the title bar, sidebars, chat, composer, menus, dialogs, and controls. Expose colors, typography, spacing, corner radii, outline color and width, shadows, and opacity. Include states such as hover, focus, selection, and unread. Let users select a visible element in the preview to find its controls.
- **Theme management.** Create, duplicate, rename, import, and export themes from the GUI. Let users undo changes, reset individual properties, and restore a readable theme if an edit makes the app difficult to use.
- **Shareable packs.** Export one versioned Harmony theme file with the settings and wallpaper assets bundled. Preview imported packs before applying them, and use built-in defaults for fields an older theme omits.

**Ready when** a user can build a theme with a custom animated wallpaper, style individual components, export the result, and import it on another installation with its appearance and assets intact.

## Update 9 · Plugins

Give users a Plugins page where they can enable optional features independently. Start with built-in plugins to establish the controls and extension points. Use those lessons to design and test a community plugin format for Harmony.

- **Plugin manager.** List installed plugins with descriptions, settings, permissions, and version compatibility. Make enable, disable, and removal easy. Provide a recovery mode that starts Harmony with plugins disabled.
- **Built-in plugins.** Use a **Message Logger** as the first example. With the plugin enabled, save edits and deletions that Harmony observes locally and let users review that history. Offer conversation exclusions, retention limits, and a clear-history control. Keep logging off by default and scope the history to the account.
- **Community authoring.** Explore a versioned plugin API with documented events, settings, and supported UI locations. Provide a sample plugin and a local development workflow. Test permission prompts and isolation before widening distribution. A public catalog can follow once installation, updates, and removal are dependable.
- **Release scope.** Ship a useful set of built-in plugins first. Include community authoring in Update 9 if the API, permission model, and recovery flow are ready. Harmony plugins use Harmony’s own format and API.

**Ready when** built-in plugins can be enabled, configured, disabled, and removed independently, and a failing plugin can be disabled through recovery mode. If community authoring ships, an external developer should be able to build and install the sample plugin using the published documentation.
