# Continuing on another Mac

Keep source and portable instructions in Git. Keep VM disks, VM SSH keys,
machine configuration and local credentials outside tracked files. The root
`.local/` directory is ignored for this purpose; it must never be force-added.
It is not a backup: back it up separately before deleting or replacing it.

## Current coordinator and remote test VM

M5 is the Apple build coordinator and schedules folding over SSH. Both Macs
can schedule other VM jobs; MG's guest-wide claim serializes guest access
(`tools/vm/AGENTS.md`). The headless `meguri-mac` guest stays on T9 attached to
M3 Max, which hosts Tart and the resident VNC connection that folding reuses.
M3 Max's per-user `mac-vm.json` names the kit's `bin/tart` and `bin/vm-ssh`
directly. Each Mac keeps its leases across its own worktrees.

The coordinator uses the existing SSD-local `bin/tart` and `bin/vm-ssh`
wrappers over SSH. Those select `TART_HOME` and the guest's identity; do not
copy their key or duplicate that logic on M5.

### Install the coordinator wrappers

The hosting Mac needs `/usr/bin/python3`, its existing Tart kit and an already
trusted, noninteractive SSH route from M5. Use a private SSH alias or
`user@host`, with credentials and a verified host key in the user's SSH
configuration. Reserved account, key and consent setup stays owner-only
(`one-time-setup.md`). Never put identifying addresses, keys, passwords or
`remote.json` in Git. The alias names the hosting Mac, not the guest.

Install the same Python 3.8+ stdlib launcher twice, as executable files named
`tart` and `vm-ssh`. Its adjacent `remote.json` contains `host` and `runtime`;
`runtime` is the absolute kit root on the hosting Mac, normally
`/Volumes/T9/Meguri VM`, with the existing wrappers under `bin/`.

With coordinator jobs idle, run this from the checkout on M5. It stages a complete private release,
publishes its stable `current` symlink atomically, then atomically replaces the
default per-user `mac-vm.json`. Each publication uses a same-directory rename.
Updates retain the earlier release; no worktree path enters the configuration.

```sh
python3 - <<'PY'
import json
import os
from pathlib import Path
import shutil
import tempfile

with open('/dev/tty') as terminal:
    print('Private hosting Mac SSH alias or user@host: ', end='', flush=True)
    host = terminal.readline().strip()
    print('Absolute remote Tart kit root: ', end='', flush=True)
    runtime = terminal.readline().strip()
if not host or host.startswith('-') or any(c.isspace() for c in host):
    raise SystemExit('Use an SSH alias or user@host')
if not runtime.startswith('/'):
    raise SystemExit('The kit root must be absolute')
settings = Path.home() / 'Library/Application Support/meguri'
base = settings / 'remote-vm'
base.mkdir(parents=True, exist_ok=True, mode=0o700)
base.chmod(0o700)
release = Path(tempfile.mkdtemp(prefix='release-', dir=base))
for name in ('tart', 'vm-ssh'):
    target = release / name
    shutil.copyfile('tools/vm/remote-wrapper.py', target)
    target.chmod(0o700)
remote = release / 'remote.json'
remote.write_text(json.dumps({'host': host, 'runtime': runtime}) + '\n')
remote.chmod(0o600)
current = base / 'current'
prior_config = settings / 'mac-vm.json'
if prior_config.is_file():
    shutil.copyfile(prior_config, release / 'previous-mac-vm.json')
    (release / 'previous-mac-vm.json').chmod(0o600)
if current.is_symlink():
    (release / 'previous-target').write_text(os.readlink(current))
config = {'tart': str(current / 'tart'), 'ssh': str(current / 'vm-ssh'),
          'vms': ['meguri-mac']}
with tempfile.NamedTemporaryFile(mode='w', dir=settings, delete=False) as staged:
    json.dump(config, staged)
    staged.write('\n')
link = base / ('.current-' + release.name)
link.symlink_to(release.name, target_is_directory=True)
os.replace(link, current)
os.replace(staged.name, settings / 'mac-vm.json')
PY
```

The stable private directory is
`~/Library/Application Support/meguri/remote-vm/current/`. Keep its releases,
JSON and hosting Mac logs private.

To roll back, wait until coordinator jobs are idle. Resolve `current` to its
release directory so the saved files stay accessible after changing the link.
Restore each entry to its state before installation:

- If the release contains `previous-target`, stage a symlink beside `current`
  using that exact target and publish it with `os.replace`. Otherwise, unlink
  the newly installed `current` symlink. Keep the release directory.
- If the release contains `previous-mac-vm.json`, stage a private copy beside
  the per-user `mac-vm.json` and publish it with `os.replace`. Otherwise,
  remove the newly installed per-user `mac-vm.json`.

