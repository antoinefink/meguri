# Master CI repair

Base `d6268d4ae9d5504a3d965f1165e419f40a371a4d`.

GitHub squashed PR #254 and the push failed six session checks. GitHub now
allows merge commits only. The ancestry repair rejoins `c40db3c97` and preserves
the original record commits and the full tree. The session and trailer checks
against the base passed in `static-checks/20261127T154520Z-9260`. All static checks
passed in `static-checks/20261127T154618Z-13390`. The historical push retains its
failure result.

The independent Codex project-standards reviewer checked the entire two new
session records and accepted them as public operational metadata. The reviewer
also confirmed the ancestry repair has an identical tree to its first parent.

| Session record | SHA-256 |
|---|---|
| `s-20261127T153847Z-4258a5b2.json` | `0fca839704f97a24875d4dca6c30f3d62d503522b23a0bc1d386da8804eb88e1` |
| `s-20261127T154018Z-4914c72f.json` | `3591bce316c8f43d2021efa60e45ee67b90947184f95d89d1a45ca38e7a59ebf` |

## Web Browse

The real Browse catalog reproduced HTTP 503 during local verification
(`sync-web-verify/20261127T154553Z-10179`). The browser response reports a
generic storage error. A focused probe exercised the same zero-wait
authority query while ordinary metadata connections closed. SQLite refused
1,085 reads with `SQLITE_BUSY` (`sync-server-test/20261127T155543Z-25595`).

The service now holds one idle metadata connection while it serves requests.
It holds no transaction, and the eight active worker slots remain the same.
The connection closes before the service releases the data root. The
independent Codex reviewer checked startup, authority, cleanup, and backup
behavior and found no blocking defects.

The regression failed before the fix when the last request closed the WAL
(`sync-server-test/20261127T155829Z-28894`). It passed with the fix, including
the final checkpoint after shutdown. The repeated stress workload saw zero
busy reads. All 43 selected checks passed
(`sync-server-test/20261127T160004Z-29433`).

PR #255 had already fixed the separate Browse search timeout. The two older
lease failures passed in later hosted shards and in isolated local Docker
runs (`selftest/20261127T154221Z-4919`, `selftest/20261127T154325Z-5889`).
