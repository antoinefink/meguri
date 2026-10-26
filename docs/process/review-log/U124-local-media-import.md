# U124/U122 — Local media-file import

Start `61b5c2ab`; original batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The current entry path refuses media in Rust and Swift. Existing native review
imports use a collection-only package, while media export launches a fixture.
These facts leave the first local media workflow unfinished.

## Contract

The spec writer adds container P16 and collection-replace R11, with narrow input
policy links in file-export E7. R1627's pre-swap media merge and retained old
media remain mandatory. Existing exact-write/trash rules, forced backup and R6
recovery own the transition. Old journal state, per-file recovery and backup
mapping must survive partial failure and process termination. Equal old/candidate
database bytes cannot select the media state without the activation witness.

Independent spec R1 is clear. Patch, intake and review are retained under
`build/unit-intake/U124-media-file-import/`. This is own-source local safety
work; Anki server work remains frozen. No new foreign behavior is asserted.

Implementation and all focused/kill/native verification remain pending. Keep
the public media-input refusal until the complete path is verified. No Unit or
milestone acceptance is claimed.

Public prose was rewritten after privacy-check findings. Final semantic review
confirms that all safety rules remain. One wording change had stopped requiring
counts to be reported before commit; the spec writer restored that requirement,
and independent final review closes it. No privacy rule or exception changed.

## Backend and native draft

Filtered implementation `20261026T141303Z-4619` and repair runs retain the owned
media candidate, R6 recovery, old-byte retention, own-journal snapshot and
pre-commit facts. Reviews R1–R4 found constructor ownership, cancellation,
cleanup lifetime, count delivery and compilation gaps. Their repairs and failed
checks remain in the intake directory. R5 closes the collection-only regression:
new media preflight runs only for a nonempty local media payload inventory.
The original post-activation provider-failure assertions remain enabled.

`core-test/20261026T164146Z-1301` passes all 82 focused package, host, engine and
storage tests with zero skips. These include actual process termination, old
and candidate recovery, equal database bytes, cancellation during hashing,
media collisions and journal state. After the neutral test rewrite, its one
affected case passes in `20261026T170310Z-12739`; the other 25 cases in that
binary were not selected. The independent wording review preserves its six
cases and all assertions.

Native code from `20261026T155641Z-79531` carries typed plan, activation and
recovery facts. R1 found that a missing result could release the owner, and that
plan counts appeared complete. Repair `20261026T162715Z-94220` closes both;
R2 is clear. Test rewrite `20261026T165425Z-7983` keeps both input cases and
every check.

`core-test/20261026T164812Z-4870` passes the 10 selected FFI mapping/progress
checks; 66 other tests were not selected. Privacy `20261026T170311Z-12973`
passes for the staged backend/native files. The source and public guard remain
separate: valid media is supported by the Rust path, but the public FFI refusal
is still enabled until native verification finishes.

WIP only. Compile Swift, run the real-host tests and inspect the built screens
next. The Mac, iPhone and iPad import/media/review/reopen/export case still
needs to pass, followed by the full original-base gate. Keep the replay helper
apart until its compile and privacy fixes pass. No Unit or milestone is accepted.
Similarity `20261026T170708Z-14266` passes for this draft.
