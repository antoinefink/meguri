# Draft: scope CloudKit type changes to notes

Status: **Under review (R2066); inactive.** This packet proposes a contract amendment. It changes no approved specification, implementation, unit status or acceptance result.

Source/start: `06f93ebf000fe0cb6262d88d43af5a78fd86d16a`, main checkout on `master`, 2026-11-02. Role: spec-writer. Sources: the Meguri specifications linked below. No reference implementation, oracle, private input or Anki source was read.

## Decision to review

Permit different field and template maps for different notes within one joined generation. Preserve valid offline operations and atomic generation adoption. Scope each type change to its affected notes; cards use their note's scope through `nid`.

This proposal concerns changed-layout operations. It does not block initial reviews and sync with unchanged layouts. It does not close Q11 or supply implementation, signed integration or final acceptance evidence.

## Reachable counterexample

Start offline with distinct standard note types A and B. Both have fields `Front(f1), Back(f2)` and one template with the same identity. Notes n1 and n2 both use A. Field and template identities match across the types. Neither type is a merging copy. No claim batch is handed during these operations.

1. Import a package with Merge note types enabled and Update notes `Never`. Its A and B both have `Front(f1), Back(f2), Extra(f3)`. Its note sharing n1's GUID uses B; its note sharing n2's GUID uses A.
2. After the import, obtain a fresh Change Note Type info result for n2. Change n2 from A to B with field map `[1,0,2]` and template identity. Supply the current schema stamp and let the required safety backup succeed.

The first operation grows both types without reordering existing fields. It rewrites n2 under A's larger layout, and moves only n1 from A to B. Update notes `Never` prevents a later package-note update from replacing these values. A retains n2, so the import does not remove A. The generation has both types as anchors and n2 as a member.

