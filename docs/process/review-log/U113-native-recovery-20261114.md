# Native recovery contract review — 2026-11-14

Approval: reviews-cloud-stabilization-approval-20261114.md. Original start:
`6fbc9a24`; integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Both additional rounds are used. No source or approved spec changed.

Round one: `cleanroom/20261114T081442Z-64290`, read-only audit and a draft.
The reviewer rejected its general historical-proof requirement and found
ordinary recovery incomplete. The revised proposal is Under review.

The second round started at `7a80be65`. Independent spec-writer session
`s-20261114T083432Z-b1d738f4` reviewed the revision. Verdict: HOLD, no P0.
The report is retained locally at
`build/cloud-resume-diagnosis/U113-native-recovery-independent-20261114.md`.

P1: a type with three fields and no notes admits two histories: Meguri renamed
fields, or another writer reordered them. Their final rows match; their maps
differ. After restart, the rows and file metadata do not resolve this case.
The approved contract supplies no authority to choose the prepared map.

A behavior decision remains: whether retained preparation may impose its
ordinal interpretation in this exact case. No such authority is assumed.
The draft also needs precise acceptance, Abort, successor-admission and
final-stamp handoff rules. Further repair stays held at the recorded limit.

No test, Unit trailer, P24 closure, gate pass or release acceptance is claimed.

## Owner policy direction

See `cloud-recovery-product-decision-20261114.md`: automatic recovery where
safe, otherwise an explicit local-or-iCloud collection choice. This settles
the user-facing policy. The detailed native recovery contract, implementation
and tests remain open; the draft and repair limit are unchanged.


## Approved extension: rounds three and four

Authority: `cloud-recovery-extension-20261114.md`. The counts are preserved.
Round three produced the reviewed K13.4/V13.6 contract, committed as
`fbc922a9`. Its publication check passes at
`private/20261114T131146Z-95832`; the actual receipt path is retained in
`build/cloud-resume-diagnosis/U113-r3-published-spec-private-20261114.log`.
This closes the behavior decision, not implementation acceptance.

Round four started at `270d9af3`. Component author `121408Z-47295`, fixture
fix `125800Z-80598`, independent review `130029Z-83353`, final partial fix
`131238Z-96242`, and read-only disposition `132203Z-4465` are cleanroom runs.
The initial fixture run stopped before the target condition. After correction,
`core-test/20261114T125948Z-82500` had three intended failures and a passing
foreign-write control. The first component passed ten focused tests at
`130043Z-83727` and 22 observer/WAL tests at `130437Z-86812`.
These passes do not cover the later partial fix.

Final disposition: HOLD. The last export does not compile:
`core-test/20261114T132216Z-4930`, missing EngineError qualification.
The final reviewer confirms the targeted writer-stamp and pause changes but
finds a new P1: a stamp mismatch skips other observers of committed changes.
The shared memory bound and first-freeze I/O ownership remain unresolved.
Typed refusal and the existing collection-choice integration are not built.
The idempotence assertion covers a repeated settled drain only.

The export stays in build; master has none of these source edits. Both approved
rounds are used. Antoine must decide whether to permit another source pass.
U113 remains unaccepted, with its P24 checks, gate and release evidence open.

## Round 5: available-evidence fallback

Owner approval: `cloud-recovery-fallback-approval-20261114.md`; source start
`cbc09dc5562fea2303c9954527ef4dbd4490364f`. The narrowed K13.4/S2.5 contract
is committed at `b636a4cb`; the approval decision is resolved, not the unit.

Author: `cleanroom/20261114T144251Z-58500`. The patch is partial. Its retained
`codex.out` lists the completed paths and required remaining source work.

Verification of the implemented portion:

- Before integration, `core-test/20261114T150147Z-66249`: 16 existing cases pass.
- `core-test/20261114T150252Z-69766`: 17 focused cases pass.
- `core-test/20261114T150413Z-70999`: all 104 Host workflow cases pass.
- `swift-test/20261114T150507Z-72210`: five connection model tests pass after
  native binding regeneration.
- `similarity/20261114T150505Z-71921`: 18 files, no flagged matches.
- `private/20261114T150241Z-69382` flagged two new comments. Both complete
  comments were rewritten without changing code or guard behavior.
  `private/20261114T150403Z-70717` passes for the resulting source.

