# Qualified dependency warnings

Start: `713fe667ba4fb334500b1e9dd402af978103307b`.
Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/policy#P3`, P14.4; `sync/cloudkit-records#K10`; `sync/cloudkit-recovery#V11`.

A real accepted collection-zone drain now qualifies dependency warnings for server versions received before its captured causal point. Later versions cannot inherit that proof. Local classification, asset retries, staging and reopen preserve coverage for an unchanged server version. Both ordinary fetches and qualified no-state record reads supply the receipt.

The separate V1 ledger stores arrival counters and accepted drain watermarks. Exact schema and stored identities are checked in the private probe before writable source access. Legacy enrollment grants no historical drain. Global `attention_known` remains false; other producers and overall completeness are unfinished.

## Review and test evidence

Author: `cleanroom/20261113T185509Z-18379`. Mechanical correction: `191022Z-27854`. Review 1, `190936Z-26523`, found six P2 issues. Repair 1: `191712Z-32794`. Review 2, `193526Z-43077`, found an unchanged-asset defect and the older performance issue below. Repair 2: `194328Z-49557`. Mechanical correction `195114Z-54925` uses the existing request accessor and removes one redundant comment. Review 3, `cleanroom/20261113T195542Z-58319`, finds no P0/P1 and no new P2. It confirms the asset repair and retains the separate older performance issue below.

- Baseline `core-test/20261113T191156Z-29383` fails exactly at the missing count after a real accepted drain: None instead of one.
- `191222Z-29702` passes seven of nine new tests; two fixture assumptions fail. `193545Z-43546` passes all thirteen selected non-timing cases after repair.
- `195233Z-56257` passes all sixteen focused cases, including unchanged-asset retry and reopen, newer-version refusal, the paused-drain race, no-state completion, shared writers and damaged admission.
- Broader `core-test/20261113T195814Z-61068` passes all 781 selected cloud Host/runtime tests; 278 unrelated cases are filtered.
- The two isolated timing cases pass without changed limits. The one-second backlog check covers Reference entries without retained apply or rekey intents; its setup time is separate.
- `190951Z-27495` and `194956Z-54036` stop on test compilation errors before execution. Their diagnostics remain saved.
- Final correctness lint `20261113T195619Z-59338` and static checks `20261113T195623Z-59955` each pass three checks.
- Similarity `20261113T195620Z-59591` passes all thirteen code files without flags or exceptions. Privacy `20261113T200258Z-61568` passes all seventeen integrated files. The enabled commit hook checks the final staged content.

## Remaining work

The older maintenance path still decodes retained rekey and ApplyIntent bodies through runtime readback and empty intake. `workflow_store.rs` and `rekey_store.rs` remain unchanged from the starting commit. A separate repair must preserve admission checks and add reachable retained-intent performance coverage. This follow-up does not establish J2.1.1 for those states.

Other K10 producers, native Swift failures, the pending batch gate, signed lanes and final device evidence remain open. No Unit, milestone or release acceptance follows from these focused results.
