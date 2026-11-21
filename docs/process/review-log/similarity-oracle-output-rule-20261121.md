# Similarity: recorded oracle output in goldens, 2026-11-21

Status: built and reviewed; merge needs a human over the hosted settings guard.

- Decision: Claude (coordinator), under Antoine's delegation of 2026-11-21,
  on escalation `E-20261121T155116Z-246e` (`docs/process/standing-decisions.md`).
- Author: infra session `s-20261121T155857Z-37e12d6c`.
- Change: `tools/similarity` (`scan.oracle_goldens`, `test_data.oracle_pin`,
  `oracle_recorded`, `scenario_sha256`), report action and counts,
  `tools/mg.d/similarity.sh` counts, `.github/similarity.sh` summary and
  settings guard (now also `conformance/oracle.json`), `provenance.md`.

## Rule

A flagged golden is reported as `oracle-output` and does not fail when it has
exactly the recorder's shape, names the current pin, carries its own path as
`scenario_id`, and its `scenario_sha256` is the D3 hash of the paired scenario
as it is now. Scenarios are never exempt. The header can be copied; the rule
that only `tools/mg oracle record` writes goldens, and the T1 `oracle compare`,
cover the observations themselves.

The Python D3 hash (YAML 1.2 core values, RFC 8785 JCS) equals the recorded
`scenario_sha256` of all 1,264 goldens in the tree.

## Independent review

Reviewer: a separate Opus agent that did not write the change, read-only,
without `oracle/`, `refs/` or private data.

Round 1 on `98af44e29`: wiring sound, findings:

- M1: extra keys in a golden or in `oracle` were exempted with the file.
  Fixed: exact key sets, short `tz` and `clock`.
- M2: a copied header passed, since `scenario_sha256` was only checked for
  form. Fixed: bound to the paired scenario's current D3 hash; docs no longer
  say the header binds a recording.
- L1: the pin came from the change's own tree. Fixed: `conformance/oracle.json`
  is part of the hosted settings guard.
- L2: the pin read was less strict than the inputs. Fixed: regular file, one
  link, no FIFO, size bound, all errors caught.
- L3: oracle-output flags said "rewrite this region". Fixed.
- L4: `tools/mg` left oracle output out of passed. Fixed.
- L5: test gaps. Added: nested golden, extra keys, corpus text in `tz` or
  `clock`, stale or copied digest, symlinked and hard-linked pin, a JSON
  scenario shaped like a golden. Not added: a bats case for an oracle-output
  file in the hosted table (the hosted job still blocks only on `flagged`).

CodeRabbit, all answered on the pull request:

- The pin read could drop a malformed suffix: fixed by the size bound.
- `clock` could carry free text (on `98af44e29`): fixed by the scenario
  binding.
- Deleting `conformance/oracle.json` was not selected: fixed, with a bats case.
- A scenario with `clock: {}` let a golden with null `tz` and `start` pass:
  fixed by type checks before the comparison, with a selftest.
- `clock.start` and `tz` took any string: they must now have the D5 forms
  (date-time, zone name), with a selftest.

Round 2 on `1c0c6d728`: M1, M2 and L2 to L4 closed; the D3 hash found
faithful to the YAML 1.2 core schema and RFC 8785 for the forms scenarios
use. New findings, all fixed in the next commit:

- A change to `conformance/oracle.json` alone was not selected by the hosted
  job, so its guard never ran. Fixed: a pin change selects both scans and
  every area (bats case added).
- `tz` and `clock` could still carry eight words. Fixed: they must equal the
  scenario's zone and clock (D5 defaults), as the package interop check does.
- A JSON number too large for a double was turned away only as a side
  effect of the number formatter. Now refused where numbers are checked.
- Hosted bats: added a case where an oracle-output golden in the change
  passes and counts as passed.

## Evidence

- `similarity-selftest`: 107 passed, calibration included.
- `selftest` (`hosted_similarity.bats`, `provenance.bats`): 63 passed.
- All 1,264 goldens in the tree meet the full binding (shape, pin, scenario
  hash, zone and clock).
- Whole tree: 2,528 files, 67 goldens reported as oracle output, 115 flagged,
  all scenarios.
