# Desktop recovery reference mechanisms

Unit start: `8506d4879dd3b76b4aa5e1ab0825409671b84549`.
Author: `s-20261116T092626Z-4487d759`, oracle-maintainer.
Isolated checkout: `/private/tmp/meguri-sync-recovery-reference`.
Original U5–U8 integration base remains `aa8565719af435051de7d5f068c0c817286b568e`.
This factual extension does not accept that batch or dispatch U9.

The finite P-M07 reference selection observes Desktop SDK 26.09.4 build `c06370ff` only.
Public reset discovery used runtime names and signatures, followed by invocation; no Anki implementation source was read.
Reference-native runtime SHA-256 remains `bf0ccafe0f15bcba59a652db68c4cfdfaa8191756f8c547839c9d134dee12784`.
Lock SHA-256 remains `56e1bef86def9f5f230d12317e3510f2176eb079928f3a75f86afb0dcc408ec8`.
No production server/core/app changes, new dependencies or private inputs enter this unit.

## Original history and independent review

- `86d7325858442d5150e0af6c7c641b650fc73770`: original author ledger-only commit.
- `55c570e2c48e8d12a2f0fd3d43d8d43b4fd8de44`: initial finite observation source.
- `03dcc2c6177973df2d45fa460b3889860f996ee7`: original independent reviewer ledger-only commit, preserved by fast-forward.
- `9e2d0263a7ed7cf0c6f59c77971a53697dc23099`: final source repair and exact synthetic action inputs.

Reviewer `s-20261116T093915Z-a1d5bd1b` accepted final source and evidence with no open findings.
Its one source finding identified omitted recovery fixture/selection paths in the digest.
The repair adds both exact paths and a focused regression that changes each copied input.
Only the Bats fixture substitutes equivalent enumeration/hash primitives; production digest behavior has no fallback.
The reviewer also accepted the finite neutral amendment wording and the exact artifact bindings.

Author ledger review confirmed one public operational record and unchanged prior bytes.
Author ledger SHA-256: `fff0f26f605efdf19c014e07fe9b35c46a5fe5618725e56dd38b02166c13426a`.
The author independently inspected the reviewer's one public metadata append.
Combined ledger SHA-256: `043a603832182472c0a050b6d970901961e491d9e7c34032eb1b106952c9fd58`.
Original records and commits remain intact. Parent owns canonical content/hash admission and integration checks.

## Final receipts

Paths below are under the isolated checkout's `build/mg/`.
All final execution/check receipts use clean source `9e2d0263a7ed7cf0c6f59c77971a53697dc23099`.
Its reference source digest is `a197f578219029d5c10811351cbc5f65071fb27a0b8376461860b3ea25edd4d5`.

| Job/arguments | Original run | Result |
| --- | --- | --- |
| `sync-server-reference recovery` | `20261116T094758Z-93201` | 3/3 matched |
| `sync-server-reference selftest` | `20261116T094802Z-93530` | 52/52 passed |
| `selftest --filter 'recovery neutral inputs affect the reference digest'` | `20261116T094754Z-91553` | 1/1 passed with original Compose |
| `static-checks` | `20261116T094806Z-93874` | 3/3 passed |
| `sync-server-reference seal` recovery | `20261116T095223Z-4330` | 3/3 sealed |
| `sync-server-reference validate` sealed run | `20261116T095252Z-4845` | 3/3 passed |

Independent validation passed 3/3 in `/private/tmp/meguri-sync-recovery-independent-review/build/mg/sync-server-reference/20261116T094921Z-95517/result.json`.
Its SHA-256 is `0b4db8085c5d955505c14ccb8fbccb2656d37ff41efe2ee4d7e1ea7a376a300c`.
Review artifacts remain in `/private/tmp/meguri-sync-recovery-review-artifacts/`.
Accepted `review.json` SHA-256: `68d2b53cc974b988c13bb77b5f51f40e6e1600b00eaf0fa3d8d6a235f43f2d6e`.
The original execution evidence SHA-256 is `e49fc59d061607a86dfe0194cf0a3af682ead877843b133ba91982f822f4f0ff`.
Its complete observation SHA-256 is `85d4b8e64fc0c10abdd20561011a620e4d25b9acfb2fcb4ad7ddba77d9535d7f`.
The neutral projection SHA-256 is `a3b6d4ff924b0fde92e962123044037981b22ab6d4c14c7b2c7e704037721a3f`.
See `server-desktop-current#D9` for observed facts and limits.

## Retained failures and scope

- `sync-server-reference/20261116T092700Z-11282`: sandbox denied Docker builder activity. Authorized host execution replaced it.
- `sync-server-reference/20261116T093110Z-26844`: all three cases recovered, but an overly broad post-normal raw digest assertion failed.
- `sync-server-reference/20261116T093247Z-29415`: excluding USNs alone still failed because config modification times also changed.
- `sync-server-reference/20261116T093402Z-34749`: retained exact rows established only config USN/mtime changes, with identical key/value bytes and every other table unchanged.
- `selftest/20261116T094319Z-73033`: no test ran because Docker's address pools were exhausted.
- `selftest/20261116T094537Z-84789`: a temporary clone-only network-free run reached the Bats fixture but failed because dev lacked the host enumerator.
  Original Compose was restored byte-for-byte; both SHA-256 values are `18c10a8c5726a330e070609596ba7c53a2f7e6687181bb15f35bfa5fa208b907`.
- Independent Docker permission failure `20261116T094903Z-95053` remains in the review archive.

Parent released capacity by removing only five unattached networks belonging to completed Sync jobs.
No temporary network override remains in source. Subsequent normal focused MG execution passed.
No failed or provisional receipt receives final credit.
The final equality check keeps exact non-col table equality through full download, fresh receiving and reopen.
After normal sync it permits only config `usn` and `mtime_secs`; every key/value byte and all other columns remain compared.

These facts do not establish candidate restore policy, P-R01/P-R03 acceptance, Desktop GUI labels, Mobile recovery or whole-family coverage.
L6's exact supported-client procedure and final-session evidence remain pending.
