# Meguri Sync amd64 packaging

Starting commit `9fecaaf3ea967b54669cf58a267a5bf0ebce5687`.
Fixer session `s-20261118T115144Z-f4c1f822`.

The amd64 package smoke stopped during npm notice collection
(`sync-server-package-test/20261118T111351Z-12297`). The separate amd64 dependency
inventory (`sync-web-deps/20261118T111808Z-22595`) identified the sole missing
notice as the Rolldown 1.2.19 GNU x64 build binding. The official tarball matched
the existing SHA-512 lock and contained no standalone license. The same-version
upstream package has the same repository and directory. Its two notices matched
the retained files byte for byte. Runtime notice selection and the lock are unchanged.

Filtered author `cleanroom/20261118T115145Z-69047` exported the four-file fix.
Independent reviewer `cleanroom/20261118T115437Z-76302` cleared the complete public
contents and exact SHA-256 of each candidate:

| Path under server/web | SHA-256 |
| --- | --- |
| DEPENDENCIES.md | `f8d9de7b39e2f8d9deaa25d1333314e25998cd365b162d3973eae3afa655c4e2` |
| notices/acquisition-records.json | `5e178266607d72aeb8fca49fc8ac9de9dc8ba2cc1da5b15c24144cd48c2d6106` |
| scripts/audit-assessment.json | `7b1797798337fd33ae3564c4e2af0d01058e99cc808187b3ae495afac320a493` |
| scripts/notices.mjs | `9b9dcfff4cf6811dcdafe5292b7ec25c87379d0205cf1764ac484c1cd7523ae9` |

The integrator verified the four exact digests and refreshed the two existing
public path hashes. The reviewer cleared the 348 ledger bytes through LF.
SHA-256
`a01b385829832beb4acec0398ecf40e8154e235ed75d90db2f16e61e9150f1aa`.
The whole-ledger admission still names an older snapshot.

The full integration gate remains open.
