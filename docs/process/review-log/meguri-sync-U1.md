# Meguri Sync U1 — Rust server boundary

U1 passes focused acceptance; the coherent batch gate remains pending.

- Original batch base: `38235917e6a025daa57625c582e90d253aae62e3`.
- Authoring base: `ad5a9474907d2380aa9cbeeeae10e80da485be79`.
- Final export: `/private/tmp/meguri-sync-implementation-20261115/build/mg/cleanroom/20261115T125427Z-89360/changes.patch`.
- Export SHA-256: `39896fa1b52da6789482276eca2a12c2958fa1a5ce32900fbdcb8943cf6fd375`.

| Evidence | Result | Receipt |
|---|---|---|
| Focused guards, provenance and registry checks | 25/25 | `build/mg/selftest/20261115T124933Z-85822/result.json` |
| Final prepare, seed and export suite | Pass; one Bats wrapper | `build/mg/selftest/20261115T125712Z-91438/result.json` |
| Actual sandbox probes | Pass | `build/mg/cleanroom/20261115T125716Z-91744/result.json` |
| Parent-run Rust/frontend corpus calibration | 94/94 | `build/mg/similarity-selftest/20261115T124429Z-83386/result.json` |
| Final independent source review | No material defects | `/private/tmp/meguri-sync-implementation-20261115/build/mg/cleanroom/20261115T125810Z-92736/codex.out` |
| Actual similarity scan against the batch base | 9/9; zero flags | `build/mg/similarity/20261115T131939Z-749/result.json` |
| Staged privacy | Pass after exact-content maintenance | `build/mg/private/20261115T132915Z-9398/result.json` |
| Static host, retry and trailer checks | 3/3 | `build/mg/static-checks/20261115T132916Z-9690/result.json` |

The `.config` allowlist contains only `core/.config/nextest.toml`. Go and speculative frameworks were removed.

Earlier privacy failures are retained at `build/mg/private/20261115T132421Z-3308/result.json`
and `build/mg/private/20261115T132625Z-5094/result.json`. Independently reviewed
exact-content maintenance admits the unchanged tooling bytes; it does not alter
credential exclusions or production server scanning.

Historical exports and failures remain retained. Obsolete image failures are at `/private/tmp/meguri-sync-toolchain-probe-20261115/build/mg/meguri-sync-go-lock/{20261115T112913Z-74944,20261115T114703Z-36737}/result.json`. Host selftest `build/mg/selftest-host/20261115T113040Z-75910/result.json` passed 28/29; missing `mac-vm` caused its failure.

Browser runtime enforcement, dependencies, Node runtime verification and final device evidence belong to later units.
