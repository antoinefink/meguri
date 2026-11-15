# U113 replay and original media reply: R21

Starting commit: `04961f7a862f52f2eaf7d6896f9572274fa890b9`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Source retained in WIP commits `5fe698f3` and `409fe38b`.

The full Rust run `20261114T230658Z-19254` had 5,142 passes, eight failures
and ten existing skips. Four replay cases used an old generation after a child
reopened. A new validated media-note rewrite now records its current generation
before committing; historical operation evidence remains unchanged. Three
positive testkit cases and one FFI case lacked accepted complete epoch metadata;
they now receive canonical metadata through actual intake. Unknown stays the
shared default.

The new regression additionally exposed a lost original media reply: the actual
transport retained its input and asset owner only on its stack, so a tracking
stop could return before consuming its completed error. The harness now retains
the exact original continuation through repeated holds and returns its outcome
once, without replay or another SDK call.

Test-first `core-test/20261115T004709Z-83951`: one success control passed;
the original-error test failed because it returned success. Final focused run
`core-test/20261115T005929Z-89863`: all 21 tests in the three affected integration
binaries passed, zero failed or skipped. The intervening compile failure
`20261115T004915Z-84357` is retained: Input lacks Clone. The correction copies
only its cloneable payload values and preserves the original asset owner.

Filtered author/fixer: `cleanroom/20261114T235807Z-28135`.
Independent review: `20261115T000631Z-31042`, no P0–P2 findings.
Compiler correction author: `20261115T005622Z-86166`; independent review
`20261115T005734Z-88328`, no P0–P2 findings.
Integrated bytes match those exports, with only the separately reviewed compiler
correction layered on the eight-file candidate.

`core-lint/20261115T010228Z-92182`: all three checks passed.
`private/20261115T010232Z-92506`: eight files passed.
`similarity/20261115T010236Z-92823`: eight compared, zero flags; only the safe
implementer report was read.

These focused results do not accept U113 or P24. The full gate
`20261114T230301Z-6779` remains cancelled and incomplete; native and Swift failures
require separate repairs and a new gate from the original integration base.
The real reverse peer review and fresh-media evidence also remain outstanding.
