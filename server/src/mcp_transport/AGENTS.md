# MCP adapter

- Original tools-only HTTP adapter; no SDK, session authority, outbound I/O or logging. Validate dispatch before deriving scopes; only the authorizer parses credentials.
- Mount only the exact `/mcp` method router; keep method rejection there, never add a second application fallback. Preserve native/browser ingress and fallback.
- Generated fixtures grant no OAuth, hosted-client or collection acceptance. Backend owns grant/publication rechecks and bounded collection work; see module docs.
