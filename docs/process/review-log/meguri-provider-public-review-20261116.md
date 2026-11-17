# Provider policy integration evidence

Integration starts at `aa17e03e`. Herdr author `t1dt` prepared the original
MP1-MP5 contract under recorded spec-writer session
`s-20261116T213712Z-7e669259`. Independent reviewer
`incoming_contract_review` found one required P2 about excluded media. The
author corrected it; two later reviews cleared the policy and exact final
integration candidate. A final review also cleared the three shorter scope
notes. Existing CloudKit contracts and the legacy freeze keep their scope.

The same reviewer read the complete contents of each file below, including
all literals, examples, tables and paths. Each contains public policy or
protocol facts and synthetic data, with no actual private values or new
licensing concern. Existing approved spec provenance remains required.

| Exact path | Reviewed SHA-256 | Lines |
|---|---|---:|
| `specs/sync/meguri-provider.md` | `a3ba46013feac12fb76e211b7ff4660bf50fa13e8055ec71426e2d3d2fa0c899` | 79 |
| `docs/process/review-log/meguri-provider-contract-20261116.md` | `70b7eb39ea3f7ea6886d3d1f82717e81be96938d078147a01947f912794ef7f9` | 61 |
| `specs/sync/one-way.md` | `8de7695fada9f38fa5d6c54c26538d025cd7e3b45b1867a1b8650417533b0812` | 390 |

Under the standing routine-admission rule, the integrator verified these
bytes and added only their exact path/hash entries. Matcher logic, tokens,
thresholds and private-data access are unchanged. Future edits remain
checked. Original failed privacy receipts `215054-71335` and `215354-73886`
are retained; no matched private token was read or reported.

Final source checks before this evidence note:

- `private/20261116T215701Z-74440`: all 12 supplied files pass.
- `static-checks/20261116T215702Z-74438`: three checks pass.
- `selftest/20261116T215705Z-75192`: the scanner regression case passes,
  with zero failures or skips.
- `similarity/20261116T215732Z-75191`: one eligible file passes with zero
  flags or exceptions; 11 documentation files are outside that scan.
- `design-docs/20261116T215229Z-73443`: document consistency passes; this
  is no rendered design approval or native-test execution.

The contract is ready for implementation. No provider client code, UI,
peer acceptance or full-gate pass follows from these records. Keep the
failed application gate and original base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Admission WIP snapshot

Independent non-author `incoming_contract_review` read the complete public
review note `docs/process/review-log/meguri-provider-admission-20261117.md`.
It found no private values or unresolved source or license concern.
The reviewed file has 85 lines and 4,671 bytes. Its SHA-256 is
`31af001e272ebcc61ca299e3d932f6f29bc5da69551389305956ef92cd9f0e40`.
The integrator verified the exact bytes and added only this path and digest.
Matcher rules, tokens and private-data access stay unchanged.
This classification grants no technical acceptance.

The WIP source and evidence pass normal privacy
`private/20261117T011638Z-48327` (40 supplied files) and static policy
`static-checks/20261117T011639Z-48624` (three checks).
Scanner regression `selftest/20261117T011637Z-47959` passes
1 case with zero failures or skips.
The earlier failed privacy runs remain retained.
Metadata similarity `20261117T010946Z-46041` passes its one eligible file;
the integrator read the filtered report. No required gate is cleared.

## First WIP execution

On clean `eb891ba8`, `core-test/20261117T012115Z-52998` passes three
selected tests with zero failures or skips. These cover FFI ownership and
the original media terminal receipt during handoff. Other tests were not selected.

`swift-test/20261117T012905Z-56916` fails before Swift compilation or tests.
The ordinary FFI library cannot resolve `serde_json` at `provider.rs:47`.
The manifest declares it only as a development dependency.
The selected Rust test build had that dependency; the Apple library build does not.
No Swift test ran. Keep the failed receipt and repair the manifest.
Both coordinated CPU windows ended at their terminal receipts.
