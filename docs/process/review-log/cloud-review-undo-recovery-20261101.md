# Cloud review Undo capture recovery — test packet

- Role/session: implementer `s-20261025T134357Z-235af9a2`.
- Supplied base: `dc68c0a5faa185b76893763bd1f0205ad76b5bb9`.
- Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
- Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Contracts: `sync/cloudkit-records#K11.4`, `#K17.1`, `#K12.3`.

Supplied independent audit diagnosis (P1): a held actual answer creates a
never-sent Revlog. Actual Undo removes the physical row. If its observer capture
fails, the live tracking image remains. After a crash, the absent tracked Revlog
hits `WholeBindingReconciliation` in `workflow_runtime.rs`'s tracked-absence
guard (supplied lines 537–542), blocking sync. This is not evidence of uploading
an undone answer. The ordinary observer already removes an unsent Revlog in
`workflow_store.rs` (supplied lines 838–842); ordinary Undo is not assumed broken.

Added independently named tests in `cloud_media_roundtrip.rs`:

- `captured_review_undo_removes_never_sent_revlog_and_survives_crash`: normal
  capture control, using the same real answer/Undo/crash/reopen path.
- `lost_review_undo_capture_recovers_absent_never_sent_revlog_after_crash`:
  capture failure armed immediately before real Undo; expected RED at ordinary
  reconcile of the absent tracked Revlog, after proving physical Undo, failed
  capture rollback, stopped tracking, WAL coverage and same-file reopen.

The helper starts from `published_review_pair(false)` and verifies Service
Save results from publication and adoption. It checks actual Card
and Revlog payloads, successful answer capture, Core/cloud holds, zero server
revision, no flights and unchanged Service requests. Existing facts expose no
independent `sent` field; the never-handed precondition is established from those
actual facts and the absence of any Service request for the new Revlog. It does
not seed a `sent` value, ACK or tracking image. The existing public child
`undo()` already wraps only `self.call(Input::Undo)`; no driver change was needed.

Neither path drives, ends the review or reconciles between Undo and SIGKILL.
Reopen uses the same files without new key bytes, verifies file/binding identity
and physical payloads/media, then verifies account identity and invokes ordinary
reconcile. Sequence comparisons use only the same engine lifetime, following
the existing Config recovery test's reopened-observation pattern.

Post-reconcile assertions describe required future passing behavior: remove the
never-sent undone Revlog from tracking; classify the actual restored Card by its
row digest against the known ACK while checking its payload and ACK payload
digest; verify accepted Service Saves for the other pending
rows; retain physical durability and peer convergence, unrelated history/media,
empty SDK pending lists, and no Service attempt at all for the undone Revlog.

This patch adds tests only. The follow-up repair must check `!sent`, `server_erev == 0` and no in-flight evidence before dropping an
absent Revlog. K12.3 handed-flight evidence must survive. Already-sent deletion
remains separate work; this packet authorizes no broad absent-Revlog exception.

Verification: one bounded source consistency check only in the filtered runner.
No compilation, tests, builds, Git, network or acceptance performed here. RED is
expected and **unrun**, including the independent control. Root owns the actual
RED-first run, compilation, independent review, provenance/privacy/similarity
checks and final evidence. Existing tests, fixtures, step bounds and timeouts
are unchanged.

## Integrator reproduction

`core-test/20261101T114812Z-24328` ran both tests in Docker after formatting:
normal capture control PASS; lost capture test FAIL at the intended ordinary
reconcile boundary with `RuntimeIncompleteWholeBindingReconciliation`. All
physical Undo, capture-loss rollback, WAL and same-file crash/reopen assertions
preceded that failure successfully. No production repair applied yet.

Independent reviewer `native_conformance_lifecycle_audit` found no P0–P2
findings in the tests-first packet. It requires the repair to retain a durable
a stored handoff flag, safe decoding of old rows, and a test that retains the
flag through upload, ACK, flight removal and reopen.

P24 still needs the offline identity test that Q11 lists.
The spec-writer audit in `cloud-p24-scan-closure-20261101.md` found no other P24
prerequisite for the narrow absent-row/never-sent repair. No acceptance claimed.

## Durable handoff repair — filtered fixer implementation

Role/session: fixer `s-20261025T162405Z-9dc656e4`, following the implementer
role. U113 remains partial. Original start
`1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`; original batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`. The integrator records the current
clean source-view/export base; this runner used no Git or history. Contracts:
`sync/cloudkit-records#K10.3`, `#K11.4`, `#K12.1`, `#K12.3`, `#K13.2`, `#K17.1`.

