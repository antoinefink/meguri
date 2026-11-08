# Native journal unit coverage

Base: `67474fe9`. Integration tooling only; no Unit acceptance.

The existing `native-durability` job now builds the engine test library and runs its `session::journal::` selection alongside the existing integration lanes. It requires the exact test-library artifact, a nonempty result, zero ignored cases and successful completion. Five package builds, the measurement lease, existing filters, deadlines and the no-arguments interface remain unchanged.

Independent review finds no P0–P2 issues. Docker selftest `20261108T223003Z-59657` passes all 19 cases, including the new lane's artifact, empty-result, ignored-result, summary and failure controls. Actual native execution remains pending the separately reviewed recovery correction; these reporting tests do not prove recovery behavior.

Native execution `20261108T224658Z-67153` now passes all 225 checks on the corrected recovery source. The added `journal_unit` lane executes 28 cases with zero ignored; all existing lanes remain passing. This is focused native evidence, not a full batch or release gate.
