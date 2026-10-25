# Bounded field-rename prerequisite for U107

`specs/notetypes/field-rename.md` reuses the neutral contract authored in
`b70649df`, `specs/notetypes/structure-changes.md` S1-S4, S7-S9, S13-S14.
The retained spec/U34 packet contains its prepared reference provenance and
golden expectations. No new reference source was read for this slice.

The bounded identity contract uses current U29 AggregateDigest and the current
plan's retained-ID requirement, with captured ordinals for unchanged-order
legacy fields. Full U34 identity/remap and schema-gate work remains outside it.
The early prerequisite does not accept U34 or change its M1 milestone.

`engine/change-stream` S1.1 is Meguri-owned operation metadata for U107's
already specified skipped-ID reporting. It adds no Anki behavior claim.