Implemented local-only, monotonic `sent` in `EntityFacts` and `NoteTracking`.
All full-image/header/note conversions carry it. New known-local identities
start false. Server-derived baseline/adoption/incoming initializers use true
conservatively; ordinary existing-record updates preserve prior history. A
same-name header write also retains any stored true (including an absent legacy
field, decoded true), so reconstruction cannot erase handoff history. A verified
collision rekey still creates a genuinely new key/name through the existing
new-pending constructor; ordinary ACK/merge never resets the flag. Entity wire
payloads, digests and Swift interfaces are unchanged.

`persist_flight` commits `sent=true` in the flight transaction before handoff,
including an existing-flight selection. A duplicate remains legal at the flight
cap. ACK header bookkeeping retains true while retiring historical flights.
Related never-sent Note/Card observer and incoming/rekey side-effect removal
shortcuts now also require `!sent`.

Whole reconciliation admits only an absent Revlog with `!sent`, zero server
revision and no flights. The removal re-reads those facts transactionally,
checks the stored open/file and post-snapshot observer ordering, then uses the
normal entity removal/payload ownership GC path. The caller holds the same
snapshot/WAL receipt and incarnation/account/full-file guards as other scan
mutations. No ACK is fabricated. The ordinary absent-Revlog observer shortcut
requires the same durable protection. Other absent kinds, handed-history and
structural limitations remain guarded; coverage/tombstone accounting is kept.

Runtime compatibility is V6, accepting exact historical markers 1–5 plus 6.
The outer schema is unchanged. Payload images and per-binding ownership markers
remain V5 (`CHECK(version=5)`); there is no global >=5 inference. Historical
markerless V5 retains its fallback. Markerless V6 is invalid and rejected before
migration. Payload preparation and the V6 marker commit together; migration no
longer writes 5 and cannot downgrade a reopened 6. Direct migration also fences
future versions before DDL. Container/environment/salvage filters are retained.

Test changes in the filtered patch (not executed by its author):

- The existing Undo crash tests check the new history flag before upload.
- A third case uploads the history row, loses the reply, undoes and crashes.
  It requires the handed state and outstanding flight to survive reconciliation.
- The SDK test verifies that the stored flag stays set after an ACK, edits and relaunch.
- Migration unit proofs cover missing-field defaults for full Note/Entity and
  both headers, V5→6 with markerless-V5 fallback, disk close/reopen at 6 without
  downgrade, exact fence 1–6, future 7 before mutation, markerless-V6 rejection,
  conservative legacy history through same-identity reconstruction, fresh-local
  false through note/header persistence, and byte preservation for other
  containers/environments and salvage. Existing rollback/image-sharing/flight
  tests remain enabled. The old V5-reader proof explicitly models its historical
  version predicate; no old binary or old WAL-probe run is claimed. Its former
  future-6 fixture is now future-7.

Changed paths are confined to cloud runtime/tracking/workflow/incoming/
publication/adoption/media construction, the rekey side-effect guard, cloud
AGENTS' version distinction, the two core test files and this log. No app/Swift,
host/open.rs, cloud-offline review log, tools, policy P24, forbidden source or
history was read or changed. The separate host-startup repair remains the other
fixer's responsibility.

Verification: one bounded source consistency pass only; compilation, formatting,
native builds, focused tests and all gates remain **unrun here**. Root must apply
and compile, run the focused Undo/historical-flight/migration tests and both
offline prerequisite cases, obtain independent review and run provenance,
privacy and similarity checks. Preserve prior known failures and their evidence.
No unit acceptance, milestone acceptance or P24 closure is claimed.

Filtered source consistency result: PASS. The bounded static pass found 15
tracking literal construction/conversion sites carrying `sent`, checked the
runtime-6 / payload-owner-5 distinction and protected observer guard, and listed
runtime-version fixtures. This is source evidence only, not compilation or test
execution.

## First integrated build and independent review

`core-test/20261101T122004Z-46454` failed compilation: the new negative fixture
uses `landed.envelope`, but the actual Service Record exposes fields. Read its
existing encrypted envelope field instead. No runtime pass is claimed.

Independent reviewer native_conformance_lifecycle_audit found one P2 fixture
error: after its actual lost-reply save, the negative case must refresh its
server image and request cursor before asserting no further writes. Preserve
the pre-handoff boundary in both never-sent cases. No further P0–P2 source
findings; runtime and migration verification are still required.

## Negative fixture repair — filtered fixer follow-up

