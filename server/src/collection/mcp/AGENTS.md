# Collection MCP

- The outer adapter supplies the server-owned path, exact authority/publication guard and bounded blocking-worker capacity. `Authority::admit` must try admission, reject active native rounds, and never retire them. Retain the guard through the final check and durable commit.
- Parse raw arguments with `Backend::parse` before conversion to a JSON map; conversion loses duplicate-member evidence. Map backend errors to MCP tool errors, not the existing HTTP `Error::status` mapping.
- `WriteRuntime::random_position` must call the core's exact seeded P4 calculation. It is deterministic in the counter, unlike GUID randomness. C7 uses native timestamp identities and existing same-ID merge semantics.
- Request opens disable automatic and last-close WAL checkpoints. The coordinator must bound admission and arrange checkpoint maintenance outside point operations. Never add validation scans, migrations, file hashing or snapshots to these paths.
