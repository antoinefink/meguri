# Provider authority after restoration

Prepared work began at `b3865f8af4e7679afe48161e54376b702ce14b52`.
This repair resumed on clean `5eaafd6860af839560b7421bd81506d8fe8878b4`.
The app batch keeps base `1416a520542a8c5daa38a23bfe398da704c3d6c9` and its
failed gate. This record grants no unit or release acceptance.

Cold Resume activated its validated stopped binding, then media reconciliation
rejected its unopened provider writer. Cancelling a replacement publication
also restored the predecessor without revalidating its provider authority.
Both paths now call the existing qualifier before reconciliation, outside
runtime, tracking and reader guards. Selection, account, generation, file,
handoff and settled publication checks remain unchanged. Live adoption abort
has different ownership and was left unchanged.

Filtered author session: `s-20261110T214025Z-20fa15fd`.
Independent reviewer session: `s-20261109T210639Z-47d06d5b`.
No new specs were needed. Reviews cover the recovery and provider contracts,
correctness and provenance; there is no UI change.

The initial source review found a negative test using an expired cold preview
as evidence of provider rejection. Its claims and typed refusals now separate
those checks. The first run also exposed a warm fixture asserting closed
authority before starting handoff; that assertion now follows the real handoff.
No production restriction was changed to fit either fixture.

`core-test/20261117T082739Z-71267` ran three tests and failed all three.
`083750Z-84861` passed both provider-choice negatives and reached the typed
IdentityUnverified refusal in both restoration cases. Its cancellation
diagnostic then rejected the changed observer fields during its entity check.
Review `cleanroom/20261117T083633Z-83794` cleared the two production calls and
requested that diagnostic correction.

Fixed production `db118bcd` in `core-test/20261117T085751Z-4688` passed the
operation success and data checks, then both positives failed a new test's
clean-drain assertion. Successful reconciliation proves coverage, not drain.
The corrected tests require the actual nine-kind ledger, matching open,
generation, file, WAL and J6 frontier, without inventing a clean drain. Pending
data and ACK checks remain. Final review `cleanroom/20261117T090507Z-23408`
found no P0–P2 issues; its export equals the reviewed author patch byte for byte.

The unchanged original app reproduction passes on clean `db118bcd` in
`swift-test/20261117T090150Z-11321`: one method, both Reopen and Resume cases,
zero failures or skips. Earlier failure receipts remain.

`core-test/20261117T091221Z-42310`, clean `3e48ab80b`, passes all four named
restoration and provider-choice regressions. Its broader 136-method run fails
overall with 15 passes and 121 failures. Older protocol fixtures need explicit
prior iCloud selection; account inputs alone grant no consent. Generic local
information and no-consent cases must stay unchanged. That fixture repair,
other broader suites and the required gate remain incomplete.
