# Stale callbacks after reopen — 2026-11-11

Role: fixer. Start: `5a4b06e8`. Original integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/cloudkit-records#K11.4`,
`sync/cloudkit-recovery#V2.2`, `#V3.4`, `#V9.2`.

The retained R1 test compared complete entity facts from before reopen with
facts read after reopen reconciliation and rejected stale callbacks. The
focused baseline `core-test/20261111T160458Z-21568` fails this comparison.
Only observation stamps differ: open counter 1 to 2 and sequence 7 to 3;
generation stays 1. All remaining serialized facts match, including content,
marks, revisions, acknowledgements and in-flight records.

Filtered fixer `cleanroom/20261111T160557Z-22030` changes only the existing
test. It checks the completed scan and derives the permitted stamps from the
stored open counter and scan proof. All other transition facts must still
match exactly. The complete snapshot immediately before stale delivery is
then preserved across each refused Saved, build and root operation. Pending
changes and runtime state are also checked after each refusal. Close still
requires complete durable entity equality and the same root acknowledgement.

No production code, time limit or retry policy changes. The original failure
remains retained. Focused binary `core-test/20261111T160738Z-22582` passes all
eight cases; lint `160822Z-22923`, staged privacy `160838Z-23848` and similarity
`160824Z-23263` pass. The initial privacy run preceded successful staging and
is not the evidence for the changed lines.

Independent filtered review `cleanroom/20261111T160913Z-24245` finds no
P0–P2 issues and exports no changes. It confirms that the stored counter and
scan proof independently justify the permitted stamps. This review is static;
the reviewer did not run the host checks. The requested generic reviewer role
file does not exist in this repository. The filtered reviewer reports this
coverage limit rather than claiming that it read that file.

No unit, milestone or full gate is accepted. The broader integration base stays
unchanged. The next full gate must include this test repair.
