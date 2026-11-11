# Preview read diagnosis — 2026-11-11

Role: fixer. Starting commit: `a9584d38`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contract: `sync/cloudkit-recovery#V13`. Status: diagnosis, not sync acceptance.

## Scope and review

The phone's retained preview failure does not distinguish a missing epoch from
a failed read or incompatible epoch facts. A Debug-only probe now records the
original active/epoch read outcome and the prepared operation's existing item
and terminal error codes. It retains no record content or identifiers. A found
record is not proof of a complete epoch. SDK calls, errors, cancellation and
Release behavior are unchanged. Existing failure nodes expose the fixed trace
through Debug accessibility values; visible UI is unchanged.

Author `cleanroom/20261111T114710Z-91653`; review `115244Z-93506` found two
P2s: lost error evidence and reentrant reset ordering. Fix
`124548Z-3041` and independent review `124813Z-16861` close those source findings.

## Native correction and checks

Initial focused tests `swift-test/20261111T115257Z-94063` pass 34 cases.
The added reentrancy case then crashes in `124825Z-17421`. Its owned test crash
stack shows repeated observer callback, `willSet`, and `begin` frames. The test
recursively creates resets before its observer unregisters. Test-only fix
`cleanroom/20261111T125042Z-22803` admits one deliberate reentry, retaining all
assertions and budgets. Review `125236Z-27288` finds no P0–P2; tests
`swift-test/20261111T125248Z-28232` pass 35 cases.

Privacy scan `private/20261111T125318Z-33149` rejects four diagnostic literals.
Mechanical rewrite `cleanroom/20261111T125432Z-33744` changes only their spelling
and exact test expectations. Source logic and UI bytes remain as reviewed.
The guard is unchanged. Final checks pass: `swift-test/125723Z-35477` (35 cases),
`lint-ui/125727Z-36602`, `private/125713Z-35130`, and
`similarity/125727Z-36879`, all on 2026-11-11. The new phone observation follows.

All failed receipts remain retained. The broader integration gate and actual
two-device review/media convergence remain outstanding. No unit is accepted.
