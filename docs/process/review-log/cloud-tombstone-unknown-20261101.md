# Original handed entity tombstone Save meets UnknownItem

Role: implementer. Source start supplied by root: `d5ff8fa0` on master.
Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Original integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Requirements: `sync/policy#P24`, `sync/cloudkit-records#K17.4`,
`conformance/cloudkit-acceptance#Q6`.

Authorship status: **UNRUN**. This filtered implementer session makes no
runtime, production-defect, P24, Unit, batch or milestone acceptance claim.

Added test:
`original_handed_entity_tombstone_save_unknown_item_drops_note_and_cards_durably`
in `core/crates/meguri-sync/tests/cloud_incoming_intents.rs`.

The existing `Pair::adopted()` fixture publishes and acknowledges real Service
records. Native `delete_note()` removes an acknowledged Note and its Cards;
`hold_receive(Point::BeforeCollectionSave)` retains their original built
conditional `MGEntity` Saves. Assertions inspect actual envelopes, conditional
system fields and durable Store in-flight entries, including exact tombstone
envelope digests, revision, writer and origin attribution. An independent Service
Modify deletes those same server records before the held request is released.
Default `TaggedMissing::UnknownItem` is asserted, with no configuration change.

Normal transport submits the original Saves and delivers Service's actual
UnknownItem results. The test matches the retained request mutations, instance,
backend batch and Service request receipt. A second latch holds subsequent fetch
intake so fetched deletions cannot substitute for the original Save-result proof.
Tracking must disappear for the Note and every Card with empty inbox, apply
intents and SDK pending queues, unchanged absent physical rows and writer
observations except the empty-batch barrier sequence described below, and
unchanged retained tracking and unrelated Note Service record.

The real host closes and opens its existing files with the same retained Service
and no supplied identity, binding, keys, ACK or tracking baseline. Before identity
verification or fetch, direct Store reads must still show no deleted tracking and
the physical rows must remain absent. Normal verification and another Sync Now
must leave no pending work or second send of the deleted names.

Scope: this test and this new log only. No production, fixture, Resume binding,
specification, limit, retry or sleep changes. No fabricated callback, original
request, acknowledgement or tracking entry. No forbidden trees, history, network,
builds or tests were accessed. One source consistency check completed: existing
API declarations and required workflow boundaries were inspected, without
compilation or runtime execution. Root owns formatting/compilation, focused Docker execution, independent
review, provenance/privacy/similarity guards and original batch/milestone gates.
If execution finds a production gap, retain the exact failure and report it;
do not mask it or close P24/Unit requirements from this handoff.

## Root execution and review, round 1

`core-test/20261101T133503Z-83189` compiled and failed after matching the
original Service UnknownItem reply: collection sequence was 19 rather than 18.
The writer effect needs diagnosis; no assertion is removed on that evidence.
Reviewer `cloud_p24_scan_spec` found one P2: compare all retained entity facts
with the original map minus exactly the deleted names, including after reopen.
The current single unrelated Note check does not cover unrelated Cards.
Privacy and similarity checks passed on the authored source before this entry.
No requirement is accepted by this WIP commit.

## Authored repair after round 1 — UNRUN

Role: fixer (implementer clean-room restrictions). Repair source start supplied
by root: `f447889f`; original U113 start and integration batch base above remain
unchanged. This repair has not been compiled or executed.

Source diagnosis confirms the extra sequence comes from the normal final empty
batch probe, rather than the UnknownItem tracking removal. `SDKRig`'s
`drive_budget_inner` calls `build` after the original reply even with empty
pending Saves. `Runtime::build_collection_batch` still calls
`ReaderLease::capture_cloud`, which runs `cloud_snapshot_barrier` even with no
named rows. The executor writer reserves a sequence after committing an empty
journal and suppresses its empty Changes event. `record_unknown_item` removes
the unchanged handed Note/Card revision in the tracking transaction only.

The test now requires exactly `observation.seq + 1`, retaining the physical-row
comparison, native-file and generation equality, unchanged
`col_mod`, undo depth, Changes events and incoming-run count. The real WAL proof must cover the observed sequence and match the native file
and generation. The test uses the existing API and creates no apply receipt.

Round-1 P2 is addressed by deriving the expected retained entity map from the
actual handed map minus exactly the deleted names. Full-map equality is checked
after the original UnknownItem reply while fetched deletion intake remains held,
on the actual reopen before identity verification, and after normal driving.
This includes every unrelated Card and every field of each retained entity.
Original conditional-Save attribution, actual Service UnknownItem receipts,
queue clearing, persisted absence and no-later-resend assertions remain.

Only the owned test and this log changed. One source consistency pass completed;
no builds, tests, forbidden trees, history or network access. Root owns execution,
independent review and provenance/privacy/similarity and batch/milestone gates.
The exact sequence and WAL assertions remain UNRUN; no production-defect fix,
P24, Unit, batch or milestone acceptance is claimed.

## Root verification after repair

`core-test/20261101T134213Z-88036`: PASS1/1. Independent round-2 review
found no P0–P2. The original failure remains retained. The proof covers the
handed tombstone clause only; the Resume-binding workflow and the wider
P24/Q6, U113/U114 and batch requirements remain open.
