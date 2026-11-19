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

## First GitHub-hosted run

Starting commit `c93b0913f7d43628df039a3ea2dbdaac37eb7537`.
[Run 44469364135](https://github.com/antoinefink/meguri/actions/runs/44469364135)
failed on AMD64 and ARM64 at `sync-web-image`. The resolver's `tee` could
not write `/out/font-resolution.txt`. The hosted runner owns the MG run
directory. The resolver runs as UID 0 with all capabilities dropped and
therefore needs write permission through the directory's owner or mode.

The workflow runs the three MG packaging commands as UID 0 on the hosted
VMs. MG creates their output directories with matching ownership. Git
trusts only the checkout path for those commands, and the resolver keeps
its existing capability restrictions.

An independent reviewer read the complete revised workflow and reported
no blocking findings. The public workflow's reviewed SHA-256 is
`65679526d734f271348260bf52c82f865fba3a20fbd2198ca6ff06d998db4110`.
The next run of the same workflow is the regression check on both runners.