These effects follow [J1–J5](../../specs/package/notetype-merge.md#j1-which-note-types-merge), [J6](../../specs/package/notetype-merge.md#j6-stage-2-notes-whose-note-type-differs), [J8–J9](../../specs/package/notetype-merge.md#j8-removing-merged-note-types), and [I9](../../specs/package/apkg-import.md#i9-notes-r1602-r1603). The second operation satisfies [M7–M8](../../specs/notetypes/change-notetype.md#m7-requests-that-change-nothing): its types exist, its maps are valid, n2 still uses A, and its schema stamp is fresh. [M9](../../specs/notetypes/change-notetype.md#m9-the-change-effects) permits the chosen field swap.

The second operation acts on the first generation's anchors and shares n2 as a member. [K19.2, Joining](../../specs/sync/cloudkit-records.md#k192-identity-and-manifest) requires one joined generation. It prohibits splitting that generation and multiple unclaimed memberships.

| Note | Original A → final B field map |
|---|---|
| n1 | `[0,1,-1]`: preserve Front and Back; Extra has no original value |
| n2 | `[1,0,-1]`: swap Front and Back; Extra has no original value |

Both maps have the same old and new type IDs and endpoint layouts. No single map represents both. [K19.2](../../specs/sync/cloudkit-records.md#k192-identity-and-manifest) allows one `TypeChange` per pair, with no note scope. [S2.5](../../specs/engine/change-stream.md#s25-structure-reports-u113-u114-synccloudkit-recordsk192) also omits that scope. [K24](../../specs/sync/cloudkit-records.md#k24-changes-other-specs-need) supplies no exclusion.

This is a representation gap. Refusing an incompatible pair avoids a guessed map but does not deliver the required operation. Sending final values alone is insufficient: [K15.1](../../specs/sync/cloudkit-records.md#k151-the-merge) and [K19.5](../../specs/sync/cloudkit-records.md#k195-adopting) require correct maps for pending edits before merge.

## Proposed semantic amendment

The amendment would preserve existing operation validation, backups, claim order and atomic adoption. It would add these obligations:

- Structure reports identify the notes affected by each type change, including a same-type remap. Tracking receives scope from the operation; it does not infer maps from row values.
- The manifest may list the same source and destination type more than once when their maps differ. For every note, the recorded steps must determine exactly one resulting map. A shared scope is allowed when those steps have identical endpoint layouts and maps.
- Joining composes each note's path in operation order. Type-wide layout maps apply before and after its moves as appropriate. A second move affects only its selected notes. Disjoint scopes do not acquire each other's maps.
- Cards follow their note through `nid`, including template ordinals and the existing cloze prefix-map rules. A card without a resolvable note cannot guess a scope.
- The recoverable ID-change operation must update every scope, member and mapping reference. Its durable intent must settle before sending resumes. It must preserve entity origins and keep each note attached to its own map through encoding, decoding and replay.
- Tracking durably retains scopes, maps and path order across observer replay, reopen, claim retry and incoming-apply recovery. Existing lost and abandoned generation rules retain enough scoped history for their inverse or restoring maps.

Exact report types, canonical scope order, duplicate handling, and the treatment of deleted or newly created notes remain review items. The amendment must define them before implementation.

## Bounded representation and compatibility

K7 caps the encrypted generation envelope at 786,432 bytes. A list of 1,000,000 per-note scopes and their maps may be larger. The member asset defined today contains no such metadata. The revised wire form must keep this data within the specified envelope and asset bounds. See [K7](../../specs/sync/cloudkit-records.md#k7-sizes-and-limits) and [K19.2](../../specs/sync/cloudkit-records.md#k192-identity-and-manifest).

The wire amendment needs a bounded, checksummed asset representation for variable-sized generation metadata, including scopes and maps. A format-versioned extension of `ma` is one candidate. Its encrypted envelope would identify the representation and bind its exact byte length and digest. The asset would use existing CloudKit asset encryption. No new plaintext field, record name or content-bearing identifier is proposed; [K3](../../specs/sync/cloudkit-records.md#k3-record-types-and-fields) and [K8](../../specs/sync/cloudkit-records.md#k8-encryption-and-metadata) remain constraints.

Before claim, the builder must measure the complete encoded envelope and request. Externalizing members alone must never permit oversized maps to overflow `e`. Readers must validate size, digest, canonical form and all scope references before adoption. Missing assets wait; corrupt assets cannot produce a partial apply. Decode and staging require explicit allocation and byte limits.

The exact asset schema, field numbers, format value, inline threshold, and behavior above the asset limit are **open**. K7's 104,857,600-byte asset assumption must be respected. Review must choose a bounded refusal or another bounded representation above that limit. Neither truncation nor splitting a joined generation is an implicit fallback.

The amendment requires an explicitly unsupported format for older readers. They must retain the generation and dependent members without applying or overwriting them, and report unsupported schema. Unknown-field tolerance must not erase scope and apply a pair-wide map. The change must extend and test [K5.4](../../specs/sync/cloudkit-records.md#k54-newer-formats) and [K20](../../specs/sync/cloudkit-records.md#k20-refusals) for control manifests as well as entities. Compatibility of every supported older reader is a review prerequisite, not assumed from its entity handling.

## Required regression and next boundary

The end-to-end fake-transport test must execute the two operations above through real reports and tracking. Use distinct values and receiver edits in Front on both notes. Reopen the sender before claim. Assert one durable generation, successful publication and atomic adoption, with n1 preserving field order and n2 swapping it. Each receiver edit must reach its correct destination. Assert card identity and scheduling preservation with the identity template map.

Cover two successive moves of one note, different template maps, ID changes followed by reopen, and a repeated claim. Force the asset path with a large scope and check the complete byte budgets. For missing, short or corrupt files, and unsupported formats, require zero partial collection writes and no replacement of server content. Include the limit behavior chosen during review.

The R2066 contract review must settle the open wire and compatibility choices before any approved specification or code changes. This packet creates no additional human approval requirement. Initial unchanged-layout review and sync work continues independently; Q11 retains its existing evidence requirements.
