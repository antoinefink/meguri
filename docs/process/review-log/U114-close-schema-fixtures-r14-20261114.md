# U114 retained-close fixtures: round 14

Start: `4f8d497d`; integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
The two existing adoption-close tests still expected publication schema V3.
Runtime installs V4 with the root-readability ledger. Restoring a damaged marker
to V3 therefore left the fixture invalid.

The fixtures now check V4 and its ledger columns, use V5 for the unsupported
newer version, and restore exactly one V4 marker. Production is unchanged.
All refusal, file identity, unchanged-byte, retention and cleanup checks remain.

Author `cleanroom/20261114T205209Z-18035`; independent review
`205456Z-21971` reports no P0–P2.

- `core-test/20261114T205718Z-24362`: both tests pass.
- `core-lint/20261114T205805Z-25839`: all three checks pass.
- `private/20261114T205757Z-24717`: source passes.
- `similarity/20261114T205800Z-24977`: one file, no flags.

The retained 43-failure comparison remains unchanged. These results repair its
two schema-fixture failures only; no current full-suite result is claimed.
Other fixture families, P24, the batch gate and peer-device acceptance remain open.
