# shellcheck shell=bash
# The macOS test VMs (U8b, U8c): the per-host config, tart's view of a VM, a
# headless start, the wait for a logged-in session, quoting for the VM's
# shell, copies by tar over SSH, stopping a test run left in a VM, and where
# a VM's lease is and whether this process runs under its holder. Sourced by
# tools/mg.d/mac-vm.sh and by the scripts that reach a VM outside tools/mg
# (sim-pool.sh, xcode_project_check.sh). Runs under macOS /bin/bash 3.2 and
# in the dev container. Functions report through MVL_* variables; only
# mvl_state, mvl_q, mvl_epoch, mvl_lease_base and mvl_lease_dir print on
# stdout.
#
# Config: $MEGURI_MAC_VM_CONFIG, else ~/Library/Application Support/meguri/
# mac-vm.json: {"tart": <wrapper>, "ssh": <wrapper>, "vms": [<name> |
# {"name", "ssh", "tart"}]} (tools/vm/AGENTS.md). Overrides (self-tests):
# MEGURI_MAC_VM_POLL_SECONDS (3), MEGURI_MAC_VM_BOOT_SECONDS (240).

mvl_config_path() {
  printf '%s\n' "${MEGURI_MAC_VM_CONFIG:-$HOME/Library/Application Support/meguri/mac-vm.json}"
}

mvl_epoch() { date -u +%s; }

# A sleep that a TERM trap interrupts at once.
mvl_sleep() {
  sleep "$1" &
  wait "$!"
}

# mvl_read_config: sets MVL_ALL to the configured VMs, a JSON array of
# {name, tart, ssh}. Returns 1 without a config and 2 for one that is not
# valid, with MVL_REASON.
mvl_read_config() {
  local cfg vms
  cfg="$(mvl_config_path)"
  MVL_ALL='[]'
  if [ ! -f "$cfg" ]; then
    MVL_REASON="no macOS test VM is configured ($cfg is missing)"
    return 1
  fi
  if ! vms="$(jq -ce '. as $c | [(.vms // [])[] | (if type == "string" then {name: .} else . end)
      | {name, tart: (.tart // $c.tart), ssh: (.ssh // $c.ssh)}]
      | if length > 0 and all(.[]; ([.name, .tart, .ssh] | all(type == "string" and . != ""))
          and (.name | test("^[A-Za-z0-9._-]+$"))) then . else error("invalid") end' "$cfg" 2>/dev/null)"; then
    MVL_REASON="$cfg is not valid: it needs {\"tart\": <wrapper>, \"ssh\": <wrapper>, \"vms\": [<name> or {\"name\", \"ssh\"}, ...]} (tools/vm/AGENTS.md)"
    return 2
  fi
  MVL_ALL="$vms"
  return 0
}

# mvl_state <tart> <name>: running, stopped (or tart's other state), or
# missing; nothing when tart cannot list its VMs.
mvl_state() {
  local list
  list="$("$1" list --format json 2>/dev/null </dev/null)" || return 0
  jq -r --arg n "$2" 'first(.[]? | select(.Name == $n) | (.State // "unknown")) // "missing"' <<<"$list" 2>/dev/null
}

# mvl_probe: the configured VMs that can run now: their wrappers are there
# (the drive that holds them is connected) and tart lists them, in the
# config's order. Sets MVL_VMS (a JSON array of {name, tart, ssh, state}) and
# returns 0, or sets MVL_REASON and returns 1 (none can) or 2 (the config is
# not valid). Reads only.
mvl_probe() {
  local rc n i=0 vm name tart ssh state reasons=""
  MVL_VMS='[]'
  mvl_read_config
  rc=$?
  [ "$rc" -eq 0 ] || return "$rc"
  n="$(jq length <<<"$MVL_ALL")"
  while [ "$i" -lt "$n" ]; do
    vm="$(jq -c --argjson i "$i" '.[$i]' <<<"$MVL_ALL")"
    i=$((i + 1))
    name="$(jq -r .name <<<"$vm")"
    tart="$(jq -r .tart <<<"$vm")"
    ssh="$(jq -r .ssh <<<"$vm")"
    if [ ! -x "$tart" ]; then
      reasons="$reasons; the tart wrapper $tart is not there (is the drive that holds the VM connected?)"
      continue
    fi
    if [ ! -x "$ssh" ]; then
      reasons="$reasons; the SSH wrapper $ssh of $name is not there (is the drive that holds the VM connected?)"
      continue
    fi
    state="$(mvl_state "$tart" "$name")"
    case "$state" in
      "") reasons="$reasons; $tart list failed" && continue ;;
      missing) reasons="$reasons; tart lists no VM named $name" && continue ;;
    esac
    MVL_VMS="$(jq -c --argjson v "$vm" --arg s "$state" '. + [$v + {state: $s}]' <<<"$MVL_VMS")"
  done
  if [ "$MVL_VMS" = '[]' ]; then
    MVL_REASON="${reasons#; }"
    return 1
  fi
  return 0
}

