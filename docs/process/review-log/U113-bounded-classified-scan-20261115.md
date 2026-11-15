# U113 bounded classified scan

Candidate started at `646cd64a`; integration starts at `4274b869`. Original gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. This repair is awaiting broader verification and unit acceptance.

The original native canonical case timed out during Reconcile at 30 seconds. A smaller same-path measurement placed 92–93% of worker time inside two durable commits per new row. The large-case commit timings were not captured. The repair groups classified row writes in transactions of at most 32 rows and 8 MiB. An oversized row runs alone. FULL/fullfsync settings, guards, deadlines and the original fixture remain unchanged.

Classification and payload facts commit together. Newer failures, repairs, deletions and concurrent observer images retain priority. Completed chunks remain durable; a failed chunk rolls back without retry. Pause hooks flush before waiting, outside tracking locks. Recovery bookkeeping follows commit. Capacity accounting includes retained allocations and stable batch slots.

Author: `cleanroom/20261115T021128Z-26874`. Independent review `022556-29992` found a capacity undercount. Fix `023226-34579` addressed that; review `023612-37674` found buffer growth could invalidate an admitted prefix and the new test used an unsupported setup call. Fix `023842-38272` corrected both. Final targeted review `024048-39851` has no required P0–P2 findings. The pre-existing bounded encoding count still uses temporary reference storage; allocation-free counting is not an approved requirement.

Verification:

- `core-test/20261115T024303Z-41313`: all 16 selected tests pass, including five new batch tests and existing classification, rollback, equal-payload and 150k-index checks. Other tests were filtered.
- `conformance/20261115T024427Z-41743`: the whole native `cloud_checks` binary passes all 11 tests, no ignored cases, with two workers and unchanged limits. The original 1,025-note canonical case passes. Whole-binary elapsed time is 156.51 seconds; this is not a per-operation timing measurement.
- `core-lint/20261115T024807Z-42865`: all three checks pass.
- `private/20261115T024811Z-43141`: no findings in the three source files. Safe implementer report `similarity/20261115T024814Z-43403`: three files compared, zero flags.

The focused native receipt does not replace full conformance. Host scan-race checks, the full integration gate and remaining iCloud acceptance evidence are still required.

Follow-up: `core-test/20261115T024942Z-44194` passes all 18 selected host checks. These cover newer observer repair/failure overtaking, partial scan failure, account/file/suspension fences and equal-payload publication. The remaining 212 cases were filtered. Full integration verification remains pending.
