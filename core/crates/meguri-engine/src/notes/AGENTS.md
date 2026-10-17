# notes/

- `update_in` compares normalized fields and canonical tags only; stale `sfld`/`csum` stay on a no-op. Tag canonicalization uses a savepoint so a no-op registers nothing.
- The deleted log is written before row removal, inside the writer op, but is outside the journal. Undo/redo never append to it. Card-removal helpers never log.
- `_in` forms compose inside their caller's transaction; propagate errors (or use `OpContext::step`) so failed composite writes roll back.