# mvl_start <tart> <name> <log>: `tart run --no-graphics` in a session of its
# own (perl setsid), so the VM outlives the caller, its process group and
# its terminal, with none of the caller's files open. Nothing stops it.
mvl_start() {
  (
    cd / && exec perl -MPOSIX=setsid -e 'fork and exit 0; setsid(); exec @ARGV or die "exec $ARGV[0]: $!\n"' \
      "$1" run --no-graphics "$2" </dev/null >"$3" 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
  )
}

# mvl_wait_ready <ssh> <tart> <name> <started true|false> <tart log> <ssh err file>:
# waits until SSH answers in a logged-in session (the console belongs to a
# user, not to the login window): about 20 s after a boot. Returns 0, or 1
# with MVL_FAIL (start: tart could not start it; ssh: no session within
# MEGURI_MAC_VM_BOOT_SECONDS) and MVL_REASON.
mvl_wait_ready() {
  local ssh="$1" tart="$2" name="$3" started="$4" log="$5" err="$6" t0 now user last
  local limit="${MEGURI_MAC_VM_BOOT_SECONDS:-240}" poll="${MEGURI_MAC_VM_POLL_SECONDS:-3}"
  t0="$(mvl_epoch)"
  while :; do
    user="$("$ssh" 'stat -f %Su /dev/console' 2>"$err" </dev/null)"
    if [ -n "$user" ] && [ "$user" != root ]; then
      [ "$started" = false ] || echo "mac-vm: $name answers with $user logged in after $(($(mvl_epoch) - t0)) s" >&2
      rm -f "$err"
      return 0
    fi
    now="$(mvl_epoch)"
    # tart run fails within seconds when the VM cannot start: no need to wait
    # out the limit.
    if [ "$started" = true ] && { [ $((now - t0)) -ge 15 ] || [ $((now - t0)) -ge "$limit" ]; } &&
      [ "$(mvl_state "$tart" "$name")" != running ]; then
      MVL_FAIL=start
      MVL_REASON="tart could not start $name: $(tail -n 3 "$log" 2>/dev/null | tr '\n' ' ' | sed 's/ *$//')"
      return 1
    fi
    if [ $((now - t0)) -ge "$limit" ]; then
      if [ "$user" = root ]; then
        last="the login window still has the console"
      else
        last="$(tail -n 2 "$err" 2>/dev/null | tr '\n' ' ' | sed 's/ *$//')"
      fi
      MVL_FAIL=ssh
      MVL_REASON="$name did not answer over SSH in a logged-in session within $limit s (last: ${last:-no answer})"
      return 1
    fi
    mvl_sleep "$poll"
  done
}

# mvl_ensure_ready <vm JSON {name, tart, ssh}> <dir for the tart log and SSH
# errors>: starts the VM headless when it is not running, then waits for a
# logged-in session. Sets MVL_STARTED (true|false) and MVL_TART_LOG (the log,
# when it started it). Returns mvl_wait_ready's result.
mvl_ensure_ready() {
  local name tart ssh state
  name="$(jq -r .name <<<"$1")"
  tart="$(jq -r .tart <<<"$1")"
  ssh="$(jq -r .ssh <<<"$1")"
  MVL_STARTED=false
  MVL_TART_LOG=""
  state="$(mvl_state "$tart" "$name")"
  if [ "$state" != running ]; then
    MVL_TART_LOG="$2/tart-run-$name.log"
    echo "mac-vm: starting $name headless (it was ${state:-in an unknown state})" >&2
    mvl_start "$tart" "$name" "$MVL_TART_LOG"
    MVL_STARTED=true
  fi
  mvl_wait_ready "$ssh" "$tart" "$name" "$MVL_STARTED" "$MVL_TART_LOG" "$2/mac-vm-ssh.err"
}

