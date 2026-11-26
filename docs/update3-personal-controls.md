# Harmony Update 3: presence and personal controls

This source candidate targets app version 1.0.2. Dirty development builds use build ID 0; a positive release build ID requires the reviewed, clean commit and the existing package pipeline.

## Received platform indicators

Friends, members and profiles display the desktop, web and mobile statuses actually received from Discord. Several clients can appear together, including their different statuses. Missing or unrecognized client fields are unknown, not an inferred desktop session. Offline/invisible events and account changes clear platform evidence. The app does not discover devices or probe a contact's computer. DM accessibility labels include the same received evidence.

The open card and DM panel use the same received-contact platform authority, separately from their legacy availability dots. Nonfriend DM conversation observations replace this authority too. The gateway dispatcher assigns a common receipt order before member-list and tracker dispatch. Each contact carries its own receipt through coalesced snapshots and member metadata rebuilding; explicit contact removals retain their own receipt too. Neither a later publication nor another person's update or a nickname/avatar change can restamp older evidence. An older member event cannot replace a newer contact snapshot, and a genuinely later same-value presence can restore platforms after a clear. Closing and reopening a card retains its latest contact evidence within the account lifetime. Retired provider/session receipts are rejected. These ephemeral receipts are not persisted or uploaded; cached member names and legacy dots remain available without granting live platform authority.

## Call duration

The call origin follows received connected/reconnecting voice transitions, before render batching. Leaving or disconnecting retires the origin; changing account retires its authority. A hidden or recreated window attaches to the existing monotonic origin rather than deriving a new session from a retained room name. A hidden window runs no periodic call-render timer. The displayed duration is `minutes:seconds`, then `hours:minutes:seconds`.

## Independent voice and stream volume

The native per-person volume menu keeps independent voice and stream gains, with Discord's curve through the slider's 200% endpoint. The exact field accepts higher whole percentages and commits with Enter or Set. Harmony rejects invalid entries and non-finite gains. It stores committed gains locally and restores them across window recreation. Voice and stream have separate defaults.

## Device-local contact timezones

A profile's timezone editor stores an explicitly chosen IANA name, scoped by provider-qualified owning account and contact. It never uploads contact IDs or modifies a remote profile. UTC instants are converted with bundled IANA rules, including DST transitions and fractional offsets. Missing and invalid saved names are labelled honestly. A failed local storage write leaves the previous preference intact. Stale editors cannot save for a different account/contact. A minute timer runs only while an annotated profile is visible and stops on window detach.

## Consistent emoji artwork

The original licensed Twemoji PNGs remain the source for the picker and inline font. The original 72px artwork strike remains first, preserving index/glyph ordering and original picture lookup. Additional premultiplied-alpha filtered strikes retain transparent margins when filtering smaller text. This trades a larger bundled font for stable rounded edges at text sizes; fidelity thresholds are not relaxed. Picker hover/footer state keeps metadata rather than pinning tile bitmaps, and no-match searches clear stale footer state. Native picker search, accessible selection, layout and release are verified with in-memory Slint, not physical desktop input.

## Verification boundaries

Fixtures use generated media, fake monotonic/wall clocks, anonymous command transport and empty owned data roots. They do not start a real Discord service, read an account store, touch real calls/devices or change OS volume. Native Windows tests, source/metadata checks, Linux module checks, Linux desktop builds and live/physical presentation remain separate gates. The durable implementation report records the exact executions and outstanding release gates; this document is not a claim that an installer has shipped.
