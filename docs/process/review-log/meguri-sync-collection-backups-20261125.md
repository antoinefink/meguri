# Meguri Sync collection backup verification

Starting commit `daef6d0d1`. The work uses temporary accounts with synthetic study data.

## Browser

Implementation session `s-20261125T075419Z-fed0406b` exported `cleanroom/20261125T075528Z-98905`. Independent source review `cleanroom/20261125T080417Z-7016` reviewed the complete frontend files. It identified wire-validation and confirmation-focus issues. Fixer session `s-20261125T080436Z-e72ed068` updated the exact trusted frontend build inventory in `cleanroom/20261125T080502Z-7771`.

Before the change, `sync-web-verify/20261125T075645Z-1431` passed all 53 checks. With the first browser change and the old backend, `sync-web-verify/20261125T080732Z-9223` recorded 54 browser passes and two failures involving focus after sign-in and returning to sign-in after a history request lost its session.

## Recovery

Session `s-20261125T080216Z-6c7e990a` added an independent native and browser HTTP recovery test in `cleanroom/20261125T080243Z-6590`. Against the original server, it uploaded its synthetic study data and media, restarted, then failed at the history route with the expected 404 in `sync-server-test/20261125T080732Z-9320`.

## Provenance

The independent plan and build-inventory reviewer (`plan_review`) checked the complete plan, all five session records, both audit files and the acceptance test. All contained public operational metadata, public source or synthetic data. The exact admission digests in the privacy scanner cover only the reviewed files. The separate frontend review checked all eight source and test files from the web implementation export. Privacy scans `private/20261125T080958Z-12683` and `private/20261125T081043Z-13191` passed after applying the reviewed digests.

The server implementation session `s-20261125T075256Z-02912996` exported `cleanroom/20261125T075528Z-99436`. Independent review `cleanroom/20261125T083212Z-26098` checked the server implementation and storage lifecycle. Follow-up work covered reset and deletion fencing, restore restart phases, on-disk validation, timestamps, temporary space limits, and sparse retention. The independent HTTP recovery test reads the restored SQLite collection through native sync and downloads the original media bytes.

The integrated browser run `sync-web-verify/20261125T085932Z-44010` passed 62 browser checks and 10 unit checks. Screenshots of populated history and restore confirmation used synthetic data. Both images are attached to PR #215.

The server run `sync-server-test/20261125T085538Z-42318` surfaced lifecycle fixtures that took a byte-for-byte baseline before the new startup backup finished. The test harness now waits for the first scheduled sweep, and affected fixtures capture their baseline after setup. The process-kill fixture still checks the media migration at its original cut point. The test that reopens a collection larger than the ordinary upload limit retains its native restore assertion.

The first hosted similarity scan identified a structural match in the backup inventory row decoder. The fixer received only the Meguri line range and rewrote the validation from the lifecycle specification.

## Pull request review

The review found that another tab could change the session account between opening Collection and restoring a point. The browser now sends the account it displays and verifies the account returned on history and restore. The server verifies the expected account before lookup or mutation. The same export puts history admission and enumeration in one transaction, moves heavy file work out of the metadata slot pool, and reports successful restore when only later cleanup fails. The server signals the sweep to stop when HTTP shutdown starts. The scheduler reclaims orphaned snapshots before a failed capture can fill the disk again.

Exports `cleanroom/20261125T090533Z-49128` and `cleanroom/20261125T091036Z-50787` contain those fixes, corruption checks, overflow tests, and the validator rewrite. The main-checkout similarity gate passed the full branch at `8500c486c` in `similarity/20261125T091934Z-57214`. The `data_review` reviewer read the complete integrated Rust files and found three test fixture issues. The independent `plan_review` reviewer checked the web files, interface specification and upstream build inventory, including the two adapted source hashes for the app shell.

## Final checks

On the final implementation, `sync-server-test/20261125T092204Z-61036` passed all 455 server tests. The native round trip verified the backed-up note, review and scheduling data and the original media bytes after server restart. The suite also exercised crash recovery, stale writes and download tickets, default-limit growth, and preservation of other accounts. The isolated fixture failures from the preceding run were corrected in `cleanroom/20261125T091951Z-58287`.

`sync-web-verify/20261125T091811Z-55010` passed 11 frontend checks and 65 browser checks. Both screenshots matched the previously attached images byte for byte. `sync-server-lint/20261125T092205Z-61197` and `static-checks/20261125T092251Z-62086` passed. The full source diff passed the similarity gate on commit `135e48b7a` in `similarity/20261125T092252Z-62178`, with 37 files scanned and zero flagged. The public source, specs and register passed `private/20261125T092323Z-63857`.

## Reviewed public snapshots

The independent cleanroom reviewers and the `data_review` and `plan_review` reviewers classified the complete files below as public source, synthetic fixtures or project verification records. The coordinator verified each final SHA-256 against its path before admission.

