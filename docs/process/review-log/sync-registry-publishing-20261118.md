# Meguri Sync registry publishing

Starting commit `8b8feb8097a6f03d0ec2517f00a03b0c862bb329` on local master.
The GitHub inspection used `99b762614cb36ed30d7b09dc1bfb8a7995ee0d4a`.

GitHub returned HTTP 204 for the repository's vulnerability-alerts setting.
Its dependency graph includes server-only Cargo versions (axum 0.8.8 and
rusqlite 0.41.1) and the frontend's Vite 8.3.8. An alert references
`server/web/package-lock.json`. The existing lockfiles supply alert coverage.

The independent workflow reviewer examined the complete final file and reported
no blocking findings. The reviewed SHA-256 is
`c5b7a8a1b6db2974d752d4c712b3eff0ba653bfce6f67beebae0959e40d95a1a`.
The source contains public configuration only. Review prompted removing
OCI-only index annotations so publication can use either manifest media type.
The rebuild and promotion path lists include the MG host job definitions.

Focused evidence on the workflow before commit:

- `static-checks/20261118T165958Z-61645` passed trailers and no-retries.
- `private/20261118T170000Z-62505` passed the workflow file.
- `similarity/20261118T165959Z-62076` passed with zero applicable source files.

The Actions run is the end-to-end check of native Linux runner builds, artifact
transfer and GHCR publication. This change does not establish an app integration
gate pass.
