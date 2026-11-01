#!/bin/bash
# tools/vm/add-sim-runtime.sh: puts the pinned iOS simulator runtime
# (tools/sim-devices.json) into the macOS test VM (U8c), once, from this
# Mac's copy: nothing is downloaded. Run by the orchestrator when doctor
# reports the VM without it (docs/process/mac-ui-vm.md).
#
# usage: tools/vm/add-sim-runtime.sh [--vm <name>] [--foldable]
#
# --foldable puts the foldable kind's own pinned runtime there instead.
#
# The VM is the first one of the per-host config (tools/vm/AGENTS.md) that
# can run, or the one named; it is started headless when it is stopped. The
# runtime image this Mac's CoreSimulator holds for the pinned identifier and
# build (`simctl runtime list`) goes by tar over SSH to ~/runtime-import/ in
# the VM, `xcrun simctl runtime add` adds it there (as a clone: no second
# copy on the VM's disk), and the import directory is removed. A VM that
# already lists the pinned runtime is left as it is. Exit codes: 0 done (or
# already there), 1 failed, 2 usage.

set -u

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
# shellcheck source=../scripts/mac-vm-lib.sh
. "$ROOT/tools/scripts/mac-vm-lib.sh"

log() { printf 'add-sim-runtime: %s\n' "$*" >&2; }
die() { log "$*" && exit 1; }

want="" pin_path=.runtime
while [ "$#" -gt 0 ]; do
  case "$1" in
    --vm) [ "$#" -ge 2 ] || { log "--vm needs a name" && exit 2; }; want="$2" && shift 2 ;;
    --foldable) pin_path=.foldable.runtime && shift ;;
    *) log "usage: tools/vm/add-sim-runtime.sh [--vm <name>] [--foldable]" && exit 2 ;;
  esac
done

pin="$(jq -c "$pin_path" "$ROOT/tools/sim-devices.json")" || die "cannot read tools/sim-devices.json"
id="$(jq -r .identifier <<<"$pin")"
build="$(jq -r .build <<<"$pin")"
label="iOS $(jq -r .version <<<"$pin") ($build)"

MVL_REASON=""
mvl_probe || die "no test VM can run: $MVL_REASON"
vm="$(jq -c --arg n "$want" 'first(.[] | select($n == "" or .name == $n)) // empty' <<<"$MVL_VMS")"
[ -n "$vm" ] || die "no VM named $want can run (available: $(jq -r '[.[].name] | join(", ")' <<<"$MVL_VMS"))"
name="$(jq -r .name <<<"$vm")"
ssh="$(jq -r .ssh <<<"$vm")"
logs="${TMPDIR:-/tmp}/meguri-add-sim-runtime"
mkdir -p "$logs" || die "cannot create $logs"
mvl_ensure_ready "$vm" "$logs" || die "the VM $name could not run: $MVL_REASON"

# has_runtime: whether the VM lists the pinned runtime as available.
has_runtime() {
  "$ssh" "xcrun simctl list runtimes --json" </dev/null 2>/dev/null |
    jq -e --arg i "$id" --arg b "$build" 'any(.runtimes[]?; .identifier == $i and .buildversion == $b and .isAvailable != false)' >/dev/null
}
if has_runtime; then
  log "$name already has the $label simulator runtime"
  exit 0
fi

# The image this Mac holds: a cryptex asset directory (Restore/<image>.dmg
# inside) or a disk image.
src="$(xcrun simctl runtime list -j 2>/dev/null | jq -r --arg i "$id" --arg b "$build" '
  first(.[] | select(.runtimeIdentifier == $i and .build == $b and .state == "Ready") | .path) // empty')"
[ -n "$src" ] && [ -e "$src" ] || die "this Mac has no ready $label simulator runtime to copy ($id); install it here first (docs/process/one-time-setup.md)"
if [ -d "$src" ]; then
  dmg="$(find "$src/Restore" -maxdepth 1 -name '*.dmg' -type f 2>/dev/null | head -n 1)"
  [ -n "$dmg" ] || die "no disk image under $src/Restore"
  rel="$(basename "$src")/Restore/$(basename "$dmg")"
  parent="$(dirname "$src")"
  base="$(basename "$src")"
else
  rel="$(basename "$src")"
  parent="$(dirname "$src")"
  base="$rel"
fi

dest="runtime-import/$id-$build"
log "copying $src ($(du -sh "$src" 2>/dev/null | cut -f1)) into $name"
COPYFILE_DISABLE=1 tar -C "$parent" -cf - "$base" |
  "$ssh" "rm -rf ~/$(mvl_q "$dest") && mkdir -p ~/$(mvl_q "$dest") && tar -C ~/$(mvl_q "$dest") -xf -"
rcs="${PIPESTATUS[*]}"
[ "$rcs" = "0 0" ] || die "copying the runtime into $name failed (tar and ssh exit codes: $rcs)"
log "adding it in $name (xcrun simctl runtime add)"
"$ssh" "xcrun simctl runtime add ~/$(mvl_q "$dest/$rel")" </dev/null >&2
rc=$?
"$ssh" "rm -rf ~/$(mvl_q "$dest")" </dev/null
[ "$rc" -eq 0 ] || die "xcrun simctl runtime add failed in $name (exit $rc)"
has_runtime || die "$name does not list the $label runtime after adding it"
log "$name has the $label simulator runtime"
