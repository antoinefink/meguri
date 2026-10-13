# conformance/ (MPL-2.0, shared by spec writers and implementers)

- `goldens/` is written only by `tools/mg oracle record <filter>`: never edit a golden by hand or bless Meguri output into one. `tools/mg oracle compare <filter>` re-runs the live oracle against the committed goldens.
- A golden goes stale when its scenario's parsed value or the oracle lock changes; comments, key order and layout do not change scenario_sha256. Re-record stale goldens in the same change as the scenario edit.
- Scenarios are YAML in the JSON-compatible subset, read with the YAML 1.2 core schema (`yes` and `2030-01-15` are strings): no aliases, merge keys, tags or duplicate keys. They define their own note types and CSS and contain no Anki test code or test data text (R1881, R1959).
- `schema/protocol.json` is the only interface to oracle workers; it also lists the driver's v0 ops and observations.
