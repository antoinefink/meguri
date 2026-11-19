# Meguri Sync browser layout feedback

Starting commit `b30180069b64da12c78e6bf8ccc0642ddae02e67`. Antoine requested
the changes on 2026-11-19. Session `s-20261119T070146Z-4b41f2e4`, filtered
export `cleanroom/20261119T070213Z-54174`.

Connection retains its server address, username and password revocation notice.
The guidance card is removed. Narrow navigation uses equal-width links with
the existing icons above their labels. The desktop account footer has an
eight-pixel bottom inset and retains its sign-out target.

The independent reviewer `registry_review` read the complete changed source
and provenance files and classified them as public. The first browser run
`sync-web-verify/20261119T071242Z-3155` caught stale adapted source hashes.
After refreshing those hashes, the reviewer cleared both resulting metadata
files. The privacy scan `private/20261119T071341Z-11750` identified app and
spec files whose existing public digests needed refresh.

| Reviewed file | SHA-256 |
| --- | --- |
| server/web/src/app.tsx | 7cfe3589678fe43b0b3b04a8960dde340a69083edf0661484c9c8d3806f30662 |
| server/web/src/style.css | a92032c83a4e55240567b309b97c3b6a27398043259da4f9c38900dd8a9f97e9 |
| server/web/tests/browser/accounts.spec.ts | 09b21f85c87e068ec933c272cade3a21f224f9b815ddcafb5596b639f1a5da64 |
| server/web/upstream-records.json | 26bd94c9535be5dbc6ad6d5ff3e3fa1615fec595111ccaa0ad106405a014608b |
| server/web/scripts/audit-assessment.json | 028cf1fb75cea50542de8827885a6ac06616c83886cac88f0aaf4aa3abf1c963 |
| specs/servers/meguri-sync-web.md | 94c2148f2528dd91396d4d6c2f2b4d1bf5ebbab3ab621a4f005a84750ddeb7c1 |

The same reviewer checked all 18 complete appended ledger records and verified
that the first 1,055 lines match the earlier admitted digest. The exact ledger
through `8c512a9a1` is public operational metadata. Its refreshed SHA-256 is
`3d3b7e4b2c4582471ea018de24fbaf4e2d2e9425ae8d0af2db509f087df755f7`.

`sync-web-verify/20261119T071505Z-16265` passed nine frontend tests and one
complete Chromium journey against the compiled Rust service over private
HTTPS. Both account roles passed the 320-pixel menu assertions. The desktop
Connection and mobile Accounts renders were visually checked. The app-wide
gate remains open in the verification register.
