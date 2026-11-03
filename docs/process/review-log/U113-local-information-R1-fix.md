# U113 local information: R1 repair

Fix input: runner-supplied, independently reviewed own-code seed from
`source4e070760`. Original U113: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Role: fixer under the implementer rules; session identity remains runner-owned.

The existing seed and tests remain. Tests were added before the two production
repairs, in `meguri-host/tests/cloud_publication_phase.rs`:

- `local_information_counts_isolated_revision_after_dirty_mark_loss_without_mutation`
- `local_information_filters_other_container_and_environment_copies_before_decoding`
- `local_information_filters_salvage_copies_before_decoding`

The first removes only a captured note's dirty mark after real publication,
edit and restore isolation. It requires a remaining revision gap and nonzero
count, unchanged collection/sidecar/tracking bytes, entity facts including ACK
and flights, runtime status, pending sendables, observations, emitted update
ids, adapter guard, service calls and key draws across repeated reads.

The other two retain two current unresolved copies and one resolved copy, then
add malformed foreign `ConflictCopy` JSON shapes in another container and
environment, or under legacy and separate salvage owners. The complete local
DTO must equal its pre-injection value; durable bytes, pending sendables,
runtime status and service calls must remain unchanged. Fixtures use the
existing test-only raw storage API under the tracking gate, after runtime open
so the global damage probe does not consume the deliberately malformed rows.

Production now uses the conservative dirty-mark-or-revision-gap predicate and
a scope-filtered metadata count. The query keeps ordinary scoped bindings as
the outer loop and looks up their copies by binding, without decoding copy
payloads or returning identifiers. The read-only freshness equality and
unknown/no-runtime paths are unchanged.

One source consistency pass completed. No build, formatting, test execution,
gate result, independent R2, or acceptance is claimed. Root owns all MG Rust,
FFI and Swift checks, provenance/privacy/similarity gates and R2. No native SDK,
adapter, Resume, registration, account mutation, KeepLocal, publication semantic
schema or UI implementation changes were made.

## Root verification

R2: clear of P0–P2 findings. The later Arc borrow correction and comment/test
wording retained that result. This is a component WIP, not unit acceptance.

| Receipt | Result |
| --- | --- |
| core-test34817 |42 selected cases passed;529 other cases filtered |
| swift-test35837 |2 actual bridge cases and7 instrumentation cases passed |
| private35102 |18 files scanned; no matches |
| similarity39227 | Pass |
| core-lint37955 |3 checks passed; clippy failed on5 new test expressions |

The first Rust attempt31431 failed on a Host Arc borrow before tests. Filtered
correction33025 fixed that expression. Lint37955 required a direct test return
and removal of four unnecessary path borrows. Correction6febdc6f preserves all
assertions and limits; core-lint73779 passes all4 checks. Failed receipts remain.
Full integration and U113 acceptance remain open.
