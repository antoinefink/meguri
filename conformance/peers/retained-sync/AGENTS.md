# Retained HTTP peer

- Test-only separate Cargo workspace. Never add it to core/server members or the runtime image.
- Real CoreHost and public sync APIs only; no substituted transport, raw open, SQL writes or server storage access.
- Parent owns namespace/account/proxy/Desktop actions, preparation review, semantic case assembly and final seal. Worker action success is never case credit.
- Keep credentials out of errors. Fixed mounts/endpoint and fixed action vocabulary are intentional; no executable or target input.
- Prepare the separate lock through `tools/mg sync-server-retained prepare-lock`, review it, then use locked builds. External package identities must be a subset of the core lock.
