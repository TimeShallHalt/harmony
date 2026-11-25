# Discord Android design reference

Examined November 21, 2026 for the Harmony Android redesign.

## Reference build

The reference is the signed Android package **com.discord**, version **354.12 - Stable**, build **354012**. The downloaded split set contains the base app, the ARM64 native libraries, English and French resources, and hdpi drawables. Android Build Tools 35's `apksigner verify --verbose --print-certs` verified all five APKs. Each certificate matches the fingerprint Discord publishes for `com.discord`. The release package declares minimum API 24 and target API 36. [Android store listing](https://play.google.com/store/apps/details?id=com.discord) · [APK distributor](https://apkpure.com/discord-talk-play-hang-out/com.discord) · [Discord's published certificate fingerprints](https://discord.com/.well-known/assetlinks.json)

| Artifact | SHA-256 |
| --- | --- |
| XAPK containing the split set | `06b340f4929de1b0ee9ea4bfa02a834dd36356c049c2f50dc527eddf4134911f` |
| Base APK | `15407f2d5431166247abfb6c8edbeb368b84ae2b17ef57e4b851db29066f7167` |
| Verified signing certificate | `3c39d23cf9367849a5c699395647fe0e5bfea5a1f1f40d8c717ddc70f8bfa113` |

The local reference files are under `.local/discord-reference/354012/`. The directory includes `com.discord.apk`, the original XAPK, individual split hashes in `verified.json`, acquisition URLs in `source.json`, and the exported REA record in `rea-archive-evidence.json`.

The obtained APK set contains native libraries for ARM64. On November 21, both public download requests with `arch=x86_64` and `nc=x86_64` resolved to the same 183,696,714-byte ARM64 XAPK as the unqualified download. Divxland’s variant catalog for 354.12 lists ARM64-v8a and armeabi-v7a. Aptoide’s public package metadata did not return a downloadable split. The APKMirror and Uptodown release pages were unavailable from the build host. An x86_64 split for build 354012 could not be verified through those sources. [APKPure package download](https://d.apkpure.com/b/XAPK/com.discord?version=latest) · [Divxland’s release variants](https://discord.en.divxland.org/download/) · [Aptoide package metadata](https://ws75.aptoide.com/api/7/app/get/package_name=com.discord)

## Measurements from the package

REA's archive inspection enumerated all 9,295 entries in the base APK. The retained inspection is `ev_e108723291246ff3771ebf01b4ea7810ad225349697e3fed29d2df38c543f019`. Its inventory is `ev_0c73335bf9df7707fadb3fccc5602723baa57b1c04afb6a5d07be986299cbda1`. Android's `aapt2 dump resources` and `aapt2 dump xmltree --file` decoded the resource values and layouts below against that same digest.

| Resource | Observed value or structure |
| --- | --- |
| `message_view.xml`, `message_start_guideline.xml` and the corresponding dimensions | Avatar 40 dp, left inset 12 dp, message column starts at 64 dp. The horizontal gap is 12 dp. Message accessories use 8 dp vertical spacing. |
| `message_view_reply_preview.xml` | A channel-spine view precedes a compact line containing an avatar, author and message content. |
| `reaction_view.xml` | The reaction contains an emoji slot and count text. |
| `chat_input_root_view.xml` | A touch-focusable, multiline native text input with 4 dp internal padding. |
| `attachment_media_mosaic_container_view.xml` and media dimensions | A recycling container for a media group. The resource table specifies 4 dp gaps, 2 dp interior corners, 16 dp outer corners and an 8 dp radius for a separate attachment. |
| `color/brand_500`, `color/brand_560` | #5865F2 and #4752C4. The app contains multiple palettes. Which neutral color a screen uses depends on the chosen theme and state. |

## Navigation and touch behavior

- Discord's June 2026 mobile screenshots show the Messages entry and servers in a vertical rail. The Messages header includes search, Add Friends and compose controls. The populated example places activity cards above the conversations. The floating identity bar spans the bottom of the navigation surface, with avatar, presence, name and a trailing bell. [June 2026 mobile screenshots](https://support.discord.com/hc/en-us/articles/40953848660759-You-Bar-FAQ)
- Tapping the identity bar opens the profile. Holding the bar opens account/status actions. Holding the avatar opens Settings. A left-to-right swipe on the bar toggles DMs and the current server. Tapping the bell opens notifications. [Discord's You Bar design notes](https://discord.com/blog/you-bar)
- A right-to-left swipe across a message starts a reply. Tapping the conversation title opens details such as members and pins. Server selection keeps the guild rail and opens the channel list beside it. [Discord's mobile layout guide](https://support.discord.com/hc/en-us/articles/12654190110999-New-Mobile-App-Updates-Layout)
- Holding a message opens actions and recent reactions. A tap on an existing reaction joins its count. The composer smiley opens the emoji, GIF and sticker selector. [Discord's mobile messaging guide](https://support.discord.com/hc/en-us/articles/360046618751-Getting-Started-on-Mobile)

The measurements above come from the signed build. The navigation and gesture descriptions come from Discord's published demonstrations. Authenticated reference screens, server-selected experiments, entitlement checks and live backend behavior were outside this inspection. The REA server lacked its optional JADX provider, so no method-level decompilation was used. Harmony's own emulator tests establish which interactions work in its implementation.
