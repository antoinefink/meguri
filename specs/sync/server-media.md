# Meguri Sync media service

Spec: sync/server-media
Requirements: Meguri Sync U2; R7–R14, R17, R18, R30; F3, F5; AE3–AE6, AE8; KTD5, KTD9, KTD13, KTD16
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/server/ (planned; probe contract conformance/meguri-sync-server#C7)

## M1. Fact boundary and envelopes

B/M/P are `server-transport#T1`. **B:** `sync/wire-format#W15–W17` owns the media result envelope, exact request/result shapes, change tuple and ZIP metadata. Media authentication uses the hostkey and requires no collection round (`sync/media-sync#Y4`). Emit `{"data":<result>,"err":""}` for successful begin, mediaChanges, uploadChanges and mediaSanity. `downloadFiles` returns a ZIP directly. `begin` returns current media `usn` and opaque string `sk`; do not require clients to use sk as a second credential. **M:** Return an empty `sk`; no secret is echoed into diagnostics or URLs. Media and collection USNs are independent; one collection replacement does not reset media.

**P:** C7 verifies current envelope handling, whether empty sk works for both targets, independent media admission and all native method statuses. A required nonempty sk must be sealed before this policy earns compatibility credit.

## M2. Bounds and names

**B:** Baseline limits are 25 requested names/changes, 50 KiB `_meta`, real names at most 255 UTF-8 bytes in NFC without `/`, and addition sizes 1 through 104,857,600 bytes (`wire-format#W17`, `media-sync#Y2`). Baseline ZIPs store entries uncompressed; readers may encounter stored or deflated entries. The download fill threshold is 2,621,440 file bytes, including the file that first exceeds it. **P:** C7 checks exact inclusive boundaries and target forms, plus backslash, NUL, controls, case and normalization behavior.

**M:** A complete upload archive must validate before any metadata mutation. Reject duplicate archive members, duplicate `_meta`, duplicate numeric references, unreferenced file entries, invalid/nondecimal entry names, encrypted/unsupported entries, bad checksums, truncated content and any unmapped addition. Validate `_meta` shape and per-entry sizes while streaming; decompressed aggregate and temporary byte budgets from `server-transport#T4`/`server-lifecycle#L1` apply to expanded archive bytes too. A name is a logical exact string, never a relative path. Reject empty names, NUL, `/`, `\\`, dot and dot-dot names; do not rewrite a rejected name to a different name. Other allowed names remain distinct by exact Unicode spelling, including case variants, independent of host filesystem collation. Store immutable blobs under generated identifiers, never remote names. No archive extraction uses supplied names as paths. These stricter checks are original service policy and need target compatibility probes where a valid client could produce them.

## M3. Changes and listing

**B:** Each effective changed name gets the next media USN. Adding identical bytes and deleting an absent name count as processed but do not advance USN. `mediaChanges` lists at most 1,000 current per-name states with USN strictly greater than `lastUsn`, ordered ascending; a live state's SHA-1 is 40 lowercase hex digits, a deletion's hash is empty (`media-sync#Y2,Y5`). Subsequent edits of a name replace its listing state, not create duplicate current rows. `mediaSanity` compares nondeleted name counts and returns `OK` or `mediaSanity`.

**M:** Retain tombstones in v1. SHA-1 is the protocol content digest, not an authorization token or a cross-account blob address. Verify actual content; an internal blob identity may use a stronger digest but cannot allow a SHA-1 collision to select another account's bytes. No media USN wraps; mutation fails explicitly when exhausted. A read at an ahead cursor returns an empty page, not an automatic restore signal. Counts and cursor equality are never content-recovery evidence (`server-lifecycle#L6`).

**P:** C7 seals per-name replacement ordering, effective-change/no-op increments including repeated names within a batch, page boundary, ahead/negative cursor and count behavior. A client-supplied USN is not permission to delete unseen history.

## M4. Atomic upload publication

**M:** Stage and validate the whole ZIP outside the account barrier. Publish immutable account-private blob bytes durably before the shared account metadata transaction can reference them. Under `server-lifecycle#L4` recheck credential generation, apply all accepted name/hash/tombstone/USN changes atomically and durably, then acknowledge `[processed,current_usn]`. This server accepts the complete valid batch, so processed equals `_meta` length; zero changes may succeed with zero. An invalid entry fails the complete batch, leaving committed metadata unchanged. Durable unused staged blobs are harmless and later cleaned under exclusive offline ownership; never remove a blob referenced by committed metadata.

**B:** `wire-format#W16` permits processed-prefix results and `media-sync#Y10.4` describes client replay/no-op behavior. **M:** No partial-prefix publication is needed for v1. Lost acknowledgement can cause replay; identical content/absent deletions remain no-ops and reviews/collection state are unrelated. A response lost after metadata commit must not make acknowledged blob bytes disappear on restart. Never claim atomicity with a separate collection transaction.

**P:** C7 seals actual target replay, no-op and all-or-nothing compatibility; C6 tests revocation before and during each blob/metadata publication phase using synthetic failures, without claiming a reference server uses this storage design.

## M5. Download publication and integrity

**B:** Requested names are packed in order, stopping after the file that takes file bytes above 2,621,440; at most 25. Numeric entries and object `_meta` follow `wire-format#W17.1`. A missing requested name yields 409 for the whole request, even if earlier names were found. Empty requests may return a ZIP containing only empty metadata.

**M:** Pin the complete selected blob/metadata view under short account coordination, then stream outside it. A later replacement or deletion cannot change the bytes already selected. A missing/corrupt blob is a storage-integrity failure: do not emit a successful truncated ZIP, silently delete metadata, or advance a cursor to hide it. Record a bounded diagnostic and refuse the affected operation; restoration requires verified data. Never infer blob ownership from a request name or hash. Offline garbage collection cannot race a live pinned download because it requires exclusive service ownership.

**P:** C7 covers partial-prefix downloads, missing-after-known names, archive checksums, empty requests, nonzero member lengths, interrupted replies, concurrent replacement, media files near compressed/body limits and fresh receiver convergence. Baseline missing-disk behavior in `media-sync#Y8.3` is not adopted as a requirement to erase evidence of service corruption.
