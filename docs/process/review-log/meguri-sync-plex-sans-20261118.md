# Bundled IBM Plex Sans

Starting commit `2e7bdea172667f5b8a1d74fc03362ca458390317`.
Implementer session `s-20261118T144527Z-c3a49a1b` and filtered export `20261118T144528Z-29083`.
Independent review `20261118T145052Z-42334` cleared the complete changed files
and the public ledger record, SHA-256 `48b2606e0e19fec6a842bb647e4f3c978d681a35c481f3e74a8979135bb05ef8` through LF.

Meguri Sync serves the official IBM Plex Sans Roman variable WOFF2 from its
own origin. The vendor archive matched its published SHA-512. The font and
distributed SIL OFL text match the upstream archive bytes. The CSS, server
asset lookup and package inventory retain the hashed local asset contract.

| Path | SHA-256 |
| --- | --- |
| server/src/webassets/mod.rs | baf934c25a81e6ac52e2053ba93fa3bace5390bd5a0c2f5b2ccf49f4ba7ac0a9 |
| server/tools/candidate-output.py | 873c3ce682b93f23caa118b0d0b7f07792d7111e80a4ee1a51a0fe7631dd46f8 |
| server/web/DEPENDENCIES.md | 008ba5ed27984c5cab72e56055f9a63e212b2688456cc33deced3ed12737df5b |
| server/web/THIRD_PARTY_NOTICES.txt | 793118f2c4b41d07e802a9d465a3eb7424d68add574e1bf7695580e539e15ab9 |
| server/web/scripts/audit-assessment.json | 513ab0531c26508fba8d4e60b164ebe34453ba77f00b022b9a9f93db8dde6300 |
| server/web/scripts/embed.mjs | 0257e92cff0d6993f8db1480899b1e06d30d768eff635ee470cf00ba9619050a |
| server/web/src/IBMPlexSans-Variable.woff2 | e978248b6b56da9e372975322a98dd9a51135d1b375e32ed6d5ca69f3aab792d |
| server/web/src/style.css | 317e57fe355f0a14593689c92f99e1e81322614f819997fed7a96b8361180477 |
| server/web/tests/browser/accounts.spec.ts | 7f72e704636d099dc1a9c7baecfe386101450f7e69f952ebe76683ffe8c586a2 |
| server/web/upstream-records.json | e939291e8440f4de76d7d1c905246df0cbeb2bad4efd9b864e92bcf500646124 |
| specs/servers/meguri-sync-web.md | 87a994f10aed9aa8ce279ef37bdcf59838c69fb7707d3f85a5d49a78dcf5bead |
| tools/tests/sync_package_context_test.py | f8aea685104a88da5e0bb52e15eb251798705d55c1cc7ea24928e678f05641a6 |

## Verification

- The staged privacy scan passed in `private/20261118T145452Z-48224`.
  The IBM font and OFL text were admitted by the exact independently reviewed
  whole-file hashes above.
- `static-checks/20261118T145514Z-51753`,
  `similarity/20261118T145513Z-51150`, and
  `selftest/20261118T145513Z-51393` passed.
- `sync-server-test/20261118T145623Z-57952` passed all four selected asset
  and HTTP route checks. `sync-web-verify/20261118T145513Z-51328` passed all
  nine frontend tests and the private HTTPS browser journey. The browser
  used the local WOFF2 bytes directly and made no external requests. The
  sign-in and 390 px Accounts renders were inspected.
- `sync-server-package/20261118T145512Z-50357` exported the same font bytes
  at `/assets/IBMPlexSans-Variable-DKYrB2Yq.woff2` and the exact notices.
  `sync-server-package-test/20261118T145512Z-50096` passed all four checks,
  including the browser journey against the exact production image.
- The full integration gate remains pending.
