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
