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
