# server/

- New first-party sources are MIT. The sole first-party dependency is `meguri-model` with defaults disabled; its files retain MPL terms. Never read reference/runtime trees to implement this workspace.
- Collections are schema-18 files selected by `current-collection`; account metadata stays outside them. Full transfer preserves whole files and opaque bytes. Never run uploaded triggers/views or rebuild unrelated records.
- Lock order: target publication barrier, then collection state, then a metadata writer if needed. Full collection operations use only an authority read and need no metadata writer. Never hash, stream, await a second barrier, or wait on a client inside these scopes. Retirement is synchronous.
- `test-hooks` is empty by default and must be absent from release artifacts. Tests use actual loopback HTTP and private temporary SQLite files; integrator runs them through MG.
- Owner-root parent is operator-controlled and cannot be renamed by another same-UID process. Keep the root and its stable lock inode outside restore replacement (`state/` only); never unlink either lock.
