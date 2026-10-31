# Native conformance scheduling

Source start: `8d7355e7a6d447737ca7a0ce97fd86dba39495e9`. Original pending
integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The previous full gate exhausted host process capacity after a native executable
passed 1006 cases. No historical leak was proved. The bounded repair preserves
all binaries and scenarios, serial binary execution, failure aggregation and
scratch ownership. It explicitly limits native harnesses and T0 CLI workers to
two, or one with MG `--jobs 1`. Docker and T1 scheduling retain their defaults.
Standalone `Suite::run` retains its behavior; `run_with_jobs` is additive.

Prepared by filtered fixer `cleanroom/20261031T135923Z-89188`, session
`s-20261025T162405Z-9dc656e4`. Independent correctness review found no P0–P2.
One test gap remains: the freshness stub supplies its own worker argument;
the production freshness call was separately inspected and supplies the bound.

Verification:

- Baseline receipt: `selftest/20261031T140625Z-91690`
  failed four of seven cases, detecting the missing bounds.
- Integrated runner: `selftest/20261031T140700Z-92389`, seven passed.
- Actual worker and CLI coverage: `core-test/20261031T140835Z-93707`,
  48 passed, zero skipped.
- Native one-worker synthetic smoke: `conformance/20261031T140938Z-94851`,
  one scenario passed. Start/exit evidence records the real executable.
  Selected tool process counts returned to baseline; scratch cleanup completed.
- Privacy: `private/20261031T140920Z-94169`, pass for all seven source paths.
- Similarity: `similarity/20261031T140923Z-94448`, pass.

The smoke test does not prove the complete native suite or explain the previous
exhaustion. The original-base full gate remains required. Individual cases can
create resources beyond their harness slot; no assertions, limits or scenarios
were removed. No unit acceptance is claimed here.
