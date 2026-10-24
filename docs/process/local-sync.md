# Persistent local sync service

This user-owned Docker service is separate from the automated sync rig. Docker
(including OrbStack) must be running, with the pinned production runtime image
already installed. The utility never pulls or updates images.

```
tools/mg local-sync up --image <complete-installed-sha256-image-id>
tools/mg local-sync status
tools/mg local-sync down
tools/mg local-sync up
```

`up` waits for listener health. It does not prove authenticated collection or
media transfer. Connection settings are generated once in ignored
`build/local-sync/client.json`; read that private file locally to configure the
app. The directory is mode 0700 and its files are mode 0600. Do not copy its
contents into code, reports, screenshots or commits. `server.env` contains only
the password hash; `client.json` retains the generated password.

The helper selects the loopback host port before first startup and saves it.
Docker always uses that fixed binding. The service restarts unless explicitly stopped. `down`
sends SIGINT and retains the container, credentials, network and named volume
`meguri-local-sync-data`. It never deletes data. Keep `build/local-sync/` with
the volume: its ownership token protects against foreign resource collisions.
Back up that private directory and the volume together through local means.

Only these dedicated resource names are managed. Foreign resources with those
names cause refusal. Existing volume data is never recursively re-owned.
