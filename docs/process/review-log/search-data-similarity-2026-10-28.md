# Search data similarity

Status: tooling verified; R1881 remains open. No Unit acceptance.
Source at assignment: `e8ef12d03eabaf3175a5503d26c3625e6202eff7`.
The pending integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The earlier two-directory command scanned zero files. The explicit
`tools/mg similarity --search-data -- conformance/scenarios/search conformance/goldens/search`
mode scans paired data, with input hashes and per-channel counts. Default
exclusions, role checks, corpus pins, thresholds and allowlist are unchanged.
Inputs outside the search roots, links, unsupported data and invalid YAML are
refused. Short inputs are reported as limited coverage, never as provenance.

Independent review 1 found area metadata treated as a case and wrapped comments
split below the detection limit. Both were repaired with regressions. Review 2
is clear. A final test-fixture wording repair is independently clear and uses
only Meguri's committed prose; no guard exception.

Receipts:

- `selftest/20261028T013234Z-67540`: 3 MG argument/role/receipt checks passed.
- `similarity-selftest/20261028T013747Z-71093`: 89 passed, zero skipped,
  including scanner calibration.
- `similarity-selftest/20261028T014717Z-76677`: 10 final data tests passed.
- `private/20261028T014718Z-76896`: pass.
- `similarity/20261028T013908Z-71834`: 33 supported source files clear.
- `similarity/20261028T014718Z-77100`: final test source clear.

## R1881 result

`similarity/20261028T013910Z-72520` failed: 140 files, 70 pairs, 3 flagged
files, zero allowlisted, 17 files below fingerprint limits. No input was skipped.
The flagged regions are SQL in `history/answers.yaml`, `history/dst-new-york.yaml`
and `history/dst-paris.yaml`. Their disposition is not approved by this log.
Similarity does not establish independent authorship or exclude short copies.

Keep the prepared search cases inactive until U81/U82. Resolve the flags,
limited coverage and spec-side provenance before acceptance. No reference or
private source was read by the infra author, fixer or reviewer. The real scanner
ran as a black box through MG; no raw reviewer report was passed to authors.

## Expanded batch 4 inputs

`similarity/20261028T062248Z-42310` rechecks the newly merged input set:
150 files, 75 pairs, four flagged files, zero allowlisted, 18 inputs below
fingerprint limits, zero errors, missing files or skips. The three SQL
regions above remain flagged. The new flag is
`conformance/goldens/search/builder/items.json`, line 251, text channel.
The generated value has not been edited by hand. Its provenance and any
required disposition remain open; the unchanged scanner is red.

Keep these cases inactive pending their owning units and provenance proof.
This scan neither accepts the generated result nor changes any threshold,
exclusion or allowlist. No raw reviewer view or reference code was read.
