# Native close and sidecar serialization

Starting commit: `712d84f3`; recorded before authoring in
`build/unit-intake/native-close-sidecar/start.json`. Original pending
integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Full native T0 failed the hosted close of `sched/day/config-values` twice:
`conformance/20261028T062248Z-42326` at step 15 and
`conformance/20261028T070615Z-84638` at step 43. The error is the final
suspend completion deadline after writer control succeeds. The second
full run has no native testkit-unit failure; the exact package count repair
is valid. Docker has zero required-case failures. The passing focused
`conformance/20261028T064413Z-68158` covers only its selected scenario.
It does not include the native unit suite or establish full-load recovery.

Source analysis found process-wide sidecar serialization across unrelated
collection directories, including durable publication. Exact phase latency
was not measured. Filtered author `cleanroom/20261028T073125Z-9113` replaces
that long global hold with per-directory native-identity locks. A short
registry operation retains weak entries, rotating bounded cleanup; each
operation retains its strong reference and guard through the read, callback
and publication. Parent aliases share the same lock. Independent review 1
is clear, including lifetimes, cleanup fairness, parent replacement and
recovery after a panic.

Missing-parent reads remain absent; impossible writes fail before callback
side effects. Directory stability still requires collection ownership.
Cross-directory leaf links are not unified: atomic publication replaces the
accessed leaf as before. No new link rejection is claimed. Checkpoints,
full-sync flags and the five-second close budget are unchanged.

- `core-test/20261028T090334Z-39536`: all seven selected sidecar tests
  passed; 25 other tests were outside the filter. Includes independent
  directory publication, alias concurrency, F3/R6/unknown fields, registry
  reclamation, replacement detection and error behavior.
- `private/20261028T090338Z-39798`: pass.
- `similarity/20261028T090338Z-39998`: pass.
- `core-format/20261028T090247Z-38856`: pass.
- `conformance/20261028T090628Z-41367`: all 32 native day-clock cases
  passed concurrently using the normal T0 worker policy.

A new full T0 must still cover the original workload before the timeout is
considered resolved. Earlier failures remain preserved. No new Unit trailer,
performance-budget exception, retry or full integration acceptance.

## Full-workload result after the repair

`conformance/20261028T091118Z-43499` passes complete dual-lane T0 on
clean commit `11c2f23d`: native unit checks pass, all 1,006 scenarios are
considered on both lanes and there are zero required-case failures.
Docker has 638 product passes; native has 637. Inactive errors/failures,
live cases, pending adapters and reference cases retain their classifications;
they are not product passes. The earlier two failed receipts remain intact.

This verifies the sidecar repair against the full workload with unchanged
deadlines, sync flags and normal worker policy. It is not a full integration
or milestone receipt. The original batch base, missing CloudKit workflow
helpers, R1881 findings, six media-driver gaps and other open gates remain.
