# Cloud media

- The ledger is local cloud state; never use manual-transfer dirty/USN state as its cursor. Only J6 `Removed` can request a tombstone.
- `ApplyIntent.media` and its note keys are one resolution. Replay decides the move before deciding rewrite keys; digest evidence, not `result_seq` from another opening, proves a committed rewrite. Consume expected intermediate feed states before removing the intent.
- Typed work is transport only. Keep scheduling policy in `../policy`; native asset ownership and engine sends remain the native executor's responsibility.
