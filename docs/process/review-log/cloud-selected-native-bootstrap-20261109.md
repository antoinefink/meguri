# Native selected bootstrap: pending

Start: `593add47`. Batch: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261109T210639Z-47d06d5b`. Export: 9875.
Contract: conformance/cloudkit-agent J3.2.

Adds the owned-Host control query, original reply projection and native
publication/adoption composition. Uses existing coordinator operations.
The SDK factory checks actual scan coverage. Adoption joins the retained
backup, activation, replacement event and original snapshot.

R1 finds two P2s: failed adoption can wait for an absent replacement event;
the selected ledger rejects matching conflict replies accepted by Core.
Both require repair with failure/cancellation and conflict coverage.
Off-feature FFI compilation fails at the new projection test helper.
No new native case has executed. Held diagnostics stay preserved separately.
Later ACK, peer apply, close and recovery evidence remains missing.
No signed run, full gate or unit acceptance is claimed.

Recorded fixer repair (unrun): adoption now joins existing terminal barriers,
checks cancellation/failure/unchanged owner and requires an already delivered
replacement; it opens no replacement wait after failure. Original cleanup
ownership stays retained. Selected complete/active conflicts require their
typed original projection, ordered original commit and final Core correlation;
J3.1 conflict admission stays strict. The private projection helper also
compiles under cfg(test) without adding off-feature exports.

Authored, unrun: actual Core/Service matching and invalid conflict publication;
existing-owner adoption refusal, failed original page delivery and cancellation
after an actual Service reply; selected ledger origin/attempt/kind/missing-commit
refusals. Shared synthetic source creation preserves the separate bootstrap
preservation test. Root owns compile/tests/format/review/guards/commit.

Repair export 51525 has clear R2 review. Adoption now checks the terminal result
before requiring the observed replacement; selected conflict evidence retains
its original commit/correlation join. Smoke stays strict. Off-feature core-test
53524 passes its projection case. Debug core-test 52445 passes projection and
held-scan cases; its fixed-edit case fails, so the run is not accepted. New
native composition and ledger cases are authored and remain unrun.
