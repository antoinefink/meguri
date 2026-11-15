# Meguri Sync backend language decision

On 2026-11-15, Antoine reopened the earlier Go choice, said both Go and Rust
were acceptable, and confirmed the Rust choice. He also required
external dependencies to be well vetted and included only for a concrete reason.

The active backend choice is Rust. React/shadcn remains the browser interface,
and development builds and tests remain in Docker. No host Go or Node
installation was performed. No production server code existed at the switch.

The repository already has public collection-record, schema conversion and
protobuf APIs in `meguri-model`, with independent round-trip tests. Reusing an
audited subset can avoid a second implementation of this compatibility logic.
It does not turn the existing sync client into a server or establish current
Desktop/Mobile acceptance. Account management, durable publication, server
reconciliation and interoperability evidence still require implementation.

The existing core declares MPL-2.0. Reuse must retain its applicable terms and
pass ownership, provenance and dependency checks; it is not permission to label
existing code MIT. Independently authored new server/frontend files retain their
separate MIT boundary. No Anki source or runtime dependency is authorized.

Common isolation, provenance, frontend and review safeguards remain applicable.
Go-specific uncommitted tooling was removed from the active implementation;
its exported patches and actual failed/passing receipts remain historical
evidence. Existing Cargo build, advisory and license tools should be reused
instead of introducing a generic dependency framework.

The Markdown plan and neutral contracts were revised in place and reviewed. This record
does not claim implementation or delivery completion.
