# Automatic MCP media transfer

Base `7354c28f1212b1b5f57a1ca22c076f195ed13bff`. The hosted Testing tool
returned `attachment_unavailable` on two PNG attempts. Exact note search
confirmed no note was created. The server could resolve and reach the public
file host. The existing error grouped several acquisition failures, so the
log could not identify the exact rejection.

Cleanroom implementation `20261123T151558Z-83078` replaced the upload page
with a binary POST authorized by the upload ID and added fixed acquisition
diagnostics. Independent review `20261123T153159Z-59915` found a stale browser
dependency reference, which was updated along with the exact web hashes.
Review `20261123T154242Z-5787` verified the final web hashes and upstream
provenance without changing dependency or advisory policy.

Two server runs failed in fixture setup (`20261123T153935Z-87919` and
`20261123T154305Z-8858`). Fixes came from cleanroom runs
`20261123T154141Z-99436` and `20261123T154429Z-15322`.
The isolated lifecycle test passed in `20261123T154542Z-21915`.
All 184 focused server tests passed in `20261123T154624Z-25537`.
Web checks `20261123T153941Z-90200` and the locked dependency audit
`20261123T153948Z-88807` passed.

Independent complete-content review classified the files below as PUBLIC.
The orchestrator matched the exact candidate hashes before updating the pins.
The initial review also classified both session records as PUBLIC.

| File | SHA-256 | Review |
| --- | --- | --- |
| `server/src/mcp_transport/descriptors.rs` | `c23fa9bc748dca54d7fc9464f896894b55ad21a238acd0128da736dd9e0accf8` | `20261123T153159Z-59915` |
| `server/tests/mcp_oauth.rs` | `088eb8ae77b445f5c3d2566678cc9d9b594f7982f466b6d40dfac878a10fbf3a` | `20261123T154520Z-20079` |

On 2026-11-26, the hosted Meguri Testing connector advertised the retired
sign-in flow for `prepare_media_upload`. A direct upload of a generated PNG
returned `attachment_unavailable`. The server recorded `attachment_url`,
which identifies URL validation. The specific rejected URL property is unknown.

The staged POST succeeded with the same PNG. Exact lookup of the created note
returned an image reference whose filename contained the SHA-256 of the local
PNG.

After refreshing the custom connection, check the tool description in a new
conversation and retry a direct PNG attachment with `files` and `file_fields`.
Record the outcome here. On success, read the note by ID and compare the image
reference to the original PNG's SHA-256. On failure, record the tool error and
the server's fixed diagnostic code. Native playback also remains open.

## Direct URL diagnostics, 2026-11-26

The direct PNG retry after a client session restart again returned
`attachment_unavailable` with server detail `attachment_url`. The active
tool description still mentioned the retired sign-in flow.

Starting commit `58ba751e7`. Cleanroom implementation
`20261126T160839Z-2630` split URL rejection into fixed diagnostic labels.
Independent review found that the existing raw at-sign check can reject an
at-sign in the query when the URL has no slash after the host. Cleanroom
`20261126T161231Z-16602` distinguished that check from parsed user
information and added regression coverage. Rejected URLs and their components
stay out of logs. A hosted retry against the diagnostic build is still due.