Remove the old `export MEGURI_MAC_VM_CONFIG=...` from every worktree's
`.local/activate` and any shell startup file or scheduler environment that set
it. Use the per-user default above for all worktrees. Editing activation files
does not change existing shells: run the following in each existing shell,
and restart persistent coordinator processes that inherited the override,
once their jobs have ended. Keep both host fallbacks disabled in activation
and scheduler environments too.

```sh
unset MEGURI_MAC_VM_CONFIG MEGURI_MAC_UI_ALLOW_HOST MEGURI_SIM_ALLOW_HOST
tools/mg doctor
tools/mg sim status
tools/mg mac-ui --suite a11y
```

Check that doctor resolves the stable wrappers and finds a logged-in guest,
sim status inventories the guest pool, and the focused Mac UI receipt says
`details.mac_runner: "vm"`. Run only one VM job at a time. Verify the default
from another worktree and an existing shell after clearing its override.
Record unavailable checks as queued, never passed.

To verify offline handling without stopping the VM or touching SSH trust,
make a private temporary copy of the installed pair and `remote.json`, change
only that copy's `host` to `meguri-offline.invalid`, and point a private temporary
`mac-vm.json` at those copies. In a subshell, set `MEGURI_MAC_VM_CONFIG` to that
temporary config, unset both host fallback variables, and run doctor, sim
status and the same focused mac-ui job. VM-dependent work must report
unavailable/queued (`mac-vm`), with no host simulator or UI execution. Sim
status may simply report unavailable rather than create a queue receipt.
Remove only the temporary private configuration afterwards. Leaving the
subshell restores the live default; run doctor again, then `tools/mg catch-up`
when ready to run queued work.

### Transport and manual VNC

The launcher execs `/usr/bin/ssh -T` with batch mode, strict host-key checking,
an eight-second connect timeout, agent forwarding disabled and no TTY. It
quotes remote arguments with `shlex.join`, preserving empty arguments, shell
command strings, binary tar streams and remote exit status. A missing SSD or
either remote wrapper exits 255, so MG treats it as SSH unavailable. Other
commands keep stdin, stdout and stderr connected end to end.

Only Tart's exact ordinary MG start, `run --no-graphics <name>`, detaches on
M3 Max, in a new session with stdin closed and stdout/stderr sent to a private
`tart-run-*.log` in the kit root. The coordinator receives its PID and log path.
Failures in the first second return their exit status and log tail; later
failures remain in that log for inspection on the hosting Mac. MG still checks
Tart state and guest readiness. A dropped coordinator connection cannot
terminate the detached guest. Nothing here stops or recreates it.

