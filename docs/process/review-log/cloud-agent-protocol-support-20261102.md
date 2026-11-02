# Signed-agent protocol support

Status: WIP. Shared checks pass; U248 and signed execution remain open.
Component start: `85bbd50d1df241af06e6ec42936c7bd00f1af763`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-agent#J1` through J7.

The test-only support validates commands and retained journal prefixes,
records durable starts, fences uncertain writes, and correlates exact exchange
results. It shares support with the app-hosted entry through the test target.

Independent reviewer: `native_bounds_review`. Initial findings covered
candidate admission, callback order, write failure, false user-step reporting,
an SDK call spelling and stale exchange results. Repairs preserve the original
assertions and guards. Final review has no required fixes.

The file tests first failed at directory traversal. Diagnostic
`swift-test/20261102T031200Z-75084` found a symlink at the first fixture path
component, with a valid parent descriptor. Only the trusted OS temp root in
generated fixtures now uses POSIX realpath. Runtime no-follow guards stay.
Diagnostic prints were removed.

Final `swift-test/20261102T032234Z-79890` passed 38 tests, no skips: 25 protocol
tests plus 13 account-input tests. Privacy `private/20261102T032247Z-81241`
and app-hosted source similarity `similarity/20261102T031807Z-78856` passed.
That scan skipped its directory argument. Exact shared-file coverage passed
`similarity/20261102T032536Z-82945`; the account test file passed
`similarity/20261102T032333Z-81566`. Together these cover all ten Swift files.
Committed-tree static checks `static-checks/20261102T032452Z-82604` passed.

Actual product and receipt checks, fixture delivery, operation recovery,
runtime integration and the app-hosted signed lane remain required. Missing
code fails as implementation_missing. No Unit trailer, VM run, full gate or
signed acceptance is claimed. Keep the batch base above for integration.
