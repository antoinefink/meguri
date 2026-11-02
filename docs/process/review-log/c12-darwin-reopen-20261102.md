# C12 Darwin reopen repair

Status: implementation pending; no Unit acceptance.
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
