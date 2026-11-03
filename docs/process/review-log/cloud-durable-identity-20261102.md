# CloudKit durable collection identity

Status: partial U113; no Unit or milestone acceptance.
Component start: 0a5806f844ad4ef5e3d86d72e6c57e7ace1151e4.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Integration base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer session: s-20261102T225401Z-cb28dce4.
Spec-writer session: s-20261102T213243Z-4ad02362.

An independent source audit finds that ordinary active-binding reopen compares
the saved device number with the current one. If a remount changes that number,
the unchanged file is isolated. V8.2 currently prescribes this comparison, so a
contract correction must precede implementation. No remount was performed.

Keep all live replacement guards. Use the retained writer descriptor for a
separate stable Darwin identity, with exact birthtime and strict legacy rules.
The first independent contract review finds three P2 issues: an older-reader
fence, invalidation of previous-open scan proof, and pairing the captured file
with the live birthtime before bind. The second review clears all three after
the contract changes. Capture failure blocks CloudKit while local reviews
remain available. Privacy17976 passes both amended specification files.

Live Mac verification awaits an unlocked session. The saved collection stays
intact. Signed SDK evidence and the batch gate are outstanding. This component
changes neither Swift nor the U114 repair limit.

Filtered export18553 finishes from source64296f8f with boundary checks passing.
The patch changes Rust only. Format35922 and source privacy36539 pass.
The first code review finds two P2 issues: publication must recheck its open
authority after the final provider reads; the old-reader witness must run the
unchanged V6 probe, rather than a test predicate. Both remain open. Focused
compilation is running; this WIP has no acceptance claim.

Repair38128 pins publication authority after the final providers. Its new
test covers eight lifecycle/provider/prior-binding combinations. Independent
round2 clears that repair. Actual old-reader proof40319/40879 runs the unchanged
V6 public open against a generated WAL-essential V7 fixture: two refusals,
complete original byte preservation and a main-only positive control. Source
hashes match the baseline. This closes the compatibility-test finding.

Verification of the second repair:

| Evidence | Result |
|---|---|
| Focused42701 | 79/80; one fixture needed V8.2 pre-bind rejection checks. |
| Repair44829 | Keeps the scan failure assertions and checks that refusal leaves stored state identical. |
| Historical mismatch case | Runtime durably isolates FileMismatch, retaining pending revisions, ACKs and identity evidence. |
| Independent round3 | Clear; privacy48343 passes. |
| Final focused48079 | 81/81 selected; 86 other cases excluded. |
| Strict lint48904 | 4/4. |
| Native wrapper42983 | 19/19; second repair leaves its source identical. |

Actual native48627 fails the new Darwin publication test: it registers an SDK
while reconciliation is still required. The public API correctly refuses
WholeBindingReconciliation. The three other new native host identity tests
pass. A test-only admission correction is required before accepting this
native result. No production guard or timing limit may change.

Direct signed Mac inspection also succeeds in source6759694a: the collection
opens, one review is accepted, and normal quit/reopen retains its state. This
is UI evidence only. Publication remains blocked by disabled placeholder
actions in the existing decision sheet. No live CloudKit round trip is claimed.

Test-only correction52236 asserts the exact pre-reconcile SDK refusal and
successful registration/validation after real reconciliation. Every other
assertion and all limits remain. Independent round4 clears the delta and
the V7 maintenance instruction. Privacy53558 passes for the changed test.
Actual native53809 passes177/177, zero failures and zero ignored tests.
This includes the real Darwin retained-FD/Collection/Runtime path and the
simulated prior-device reopen with pending/ACK preservation. It is not an
actual remount or SDK network trial. The original failed run48627 stays.

Committed577e8d23 has final lint55496 passing4/4, similarity55303 passing and
static55359 passing3/3. Static verification uses the original integration
base. Final privacy54876 passes. The focused identity component is verified;
U113, U114, actual remount, signed SDK workflow and full batch acceptance
remain open. The next UI component must implement the specified publication
decision flow rather than clear or bypass its pending decision.
