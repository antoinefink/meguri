# Native replay accessibility diagnostic

Starting commit: `d06dedc3`; original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Filtered fixer: `s-20261103T133750Z-f276f58e`.
Exports: `11176`, `16629`, `22941`.
Only the synthetic replay test changes; product behavior is unchanged.

The original package run `7686` passes 753/754 cases. Focused `13632` shows a
WebKit remote child outside the test's protocol reader, unchanged over ten seconds.
The public client probe `24928` compiles and records trust=false and error -25208
for two count calls. It returns no native nodes. This proves neither permission
failure nor missing product controls. Actual value-copy support is still unknown.
Native link counts, identifiers, labels, unique matches and activation assertions
remain enabled and fail. No accessibility or unit acceptance is claimed.

Independent review `build/cloud-replay-public-probe-review-20261109.md` confirms
bounded own-process public queries, background ownership, typed values and fixed
facts only. An extra DOM query was removed to retain the original time budget.
The final compiler correction changes only one public role constant; root checked
byte equality apart from that replacement. No signature or consent was changed.
The next step is one bounded value-copy check, then the proper native UI harness
if this package process cannot expose its tree. No longer wait is planned.