# mvl_q <word>...: the words, each single-quoted for the VM's shell (zsh).
mvl_q() {
  local out="" a
  for a in "$@"; do
    out="$out '$(printf '%s' "$a" | sed "s/'/'\\\\''/g")'"
  done
  printf '%s' "${out# }"
}

# mvl_copy_in <ssh> <vm name> <checkout root> <absolute path>...: copies files
# and directories to the same absolute paths in the VM (tar over SSH),
# replacing what is there; paths that do not exist here are skipped. Only
# directories inside <checkout root> are replaced. Returns 1 with MVL_REASON
# when a copy fails.
mvl_copy_in() {
  local ssh="$1" vm="$2" root="$3" p parent base rcs
  shift 3
  for p in "$@"; do
    [ -e "$p" ] || continue
    case "$p" in
      "$root"/?*) ;;
      /?*)
        if [ -d "$p" ]; then
          MVL_REASON="will not replace the directory $p in the VM: only this checkout's directories are copied"
          return 1
        fi
        ;;
      *) MVL_REASON="not an absolute path: $p" && return 1 ;;
    esac
    parent="$(dirname "$p")"
    base="$(basename "$p")"
    echo "mac-vm: copying $p into the VM" >&2
    COPYFILE_DISABLE=1 tar -C "$parent" -cf - "$base" |
      "$ssh" "rm -rf $(mvl_q "$p") && mkdir -p $(mvl_q "$parent") && tar -C $(mvl_q "$parent") -xf -"
    rcs="${PIPESTATUS[*]}"
    if [ "$rcs" != "0 0" ]; then
      MVL_REASON="copying $p into the VM $vm failed (tar and ssh exit codes: $rcs); see log.txt"
      return 1
    fi
  done
  return 0
}

# mvl_copy_out <ssh> <vm name> <absolute path>...: copies files and
# directories back from the same absolute paths in the VM, replacing what is
# here. Returns 1 with MVL_REASON when one is not there or a copy fails.
mvl_copy_out() {
  local ssh="$1" vm="$2" p parent base rcs out probe
  shift 2
  for p in "$@"; do
    case "$p" in /?*) ;; *) MVL_REASON="not an absolute path: $p" && return 1 ;; esac
    parent="$(dirname "$p")"
    base="$(basename "$p")"
    out="$("$ssh" "test -e $(mvl_q "$p")" 2>&1 </dev/null)"
    probe=$?
    case "$probe" in
      0) ;;
      1) MVL_REASON="the VM $vm has no $p" && return 1 ;;
      *) MVL_REASON="could not check for $p in the VM $vm (ssh exit $probe: $(tail -n 1 <<<"$out"))" && return 1 ;;
    esac
    rm -rf "$p"
    mkdir -p "$parent" || { MVL_REASON="cannot create $parent" && return 1; }
    "$ssh" "tar -C $(mvl_q "$parent") -cf - $(mvl_q "$base")" </dev/null | tar -C "$parent" -xf -
    rcs="${PIPESTATUS[*]}"
    if [ "$rcs" != "0 0" ]; then
      MVL_REASON="copying $p back from the VM $vm failed (ssh and tar exit codes: $rcs)"
      return 1
    fi
    if [ ! -e "$p" ]; then
      MVL_REASON="copying $p back from the VM $vm brought nothing back"
      return 1
    fi
  done
  return 0
}

# mvl_copy_to <ssh> <vm name> <path> <VM dir>: copies a file or directory
# into <VM dir> in the VM under its own name, replacing what is there there
# (for inputs outside the checkout, which never go to their own path).
# Returns 1 with MVL_REASON when the copy fails.
mvl_copy_to() {
  local ssh="$1" vm="$2" p="$3" dir="$4" base rcs
  base="$(basename "$p")"
  echo "mac-vm: copying $p into the VM, to $dir" >&2
  COPYFILE_DISABLE=1 tar -C "$(dirname "$p")" -cf - "$base" |
    "$ssh" "rm -rf $(mvl_q "$dir/$base") && mkdir -p $(mvl_q "$dir") && tar -C $(mvl_q "$dir") -xf -"
  rcs="${PIPESTATUS[*]}"
  if [ "$rcs" != "0 0" ]; then
    MVL_REASON="copying $p into the VM $vm failed (tar and ssh exit codes: $rcs); see log.txt"
    return 1
  fi
  return 0
}

