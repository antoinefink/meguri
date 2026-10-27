# Maintenance integration, 2026-10-27

Status: incomplete; no unit, batch, milestone or release acceptance.

Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
This log supplements the existing U124a, U126, U127, U132 and U133a logs.
It does not add a seventh whole-unit review round.

## Original gate

`gate/20261027T193251Z-31655` started on clean `c6c4e223`.
Prepared oracle and spec commits arrived on master during execution, through
`a929195a`; later checks therefore cannot prove one unchanged integration tip.
The orchestrator stopped this invalidated run rather than finish the remaining
configuration and UI checks against mixed inputs. Its terminal result is
`error/cancelled`, not pass. Preserve its result, completed nested results,
`results.jsonl` and log. T1 and unfinished Apple checks remain unrun.

Completed stages included:

- Rust: 3,302 passing tests, two failures; the complete receipt also retains
  skipped tests. The active failures were collection-entry cancellation at
  public `Committing`, and the incomplete exact search-error message table.
- Native durability: 25 passing checks.
- Conformance: failed native testkit checks, despite no product scenario
  differences in its reported T0 lanes. The generated fixture expected an
  uncanonicalized path, and native testkit execution lacked the case-sensitive
  scratch already used by CLI execution.
- Tooling: 721 passing selftests, five failures. The lease test used elapsed
  time instead of proving contention; three remaining wrapper failures concern
  actor/fault-proxy ownership; the manual-transfer failure is frozen scope.
- Golden/schema checks, harvest report, host/image/private selftests, design
  documents, milestone manifest and UI lint passed their completed stages.
- Swift core: 128 tests; its separate Profile stage passed. Swift package stage:
  549 tests passed. Both Debug app builds passed. These completed results do not
  supply the cancelled configuration/UI checks or a passing full gate.

## Scoped repairs and independent review

Filtered authoring sessions used the existing implementer/fixer records. Review
read only Meguri code, own contracts and exported patches. No Anki source,
private data, raw similarity report, VM driving or test execution by reviewers.

- `1365cd7f`: spec-writer clarification of package/container P16.3/P16.4.
  Reversible preparation stays cancellable; public `Committing` follows proven
  activation and is delivered outside core/sink locks.
- `2ec0ba9d`: entry repair. Owned activation, safety, media and recovery facts
  survive late cancellation. Initial/replacement and media/no-media tests retain
  the real post-rename directory-sync fault and both public cancel boundaries.
  Review R1 rejected moving the test to an internal checkpoint alone. The
  clarification and revised implementation passed independent source review R2.
- `e2a5cbbc`: all six approved search failure messages and exact payload
  substitution. Independent source review clear; no query engine or limit change.
- `b46f4c96`: native conformance scratch and generated fixture path repair.
  Cargo builds outside scratch; a complete successful JSON stream must identify
  nonzero executable targets before any run. Every target executes under the
  run-owned case-sensitive volume with child-only TMPDIR. Failures aggregate.
  CLI/replay use the same ownership and cleanup. R1 found a cancellation-test
  teardown gap; the filtered fixer repaired it and R2 was clear.
- `abb553ab`: deterministic lease contention and a complete neutral source
  fixture. Independent source review clear. The worker-holder fixture addition
  is not claimed to resolve the actor/fault-proxy failures below.

Review packets and authoring receipts are retained in
`build/unit-intake/maintenance-gate-fixes/`,
`build/unit-intake/conformance-native-gate-fix/` and
`build/unit-intake/shared-tooling-gate-fix/`.

## Focused verification

- `core-format/20261027T204449Z-54150`: pass.
- `private/20261027T204520Z-55648`: all eleven repair/spec paths passed.
- `core-test/20261027T204623Z-59501`: 51 selected regressions passed, including
  the complete collection-entry binary, exact search-message/property tests and
  the backup adapter promotion/path test. Filter exclusions are retained in the
  receipt; this is not a complete workspace pass.
- `selftest/20261027T204623Z-59500`: 22 passed, three failed. Lease contention,
  native scratch/target enumeration, error aggregation and cancellation tests
  passed. The three enabled actor/fault-proxy tests remain failed.
- Diagnostic `selftest/20261027T205252Z-72905` proved the plain no-server oracle
  call succeeds, then actor compare fails with `sync-faults`: the neutral stub
  does not answer the retained-route ownership discovery. These are frozen
  server-only failures under review-first-delivery; no server repair follows.
  The temporary owned diagnostic patch was removed after capturing this result.
