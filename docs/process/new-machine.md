# Continuing on another Mac

Keep source and portable instructions in Git. Keep VM disks, VM SSH keys,
machine configuration and local credentials outside tracked files. The root
`.local/` directory is ignored for this purpose; it must never be force-added.
It is not a backup: back it up separately before deleting or replacing it.

## Local test VM

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
paths. Source it before running jobs. It changes the current shell only.

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
runs in the checkout, `tools/mg prune --apply` (or `--cleanroom --apply`)
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
