# Meguri Sync collection reconciliation

Spec: sync/server-collection
Requirements: Meguri Sync U2; R4, R7–R10, R12–R14, R18; KTD6, KTD7, KTD8
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/server/ (planned; probe contract conformance/meguri-sync-server#C4,C5)

## C1. Fact boundary and initial state

Fact classes B/M/P are `server-transport#T1`. This is a server-side contract; client-specific scheduling, UI, undo, backups and held-back reviews in existing Meguri specs are not server requirements.

**M:** A ready account owns an independent schema-18 collection with no user notes, cards, reviews, graves, or media. Create no stock templates, CSS, assets or borrowed default note types. Store account metadata outside the collection. New collection `mod` is 0; `usn` is 0; `ls` is 0. Choose its `crt` in epoch seconds and `scm` in epoch milliseconds at creation. Exact ancillary metadata needed for an empty valid file is a P/C4 result, not permission to import stock content.

**B:** `sync/meta-status#M3` records a new account's `mod` 0, equal-mod precedence over schema mismatch, and `empty` meaning no cards. **M:** `meta` returns committed collection `mod`, `scm`, `usn`; server clock `ts` in seconds; `msg:""`, `cont:true`, `empty` from card presence, current media USN, and `hostNum:0`. No `meta` read mutates collection stamps. Empty-card metadata is never authority to overwrite an account or proof that its other tables/media hold nothing. Migration directions remain explicit client/operator decisions.

**P:** C4 seals all initial stamps and required schema objects/default metadata, plus target behavior for two fresh sides, metadata-only/no-card accounts, mismatched schema, equal mod with different scm, existing-server migration and a fresh receiver.

## C2. Normal-round selection and output

**B:** `sync/normal-sync#N3–N9` and `sync/wire-format#W4–W13` own baseline order, bodies and results. The admitted round captures committed server USN U and client's `minUsn` L, plus `lnewer`. Outgoing graves, objects, tags, cards, notes and reviews are the server changes with USN at least L. Responses describe the outgoing state before the corresponding incoming changes are merged. A single transaction includes all mutations through finish. Do not advance the committed USN for a partially processed request or chunk.

**M:** Use a retained transaction and account coordinator under `server-lifecycle#L2`. Export outgoing row identities/content from the round's consistent view so that incoming chunks cannot erase outgoing obligations or create repeated pagination. Outgoing chunks contain at most 250 tuple occurrences, and terminate with `done:true`; exact sender split and last-empty-chunk behavior follow `wire-format#W7`. Skip no outgoing records because an ID exceeds JavaScript's exact-number range. No server scheduler, card generation, local undo, or tag editor action runs during sync.

**P:** C5 seals inclusive USN selection, snapshots, response-before-merge, chunk boundaries and allowed phase repetition/order; do not extrapolate a client implementation into an unobserved server rule.

## C3. Deletions and object conflicts

**B:** `normal-sync#N4.3,N5.2–N5.9,N6.4` supplies server/peer behavior: apply exact note, card and deck graves without cascading related rows, add grave by insert-or-ignore, and let deletion beat edits already present when it applies. An absent target can still acquire a grave. For note types, decks and presets, an incoming object is accepted unless the stored modification seconds are strictly greater. Accepted note types with a changed field/template count fail the round; acceptance does not generate cards or rewrite notes. Equal object stamps accept incoming content; response-before-merge can leave peers holding exchanged equal-stamp versions. Deck/notetype name collisions use pinned `unicase`, repeatedly appending `+` and stamping the renamed incoming object at server time and U; presets need no unique name. Preserve the documented nonconvergent equal-stamp outcome rather than inventing a tie breaker.

**B:** Tags/config/creation time follow `normal-sync#N5.5–N5.7` in the server role. Existing case-equivalent tags retain spelling and state; new ones get U, collapsed 1, config NULL. The newer side sends the whole config set and creation time, not a keywise merge. If `lnewer` is true the server accepts that set, removing omitted keys and writing received rows with U and merge-time seconds. Otherwise it sends its set. Unsent optional config/creation time is not an empty replacement.

**P:** C5 verifies all these server-role rules, especially a same-round grave followed by a tuple/object for its ID, conflicting grave types, default deck deletion, missing hierarchy, case collisions, config absence versus null, and stamps on renamed or ignored objects. The old client-side no-NFC or tag-parent deviations must not silently become claims about the current reference server.

## C4. Row merges and repeat occurrences

**B:** `normal-sync#N6.1–N6.5` owns the baseline. Review IDs insert only when absent; no duplicate review is assigned a replacement ID. An incoming card/note replaces an absent or nonpending row. A server row is pending for this round when its USN is at least L; a pending row with mod at least the incoming mod stays. Equal card/note stamps can leave each peer retaining its own content. Received arrays are ordered sequences; each occurrence sees earlier occurrences, including across chunks. Never pre-collapse IDs by largest mod or last occurrence. Every occurrence counts against 250.

Accepted notes remove ASCII controls except TAB/LF from each U+001F-separated field, without NFC normalization; recompute sort field/checksum under `text/checksum-and-sort-field#C2–C4` and `text/strip-html#H5`. Require the referenced note type and exact field count; set note flags 0 and data empty, and preserve tuple stamps. A note retained by the pending-row rule is not newly validated. An earlier accepted invalid note fails even if a later occurrence would replace it. Cards do not require existing note/deck rows. Card data conversion is `model/card-data-json`; no unrelated scheduling edit is made.

**P:** C5 seals row selection after same-round changes, received-USN effects on later occurrences, notes' derived-column/storage affinity, unknown card data, and review collision behavior with full row observations. The baseline's unknown-card-data deviation remains an explicit channel limitation, not a claimed reference fact.

## C5. Sanity and stamps

**B:** `wire-format#W12` compares six structural counts after setting due/grave counts to zero. Return `status:"ok"` with compared `c` and `s` when equal, `status:"bad"` otherwise. `normal-sync#N9,N10` records commit before `finish` acknowledgement, rollback on abort/error, and ordinary reconciliation after lost finish replies. Finish returns integer T in epoch milliseconds; the client stores collection USN U+1. Rows retain their specified per-row USNs rather than all being renumbered to U+1.

**M:** Successful finish stores server `col.usn` = U+1, `col.mod` = T and `col.ls` = T, leaves schema/creation stamps unchanged, and acknowledges only after durable commit with generation ownership rechecked. T is the greater of the sampled server epoch milliseconds and the committed `col.mod` at round admission. This keeps an older client from skipping an unseen C7 change when another client syncs in the same tick or the server clock steps back. Never wrap a signed 32-bit USN: stop mutation with an explicit storage/service failure if an increment cannot be represented. A failed sanity check makes the round unfinishable and rolls it back. Neither a client disconnect nor retry is proof that finish did or did not commit. A repeated finish without a live round returns 409; the client reconciles normally.

**P:** C5 seals exact server finish `ls`/mod/usn transitions, same-clock finish behavior, sanity failure retirement, malformed-count handling and lost-ack replay. Until sealed, M stamp details are design policy with compatibility pending, not current-reference observations.

## C6. Full transfer and codec channel

**B:** `wire-format#W14`, `one-way#O6.2,O6.4,O7.4` record full collection bytes, exact `OK`, client upload preparation, and download's incremented server USN/current mod. Full upload/download does not transfer media. Schema-18 format and schema-11 object conversion are `storage/schema-18`, `model/schema11-json`, `model/protobuf-records` and `model/json-numbers`.

**M:** Accept supported verified full-transfer file forms, validate their integrity, and publish through `server-lifecycle#L3`. Store at schema 18. Do not apply client upload preparation a second time or reset every unknown file value. A download pins a committed self-contained SQLite snapshot; required server download stamps are durable before response, and the snapshot's bytes do not change if a later upload replaces the account. Keep foreign nonexecuted file objects and opaque supported blobs on untouched full transfers, but never execute an uploaded trigger/view or attacker-supplied function during validation or mutation. Reject unsafe objects before activation if they cannot be retained safely; report that limitation through C4 rather than silently deleting them. No server tables, account IDs, credentials, media paths or restore flags enter transferable collections.

**M:** All collection connections register `text/unicode-primitives#U2`, including the Unicode 12.1 three-character folding quirk. Preserve unknown JSON through `other` as `schema11-json#S3` permits; preserve unknown protobuf bytes on unchanged/full-file channels and on compatible binary edits per `protobuf-records#P4,P5`. Unknown binary fields with no schema-11 representation cannot travel over JSON normal sync (`wire-format#W6.5`); do not promise otherwise. Do not regenerate unrelated templates or presets. No schema migration earlier than 18 is required until a current-client probe establishes a necessary input form; schema-11 objects on the normal wire are independently required.

**P:** C4 seals exact current upload/download versions, post-upload/download stamps, WAL-independent file integrity, optional older migrations, noncanonical/unknown fields and foreign-object acceptance. A missing conversion fact blocks only the affected compatibility claim, never permits translation of existing production code.

## C7. Standalone MCP note writes

**M:** A collection MCP write joins the account publication barrier in `server-lifecycle#L4`. Revalidate the exact grant, account credential generation and pinned collection generation before admitting a short standalone SQLite transaction and before committing it. A live native round owns its retained writer. While this writer or a competing publication prevents admission, return the bounded retryable busy outcome in `servers/meguri-collection-mcp#M7`. Never abort a native round to admit an MCP write. A request already pinned to a replaced generation fails without mutating either generation. One account's blocked writer does not block unrelated accounts.

**M:** On create, use the persisted note, tag, sort-field and checksum rules in `notes/note-records#A1–A3,N1,N2`, `tags/tags#T4` and `text/checksum-and-sort-field#C1–C4`. Use the clock-based identity contract in `storage/ids-usn-mtime#I1–I3` and the GUID format and generation contract in `text/guid`. Allocate under the writer and keep existing IDs and GUIDs stable. Edits patch the live row after checking the submitted revision against current persisted state and the field layout. Normalize new field values and preserve untouched field bytes. Apply additions and removals to the stored tag set and canonicalize through the existing registry. Recompute `sfld` and `csum` from the saved fields when the note changes. Never overwrite flags or data from the sync-ingest path and never mutate unrelated rows. A valid no-op keeps all persisted state, stamps and the revision unchanged.

**M:** Generate the cards required by `notes/card-generation#G2–G8,G10` for the saved note. Use `notes/deck-placement-and-position#D1–D3,P1–P4` for the new cards' home/placement, position and any `nextPos` update. Existing cards retain their identity, placement and scheduling state. Store the note, new tags, new cards and any counter update in the same durable transaction. An invalid input, invalid note type structure or required card failure applies none of the mutation. Use the existing SQLite schema and page-local updates. Preserve `col.scm`, `col.crt`, `col.ls`, unknown untouched values, and account/native/media metadata.

**M:** Read the current committed server USN U under that writer. Stamp the changed note and each new card, tag and changed config row with U. Note, card and config modification timestamps use sampled epoch seconds. Set `col.usn` to U+1 and `col.mod` to the greater of sampled epoch milliseconds and its previous value plus one. Reject stamp overflow before committing. Native rounds select these rows by C2's inclusive USN rule. The native finish and download paths must not roll a committed publication's `col.mod` backward. Full upload starts a different publication under C6. Acknowledge after the commit. An interrupted response alone does not establish whether a write committed.

**M:** MCP creation and edits that add cards use the native timestamp ID space. Independent offline allocations can choose the same ID. Apply C4's existing merge rules to matching note and card IDs, including notes with different GUIDs. A collision can leave one surviving row or leave the peers with different rows on a tied modification stamp. Apply C3 to graves and whole-config selection, including `nextPos`.

**P:** Verify native sync from a common baseline after creation and editing with distinct note and card IDs, including notes created independently on a device and through MCP. Cover same-second and same-millisecond clocks, a backward clock, and an intervening normal round from a different device. Verify note and card identities and references, derived fields, tags, counter and inclusive USN selection on each synced device. Exercise overlapping position allocation with both `lnewer` values and verify the next allocation follows P1–P4 using the config chosen under C3. Cover both edit and no-op behavior, lost replies and rollback at a deliberate write failure.

**P:** Use separate generated fixtures to characterize matching note IDs with different GUIDs and matching card IDs. Record final rows and references on the server and peers under C3 and C4. Acceptance for incremental delivery across independent creations uses distinct note and card IDs.