- `conformance/20261027T204623Z-59572`: failed. Actual Cargo JSON
  identifies 46 test executables with a successful terminal build, including the
  `harness = false` conformance target. That target then failed to find its
  relative scenario directory: direct execution must preserve Cargo's package
  working directory, not the workspace directory. This needs a focused harness
  repair; target discovery alone is insufficient. Other test binaries continued.
  The subsequent 748-scenario CLI report has zero failing active T0 cases;
  inactive errors, differences and pending cases remain explicitly recorded.

The later oracle-only commits do not change the reviewed Rust/tooling inputs;
all receipts retain their own exact source and tree state.

## Remaining boundaries

U127's six-round performance disposition and escalation
`E-20261027T193208Z-235c` stay open. The measured maximum exceeds the unchanged
2-second limit. No new performance experiment, waiver or Unit trailer is implied.

Use the original base again for the next full gate, including all new commits.
Keep the frozen checks and their failed results. No pass is inferred from them.
Release still needs Apple's account step, Antoine's device checks and trial days.
Proceed toward iCloud after repairing and checking the active changes.

## Held vocabulary merge and final numeric repairs

`b3f84156` adds strict parsing for both held-scenario handoff sections, without
activating adapters. Independent review found the cached browser-mode actor
check; the filtered fixer repaired it and the second review was clear.
`core-test/20261027T210411Z-85225` passed 13 focused tests.
`conformance/20261027T210619Z-86562` parsed all 166 prepared cases with matching
recorded hashes: zero invalid cases, 147 pending and 19 unsupported observations.
Those inactive outcomes are prerequisites for intake, not product passes.
`e9d8914d` merged only `oracle/batch-2`, including its held-scenario history.

`c6cf0af1` corrected the native executable working directory described above.
`selftest/20261027T210206Z-83478` passed all 22 native scratch, package-directory,
lease and cancellation checks. Actual dual-lane T0 remains required.

The first post-merge golden check (`goldens-check/20261027T210939Z-89761`)
rejected three valid observation integers using authored scenario limits. The
focused run (`core-test/20261027T211231Z-92144`) also found stale package-count
and malformed-search diagnostic expectations. Preserve both failed receipts.

`a55af9b3` clarifies the existing observation/scenario contract. `303ebf64`
separates observation conversion from authored-input bounds. Generic observation
parsing retains C3 floating conversion beyond the 64-bit range and JSON negative
zero, while authored integer lexemes retain D2 bounds. All original golden
values and hashes remain unchanged. Package coverage includes every recorded
case; no adapter or capability is activated. Independent context review R3 was
clear. `289b42ad` adds a non-panicking YAML minimum-integer range check and
flattens the conditional flagged by lint; its independent scoped review was
clear. These are integration-delta reviews, not additional whole-unit rounds.

Final focused receipts on the repaired source:

- `core-lint/20261027T215643Z-16607`: pass.
- `core-test/20261027T215654Z-16966`: 91 passed, zero skipped.
- `goldens-check/20261027T215805Z-17686`: 1,948 files, zero schema errors.
- `selftest/20261027T214750Z-11914`: schema conversion selftest passed.
- Privacy scans of the nine-file repair and final three-file delta passed.

The original batch base, frozen-server dispositions and U127 escalation remain
unchanged. No Unit trailer or integration acceptance is added by these repairs.

## Fallback-drop test preparation

`gate/20261027T220106Z-19754` ran on clean `85b9ebc0`. Static checks,
canaries, lint and dependency policy passed. Its Rust stage captured
`meguri-host::lifecycle::production_backup_registration_allows_fallback_drop_and_reopen_without_close`
failing the immediate weak-engine assertion. The orchestrator stopped the gate
after capturing that active failure, avoiding remaining verification of the
known-bad test. Its terminal result is cancelled/incomplete, not pass; the
remaining T0/T1 checks were not supplied.

Independent source diagnosis found two finite engine owners: derived builders
and scheduled maintenance. Both can temporarily hold an engine reference.
The production backup provider remains weak.
This regression deliberately refuses normal close using an export handoff; it
does not prove successful close's provider-join guarantee. D7 permits refresh to
stop at its next batch. The filtered fixer prepares this provider-cycle test by
waiting on the existing derived and maintenance idle barriers before taking the
handoff. Every immediate final weak-engine, writer, reader, retained-provider
and reopen assertion stays unchanged. No sleep, polling, retry, production
change or timing-limit change was introduced.

The first filtered export omitted a type qualifier. Independent review and
`core-test/20261027T221436Z-52539` both caught the compile error; retain that
failed receipt. The corrected export from `cleanroom/20261027T221515Z-53151`
uses the file's existing fully qualified duration convention. The scoped final
review is clear. `core-test/20261027T221606Z-53831` passed all 21 lifecycle
tests with zero skips. `private/20261027T221616Z-54249` passed. These are scoped
integration repairs and do not add another whole-unit review round or acceptance.
