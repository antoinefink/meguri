# Harvest coverage owners

Spec: conformance/harvest-followups
Requirements: R1877-R1882, R1889, R1907, R1960
Anki: 26.09.3
Scenarios: conformance/catalog/harvest.csv

## H1. Classification and completion

The U4 supplement classifies the remaining 480 catalog rows from individual
behavior review at the pin. It leaves the earlier 699 rows unchanged. A
classification is a scope decision, not evidence that the behavior passes.
`deferred` retains a required coverage obligation under `conformance/dsl` D14.
It does not waive R1880 or permit a release with missing required coverage.

Each newly deferred row names its unfinished provider or assertion owner and
the behavior still to cover. U246 owns missing differential scenarios when an
accepted implementation already provides the behavior. U104 owns the final
Stats matrix and U91 the search result corpus. The named provider units own
their real operations; none may substitute fixture writes for those operations.
Any related existing scenarios must be checked for the whole behavior before
they are linked. Golden expectations come from oracle runs, never from the
harvested assertions. Native UI obligations require the corresponding native
tests as well as any engine observations.

## H2. Earlier deferred rows with incomplete owner tracking

This table adds ownership only. It changes neither the classification nor the
status, reason or scenario links of the baseline rows. An owner must add the
missing proof and then update the catalog. An already accepted provider is not
a reason to mark a row done.

| Harvest id | Unfinished owner | Coverage obligation |
|---|---|---|
| h0f4f7e722f1e | U107, U120, U246 | Incoming preset identity is retained only when the destination id is free; prove occupied-id handling. |
| h1df457b7a669 | U246 | Prove scheduling uses built-in defaults when both the requested and default presets are missing. |
| h3d8240dabf39 | U107, U120, U246 | Prove incoming legacy preset JSON is added through the real sync/import writer. |
| h3dc2e6c7647a | U246 | Prove a deck with a missing preset schedules using the default preset. |
| h69f9ba4c6483 | U107, U120, U246 | Prove the legacy incoming-preset path creates the required preset record. |
| h6a74f5728f8c | U246 | Prove lookup of a missing preset falls back to the stored default. |
| h774fd17fd8ea | U107, U120, U246 | Prove incoming preset ids are not reused over an occupied destination id. |
| hd3586154cc8d | U49, U246 | Add a real preset-removal undo scenario that restores the preset and affected assignment. |
| hd709478326d6 | U246 | Observe missing-id fallback through the real scheduling provider, not merely decoded defaults. |
| hf1fc2f580d06 | U107, U120, U246 | Prove free incoming preset ids survive the complete transfer. |
| haa85eb491ea1 | U232, U246 | Cover duplicate-name preset creation and its timestamp-derived unique name in the options flow. |
| h25a2a4a1f100 | U41, U246 | Add deck-tree count observations from the real queue/count provider. |
| h26a096646ed7 | U156, U195, U246 | Verify pickers omit the empty default deck where the picker contract requires it. |
| h34d675666d1c | U156, U195, U246 | Verify empty-default filtering in the second picker/listing path. |
| h5e505e6b3fb6 | U156, U195, U246 | Verify a picker that excludes filtered decks does not offer them as normal destinations. |
| h8673919e1db4 | U41, U246 | Prove deck-tree counts respect active daily limits. |
| hdddcc971bba4 | U41, U246 | Prove hierarchical deck counts through queue-backed tree observations. |
| h151bd9c1b80b | U48, U246 | Cover current/original/saved new position through real Reset and Set Due Date operations, including filtered home due. |
| h095feffe1089 | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| h2a067e44c2ac | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| h2dc9493eef0e | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| h7f11536c09e7 | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| hb2bdcde89b3a | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| hf42c35381599 | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| h1f6151e74934 | U64, U246 | Add actual IO card-markup observations for clozes with several numbers; U63 shape decoding alone does not close it. |
| h3448f09e0c2c | U64, U246 | Add the IO element/render-data observation; accepted storage-format parsing is not markup proof. |

## H3. Display proof and original graphics

The six display rows above still need an explicit assertion route. U246 owns
the per-row mapping to observable marker recognition, hidden-button behavior
and native action semantics. U66/U67 own the corresponding page and native
handler integration. The oracle's card HTML is not the final display transform,
so a render observation alone must not be called full coverage. The owner must
identify the exact assertion for each row before changing its status.

`render/display-transform` D4 and revised R526 define Meguri's original replay
graphic. Keep that independent design and R1907's approved replay-markup
normalization. No third-party graphic, CSS or JavaScript asset may enter a
scenario, expected snapshot or implementation as part of this follow-up
(R1960). The original graphic has its own explicit assertions; interoperability
checks retain the outer anchor, native action, numbering and visibility
contracts. This tracking document adds no comparison masking or allowlist.
