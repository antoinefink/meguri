# U19: avoid duplicate T0 execution in the T1 gate

Start: `e129c7ed5e958528fe5cfde4b6b25db1e61ffcd9`.
Infrastructure session: `s-20261018T170659Z-cfb2f362`.

The U38 gate's live-oracle job took 220,466 ms, while its actual T1 report
recorded 18,207 ms. The job repeated native testkit tests, T0 replay and a
freshness pass that also replayed T0 after fresh dual-lane T0 had passed.

Add explicit `conformance --live-only` for fresh T1 workers. Default T0 and
standalone `--live` behavior stay unchanged. Live-only reports only its T1
counts. T1 already rejects missing/stale goldens, so this mode does not run
the refreshing T0 preflight. The gate selects it after dual-lane T0; extend
T0 triggers to include oracle-tool changes. No cached result or new runner
dependency is introduced.

Five focused Bats cases pass: no T0 execution/refresh in live-only mode,
unchanged standalone live behavior, T1 error/stale/missing result handling,
Docker-lane rejection and T0/T1 trigger pairing. Two cases first failed RED
on the missing mode/old gate arguments. The report-schema fixture was fixed
to include its required integer pending count.

Remaining: independent review, a full clean gate against the start commit
and comparison of the actual T1 case/status set with the U38 baseline.
No clean-room excluded sources were read; no UI change is involved.
