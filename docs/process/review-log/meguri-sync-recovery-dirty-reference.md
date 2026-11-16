# Dirty offline-media reference facts

Continuation of the U3 recovery-fact investigation, based on `146cb76fd38c2a8ff724b837df979d25eabc71f9`.
Original U5–U8 batch base remains `aa8565719af435051de7d5f068c0c817286b568e`.
Only the owned clone `/private/tmp/meguri-sync-dirty-media-reference` changed; no canonical or active gate checkout changed.
No U9 implementation, production/core/app code, dependency, policy/guard/allowlist, PR or push is part of this work.

## Preserved history and review

- `cbe78d0faa55fd4d6798e01126bc9075dfcf8aeb`: original session-only record, oracle-maintainer `s-20261116T124044Z-c9976196`.
- `9c6aeb517d15de4b6d53dc4c65b6e94c037d8475`: original exact-equality restore attempt.
- `a99ab5f607fb3a3588f372dc3e70abfc06392265`: final finite observation and separate clean-receiver source.

Reference source digest: `bd71b60d1b757b3f6ba160c3fe845293fb6a0a182f59edb2ce6dc8175785a5a1`.
Desktop SDK remains 26.09.4 build `c06370ff`, runtime SHA-256 `bf0ccafe0f15bcba59a652db68c4cfdfaa8191756f8c547839c9d134dee12784`.
Lock SHA-256 remains `56e1bef86def9f5f230d12317e3510f2176eb079928f3a75f86afb0dcc408ec8`.
All media, note types and study content are original synthetic inputs. No private collection, token corpus, installed app or Anki implementation source was read.

Reviewer `s-20261116T093915Z-a1d5bd1b` independently accepted source, fixture/runtime/report bindings, data separation and neutral claims with no open findings.
The same reviewer accepted the complete original public metadata append: one record, unchanged admitted prefix and exact original-commit identity.
Ledger SHA-256: `0639f5e228c45cbb505b97ae522491b0d9ab685ac1916a75e16e664ff1d06660`.
Parent owns canonical exact-content admission and required integration checks.

## Original failure and exact outcomes

Original clean-source goal run `sync-server-reference/20261116T124551Z-36888` remains FAIL, 0/1 matched.
Whole snapshot-media equality and independent-witness equality were false; neither comparator was relaxed or replaced by subset equality.
At cursor two/two, real offline media addition, replacement and deletion leave two local files with different names/bytes from the two-file chosen snapshot.
Explicit full collection download restores exact non-col tables and card/review rows.
Public media reset restores the two server names but retains/uploads the local-only file: upload result `[1,3]`, then media sanity `OK`.
An independent SDK witness receives that same three-file union. This is failed in-place authoritative restoration for the fixture, not a server protocol defect.

The later `dirty-inplace-observed` case has a distinct identity and explicitly records exact restoration as false.
Its companion `dirty-clean-receiver` starts only after the protected stopped server snapshot is reestablished.
The preserved closed generated SDK collection/media directory includes its tracking files and remains byte-identical; that path is never opened or synchronized during the clean procedure.
This is not a backup of Desktop GUI-global preferences, Keychain or credentials outside that test directory. No GUI profile copy/export was exercised.
A separate fresh SDK collection/media directory, renewed authentication, full download and media sync reach exact chosen non-col table hashes, card/review rows and the whole filename/hash map.
Reopen and another independent SDK receiver agree; the clean procedure uploads no media.
No old-directory data is silently deleted or merged. A fresh receiver alone does not remove the diagnostic upload from a contaminated server.

## Clean final receipts

Paths are under the owned clone's `build/mg/`, on clean source `a99ab5f607fb3a3588f372dc3e70abfc06392265`.

| Job | Original run | Result |
| --- | --- | --- |
| `sync-server-reference recovery-dirty` | `20261116T125044Z-44229` | 2/2 facts matched: negative in-place outcome and clean-receiver mechanism |
| `sync-server-reference selftest` | `20261116T125049Z-44939` | 54/54 passed |
| `static-checks` | `20261116T125053Z-45543` | 3/3 passed |
| focused existing digest Bats regression | `20261116T125059Z-45974` | 1/1 passed, including both new neutral input paths |
| `sync-server-reference seal` final run | `20261116T125829Z-66983` | 2/2 facts sealed |
| `sync-server-reference validate` sealed run | `20261116T125851Z-69251` | 2/2 passed |

Independent MG Docker validation passed 2/2 in `/private/tmp/meguri-sync-dirty-media-independent-review/build/mg/sync-server-reference/20261116T125455Z-57980/result.json`.
Its SHA-256 is `e3b2a7983918f504c525446e7d2956a04f55089dfe3acc854cc014be6554a651`.
Accepted bindings: `/private/tmp/meguri-sync-dirty-media-review-artifacts/review.json`, SHA-256 `ba4ed43b386df7a94f22db7098fbaf932bebbac9d6940bedf5803fc7bcfb32cc`.
That archive retains the original red run, final observations, independent validation, source/publicness and ledger reviews.
Execution evidence SHA-256: `8568f76127e6d9fd027e433086e67fb0dd5b28b21b4b6b4e4ae86d046cb6d88b`.
Complete observation SHA-256: `d9bbd92b664a6c5518c741bedd3cb34a6aa1a1260aa35b5b72ed5a840cd85666`.
Neutral projection SHA-256: `250e997c5b07353c70224d2232723aacdcf93ae842568545b6fe8c80b4ec6fc3`.

These are two facts from one bounded reference fixture. They establish no candidate P-R01/P-R03, GUI profile controls, actual Mobile, universal conflict rule or batch acceptance.
See `sync/server-desktop-current#D10`. GUI/Mobile evidence remains a separate final obligation, not a new U9 storage prerequisite.
