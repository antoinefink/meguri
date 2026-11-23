# MCP export replay test

Unit `mcp-export-replay`. Starting commit `54c1a38c999aba5e1969d0d1411335ca4f4a8e91`.

On PR #177, [the original server job](https://github.com/antoinefink/meguri/actions/runs/45260808842/job/137994887380)
recorded `storage_unavailable` from one redemption and a successful SQLite
response from the other. The same commit passed on the next attempt. The
original test let the two requests overlap during storage preparation as well
as ticket consumption. The log gives no narrower cause for the storage error.

Filtered fixer run `cleanroom/20261123T210802Z-70182` pauses a prepared
redemption before its final ticket check. A second request consumes the ticket
before the test resumes the first. The test requires the exact replay error,
valid SQLite bytes, and zero reserved bytes after cleanup. It also checks the
error and reservations when temporary storage is deliberately full.

Independent reviewer `s-20261123T210922Z-679e2ae7` found no issues in
`cleanroom/20261123T210937Z-76071`. The reviewer read each complete file and
classified it PUBLIC. The orchestrator matched these exact paths and digests.

| Path | SHA-256 |
|---|---|
| `server/src/mcp_service/authority_tests.rs` | `90a5dc00e86837d8ea4a86e9dfce8c6b2e319969ddd1513d52c496da232f6797` |
| `docs/process/provenance/sessions/s-20261123T210617Z-c3de3209.json` | `f95f0e413f5696ced6188a5225ebed3d51839881c8b723abc42be2f9fa437761` |
| `docs/process/provenance/sessions/s-20261123T210922Z-679e2ae7.json` | `16e0210dadd8776d540673ccc43b5c491321e859546bf15b773a09be1c60fc05` |

The original case passed alone in Docker (`sync-server-test/20261123T210858Z-73076`).
On the candidate, all seven export unit tests passed
(`sync-server-test/20261123T211007Z-77296`), and all seven download HTTP tests
passed (`sync-server-test/20261123T211041Z-78320`).
The staged source passed `private/20261123T211054Z-79319`. The pull request
runs the hosted similarity check.
