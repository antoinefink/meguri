# Fixed original-role edit handoff

Recorded implementer; supplied master base `593add47`; original batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Specs: conformance/cloudkit-agent#J3.2, notes/note-records#N2/U3,
storage/ids-usn-mtime#S3/S5.

Implemented only the retained original capability's fixed edit and selected Mac
`.edit` composition. Existing `Collection::run` admits the normal Update Note
UserUndoable/undo spec; its transaction validates the private baseline, then
calls the same `notes::update_in` used by `notes::update`. No new engine guard,
raw writer, second Host, replay or callback reentry. The capability retains the
actual OpResult and bounded committed facts. Generation-bound ordinary WAL
flush/coverage avoids taking the lifecycle control lane from a job it may drain.
Committed readback checks canonical content, relations, derived metadata and
local stamps; a bound Host additionally requires the actual captured revision
and exact observed generation/commit seq. Typed results contain no native IDs.

Authored, unrun: real FFI Host/job/updater coverage for original minimum ID with
a lower current row, Q at either ordinal, all preserved note bytes and card/deck
relations, normal metadata/stamps/col.mod, commit seq and canonical digest,
flush/reopen, repeat and successor-owner refusal, changed-other-field and
already-matching-row refusal, storage.commit.before failure and writer-barrier
cancellation. The faultpoint is process-wide, as in the existing seam; focused
verification must isolate that test process. Exact instrumentation registry
expectation includes both added exports.

Not run: builds, tests, CloudKit, git or external checks. One source consistency
check belongs to this handoff; the integrator owns compilation, focused checks,
independent review, fixes, provenance/privacy/similarity and commits. Actual
bound-Host local-revision coverage and native composition execution remain
unrun/uncovered by the new real-Host test (its unbound revision is None).
Durable interrupted-operation inspection, source ACK and peer apply/recovery
remain Missing. Publish/adopt/control inspection and the prepared
AgentGeneratedBaseTests.swift addition were untouched. No lane/unit acceptance
or signed execution claim.

Recorded fixer repair (unrun): unchanged card/deck assertions now compare every
stored row and cell across all pages. The exact nextest reservation and 20s/5s
bounds remain. A separate serialized native Core/Service case now authors the
previously uncovered bound-Host branch: normal publication, independent
validateOwned binding, real InteractiveLane fixed edit, actual nonzero revision,
exact generation/Changes seq/digest/Q change, flush/reopen and joined shutdown.
It does not change the bootstrap preservation test or claim interrupted-edit
recovery. All prepared seed work is retained; root owns execution and acceptance.

R2 review is clear after the exact test reservation and complete stored-row
comparison repair. Debug run 52445 executes nine cases: eight pass; the real
fixed-edit success path returns an invalid-role error. Preserve that failure
and every assertion. A bounded content-free trace is required before repair.
The new bound-Host revision test is authored but has not executed. This is WIP.
