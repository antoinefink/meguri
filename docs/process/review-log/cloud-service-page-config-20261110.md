# Service process initial page-size configuration

2026-11-10. Recorded fixer; own-source implementation from the supplied bounded
handoff. Starting clean base: `cda52ac6`. Original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9` (both supplied by the parent).

The process wrapper accepts optional `pageSize: Int? = nil`, rejects supplied
values outside 1...200 with the fixed configuration failure before launch, and
adds arguments only for an explicit value. Both transport modes keep their
existing framing. The internal CLI parses each recognized option at most once
in either order, checks ASCII digits and integer bounds, and rejects invalid
arguments before storage access with the existing fixed startup classification.
Only the initial `Config.page_size` is selected; Service persistence, defaults,
pagination, tokens, frame limits and production adoption bounds are unchanged.

The five existing selected-bootstrap process constructors explicitly request 32.
No assertions, helpers or other contents in that file were changed by this fixer.
SDK, automatic tests, incoming runtime, FFI, Bridge, app hooks and product
coordinator sources were not changed.

Authored CLI regressions retain actual saved records, system fields and assets
across 32+9 snapshot pages, a real intervening update, original-token replay and
process reopening without an override. A conflicting initial size on a later
reopen must preserve the stored size and token. Both transports and option orders
are covered. Boundary sizes 1/200 and malformed, missing, duplicate, unknown,
overflow and trailing arguments are covered with fixed errors, empty stdout and
no storage creation, including nonexistent sentinel paths. Existing default-page,
database-token, startup-classification and binary cancellation/alignment cases
were left intact.

Authored wrapper regressions cover explicit 32 versus default 200, full actual
record equality, snapshot continuation/replay after a real update and reopen,
persisted page size, and invalid-size refusal before storage creation in both
transports. Selected native fixture assertions and failure/join semantics remain
for the parent's execution.

One source-only consistency check is the runner's sole verification step. No
tests, builds, jobs, Git or network were run; no forbidden/private sources, Anki
sources or raw logs were read. Regression cases are authored, not executed.
There is no unit, gate, signed-sync, acceptance or passing-test claim.

Parent handoff: execute the supplied focused CLI and wrapper commands, the
selected existing-peer adoption case, and the retained complete five-selected
native-case selection. Parent owns compilation, review, formatting if required,
provenance/privacy/similarity checks, commits and integration evidence.

## Root verification

Independent source review: no P0–P2 findings. Root also selected 32 at the two
fresh stores in CloudSelectedOriginalExchangeTests; no other code there changed.
`core-test/20261110T014825Z-35024`: all 10 CLI tests passed, none skipped.
`swift-test/20261110T014849Z-35596`: all 13 tests passed. This includes the Swift
process suite and the previously failing publication/peer-adoption case.
`private/20261110T014912Z-36820` and
`similarity/20261110T014917Z-37076`: passed.
No signed device exchange or unit acceptance is claimed.