New remote posture sessions use an M3-resident supervisor and holder under
`runtime/vnc-owner-v1`, using the actual hosting `bin/tart`. M5 schedules folding;
both Macs may schedule ordinary jobs under the shared guest claim. Deploy and
preflight before operator maintenance:
`tools/mg sim-posture deploy`, `tools/mg sim-posture preflight`, then
`tools/mg sim-posture maintenance-prepare` and `tools/mg sim-posture status`.
The maintenance command declares, for that invocation only, that the operator
has externally suspended dispatch on BOTH Macs and drained outstanding jobs
and leases; MG cannot detect that administrative pause. Everyday `prepare`
only reuses the connected hosted owner; initial boot/migration or a supported
stopped-reservation resume requires maintenance. A disconnected hosted owner
still cannot be replaced by this runtime, even in maintenance. See
[resident deployment, migration and connection reuse](sim-posture.md#resident-deployment-and-the-one-drained-migration)
for exact commands, prerequisites and pending lab acceptance. Control calls
are short reconnectable SSH RPCs; Tart, RFB and private state survive their
loss. There is no M5 VNC tunnel for a new remote session. An existing hosted
connection is reused without a stop/start. Do not connect another RFB client.

The following lifecycle and forward details apply to retained legacy/local
routes. They do not authorize a new remote foreground VNC session or a second
restart of the resident route.

Preparation, manual `duo-fold.sh start` and `stop` support only the configured
primary VM (the first entry), using its one M5 scheduler lease and the single
existing main-checkout `build/vm-fold` state directory. Secondary lifecycle
requests and state naming another VM are refused before teardown/start.
They share MG's scheduler mutex primitives and
refuse active guest simulator acquisitions, unknown lease expiry/inventory or
running guest test jobs before stopping. Expired records, including orphans,
are read without changing them. Repeated preparation never restarts an existing
session. Read-only status/inspection can verify a previous-format session;
its teardown and any ambiguous state require operator recovery, never PID cleanup.
A fresh preparation checks the control socket's actual bind/listen before
stopping, including macOS's maximum 103-byte socket pathname. Shorten the main
checkout path if needed; no alternate state directory is created. SSH forward
readiness uses macOS `lsof` evidence for the owned foreground SSH child's exact
loopback listener, never a VNC probe connection. The holder reports actual RFB
loss even while FIFO-idle; readiness requires live transports. A disconnected
post-prepare status fails preparation; read-only status can successfully report
disconnected. Linux/Docker tests substitute the macOS OS observation, not a live
M5 transport acceptance check.

A failed preparation keeps restoration armed after a completed shutdown,
including a failed subsequent stopped-state query. It attempts ordinary headless
service only after confirming stopped state and the owned remote run's exit.
A lost/255/timed-out shutdown or unknown remote run completion retains evidence
and refuses another start. Rerunning the installed wrapper or observing Tart's
state cannot recover a lost command exit. These cases, unowned historical
sessions and unavailable owners need operator reconciliation on M3 and M5;
`stop` is not a guaranteed repair. Retain private evidence, do not clear PID/
owner/pending records or prepare again to bypass the refusal. No automatic
recovery command is provided for that unresolved state.

Release the acquired simulator with `tools/mg sim release <udid>` before
`tools/vm/duo-fold.sh stop`; wait for guest test jobs too. Raw Tart/holder/tunnel logs and the session
password stay in private main-checkout state; never copy them into results.

## Moving a local VM (historical alternative)

The VM may live at `.local/meguri-vm/` instead of on T9. A stopped VM can be
copied with its Tart application and dedicated SSH directory. Preserve the
sparse disk image, configuration, NVRAM, permissions and SSH host checks.
Leave runtime sockets and old download caches behind. Keep the original until
the copied VM boots and answers over SSH.

The Tart and SSH wrappers must resolve their own directory, rather than refer
to `/Volumes/T9`. Tart's `TART_HOME` must point to the copied `tart-home/`.
Keep the dedicated SSH key private; never put it in documentation or logs.

Configure the existing runner through
`~/Library/Application Support/meguri/mac-vm.json`, or set
`MEGURI_MAC_VM_CONFIG` to an ignored local JSON file. The format is documented
in `tools/vm/AGENTS.md`. No simulator or host-screen fallback is needed.

Run `tools/mg doctor` after configuration. Run `tools/mg sim status` to check
the pool through the normal runner. A copied VM may have older Xcode or pinned
simulator runtimes; record those differences before running Apple checks.
Use `tools/mg vm-xcode-stage` for supported Xcode staging, under its VM lease.

## Host tools

Docker development runs in OrbStack. Its machine and volumes may contain other
projects: never copy or reset the whole OrbStack store for Meguri. Meguri's
Docker build cache is disposable and can rebuild on the new Mac. An existing
dedicated cache can instead be selected with `MEGURI_DOCKER_CARGO_TARGET`. It
applies only to the checkout that owns that directory; every other worktree
keeps its own cache volume (`tools/AGENTS.md`).

Install the Rust version, components and Apple targets in
`core/rust-toolchain.toml`. Native Apple builds need this host toolchain; Docker
Rust checks use the pinned development image. Source `~/.cargo/env` if the new
shell does not yet find Rust.

When present, `.local/activate` contains this machine's project-scoped tool
paths. Source it before running jobs. It changes the current shell only. The
current remote VM uses the default per-user config above; activation must not
restore the obsolete worktree-local `MEGURI_MAC_VM_CONFIG` override.

```sh
source .local/activate
tools/mg doctor
tools/mg core-test --canaries
```

Apple account sign-in, license consent and other reserved steps remain in
`one-time-setup.md`. A migrated app or profile does not prove that signing works
on the new Mac. Do not copy personal Keychains or account secrets into Git.

## Run output

Every `tools/mg` run leaves a folder under `build/mg/`, and nothing removes
them automatically. `tools/mg doctor` warns when a checkout's `build/mg` passes
20 GiB. `tools/mg prune` shows what would go and deletes nothing;
`tools/mg prune --cleanroom` does the same for old clean-room runs. Once no job
runs in any checkout, `tools/mg prune --apply` (or `--cleanroom --apply`)
deletes it there. Prune never touches Docker volumes, Rust targets, derived
data or anything else outside `build/mg`. It never deletes a run named in a
tracked or uncommitted file, a branch, a state file or the receipt of a run it
keeps; `tools/AGENTS.md` gives the full rule. Pruning before a
move leaves less to copy.

## Resume prepared work

Preserve ignored `build/` handoffs, exported patches and review receipts when
moving the checkout. Git alone does not contain them. Audit their recorded base
against master, then resume them without reimplementing completed work.
Old receipts remain historical evidence; repeat affected checks when the host,
toolchain, signing identity or runtime has changed. Keep the original integration
batch base and do not convert a queued or unrun check into a pass.
