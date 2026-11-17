# Temporary phone smoke endpoint

Diagnostic only; Desktop31, Retained8, Mobile10 and final gates remain unchanged.
Use a current clean canonical `sync-server-package` receipt, then:

```
tools/mg sync-server-mobile-smoke prepare
tools/mg sync-server-mobile-smoke start
tools/mg sync-server-mobile-smoke status
tools/mg sync-server-mobile-smoke stop
```

Prepare pulls the reviewed `cloudflare/cloudflared:2026.11.0` manifest digest
`sha256:b94b96edf2c9c4360f8879c8580cfaa99077f19c849ec595ca2f711b27d4f344`
and records the actual image/config/version. No host installation or Cloudflare
account is needed. All modes support MG `--dry-run`.

Start creates a new owned bridge and tunnel before initializing the exact package
with its unique HTTPS origin and actual tunnel IPv4 peer. The existing local
helper publishes only `127.0.0.1:18449`; an occupied port fails. The backend has no
public publish. Start requires matching private/public health responses and live
owned children; this grants no phone or compatibility success.

Open the reported HTTPS origin in the normal setup UI. The owner privately reads
`build/sync-server-mobile-smoke/setup.txt` locally to complete setup, creates a
fresh synthetic admin with a chosen test password, then uses those same synthetic
credentials and endpoint on the phone. Never paste the setup file, credential or
password into reports, MG output or artifacts. No real profile/account is used.

Stop removes the tunnel first, stops the exact backend, and proves both absent
before removing the empty owned bridge. It retains the private instance and data
volume. Ownership uncertainty refuses destructive cleanup and preserves evidence;
investigate locally. A second start refuses this retained instance; do not delete
records to bypass ownership. This single-use utility does not resume old tunnels
or move an account to a different generated origin. No source/Git writes occur.

Author-copy tests are unexecuted. Parent integration owns full-file public review,
focused policy tests, pinned-tool preparation, actual route verification and the
owner phone handoff; formal device evidence and final acceptance remain separate.
