# CloudKit payload checkpoint, 2026-10-31

U113 and U114 remain unaccepted. The original batch base is
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
This checkpoint starts from `d9df5e6ce59a6c60de93eda65f1dc208d745acf7`.

| Evidence | Result |
|---|---|
| `swift-test/20261031T163555Z-78197`, clean baseline | Policy and snapshot pass; media error-type regression reproduced |
| `swift-test/20261031T163917Z-81035` | 26 native media and payload tests pass |
| `core-test/20261031T164101Z-83279`, storage repair | 151 pass; five failures retained |
| `core-test/20261031T164407Z-84512`, memory | Four 40 MiB payloads pass at 169,648,570 bytes peak Rust heap; repeated-revision recovery fails at IPC |
| `core-test/20261031T165001Z-89795`, focused repair | Five pass, including real edit-back safety; one publication diagnostic failure retained |
| `swift-test/20261031T164525Z-85341` | Snapshot passes; six policy cases still fail because the fake misroutes root records |
| `swift-test/20261031T170437Z-97804` | All 173 affected tests pass across 12 suites |
| `core-test/20261031T165654Z-92377` | 157 pass; idle-peer timeout failure retained |
| `core-lint/20261031T171554Z-4459` | Pass: formatting, Clippy, crate layers and prohibited-service scan |
| `core-test/20261031T170924Z-2019` | 502-entry workflow and partial-frame checks pass; large recovery still failed |
| `core-test/20261031T172519Z-7775` | Recovery data checks pass; heap bound fails at 378,015,193 bytes |
| `private/20261031T172526Z-8029` | No match in all 46 changed files; guard unchanged |
| `core-lint/20261031T164804Z-88543` | Fails; import, closure and tuple findings retained |

Independent review cleared scoped migration, lazy flight storage, targeted blob
collection, native media compatibility, fixture metadata, the edit-back repair,
and the subsequent publication diagnostic repair. The canonical fixture zone and bounded idle-command receive are repaired.
The repeated-revision memory bound and final integration checks remain open. Earlier failed receipts stay failed.

First-adoption assets, conflict reads and retries, structural generations,
rekeying, remaining contract tests, and full native memory evidence remain open.
Required integration gates and final device evidence remain outstanding.

Source reports stay with their MG exports. This file holds the combined check record.

Final checkpoint checks pass: `core-lint/20261031T172739Z-8646`,
`similarity/20261031T172743Z-8903`, and `private/20261031T172755Z-9241`.
The 300 MB test stays enabled and red. This is WIP, without a Unit trailer.
