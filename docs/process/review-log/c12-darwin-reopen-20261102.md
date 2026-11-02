# C12 Darwin reopen repair

Status: repair under focused verification; no Unit acceptance.
Component start: 5b0a661d100019136038a4562500b83af08eccd5.
Code start: b264c2a67f5cb3af54ef617935c0e822a0538567.
Integration base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.

Antoine requested direct Mac testing on 2026-11-02. Xcode MCP uses My Mac
for this feedback pass; required VM and release evidence remains outstanding.

The direct Unit run executes three selected production-owner methods: two
import/review/close/reopen checks pass; the media/adoption method fails before
its workflow because its generated input environment is absent. That failure
is retained in build/direct-mac-xcode-reviews-20261102.json. MG supplies the
real Rust-generated fixture and native helper for the isolated method at
swift-test/20261102T201118Z-239: one pass, no failures or skips. Its network
transport is generated; this supplies no actual CloudKit network result.

The current Mac app builds and launches through Xcode MCP. Direct inspection
shows that ordinary collection opening fails. The debugger identifies the
pending-answer journal's native identity check. Read-only inspection inside
the app establishes equal canonical path and inode, a different device number,
and a complete version-1 journal containing only its header. No collection,
journal, account or settings were changed by diagnosis. No identity values
or collection contents are retained in this log.

Spec-writer bf50 prepares the C12 amendment. The reviewer finds two P2 issues.
The new header must keep the counter reached during recovery. The report must
count answers applied by recovery separately from the migration. Both changes
are reviewed and cleared. A further review clears the rewritten header paragraph.
Spec commit b264c2a6 records the local format change; staged privacy7910 passes.
The report uses fixed Rust tracing after durable migration and before Ready.
Existing replacement and corruption refusals remain; no new UI or FFI is needed.

Implementer264d starts filtered authoring from clean b264c2a6, original96982.
The source copy is sealed. No implementation, native API or actual repaired
reopen pass is claimed yet. A separate Xcode snippet encounter supplies no
native API proof because compilation/linking did not complete successfully.

Prepared SDK-runtime work is retained separately in
build/cloud-agent-injection-reviewed-seed-20261102.patch. It passed the shared
21-wrapper focused run96270; final test-context wording passes focused5433
and privacy. Actual embedding, signing and frozen launch remain unverified.
Xcode's automatic catalog extraction is preserved separately in
build/direct-mac-catalog-extraction-20261102.patch, outside accepted UI changes.

The generated-file API probe subsequently succeeds inside the running Mac app:
fgetattrlist returns0 with40 bytes and the requested nonzero UUID attribute.
Only result flags and layout are retained; the temporary file is closed and
removed. This proves current app access to the API, not the Rust implementation.

Filtered8862 implements the storage reader, versioned journal migration and
recovery integration. MG formatting completes. Focused Docker run14064 passes
17/17 storage/journal unit cases; run15044 passes21/21 production recovery cases.
Privacy14339 passes every changed source file. Independent code review finds
one P2: the worker must retain the published journal binding through append
reopen. A file substituted in that interval must refuse open. That repair and
its boundary case remain required, along with native C12 suite selection and
gate triggers. No repaired app launch, native Rust result or Unit acceptance
is claimed by this WIP checkpoint.

Fixer4626 exports15841 from c8d179b1. Round2 independent review clears the
published-binding repair and native suite routing with no remaining P0–P2.
The worker retains the landed file through append and checks its inode, exact
bytes, path and live collection before changing state. Both migration reasons
are covered by a byte-identical substitution onto another single-link inode.
Focused Docker17656 passes18/18 selected unit cases; Docker18392 passes100/100
recovery, lifecycle, fault, suspend and WAL cases without skips. Wrapper17418
passes16/16. The unprivileged formatting attempt16774 cannot reach Docker;
the authorized OrbStack run17131 completes. No test assertion or limit changes.

Privacy18131 flags only the new regression function. A filtered whole-region
rewrite18901 is required before committing; production code stays unchanged.
The original native-durability process is now running directly on this Mac.
No repaired user-collection launch or actual CloudKit network pass is claimed.

Native19394 completes165/165 without skips:19 suspend,20 WAL,21 recovery,
1 collection FFI,15 cloud records,29 cloud recovery,59 cloud unit and the exact
Darwin FD identity case. Independent comparison clears privacy rewrite18901;
only test-local names/prose change. Format21447 and privacy21733 pass.
Lint21993 finds one type-complexity diagnostic in the C12 service helper;
that source cleanup remains required. No lint suppression is permitted.

Filtered22490 names the existing tuple with a private alias. Independent
review confirms identical representation and behavior with no suppression.
Core-lint53959 then passes4/4 and privacy54419 passes. Final rewritten
regression22763 passes1/1. Static22858 passes3/3 against the original batch
base; similarity23238 passes11 code files with no flags or exceptions.

Team-signed standalone Mac build31742 passes with no warnings or errors;
signature, cloud configuration, app/widget architecture and normal sandbox
checks pass. It contains source0798da7f, before the equivalent alias cleanup.
Xcode MCP stops the older debugged copy. CUA launches exactly that new product,
but cannot capture a window. Doctor54708 confirms host-session is missing:
the Mac is locked or its display is asleep. A bounded app-only stack sample
shows a normal idle main event loop; it establishes no repaired reopen result.
The operator has been asked to unlock the Mac for direct visual verification.
No manual journal reset, collection replacement or security bypass occurred.
Actual user-collection reopen and real CloudKit network proof remain unverified.
