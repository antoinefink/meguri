# Oracle source-boundary check

Start: `a450f854`.
Full integration base remains `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

Selected T1 gate `085412Z-97086` ran live conformance successfully:
`conformance/085414Z-97642` has 662 passing, 15 pending and two reference
scenarios, with no failures. Thirteen pending cases need actor/sync adapters;
two need package/backup adapters. These are missing coverage, not passes.

Oracle tests `085511Z-99777` stalled in the host source-reference check.
Its recursive grep spent over ten minutes of CPU reading the Swift
CompilationCache data file. The old check excluded Rust target matches only
after traversal and had no Swift build exclusion. The selected gate was
stopped through documented cancellation. Its incomplete error result is
retained; the oracle test suite is not accepted.

The repair keeps the exact reference pattern and core/app scope. It checks
tracked working-tree files separately from non-ignored untracked files.
That separation retains source tracked under ignore rules. Git errors fail
the check; diagnostics print filenames only. Oracle tests now also trigger
when their host-check script changes.

Six regression cases cover ignored compiler output, tracked and untracked
references, tracked ignored source, scan errors and clean source. Red run
`selftest/091044Z-16500` exposed four old defects. First repair
`091114Z-16909` still missed tracked ignored source; the separate tracked
pass corrected it. Final `091320Z-17970` passed all six cases.

Focused host probe `091412Z-18472` failed before the guard ran because its
temporary harness did not load the function. The corrected harness ran the
actual guard in `091519Z-19510`: pass, under one second. Its temporary MG
family was removed. No oracle source was read by this repair.

Independent source review found no P0–P2 findings. Gate-plan check
`091834Z-21716` passed and selects the oracle suite for this script change.
Complete oracle test run `091933Z-22774` passed all 476 cases with no skips,
including every host check and the repaired source-boundary check. The full
integration gate remains required.
No unit acceptance is claimed.
