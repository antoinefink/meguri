# Swift asset read recovery — source handoff

Role: implementer. Session: `s-20261025T134357Z-235af9a2`.
Supplied starting master: `5875adc7` (executable clean base `015807f3`).
Original U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Scope: P24 K13.1/P4, Q11, K22.2; tests first, no behavior fix.

The test retains one Service while AppModel owners A and B run in sequence. The public Note service writes both edits. A keeps automatic sync disabled so its local edit stays pending before B uploads the remote change.

Each SDK fetch delivers real Service pages through its registered delegate. The coordinator receives the resulting events and runs the retry. The final upload uses Sync Now. No direct retry or issued-command call drives this test.

The opt-in wire seam withholds one matching payload delivery after B has closed and returns five failed asset deliveries only after actual accepted Read frames have drained. It verifies the selected server envelope/system fields/payload before injecting failure. Actual Read receipts, real drain records, production backend retry command starts/completions and the existing read-only FFI inbox observer supply the assertions. Failure counts 1–5, a sixth drain with no read, same-files reopen count 0, and the first post-open drain's successful same-version real lease/apply are required. Service Saved body, conditional base, ACK digests/system fields, subsequent reopen and unrelated Note/media preservation are asserted.

Authorship is UNRUN: no build, test, Git, history, network or forbidden-source access in this filtered session. Root owns regenerated bindings, Rust-first verification, focused native compilation/run, independent review and all batch/provenance/privacy/similarity gates. No P24 closure, Unit/release or Apple SDK claim.

Root must compile and execute this source. The existing ten-second command barrier remains in place, and absent dispatch must fail. The shared support behavior changes only when the new fault is armed. Production code and FFI exports are untouched.

The single source check found balanced Swift delimiters/conditional guards, no trailing whitespace or conflict markers in the four owned paths. Source review then tightened terminal close assertions and records unexpected accepted reads before fault validation, so a late sixth request cannot disappear from the ledger. No compilation or execution evidence is inferred from this check.

## Independent review, round 1

Reviewer `native_bounds_review` found one P2: the large Note payload exceeds
the synthetic stdio bridge's 1 MiB JSON frame cap. Its integer-array encoding
needs several MiB. Keep that cap and the production asset threshold; provide
a bounded fixture transport before executing this test. No other P0–P2 was
found in the source review. Native compilation and execution remain UNRUN.