# mvl_stop_tests <ssh>: stops every UI test run in the VM (xcodebuild
# test-without-building and the test runner it drives: SIGTERM, then SIGKILL
# halfway through MEGURI_MAC_VM_STOP_SECONDS, 20) and confirms none is left.
# Only the lease holder runs tests in a VM, so for the holder any such
# process is a run left behind (a lost SSH session, a killed job), and two
# runs would share the VM's one login session. Returns 0 when none is left
# (MVL_STOPPED=1 when it stopped one); 1 when one still runs or the VM did
# not answer, with MVL_REASON.
mvl_stop_tests() {
  local t="${MEGURI_MAC_VM_STOP_SECONDS:-20}" out rc
  MVL_STOPPED=""
  MVL_REASON=""
  # The brackets keep the shell that runs this command from matching itself.
  out="$("$1" "p=$(mvl_q 'xcodebuild[ ]test-without-building|UITests-Runner[.]app/Contents/MacOS/')
if pgrep -f \"\$p\" >/dev/null; then
  echo 'mg-mac-vm: stopping'
  pkill -f \"\$p\"
  i=0
  while pgrep -f \"\$p\" >/dev/null && [ \$i -lt $t ]; do
    [ \$i -ne $((t / 2)) ] || pkill -9 -f \"\$p\"
    sleep 1
    i=\$((i + 1))
  done
fi
if pgrep -f \"\$p\" >/dev/null; then echo 'mg-mac-vm: still running'; else echo 'mg-mac-vm: none'; fi" 2>&1 </dev/null)"
  rc=$?
  if grep -qx 'mg-mac-vm: none' <<<"$out"; then
    ! grep -qx 'mg-mac-vm: stopping' <<<"$out" || MVL_STOPPED=1
    return 0
  fi
  if grep -qx 'mg-mac-vm: still running' <<<"$out"; then
    MVL_REASON="it was still running $t s after SIGTERM and SIGKILL"
  else
    MVL_REASON="the VM did not answer (ssh exit $rc: $(tail -n 1 <<<"$out"))"
  fi
  return 1
}

# ---------------------------------------------------------------------------
# Leases: one job at a time per VM (tools/mg.d/mac-vm.sh), beside the host
# lease (tools/mg.d/host.sh). A lease's holder.json names the pid of the
# tools/mg run that holds it (or of a recording worker).

# mvl_lease_base <checkout>: where the host lease and the VMs' leases live,
# shared by every checkout of the repository: the main checkout's build/mg
# (the directory of $MEGURI_HOST_LEASE_DIR when it is set).
mvl_lease_base() {
  local common
  if [ -n "${MEGURI_HOST_LEASE_DIR:-}" ]; then
    dirname "$MEGURI_HOST_LEASE_DIR"
    return 0
  fi
  common="$(git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || common=""
  if [ -n "$common" ] && [ "$(basename "$common")" = .git ]; then
    printf '%s/build/mg\n' "$(dirname "$common")"
  else
    printf '%s/build/mg\n' "$1"
  fi
}

# mvl_lease_dir <checkout> <vm name>: that VM's lease.
mvl_lease_dir() { printf '%s/mac-vm.%s.lock\n' "$(mvl_lease_base "$1")" "$2"; }

# mvl_lease_held <lease dir>: whether this process runs under the holder of
# the lease or of its nested slot (that pid is this shell's or one of its
# ancestors'): a job's own scripts use what the job leased, and must not wait
# for the lease the job holds.
mvl_lease_held() {
  local pid
  for pid in "$(jq -r '.pid // empty' "$1/holder.json" 2>/dev/null)" "$(jq -r '.pid // empty' "$1/inner/holder.json" 2>/dev/null)"; do
    [ -n "$pid" ] || continue
    ps -A -o pid= -o ppid= 2>/dev/null | awk -v p="$$" -v want="$pid" '
      { up[$1] = $2 }
      END { for (i = 0; i < 200 && p + 0 > 0; i++) { if (p + 0 == want + 0) exit 0; p = up[p] } exit 1 }' && return 0
  done
  return 1
}
