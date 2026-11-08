# iCloud staging transport wait

U113/U114/U151 remain unaccepted. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Unit starts remain recorded in their existing logs. This repair started at `3fcdc237`; its first source checkpoint is `c6fd2697`.

## Contract and source

Spec session `s-20261108T190858Z-d79453ef` landed the reviewed V14.3/V26.2 clarification in `3fcdc237`. Independent contract review found no required fixes. Source and repairs use filtered session `s-20261103T133750Z-f276f58e`; exports: `cleanroom/20261108T190940Z-9992`, `20261108T192531Z-25034`, `20261108T193031Z-30808`, `20261108T193241Z-32177`. The last export includes the prior repairs.

The accepted live download retains its exact page or payload after specific original SDK transport failures. Resume is explicit and respects every reported retry delay. Temporary account-read failure can retain waiting state but grants no request or activation authority. Validation refusals remain terminal. The continuation stays in memory; existing restart cleanup remains.

## Independent review

Two reviewers checked contract conformance, correctness and the failure boundaries. Round 1 found private test-field accesses, lost retry delays and automatic work after temporary account unavailability. Round 2 found a later record callback could still lose a longer delay. Round 3 found no remaining P0–P2 issues. The final partial-error fixture invokes the original installed callback and retains exact-issued and unrelated-item assertions. Native visual evidence remains pending.

## Verification

- Baseline `swift-test/20261108T191103Z-11681`: pass, one real-row adoption-to-review test.
- Initial Rust `core-test/20261108T192300Z-21933`: failed compilation in two new test accesses; retained.
- Initial native `swift-test/20261108T192558Z-26543`: 44/45 test functions passed. The partial-error fixture used an SDK getter that normalizes its input; the repaired fixture drives the original callback without changing production matching.
- Repaired Rust `core-test/20261108T193600Z-36418`: 25/25 passed.
- `core-lint/20261108T193709Z-38641`: passed.
- Privacy `private/20261108T193542Z-36053`: one test comment flagged. That comment was removed; the guard was unchanged. `private/20261108T193650Z-37266`: passed.
- Repaired native `swift-test/20261108T193654Z-37538`: 49/49 native test functions passed; its separate instrumentation runner also passed all seven registry tests.
- App-level `swift-test/20261108T193837Z-39808`: 19/19 passed, including retained download recovery and real-row review. This closes the retained transient-page suite failure; older failed receipts stay retained.
- Similarity `similarity/20261108T193903Z-41249`: 741 source files passed; 1,474 excluded inputs, no allowlist entries. No reference-source report was opened.

## Open evidence

The earlier phone screenshot showed No Decks; no usable phone import or two-device review/media convergence is claimed. The phone was not detected at this resume. Cold reopen and the independent deck picker remain the next live checks. The full batch gate, retained Core14413 responsiveness failure, destructive-reset proof and final acceptance remain open. This log grants no unit or release acceptance.

## Cold-open observation

Signed physical run `run/20261108T195514Z-52888` and preserved inspection `drive/20261108T195629Z-55396` completed. The inspection preserved the invocation and captured Home showing Couldn’t Open Collection, with deck actions disabled. This is a functional failure, despite the successful inspection receipt. Earlier inspection `drive/20261108T194918Z-50387` did not run its script: automation setup timed out; Antoine then unlocked the phone.

No persistence repair was selected from the screenshot alone. Filtered export `cleanroom/20261108T200145Z-58615` adds a Debug-only, fixed-category probe on the existing retry control. It queries global lifecycle after failure and exposes no associated details or paths. Independent source review is clear. `swift-test/20261108T200728Z-61947` passed both diagnostic and original-file recovery tests. Actual category observation remains pending.

Preserved phone inspection `drive/20261108T201042Z-67344` passed two steps with verified continuity. The fixed diagnostic reports `coreResult`, `invalidArgument`, `failed.openStep`, `review-session`. Source review narrows this initializer to native binding and pending-answer journal open/recovery. No data repair follows from that category alone.

Filtered export `cleanroom/20261108T201431Z-69450` adds exact fixed journal-marker categories only. Independent review is clear; `swift-test/20261108T201925Z-70929` passed both focused tests, and `private/20261108T201930Z-72034` passed. Associated error text is never exposed. The actual marker remains pending phone observation.
