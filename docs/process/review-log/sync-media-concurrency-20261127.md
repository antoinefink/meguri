# Meguri Sync media download concurrency

The unit started at `be8c979e9`. Y8.6 of
[media sync](../../../specs/sync/media-sync.md) describes the server hint,
serial fallback, and ordered journal commits.

The client baseline passed 41 media reconciliation cases
(`core-test/20261127T140149Z-71922`). The server's pinned-download
baseline passed (`sync-server-test/20261127T140612Z-77537`). The
filtered runs `20261127T140109Z-70125`, `20261127T141605Z-94218`,
`20261127T141401Z-88286` and `20261127T141910Z-855` cover
implementation, independent review and fixes. Review checked bundle
authority, worker lifetimes, and stale folder observations.

The first client run exposed an incorrect test assumption. After a pause,
a file already saved with the right bytes can be recovered without sending
it again. The routine pause case also caught a missing transfer count.
The fix records verified transfer effects before checking authority to save
a file. That case failed on its own before the fix
(`core-test/20261127T141857Z-310`).

The corrected media selection passed all 29 server tests
(`sync-server-test/20261127T141800Z-97448`). Four requests on one account
reached the download boundary together. A collection metadata request
finished while the four downloads waited. The test checked all file bytes. The default server limit is 16 native requests.

The isolated loopback probe used a Docker test process limited to two
CPUs. It fetched 16 generated files totaling 32.5 MiB and checked the
contents. One request at a time took 0.990 seconds. Four at a time took
0.647 seconds (`media-download-local-probe/20261127T141444Z-90163`).
These are server transfer measurements on a generated fixture.

[PR #253](https://github.com/antoinefink/meguri/pull/253) carries
verification for the final branch and the status of the inherited
full integration gate.