Independent review `cleanroom/20261114T150127Z-65745` is still running.
This is WIP, with no Unit/P24/gate or release acceptance. Round 6 remains for
required gaps and review findings. The live Mac and phone still show unknown
sync status; a phone request reports incomplete. No new peer-delivery claim.

## Round 6: partial repair

Source starts at `3817df62`. Author: `cleanroom/20261114T151140Z-78575`.
The retained handoff reports changes for findings 1–7. Finding 8 remains
unimplemented; replacement can still fail on the retained ordinary work.
The operation-conflict case for the local action also remains open.

- `private/20261114T153439Z-89538`: source passes.
- `core-test/20261114T153503Z-89842`: five new regressions pass.
- `core-test/20261114T153633Z-90224`: all 109 Host cases pass.
- `core-test/20261114T153742Z-91176`: all 22 engine controls pass.
- Earlier native presentation: `swift-test/20261114T151240Z-79030`, eight pass.

Independent review `cleanroom/20261114T153423Z-89103` is running. Swift
bindings, remaining focused checks and actual rendered controls still need
verification. Both approved additional rounds are used. No further source
repair is authorized by this record. U113, P24, gates and release stay open.

### Final round-6 disposition

Review `cleanroom/20261114T153423Z-89103` is terminal: no P0, five P1 and
two P2 findings. Its full `codex.out` retains the counterexamples and required
tests. Required areas: replacement completion, outgoing note-type changes,
Undo/Redo, preview memory and the waiting-operation exit.

`swift-test/20261114T154445Z-97546` passes 12 bridge/instrumentation tests.
`swift-test/20261114T154753Z-1380` passes 35 diagnostics/presentation tests.
`static-checks/20261114T154853Z-4036` passes; shared similarity
`similarity/20261114T154853Z-4156` reports 34 files and no flags.

These results do not close the source findings or the broader FFI failures.
The unit remains unaccepted. A proposal for at most rounds 7–8 is prepared
in `build/cloud-resume-diagnosis/U113-integration-rounds-owner-decision-20261114.md`.
Further U113 source repair needs the owner's decision. Independent app work
continues while that decision waits.

## Round 7 integration result

Authority: `cloud-recovery-integration-extension-20261114.md`. The source
starts at `d793c196`; author `cleanroom/20261114T161216Z-22839` exports
23 files. The export remains retained locally, pending round 8.

`core-test/20261114T163922Z-34353` ends in error after interruption:
two tests reported pass, five failed, and the adoption test was still running
past 120 seconds. Its result is not a suite pass. The failures include invalid
sent-creation Undo expectations, fixture counts, cancellation, and incomplete
adoption. `swift-test/20261114T164225Z-35243` fails compilation in the new
coordinator guard, which throws a value that is not an Error.

Independent review `cleanroom/20261114T163901Z-33878`: HOLD, no P0,
three P1 findings. Field/template pages lack a byte bound; publication Cancel
is unavailable in valid waiting states; sent-creation Undo tests contradict
the existing send barrier. Coverage for replacement scope, restart, mixed
work and native actions is incomplete. Two P2 findings concern typed Busy
presentation and Stop cancelling an operation implicitly.

The reviewer inspected operation-specific cleanup and found no source defect
that deletes the old work before success. Execution coverage remains open.
`private/20261114T164240Z-36411` rejects comments in four R7 files; those
comments must be rewritten with the guard unchanged.

Round 8 starts at `f4b3160d` from the complete retained R7 export and the
review report. This is the final authorized round. No Unit, P24, gate or
release acceptance is claimed.

## Round 8 verification checkpoint

Author `cleanroom/20261114T165045Z-39765` exports the complete repair.
Publication follow-up `171329Z-53043` changes one full comment and two test
names only. The source bodies and assertions are unchanged.
`private/20261114T171740Z-56825` passes; the earlier rejected scan is kept.

- `core-test/20261114T171106Z-51872`: six pass, five fail. Adoption completes;
  failed-switch cancellation, competing-root cancellation, large-note memory
  and large field/template memory controls pass. Outgoing batches, deletion
  lifecycle and the expanded publication bootstrap still fail.
