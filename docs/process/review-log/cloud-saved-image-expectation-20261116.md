# Saved-image journal expectation — 2026-11-16

Starting commit: `80b026418a03de77846e1d897d8d7d7e387bfb03` (clean master).
Source repair: `6a1e6ec9f908deab4fd3d74a04f0d7b756258b7e`.
Keep integration base `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Cause and repair

The saved-image test expected an `AgentFact` without a digest. The producer
records the saved envelope's digest. The retained iPhone app-hosted failure in
`xcode-check/20261116T023911Z-29156` shows that difference for both held and live
images; the other fact fields match.

The test now computes the expected SHA-256 from the synthetic envelope and
formats it independently of the producer's helper. Its complete fact comparison
still checks the sequence, engine and held/live label. The fixture's logical
image digest remains distinct. A missing or wrong envelope digest still fails.
The delivery-before-commit refusal and preceding saved-operation order check
remain unchanged. No production code, timeout or contract changed.

This follows the original-image evidence in the approved
`conformance/cloudkit-agent` J4/J5.1/J7 contract and the adjacent journal-reader
checks. It implements no section marked under review.

## Review and checks

Author: filtered run `cleanroom/20261116T092140Z-92102`.
Independent review: `20261116T092419Z-3697`, no P0–P2 findings. Its exported patch
matched the seed; review made no changes.

Focused native checks ran serially in the dedicated VM on clean `6a1e6ec9`:

| Check | `mac-hosted-test` receipt | Result |
|---|---|---|
| Saved image sequence and held flag | `20261116T092758Z-14476` | One pass, zero skips |
| Original manual journal reader | `20261116T092856Z-17903` | One pass, zero skips |
| Earlier automatic journal reader | `20261116T092939Z-20089` | One pass, zero skips |
| Historical conflict and fetched image | `20261116T093024Z-23244` | One pass, zero skips |

Privacy `private/20261116T092603Z-7171` passes for the changed source.
Similarity `similarity/20261116T092601Z-6884` passes: one file scanned, zero
flags and zero allowlisted files. Only the filtered implementer report was read.
Static checks `static-checks/20261116T092629Z-9316` pass all three checks.

These are Mac protocol-test results, not live CloudKit or peer-convergence
evidence. The original mobile app-hosted lane and the broader Xcode/snapshot
suites await their required checks. No Unit or milestone acceptance is added.

## Mobile source-check follow-up

The retained native result from phone Snapshot
`20261116T220046Z-78844`, clean `cf61cbc9`, executes and passes all 35
`CloudKitSmokeObservationTests` methods. This includes the original failing
`collectionSaveRecordsTheHandedSequenceAndHeldFlag` and the three adjacent
journal-reader/history methods listed above. The integrator inspected the
actual test-case results, rather than inferring them from image counts.
This supplies the pending mobile protocol-test execution, with no new run.
It is not a live account exchange or a full-matrix pass.
