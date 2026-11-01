# Internal cloud fixture bridge

- The 1 MiB frame cap is for small synthetic fixtures, not production sync.
- Preserve the original Service `Response`; `SystemFields` and `Token` are opaque Service values, not CloudKit archives.
- Protocol errors may occur after commit: never retry automatically. Stdout is legacy JSON+LF unless `--binary-assets-v1` explicitly opts into a JSON header+LF then indexed raw bodies (4 MiB/body, 16 bodies, 8 MiB aggregate). Header cap stays 1 MiB; only schema-defined `Asset.bytes` leaves carry references. Protocol errors remain fixed JSON lines. Swift support must match these bounds and keep logical receipts distinct from raw packet evidence.
