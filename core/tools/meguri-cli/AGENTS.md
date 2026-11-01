# Internal cloud fixture bridge

- The 1 MiB frame cap is for small synthetic fixtures, not production sync.
- Preserve the original Service `Response`; `SystemFields` and `Token` are opaque Service values, not CloudKit archives.
- Protocol errors may occur after commit: never retry automatically. Stdout is JSON only.
