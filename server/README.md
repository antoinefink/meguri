# Meguri Sync, local Docker workflow

Run these commands from a clean, committed checkout. Docker/Compose and the
existing MG development/browser images and locked dependency preparation must
already be available. Build and verification run through MG; no host Rust, Node
or Python server is needed. Images remain local.

An external HTTPS proxy or tunnel is an operator prerequisite. Choose one exact
public HTTPS origin, without a trailing slash or explicit default port. Route
that origin to the loopback backend, preserve the browser's Origin, replace
forwarded headers with authoritative values, and send `X-Forwarded-Proto: https`
and one `X-Forwarded-For` client IP. Configure the exact IP from which the backend
sees the proxy connection; Docker may translate a host proxy's peer address.
Do not trust a subnet or user-supplied forwarded headers. Keep backend port 8080
private. TLS/proxy installation is outside this workflow. Secure cookies and
exact origin/peer checks remain enabled.

```sh
# Build the pinned server/Dockerfile from an MG-generated narrow context.
tools/mg sync-server-package

# Use your own HTTPS origin and the proxy's actual backend peer IP.
export MEGURI_SYNC_ORIGIN=https://sync.example.test
export MEGURI_SYNC_TRUSTED_PEER=192.0.2.10
# Optional: MEGURI_SYNC_PORT selects the loopback backend port (default 8080).
tools/mg sync-server-local init
tools/mg sync-server-local start

# Isolated rendered browser setup/login/account, native content/media, restart proof.
tools/mg sync-server-package-test

tools/mg sync-server-local stop
# Resume the same image and persistent volume:
tools/mg sync-server-local start
```

`init` records the immutable image ID and a new named volume in
`build/sync-server-local/instance.json`. Docker copies the image-owned private
`/data/owner` layout into the fresh volume. The service runs as UID/GID 10001,
with a read-only root filesystem and no capabilities. No root initialization or
permission repair runs. Use this named volume rather than an arbitrary bind
mount; do not change its owner or modes. `server/compose.yaml` supplies the same
service configuration to every MG local start. Stop retains the volume. Keep
the instance directory: it identifies the image, volume and lifecycle owner.
Do not run overlapping operator commands or remove the volume to resolve errors.
If volume creation fails and Docker proves the candidate absent, `init` discards
its candidate record and can be run again. An uncertain Docker observation keeps
the record; restore Docker access and inspect the recorded volume and owner label
before taking further action. Never delete a volume or record on uncertainty.
If service initialization fails after creating the owned volume, keep the record
and `setup.txt` private. Use the existing `rotate-setup` command below to complete
an interrupted initialization, then start. If rotation refuses the stored state,
retain the failed receipt, instance directory and volume for investigation; do
not reinitialize or replace data. A failure only removing the init container may
leave an initialized service: resolve that recorded cleanup before recovery.

Read the mode-0600 `build/sync-server-local/setup.txt` locally, open
`<public-origin>/setup`, and use the one-time code to create the administrator.
Keep that file private and delete it after successful setup. Later visits use
Login. Administrators create invitations; members choose their credentials and
find their server URL on Connection. Enter that URL and the account credentials
in a supported client's custom sync-server settings, including its media sync
setting where separate. Follow the client's explicit initial upload/download
choice; use synthetic data for local testing. This packaging smoke uses a
neutral native-protocol client; it does not establish Desktop or Mobile release
compatibility. The exact-client matrix is separate evidence.

MG result paths are printed on stdout. Build receipts retain the source commit,
manifest/lock hashes, imported-component provenance, Cargo audit and cargo-about
notices, frontend audit, actual production features, asset hashes, immutable
image IDs and a complete exported runtime inventory. The smoke starts the actual
runtime image on a fresh volume, submits real setup/login forms and renders connection/account state in Chromium
over private HTTPS, checks native
note/card/review/media bytes, then restarts on the same volume and reads them
again, including a fresh browser sign-in and rendered administrator account.
The browser uses the existing locked Docker web tool image and prepared Chromium,
separate from the scratch runtime. It performs no request or test retries and publishes no ports. It removes
only its owned synthetic containers and volume; operator data is separate.

The runtime contains one static executable and license/build evidence. It has
no Node, Python, package manager, reference client, test hooks or build tree.
`server/NOTICE` explains the license boundary. Complete, unmodified model source
and workspace inputs, plus generated third-party license texts, are available
offline at `/usr/share/meguri-sync/` in the image and in the package result's
`product/` directory (`covered-source.tar` and `rust-notices.json`). The model,
server and first-party browser code are MIT; third-party terms and notices
remain unchanged. `musl-tools`, `musl` and `musl-dev` are pinned to
`1.2.5-3.1~deb13u1` from authenticated Debian snapshots dated 2026-11-15.
`static-packages.txt`, `static-compiler.txt` and `static-inputs.sha256` retain the
actual package versions, compiler identity and static input hashes.
`musl-tools` is a build-only dependency for static linking; its runtime notice and compiler/standard-library notices ship
with the executable. The context builder rejects forbidden paths, recognized credential/key filenames and private-key input,
links and generated dependency/build directories. It uses only committed files.

If setup needs rotation before any administrator exists, stop the service and rotate
its existing setup credential using the same image and volume:

```sh
tools/mg sync-server-local stop
tools/mg sync-server-local rotate-setup
tools/mg sync-server-local start
```

For lost administrator access, prepare a private, owned mode-0600 file containing
the new password (one transport newline is allowed), then use the existing
recovery command. It stops the recorded lock owner before opening the same
volume. Add `--reenable` only when deliberately recovering a disabled admin.
No password is put on the command line or in the normal service log.

Select an existing, ready administrator by its opaque account ID, which differs
from its username. Obtain the ID from the existing authenticated accounts API
when available (`/api/admin/accounts`, or `/api/me` for your own identity), or
inspect the stopped service's local accounts SQLite database using
operator-chosen read-only tooling. The runtime has no SQLite CLI. Do not modify
account data for this lookup. Set `ADMIN_ACCOUNT_ID` to that ID before running:

```sh
tools/mg sync-server-local stop
MEGURI_SYNC_RECOVERY_FILE=/absolute/private/password.txt \
  tools/mg sync-server-local recover-admin "$ADMIN_ACCOUNT_ID"
tools/mg sync-server-local start
```

Remove the private password file after recovery. Keep failed MG evidence. This
workflow does not publish, deploy, upgrade, back up or restore a service.