| File | Reviewed SHA-256 |
|---|---|
| `docs/process/verification-status.md` | `5accd9f059712aa180d62515ab215d93e886faf911a4fbc3e2dbb93a662e8764` |
| `server/src/accounts/backups.rs` | `d7fccc4c211bf95228b7a63a20c89477922073ffecd000a4ee43809fc43c4001` |
| `server/src/accounts/deletion.rs` | `a33371a855de6157e3ba332305384d8622943e270640735d90e6ce7da444bb4e` |
| `server/src/accounts/mod.rs` | `ffb70aadc3e86cbba4b71fa7aacac9d0241bb5d570a3fe4132ff91957ba6ac99` |
| `server/src/collection/schema.rs` | `b6e1a6d0ad4657ad6d56b9d62cbb02e86ca153564335cceb6cdefd8c858accd2` |
| `server/src/httpapi/mod.rs` | `5928daeba0ce60e79881ce7043d6202e4a28e70ec8c44324f4540eb1fb08b864` |
| `server/src/httpapi/tests.rs` | `e060bad11937cdd2a4f56a04103c6eecf2d34ac8d34c966d663ddfac01203960` |
| `server/src/mcp_service/export.rs` | `463edb515478d490e6d07a1010edada22a852bbd3c451000228d245ad35fca1d` |
| `server/src/storage/backups.rs` | `baf12de62a3bd512fb3d90ce639fbe94c003d5cc0f547bdb40ccb4cb2d1dd7ab` |
| `server/src/storage/collection.rs` | `e27e2657c752118e14a57eee533d7070b2e40f29348c732b88058382a57c7537` |
| `server/src/storage/deletion.rs` | `a40673bd3bbe0871786e7fbbf6bb0b80ebd3f026acf28a636b19638528d15ec9` |
| `server/src/storage/mod.rs` | `7285b36d0b29aa93c82cb32b632239ea4b58cfd658eab661769dc0a756e05d46` |
| `server/src/storage/reset.rs` | `eb4b91a93c9976b7b07d3212f58e1e8e62b4deea0a9346e5e7c824c7dd052ee9` |
| `server/src/storage/root.rs` | `a060ba8e3453cae2c6ef4f4eb79287e634b9106138649dcfc12739b753a4df5a` |
| `server/src/storage/semantic.rs` | `50df314a245ee5e41294b8c732ec0a5de9a4b8eab5fe1a9b0e4a1fd5e60de710` |
| `server/tests/backup_round_trip.rs` | `6549998c8c16276260dc3acd8f05ec395dca05f29c29072a8d25c9cee6a2a1fd` |
| `server/tests/backups.rs` | `04217e3055f0b6a346f277d2ed22540847c9318ac4256a81a3c5a5fea1de6873` |
| `server/tests/deletion.rs` | `ab0089c0ae552f21ca4cced65c799cdaf3f49e2605c5a66aa8c712dc31827cbe` |
| `server/tests/mcp_oauth.rs` | `fb74eaa7b7aa836199eb566f51034baf429477c3303d7c8dd049dd56932cc578` |
| `server/tests/media.rs` | `9aa88101ecdefc8c46120f1adac02655aa3bfe628f458dca7b6f80a6b3297a93` |
| `server/tests/media_recovery.rs` | `d786705aa844ae1cc1e41c9164a6f65975bd6d94355c2bc926cf48559d7e0d8f` |
| `server/tests/support/mod.rs` | `fbaccbd87526b237ac1db6d1a2f17ff810838bb15366409b5f0ef254d4cecce8` |
| `server/web/DEPENDENCIES.md` | `062796f5dd9be0fb9339cec1c3174aabd2db8b92d21d443fd95637239de556a9` |
| `server/web/scripts/audit-assessment.json` | `cda96348d6084e7bccc0af148fd92275b8931145b2ed252e484decf62aafcae5` |
| `server/web/src/app.tsx` | `2d32bd613d50aa1df4e388595c11bbeeed3ba1ffb4dbd780464085205753e20d` |
| `server/web/src/style.css` | `39e6e70a5d5118f450375b13c90faa1e80f6d1de4d50f2047e348e16b5bfbd2b` |
| `server/web/tests/api.test.ts` | `07eedde417f5b6a0a338c665cf42673b01d56a852fda07c5fc011cccd9ea19be` |
| `server/web/tests/browser/accounts.spec.ts` | `e212183bbaeedd51ec25b8302607b0ec7dcbb75333dba4b0ea861f179fb8a0d3` |
| `server/web/tests/browser/backups.spec.ts` | `d550ece559a79d0c461b44215eb1d68e1214f19d158249c9fd07b786d82fd6ed` |
| `server/web/tests/browser/mcp.spec.ts` | `261b286236452b954829529242b040eb4d3784019eeb0e321b2f10ae7ece1f10` |
| `server/web/upstream-records.json` | `a3a66b858a72331499de6289e4745f834d34105b52655d1c22133fe0c599b099` |
| `specs/servers/meguri-sync-web.md` | `55272c12aad4637f57353ffdac921fd6bda1ae24bd7564d6eb5280df4bfc8c37` |
| `specs/sync/server-lifecycle.md` | `ae5b10969d39c9bae94095fdb81d025d63b502cdb1d87720e5983fdd9559c5bf` |