- `core-test/20261114T171235Z-52687`: both FFI switch tests pass. An outstanding
  original attempt stays Busy; a settled failed attempt permits cancellation.
- `core-test/20261114T171703Z-56456`: short part-page projection and raw
  identity traversal passes.

Independent review `cleanroom/20261114T171100Z-51496` is running. Native
integration, the full cleanup-scope matrix, required wider checks and the
peer round trip remain open. Both authorized rounds are used. This saves
partial work and failure evidence; it does not accept the unit or any gate.

### Final round-8 review

`cleanroom/20261114T171100Z-51496`: no P0, three P1s remain. The ordinary
selector excludes note types; identity invalidation loses the outstanding-save
barrier; adoption waiting for review offers no Cancel. Two P2s concern progress
cancellation and its required confirmation. The cleanup-scope matrix remains
incomplete. The unit is not accepted.

A separate read-only audit distinguishes fixture proof gaps from product
failures and leaves the exact bootstrap constraint unresolved. Native
`swift-test/20261114T172338Z-60626` passes all nine focused tests. It follows
retention of a generated test bundle left by a failed compile; the failed
signing run is retained. Similarity `20261114T172007Z-59022` scans 23 source
files with no flags. No wider gate or peer pass follows from these controls.

### Rounds 9–10: verified progress and retained failures

Approval: `cloud-recovery-r9-r10-approval-20261114.md`.
R9 starts at `f6f7efff`; R10 starts at `149069db`. Authors:
`cleanroom/20261114T180700Z-81049` and `182546Z-334`.

R9 review `181834Z-92363` finds one P1 in native cancellation and one P2
in toolbar Close. R10 review `183608Z-7223` finds no P0/P1 in the scoped
correction. Busy feedback on the publication sheet remains a P2.

R9 Host `core-test/20261114T181856Z-93399` passes 12 of 13 tests. The
remaining failure proves a fixture binding-ID collision. R10 uses a shared,
scenario-local deterministic stream across real reopens; constraints remain.
`core-test/20261114T183630Z-7649` passes all 13, including the original
large-note memory checks. FFI `181947Z-94566` passes three controls; all
seven Rust production file patches are identical between R9 and R10.

R9 native `swift-test/20261114T182042Z-95468` passes 32 tests. R10
`183717Z-8830` passes 32 of 33 tests. The new parameterized test has 11
issues. Its ownership assertion spans a real task handoff; three account
cases call startup composition while the host is open. The lost-reply case
also reports a separate stale StopEngine command. The exact cause of that
command failure is unresolved. These failures are retained, not waived.

The read-only diagnosis is retained under
`build/cloud-resume-diagnosis/U113-r10-native-failure-diagnosis-20261114.md`.
Both authorized rounds are used; a new bounded decision is requested before
further behavior repair. Full matrices, P24, wider checks and the peer trial
remain open. No Unit or acceptance claim is made.

Publication finish `cleanroom/20261114T184343Z-15121` removes one test
comment only. Privacy `184514Z-17033`, static `184524Z-17655` and
similarity `184518Z-17315` pass; the latter scans 19 files with no flags.
The failed native result remains failed. This checkpoint is WIP.

### Remaining compiler and layering check

`core-lint/20261114T185746Z-29619` reports two passed checks and one
failure. Layering and the compiled-name check pass. Clippy stops on
`type_complexity` for the return tuple in `cloud/resume_runtime.rs:439`.
The full lint job remains failed; complete lint coverage is not proven.
No lint policy, assertion, timing limit or source behavior was changed.
This result does not resolve the native failure or accept U113.

### Round 11: native reproduction isolated

Starting commit: `69e3609c`. Author `cleanroom/20261114T195741Z-65058`
corrects the two fixture preconditions and adds a passive test trace.
Compile correction `200950Z-74175` protects two captured test scalars;
production code and test limits are unchanged. Initial native compile
failure `swift-test/20261114T200720Z-72017` is retained.

`swift-test/20261114T201150Z-74725` executes all five cases with one
issue, in the lost-reply case. The StopEngine command is issued by cloud
refresh during post-cancel drain, before close. It fails generation/binding
validation. Independent review `200711Z-71640` retains two P1 coverage
findings and one P2 trace finding. They require repair; this fixture and the
unit are not accepted. No reported failure is suppressed.