Fixer `s-20261025T162405Z-9dc656e4`; original U113 start
`1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`, batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9` retained.

Replaced the invalid `Record.envelope` access with a typed match of the actual
Service Record's `e` field, requiring `Field::EncryptedBytes` and panicking for
a missing or wrong field. Those exact bytes go to `codec::receive`. Only the
handed-history branch refreshes the now-mutable server/request baselines, after
proving the saved payload matches the actual new history and tracking retains
`sent=true`, zero server revision and its matching flight. Both never-sent
branches retain their original baseline and no-Service-attempt assertions.
The negative case now requires the exact existing driver error message for
`WholeBindingReconciliation`, retaining its tracking and physical-row checks.
Physical Undo, failed capture, WAL, SIGKILL, reopen, account and scan assertions
remain intact. Earlier offline tests and production files are unchanged.

Verification: one bounded source consistency check only; compilation, formatting,
runtime tests and gates are **unrun here**. Root owns migration library tests,
Root must verify the final fixture and all required guards. No acceptance claimed.

Source consistency result: PASS. Exact-delta comparison confirmed only the
specified helper edits, unchanged earlier offline tests and other test-file
bytes, and handed-only baseline refresh after all required handoff proofs.

`core-test/20261101T122306Z-49220` PASS11/11 migration checks.
`core-test/20261101T122450Z-50298`: both original Undo cases and the real SDK
handoff/ACK/reopen test PASS; the new handed case fails at Undo, before crash.
The independent review confirms M9.6/M9.7 forbid Undo after the upload barrier
flags its step Pending. A lost reply retains that refusal. The test must cover
this real refusal without clearing flags. An absent-row scan check needs a
separate actual writer removal, rather than claiming user Undo can create it.

## Handed-history negatives split — filtered fixer follow-up

Fixer `s-20261025T162405Z-9dc656e4`; original U113 start
`1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`, batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9` retained. Owned scope is this log,
`cloud_media_roundtrip.rs` and testkit `cloud/child_driver.rs` only.

Replaced the invalid handed-Undo negative with two named cases sharing the real
publication/adoption/answer/End/lost-Save-reply setup. The refusal case reads
`Collection::peek_undo(Global)` through a thin IPC command, requires the actual
`upload_pending` reason, invokes existing actual Undo and requires refusal plus
the same reason. It checks unchanged physical payloads/media and tracking, then
SIGKILL/same-file reopen, verified identity and ordinary successful reconcile
retaining the live history and outstanding flight. It exercises no absence guard.

The separate core-writer negative uses a thin IPC command forwarding only
`Collection::run` (`OpSource::LocalApi`) and `scope.remove_revlog`, returning the
actual committed sequence/generation. No capture fault is injected: the ordinary
handed-absence observer refusal must stop tracking while the physical commit
stands. Assertions cover actual absence, unchanged tracking with sent history,
zero ACK and matching real flight, WAL coverage, durable absence through SIGKILL
and same-file reopen, verified identity and exact
`RuntimeIncompleteWholeBindingReconciliation` refusal. Its actual Card snapshot
retains the answer's fields with `last_review_id` derived from remaining history;
it is never described as Undo restoration. Both cases retain unaffected rows,
media and the Service state/request boundary after actual handoff.

The original two never-sent tests and their executed path, and all earlier
offline cases, are preserved byte for byte. No production, policy, engine or scan
guard changes; no seeded tracking/ACK/sent values, raw SQL or cleared upload
flags. One bounded source consistency check only; compilation, formatting,
focused tests and gates remain **unrun here**. Root owns focused verification,
independent review and the full gate. No acceptance claimed.

Source consistency result: PASS. Exact comparison preserved the original
never-sent tests and executed source, earlier offline cases and later helpers;
the driver delta is confined to real peek-reason and writer forwarding.

## Final focused integrator proof

`core-test/20261101T123413Z-55159` PASS5/5: normal Undo; lost Undo capture with
same-file crash recovery; handed Undo refused with its actual pending reason;
handed history removed by a real writer and retained by the absence guard after
crash; real SDK handoff/ACK/edit/reopen metadata persistence. Both former test
failures remain retained. `core-test/20261101T122306Z-49220` PASS11/11 migration
checks; `core-lint/20261101T122900Z-53600` passes before the last thin driver/test
addition. The final broader CloudKit checks will cover that addition.

Independent reviewer native_conformance_lifecycle_audit found no P0–P1 needing
repair and no new P2 in the final split. The original fixture finding is fixed.
Scoped privacy and full change similarity checks pass. Original batch base stays
unchanged. No unit, milestone, signed SDK result or release is accepted here.
