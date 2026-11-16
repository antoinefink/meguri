# server/

- New first-party sources are MIT. No core dependency before U5's audited public-model reuse. Never read reference/runtime trees to implement this workspace.
- U4's `substrate-v1` generation marker is deliberately not a compatible collection. U5 must install and validate a real collection through `AccountStores`; never advertise an empty file as one.
- Lock order: target publication barrier, then short metadata transaction. Never hash, stream, await a second barrier, or wait on a client inside either. The round-retirement hook is synchronous and must not wait for clients.
- `test-hooks` is empty by default and must be absent from release artifacts. Tests use actual loopback HTTP and private temporary SQLite files; integrator runs them through MG.
- Owner-root parent is operator-controlled and cannot be renamed by another same-UID process. Keep the root and its stable lock inode outside restore replacement (`state/` only); never unlink either lock.
