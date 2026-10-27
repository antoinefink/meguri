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

## Broad verification and focused harness closure, October 27–28

`gate/20261027T221731Z-55732` used clean `524845cc` and the original batch
base. It preserved one unchanged tracked source throughout execution. Completed
results include:

- `core-test/20261027T221841Z-83505`: 3,492 passed, zero failures, ten skips.
- `native-durability/20261027T224209Z-92680`: 25 passed.
- `conformance/20261027T224303Z-93817`: both lanes passed; 914 scenarios,
  638 product passes, zero active failures. Pending, skipped and reference
  outcomes remain in its report; they are not product passes.
- `goldens-check/20261027T225539Z-10870`: 914 checked, zero changed goldens.
- Harvest report, host/image/privacy selftests, design documents, manifest
  validation and UI lint passed. The design letter-suffix fix remains verified.
- Swift core, the optimized five-millisecond bridge check and Swift UI package
  suites passed. Debug iOS/macOS builds passed. Xcode configuration checks
  completed Mac Debug/Profile/Release and iPhone/iPad Debug build checks; the
  VM Mac unit stage passed 625 tests with four recorded skips.

The broad selftest (`selftest/20261027T225604Z-11753`) passed 729 and failed
four. Three are the retained frozen actor/fault-proxy wrappers. The active
failure was a test-domain error: the positive T1 selector assertion also
required inactive cases without an area manifest, including the new stats
cases. Independent source review established the actual D9.1 activation rules.
The filtered fixer changes only that assertion's domain: supported scenario
suffixes, top-level area metadata, the runner's character globs, selection of
every active nonfrozen case, and unconditional exclusion of every frozen case.
Selectors, activation metadata, argv and trigger requirements stay unchanged.
In-memory cases prove the domain and frozen exclusions.

Native conformance also exposed Criterion's implicit relative output directory
in two existing benchmark-harness tests. Exactly sixteen generated files were
archived after their conformance producer finished, with a hash manifest at
`build/unit-intake/maintenance-gate-fixes/criterion-output-archive.json`. No
source was removed. The fix gives each test an explicit temporary output root
whose owner outlives Criterion. All measurement settings and assertions stay.
The initial filtered author attempt refused the untracked output before any
author ran; the later export from `cleanroom/20261027T233039Z-86389` was
independently source-reviewed and clear. Its source view names the real master
base `524845cc`; its synthetic filtered Git base is not a master commit.

The Xcode Mac app-hosted stage reported 86 passes, two failing tests and two
skips. One failure is the synthetic manual-transfer credential test, with
missing-entitlement error -34018 in the ad-hoc lane; it remains frozen-scope
and signed-lane evidence, with no credential-store inspection or server work.
The other is ScreenSnapshots: 66 pixel mismatches across home.deckList (4),
settings.sync (14), settings.importExport (18) and onboarding.collectionImport
(30). Gallery known issues are separate. The WebKit failed-roundtrip test
actually passed; no WebKit repair was justified.

After capturing these failures the orchestrator stopped the remaining broad
run for focused diagnosis. Its terminal gate and Xcode results are cancelled
and incomplete, never pass. Remaining configuration/runtime checks, other T0
checks and T1 are unrun; all acceptance boundaries remain open.

Focused post-gate verification of the two harness repairs:

- `core-test/20261027T234557Z-97977`: all five benchmark-harness tests passed.
- `selftest/20261027T234647Z-98486`: all eleven conformance-tooling checks passed.
- `core-lint/20261027T234648Z-98709`: pass.

## Visual triage and priority

`snapshots/20261027T234856Z-174` compared one synthetic home.deckList typical
Mac cell without recording. It retains the known signature mismatch and frozen
credential-lane failure. The integrator inspected its actual and diff PNGs: the
old baseline has the custom icon/chevron rows, while the current frame uses a
native hierarchical List. This reflects the requested native redesign, not a
reason to restore the old custom rows. No baseline or tolerance was changed.
The current count colours and wording still need N11.5's trailing secondary
counts and To Review wording. U152 and its related UI work have no completion
trailers and remain unfinished; this capture is not design or release approval.
Keep all 66 failures for the owning UI units and final native design evidence.

The backup/integrity source has focused and broad core/bridge evidence above.
The unfinished UI acceptance, signed/frozen lane limitations and U127's open
performance escalation stay explicit. Follow Antoine's next priority with an
independent, store-only U113 dependency slice, without advertising a provider,
writing collection data or accepting any unit. Preserve the original batch
base for every pending change. Later integration and milestone acceptance still
require their complete active evidence; these receipts do not waive it.

Closure checks for the harness-only delta and this evidence log:
`private/20261027T235715Z-8097`,
`static-checks/20261027T235716Z-8257` and
`similarity/20261027T235716Z-8256` passed. The similarity job's own summary
retains its scanned/skipped inputs; this is source-code verification, not the
open R1881 search-data check. Independent factual review of the log was clear.
