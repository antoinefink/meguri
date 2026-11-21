# Meguri Sync account data reset

The task started at `bd61d75d0`. The branch also includes `6a8aa4de4`,
which reached master during implementation. W3 defines the data reset for
ready accounts and the Accounts page layout.

Implementation session `s-20261121T104346Z-88a8cb83` exported
`cleanroom/20261121T104537Z-80937`. Independent session
`s-20261121T104957Z-0dc3ceed` reviewed the complete candidate. Its review
`cleanroom/20261121T105839Z-66676` found two reset races and a TypeScript error.

Fixer session `s-20261121T110138Z-542c0909` reserved staging space before
purge, read the epoch and pending intent in one snapshot, and repaired the
browser tests. The final export is `cleanroom/20261121T111013Z-18855`.
Independent reviews `cleanroom/20261121T110600Z-366` and
`cleanroom/20261121T111120Z-26320` checked those fixes and the frontend inventory.
All three session records were reviewed as public operational metadata.

The complete W3 file was independently classified as public. Its exact
SHA-256, `41301426cd9758ae87f7b3b73a43acbfe054ea12e16ccf2225b0109f803b2b38`,
is the only admission change. The staged scan `private/20261121T111203Z-35916`
passed.

The final reset logic passed all 34 selected server checks in
`sync-server-test/20261121T110742Z-12507`. The previous browser run
`sync-web-verify/20261121T110629Z-2571` passed 42 of 44 cases. The two Accounts
assertions now check the section heading and its separate count status.

## PR review

Antoine approved the Accounts layout in the HTML preview. CodeRabbit noted
that a reset between the metadata read and the collection measurement could
mix old media usage with the new collection size. Both browser endpoints
now measure each account under one publication barrier.

Session `s-20261121T112925Z-7ec85b35` updated W3. Independent
review `cleanroom/20261121T113040Z-49101` classified the complete spec and
session record as public. The admitted W3 digest is
`5b80364781b6b1eaa16529b666cfd1ca3e6a76098dcb8c2282e7406810640bea`.

The code review `cleanroom/20261121T113325Z-70782` checked the exact storage
fix from `cleanroom/20261121T113202Z-61008`. The real HTTPS Accounts and own
storage regression passed with the other 31 cases in
`sync-server-test/20261121T113406Z-72645`.

`sync-web-verify/20261121T113408Z-73237` passed all 44 browser cases and
all nine frontend tests with the storage fix. The offline preview also
passed its reset check in `sync-web-browser/20261121T111414Z-57474`.

The first Linux server job received 403 in the existing publication order
test. The same case failed on its own in
`sync-server-test/20261121T113626Z-85444`. Upload staging had reached the
publication test hook before the final commit. The fix in
`cleanroom/20261121T113728Z-90245` gives staging its own hook and keeps
the existing assertion and publication order.
