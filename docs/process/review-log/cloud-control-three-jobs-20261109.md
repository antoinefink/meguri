# Native control calls on retained jobs

Component start: `ba6bd3f3`. Original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
The parent units U113, U114 and U151 remain unaccepted.

## Scope and source review

validateControl, continueMediaAfterSend and prepareFlush use the existing
original-Host pool, typed results and ordered Swift job helper. One PolicyWork
crosses flush apply, pending update and Refresh. Quota work marks its first
effect before upload suppression, parking and pending changes. Original
owner/generation checks and public synchronous interfaces remain.

Export `54398` received independent Rust and Swift reviews. One P2 cleanup
removed three unused internal Runtime wrappers and their dead-code attributes
in `57616`. Core `58536` then fails compilation before tests: a new test used
unwrap_err with an output type without Debug. The compiler also identifies an
unused private helper. Export `59188` removes that helper and uses an explicit
error match that still fails on unexpected success. All original assertions,
deadlines and limits remain. Final source review closes the cleanup finding.
The final export passes privacy; two new explanatory comments are omitted.

The absolute deadline is sampled after job/Driver queue delay. The existing
relative timeout after that sample remains; this component does not add a
stricter deadline at every downstream wait.

## Verification

Focused Rust policy/media/adoption and native control/flush checks are running.
The failed build is retained and counts as no passing tests. No acceptance or
Unit trailer is added. Asset-read-pass admission, completion reporting and the
original batch gate remain open.

## Focused results and fixture identity mismatch

Native `61008` builds and passes all 11 selected checks, including the three
new control-call result proofs, background flush and expiry cancellation.
Core `60724` passes 206 of 207. The new full-state flush parity test compares
two independent fixtures. Their generated binding and SDK identities differ;
all other state/output text matches after mapping only those identities.
Independent diagnosis and an assertion-preserving fixture repair are pending.
No actual failure is accepted as a pass. No Unit trailer is added.
