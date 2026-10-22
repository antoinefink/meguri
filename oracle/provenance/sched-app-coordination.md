# App-side scheduling coordination (U51)

Spec-writer notes for `specs/sched/app-coordination.md`. The spec is
Meguri-defined (no oracle parity); its rules follow from
`specs/sched/day-clock.md` and `specs/sched/fsrs-policy.md`. Read at tag
26.09.3 (`refs/anki-26.09.3`) only for the one sentence comparing Anki's
desktop client; no code was copied and no probe was run.

| Spec section | Anki files read | Read for |
|---|---|---|

A3's instants were computed from `specs/sched/day-clock.md` D3 and D4 with
an independent script and the system's zone data, and agree with the
`sched/day/offset-change-recompute` golden and its deviations
(1901152800, 1901239200).
