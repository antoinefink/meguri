# Current-reference probe provenance

The adapter uses public wheel APIs and original synthetic inputs.
It contains no server implementation translated from a reference source.

For the idle probe design, the oracle maintainer inspected the existing pinned
26.09.3 checkout's `rslib/src/sync/http_server/user.rs` and sync HTTP-client API.
That inspection supplied no current-release expiry fact. The current wheel must
be measured separately. Only neutral observations may leave this quarantine.

During the schema-aware content-hash repair, the maintainer also checked the pinned
26.09.3 storage schema's table and collation declarations. The actual current-wheel
transfer checks, rather than that older source, determine the recorded outcomes.
