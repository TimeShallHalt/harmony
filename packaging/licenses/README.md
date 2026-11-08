# Supplemental dependency notices

`dependency-licenses.json` binds each supplemental file to its SHA-256 and source
URL. The packager validates those hashes and fails if any dependency has neither
an original bundled notice nor a mapped supplement.

Most supplements are unchanged license files fetched from the repository commit
recorded by the exact published crate's `.cargo_vcs_info.json` (or the pinned
Cargo Git dependency revision). The original author/copyright notices are kept.

Eight published crates have no standalone license file at their recorded commit:
from_variants 1.0.2, from_variants_impl 1.0.2, hpke-rs 0.6.1,
hpke-rs-crypto 0.6.1, hpke-rs-libcrux 0.6.1, hpke-rs-rust-crypto 0.6.1,
realfft 3.5.0 and serenity-voice-model 0.3.0. Their unchanged published
`Cargo.toml.orig` declarations/authorship are included, alongside the appropriate
standard license text from SPDX license-list-data v3.26.0. These standard texts
are labelled as such; no upstream copyright notice/year has been invented.
Their complete published sources are retained in
`vendor/corresponding-source/` and included in the accompanying source archive,
including the MPL-covered hpke sources. They are provenance/source copies, not
new Cargo path overrides or modified implementations.

This preserves the upstream declarations and source notices; it is not a claim
of independent legal advice. The application's GPL-3.0-or-later license remains
unchanged.
