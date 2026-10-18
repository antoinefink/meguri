# Media journal lifecycle

- All MediaStore clones share one optional connection. `try_close` never waits for an active journal guard; the engine owns its deadline. `resume` explicitly reopens it. Never reopen from `db()`: retained handles must refuse access while suspended.
- Recovery and stage registration share that same journal boundary. Keep a guard for the whole operation that touches its staged files, so suspension cannot close the journal midway through it.
