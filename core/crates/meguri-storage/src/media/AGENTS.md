# Media journal lifecycle

- All MediaStore clones share one optional connection. `try_close` never waits for an active journal guard; the engine owns its deadline. `resume` explicitly reopens it. Never reopen from `db()`: retained handles must refuse access while suspended.
- Recovery and stage registration share that same journal boundary. Keep a guard for the whole operation that touches its staged files, so suspension cannot close the journal midway through it.
- Journal v2's `live` means file facts, not sync state. `sync_state` owns protocol live/deleted/awaiting/none; outstanding downloads persist separately even when facts exist. Never advance USN past an unsaved intent. Page/batch APIs require a bound owner and reset epoch; the engine must supply consent and media-job exclusion.
- `collection_import` recovery directories belong to R6, never J4. Its immutable old journal and payloads survive until proven DB/media completion. Ordinary store calls refuse while the shared import owner is retained; recovery uses the same connection.
