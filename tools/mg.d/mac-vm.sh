# Mac UI suites in the macOS test VM (U8b). Sourced by tools/mg.
#
# Every Mac XCUITest run (mac-ui, a11y --platform mac, drive --platform mac)
# builds on the host and runs its tests in a macOS VM on this Mac
# (docs/process/mac-ui-vm.md, tools/vm/AGENTS.md), so it never takes over the
# host's screen, needs no host session and runs as often as needed. The VMs
# are named by a per-host config, never by the repository:
# $MEGURI_MAC_VM_CONFIG, else ~/Library/Application Support/meguri/mac-vm.json:
#
#   {"tart": "/Volumes/T9/meguri-vm/bin/tart",
#    "ssh": "/Volumes/T9/meguri-vm/bin/vm-ssh",
#    "vms": ["meguri-mac", {"name": "meguri-mac-2", "ssh": "<its wrapper>"}]}
#
# `ssh` is a wrapper that runs its arguments as a shell command in that VM;
# a VM entry may name its own (and its own tart). A run takes the first VM
# whose lease is free (mac-vm.<name>.lock beside the host lease, shared by
# every checkout), starts it headless when it is not running, waits for a
# logged-in session, copies this checkout's test products and app/Fixtures to
# the same absolute paths in the VM, runs xcodebuild test-without-building
# there and copies the result bundle back. Without the config, the drive or
# the VM, the suites are queued (warn, user step mac-vm), or with
# MEGURI_MAC_UI_ALLOW_HOST=1 take the host path (host lease and hourly gate,
# host.sh). A VM that does not start or answer is an error, never a reason to
# use the host's screen.
#
# Overrides (self-tests): MEGURI_MAC_VM_CONFIG, MEGURI_MAC_VM_POLL_SECONDS (3),
# MEGURI_MAC_VM_BOOT_SECONDS (240), MEGURI_MAC_VM_LEASE_WAIT_SECONDS (3600),
# MEGURI_MAC_VM_STOP_SECONDS (20).

MG_MAC_RUNNER=""
MG_MAC_VM=""
MG_MAC_VM_DETAIL='{}'
MG_MAC_VM_RUN_ERROR=""
MG_MAC_VM_TESTING=""
# Set (to the reason) when a test run that lost its SSH session could not be
# stopped in the VM: this job starts no further run there.
MG_MAC_VM_LOST=""
MG_MAC_VM_MARK="mg-mac-vm: remote exit"

mg_mac_vm_config_path() {
  printf '%s\n' "${MEGURI_MAC_VM_CONFIG:-$HOME/Library/Application Support/meguri/mac-vm.json}"
}

# mg__mac_vm_read_config: sets MG_MAC_VM_ALL to the configured VMs, a JSON
# array of {name, tart, ssh}. Returns 1 without a config and 2 for one that
# is not valid, with MG_MAC_VM_REASON.
mg__mac_vm_read_config() {
  local cfg vms
  cfg="$(mg_mac_vm_config_path)"
  MG_MAC_VM_ALL='[]'
  if [ ! -f "$cfg" ]; then
    MG_MAC_VM_REASON="no Mac UI test VM is configured ($cfg is missing)"
    return 1
  fi
  if ! vms="$(jq -ce '. as $c | [(.vms // [])[] | (if type == "string" then {name: .} else . end)
      | {name, tart: (.tart // $c.tart), ssh: (.ssh // $c.ssh)}]
      | if length > 0 and all(.[]; ([.name, .tart, .ssh] | all(type == "string" and . != ""))
          and (.name | test("^[A-Za-z0-9._-]+$"))) then . else error("invalid") end' "$cfg" 2>/dev/null)"; then
    MG_MAC_VM_REASON="$cfg is not valid: it needs {\"tart\": <wrapper>, \"ssh\": <wrapper>, \"vms\": [<name> or {\"name\", \"ssh\"}, ...]} (tools/vm/AGENTS.md)"
    return 2
  fi
  MG_MAC_VM_ALL="$vms"
  return 0
}

# mg__mac_vm_state <tart> <name>: running, stopped (or tart's other state),
# or missing; nothing when tart cannot list its VMs.
mg__mac_vm_state() {
  local list
  list="$("$1" list --format json 2>/dev/null </dev/null)" || return 0
  jq -r --arg n "$2" 'first(.[]? | select(.Name == $n) | (.State // "unknown")) // "missing"' <<<"$list" 2>/dev/null
}

# mg_mac_vm_probe: the configured VMs that can run now: their wrappers are
# there (the drive that holds them is connected) and tart lists them. Sets
# MG_MAC_VMS (a JSON array of {name, tart, ssh, state}) and returns 0, or
# sets MG_MAC_VM_REASON and returns 1 (none can) or 2 (the config is not
# valid). Reads only.
mg_mac_vm_probe() {
  local rc n i=0 vm name tart ssh state reasons=""
  mg__mac_vm_read_config
  rc=$?
  [ "$rc" -eq 0 ] || return "$rc"
  MG_MAC_VMS='[]'
  n="$(jq length <<<"$MG_MAC_VM_ALL")"
  while [ "$i" -lt "$n" ]; do
    vm="$(jq -c --argjson i "$i" '.[$i]' <<<"$MG_MAC_VM_ALL")"
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
    state="$(mg__mac_vm_state "$tart" "$name")"
    case "$state" in
      "") reasons="$reasons; $tart list failed" && continue ;;
      missing) reasons="$reasons; tart lists no VM named $name" && continue ;;
    esac
    MG_MAC_VMS="$(jq -c --argjson v "$vm" --arg s "$state" '. + [$v + {state: $s}]' <<<"$MG_MAC_VMS")"
  done
  if [ "$MG_MAC_VMS" = '[]' ]; then
    MG_MAC_VM_REASON="${reasons#; }"
    return 1
  fi
  return 0
}

# mg_mac_vm_user_step: the one-time step behind a missing VM.
mg_mac_vm_user_step() {
  mg_user_step mac-vm missing true "Connect the drive with the Mac UI test VM" \
    "Mac UI suites run only in the macOS test VM, never on this Mac's screen: connect the T9 drive, or set the VM up and name it in $(mg_mac_vm_config_path) (docs/process/mac-ui-vm.md). Until then they are queued."
}

# mg__mac_vm_detail <JSON object>: merged into details.mac_vm.
mg__mac_vm_detail() {
  MG_MAC_VM_DETAIL="$(jq -c --argjson d "$1" '. + $d' <<<"$MG_MAC_VM_DETAIL")"
  mg_detail mac_vm "$MG_MAC_VM_DETAIL"
}

# mg__mac_vm_fail <id> <reason>: the VM could not run the suites: an error,
# never fail, and never a run on the host instead.
mg__mac_vm_fail() {
  mg_failure "$1" "$2"
  mg_message "$MG_JOB: the test VM could not run the suites: $2"
  mg_status error
}

# ---------------------------------------------------------------------------
# Choosing the runner

# mg_mac_ui_begin: where this job's Mac XCUITest runs go, decided before it
# builds: the VM when one can run (MG_MAC_RUNNER=vm); without one, the host
# when MEGURI_MAC_UI_ALLOW_HOST=1 and the hourly gate is open (host);
# otherwise the run is recorded as queued (warn) and this returns 1. A config
# that is not valid is an error (returns 1).
mg_mac_ui_begin() {
  local rc reason
  MG_MAC_RUNNER=""
  mg_mac_vm_probe
  rc=$?
  case "$rc" in
    0)
      MG_MAC_RUNNER=vm
      mg_detail mac_runner '"vm"'
      return 0
      ;;
    2)
      mg_failure mac-vm-config "$MG_MAC_VM_REASON" "tools/mg doctor"
      mg_message "$MG_JOB: $MG_MAC_VM_REASON"
      mg_status error
      return 1
      ;;
  esac
  if [ "${MEGURI_MAC_UI_ALLOW_HOST:-}" = 1 ]; then
    if ! reason="$(mg_mac_ui_gate_open)"; then
      mg__mac_ui_queued "$reason"
      return 1
    fi
    echo "$MG_JOB: no test VM ($MG_MAC_VM_REASON); MEGURI_MAC_UI_ALLOW_HOST=1: running on this Mac's screen" >&2
    MG_MAC_RUNNER=host
    mg_detail mac_runner '"host"'
    return 0
  fi
  mg_detail queued true
  mg_detail mac_vm "$(jq -nc --arg r "$MG_MAC_VM_REASON" --arg c "$(mg_mac_vm_config_path)" '{reason: $r, config: $c}')"
  mg_mac_vm_user_step
  mg_message "$MG_JOB: queued: $MG_MAC_VM_REASON; Mac UI suites run only in the test VM (docs/process/mac-ui-vm.md), never on this Mac's screen"
  mg_status warn
  return 1
}

# mg_mac_ui_take: after the build, what the runner needs: the VM's lease,
# start and copy (mg_mac_vm_take), or on the host its lease, the .dev app
# guard and the hourly gate. Records the result and returns 1 when the job
# cannot go on.
mg_mac_ui_take() {
  if [ "$MG_MAC_RUNNER" = vm ]; then
    mg_mac_vm_take
  else
    mg_host_lease && mg_app_mac_guard && mg_mac_ui_gate
  fi
}

# mg_mac_vm_take: a VM lease, the VM running with a logged-in session and no
# test run left in it, and this checkout's test products and fixtures in it.
# Sets MG_MAC_VM.
mg_mac_vm_take() {
  local name
  mg__mac_vm_lease || return 1
  mg_on_exit mg__mac_vm_stop_orphans
  mg__mac_vm_boot || return 1
  # A run that an earlier job could not stop (its SSH session was lost, or the
  # job was killed) ends before this job's run starts: until then the VM gets
  # no new run.
  name="$(jq -r .name <<<"$MG_MAC_VM")"
  if ! mg__mac_vm_stop_tests; then
    mg__mac_vm_fail mac-vm-stop "a UI test run that an earlier job left in $name could not be stopped: $MG_MAC_VM_STOP_REASON; no new run starts there until it is"
    return 1
  fi
  if [ -n "$MG_MAC_VM_STOPPED" ]; then
    echo "mac-vm: stopped a UI test run that an earlier job left in $name" >&2
    mg__mac_vm_detail '{"stopped_left_run": true}'
  fi
  if ! mg_mac_vm_copy_in "$MG_ROOT/$MG_APP_DD/Build/Products" "$MG_ROOT/app/Fixtures"; then
    mg_message "$MG_JOB: the test VM could not run the suites: the test products could not be copied into it"
    mg_status error
    return 1
  fi
  return 0
}

# ---------------------------------------------------------------------------
# VM lease: one run at a time per VM, across every checkout.

mg__mac_vm_lease_dir() { printf '%s/mac-vm.%s.lock\n' "$(dirname "$(mg_host_lease_dir)")" "$1"; }

# mg__mac_vm_lease: takes the first free VM of MG_MAC_VMS, waiting while all
# are leased; a lease whose holder died is removed (under the host lease's
# mutex helpers, on the VM lease's own mutex file). Released when the job
# ends.
mg__mac_vm_lease() {
  local wait="${MEGURI_MAC_VM_LEASE_WAIT_SECONDS:-3600}" poll="${MEGURI_MAC_VM_POLL_SECONDS:-3}"
  local start n i vm name dir holder holders announced=""
  start="$(mg__epoch)"
  n="$(jq length <<<"$MG_MAC_VMS")"
  mkdir -p "$(dirname "$(mg_host_lease_dir)")" || {
    mg__mac_vm_fail mac-vm-lease "cannot create $(dirname "$(mg_host_lease_dir)")"
    return 1
  }
  while :; do
    holders=""
    i=0
    while [ "$i" -lt "$n" ]; do
      vm="$(jq -c --argjson i "$i" '.[$i]' <<<"$MG_MAC_VMS")"
      name="$(jq -r .name <<<"$vm")"
      dir="$(mg__mac_vm_lease_dir "$name")"
      if mkdir "$dir" 2>/dev/null; then
        mg__lease_write "$dir" false
        mg_on_exit "mg__mac_vm_release '$dir' $$"
        MG_MAC_VM="$vm"
        mg__mac_vm_detail "$(jq -c --arg n "$name" --argjson w "$(($(mg__epoch) - start))" '{name: $n, lease: (. + {waited_s: $w})}' "$dir/holder.json")"
        [ -z "$announced" ] || echo "VM lease: took $name after $(($(mg__epoch) - start)) s" >&2
        return 0
      fi
      holder="$(cat "$dir/holder.json" 2>/dev/null)"
      if mg__lease_stale "$dir" "$holder"; then
        mg__mac_vm_remove_stale "$dir" "$holder" || mg_sleep "$poll"
        continue
      fi
      holders="$holders; $name: $(jq -r '"\(.job) in \(.worktree) since \(.started_at)"' <<<"$holder" 2>/dev/null || echo "another job")"
      i=$((i + 1))
    done
    if [ $(($(mg__epoch) - start)) -ge "$wait" ]; then
      mg__mac_vm_fail mac-vm-busy "every test VM stayed leased for $wait s (${holders#; })"
      return 1
    fi
    if [ -z "$announced" ]; then
      echo "VM lease: waiting for the VM (${holders#; })" >&2
      announced=1
    fi
    mg_sleep "$poll"
  done
}

# mg__mac_vm_remove_stale <lease dir> <holder JSON read>: removes the lease
# if it still holds what was read and its holder is dead.
mg__mac_vm_remove_stale() {
  host_lease_lock "$1" || return 1
  if [ "$(cat "$1/holder.json" 2>/dev/null)" = "$2" ] && mg__lease_stale "$1" "$2"; then
    echo "VM lease: removing a stale lease ($(jq -r '"\(.job) in \(.worktree), pid \(.pid)"' <<<"$2" 2>/dev/null || echo unknown))" >&2
    rm -rf "$1"
  fi
  host_lease_unlock
}

# mg__mac_vm_release <lease dir> <pid>: releases the lease <pid> holds.
mg__mac_vm_release() {
  local locked=0
  host_lease_lock "$1" && locked=1
  [ "$(jq -r '.pid // empty' "$1/holder.json" 2>/dev/null)" != "$2" ] || rm -rf "$1"
  [ "$locked" = 0 ] || host_lease_unlock
}

# ---------------------------------------------------------------------------
# Starting the VM

# mg__mac_vm_start <tart> <name> <log>: `tart run --no-graphics` in a session
# of its own (perl setsid), so the VM outlives this job, its process group
# and its terminal, with none of the job's files open. Jobs never stop it.
mg__mac_vm_start() {
  (
    cd / && exec perl -MPOSIX=setsid -e 'fork and exit 0; setsid(); exec @ARGV or die "exec $ARGV[0]: $!\n"' \
      "$1" run --no-graphics "$2" </dev/null >"$3" 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
  )
}

# mg__mac_vm_boot: starts the leased VM headless when it is not running, then
# waits until SSH answers in a logged-in session (the console belongs to a
# user, not to the login window): about 20 s after a boot. On failure
# records an error and returns 1.
mg__mac_vm_boot() {
  local name tart state started=false log="" t0 now user err last
  local limit="${MEGURI_MAC_VM_BOOT_SECONDS:-240}" poll="${MEGURI_MAC_VM_POLL_SECONDS:-3}"
  name="$(jq -r .name <<<"$MG_MAC_VM")"
  tart="$(jq -r .tart <<<"$MG_MAC_VM")"
  err="$MG_RUN_DIR/mac-vm-ssh.err"
  state="$(mg__mac_vm_state "$tart" "$name")"
  if [ "$state" != running ]; then
    log="$MG_RUN_DIR/tart-run-$name.log"
    echo "mac-vm: starting $name headless (it was ${state:-in an unknown state})" >&2
    mg__mac_vm_start "$tart" "$name" "$log"
    started=true
    mg_artifact log "$log" "tart run $name"
  fi
  mg__mac_vm_detail "$(jq -nc --argjson s "$started" '{started: $s}')"
  t0="$(mg__epoch)"
  while :; do
    user="$(mg_mac_vm_ssh 'stat -f %Su /dev/console' 2>"$err")"
    if [ -n "$user" ] && [ "$user" != root ]; then
      [ "$started" = false ] || echo "mac-vm: $name answers with $user logged in after $(($(mg__epoch) - t0)) s" >&2
      rm -f "$err"
      return 0
    fi
    now="$(mg__epoch)"
    # tart run fails within seconds when the VM cannot start: no need to wait
    # out the limit.
    if [ "$started" = true ] && { [ $((now - t0)) -ge 15 ] || [ $((now - t0)) -ge "$limit" ]; } &&
      [ "$(mg__mac_vm_state "$tart" "$name")" != running ]; then
      mg__mac_vm_fail mac-vm-start "tart could not start $name: $(tail -n 3 "$log" 2>/dev/null | tr '\n' ' ' | sed 's/ *$//')"
      return 1
    fi
    if [ $((now - t0)) -ge "$limit" ]; then
      if [ "$user" = root ]; then
        last="the login window still has the console"
      else
        last="$(tail -n 2 "$err" 2>/dev/null | tr '\n' ' ' | sed 's/ *$//')"
      fi
      mg__mac_vm_fail mac-vm-ssh "$name did not answer over SSH in a logged-in session within $limit s (last: ${last:-no answer})"
      return 1
    fi
    mg_sleep "$poll"
  done
}

# ---------------------------------------------------------------------------
# Commands and files in the leased VM

# mg_mac_vm_ssh <command>: runs a shell command in the leased VM (its login
# shell is zsh; quote words with mg_mac_vm_q), stdin from /dev/null.
# mg_mac_vm_ssh_in keeps stdin.
mg_mac_vm_ssh() { "$(jq -r .ssh <<<"$MG_MAC_VM")" "$1" </dev/null; }
mg_mac_vm_ssh_in() { "$(jq -r .ssh <<<"$MG_MAC_VM")" "$1"; }

# mg_mac_vm_q <word>...: the words, each single-quoted for the VM's shell.
mg_mac_vm_q() {
  local out="" a
  for a in "$@"; do
    out="$out '$(printf '%s' "$a" | sed "s/'/'\\\\''/g")'"
  done
  printf '%s' "${out# }"
}

# mg_mac_vm_copy_in <absolute path>...: copies files and directories to the
# same absolute paths in the leased VM (tar over SSH), replacing what is
# there; paths that do not exist here are skipped. The UI tests read the app
# bundle and fixtures at their build paths. Only this checkout's directories
# are replaced. Records a failure (mac-vm-copy) and returns 1 when a copy
# fails.
mg_mac_vm_copy_in() {
  local p parent base rcs
  for p in "$@"; do
    [ -e "$p" ] || continue
    case "$p" in
      "$MG_ROOT"/?*) ;;
      /?*) [ ! -d "$p" ] || { mg_failure mac-vm-copy "will not replace the directory $p in the VM: only this checkout's directories are copied"; return 1; } ;;
      *) mg_failure mac-vm-copy "not an absolute path: $p"; return 1 ;;
    esac
    parent="$(dirname "$p")"
    base="$(basename "$p")"
    echo "mac-vm: copying $p into the VM" >&2
    COPYFILE_DISABLE=1 tar -C "$parent" -cf - "$base" |
      mg_mac_vm_ssh_in "rm -rf $(mg_mac_vm_q "$p") && mkdir -p $(mg_mac_vm_q "$parent") && tar -C $(mg_mac_vm_q "$parent") -xf -"
    rcs="${PIPESTATUS[*]}"
    if [ "$rcs" != "0 0" ]; then
      mg_failure mac-vm-copy "copying $p into the VM $(jq -r .name <<<"$MG_MAC_VM") failed (tar and ssh exit codes: $rcs); see log.txt"
      return 1
    fi
  done
  return 0
}

# mg__mac_vm_xctestrun <config>: this checkout's UI-plan xctestrun (or
# MG_APP_TEST_PLAN's, built with MG_APP_TEST_SCHEME) built for the
# configuration (the newest, when several SDKs left one). Debug and Profile
# builds write the same file name; its test root says which.
mg__mac_vm_xctestrun() {
  local f
  # shellcheck disable=SC2012 # newest first; the names are Xcode's
  ls -t "$MG_ROOT/$MG_APP_DD/Build/Products"/"${MG_APP_TEST_SCHEME:-Meguri}_${MG_APP_TEST_PLAN:-UI}"_macosx*-arm64.xctestrun 2>/dev/null | while IFS= read -r f; do
    if grep -q "__TESTROOT__/$1/" "$f"; then
      printf '%s\n' "$f"
      break
    fi
  done
}

# mg_mac_vm_xcodebuild_test <prefix> <destination> <config> [xcodebuild args]:
# the UI plan's tests in the leased VM, from the products copied there
# (test-without-building -xctestrun), with this job's TEST_RUNNER_ variables.
# Writes <prefix>.log here and copies <prefix>.xcresult back. Returns
# xcodebuild's exit code; when the VM could not run the tests (no xctestrun,
# a lost SSH session, a result bundle that did not come back) records the
# failure, sets MG_MAC_VM_RUN_ERROR and returns 1. A run whose SSH session was
# lost may go on in the VM: it is stopped before this returns, or when it
# cannot be, MG_MAC_VM_LOST is set.
mg_mac_vm_xcodebuild_test() {
  local prefix="$1" dest="$2" config="$3" xctestrun n envs=() rc code dir base vm rcs probe out copy_error=""
  shift 3
  MG_MAC_VM_RUN_ERROR=""
  vm="$(jq -r .name <<<"$MG_MAC_VM")"
  dir="$(dirname "$prefix")"
  base="$(basename "$prefix")"
  xctestrun="$(mg__mac_vm_xctestrun "$config")"
  if [ -z "$xctestrun" ]; then
    echo "no $config ${MG_APP_TEST_PLAN:-UI}-plan xctestrun in $MG_APP_DD/Build/Products" >"$prefix.log"
    mg_failure mac-vm-xctestrun "no ${MG_APP_TEST_PLAN:-UI}-plan xctestrun for $config in $MG_APP_DD/Build/Products; build it first" \
      "tools/mg build-app --platform mac --config $config"
    MG_MAC_VM_RUN_ERROR=1
    return 1
  fi
  for n in $(compgen -e); do
    case "$n" in TEST_RUNNER_*) envs[${#envs[@]}]="$n=${!n}" ;; esac
  done
  echo "$MG_JOB: xcodebuild test-without-building in the VM $vm" >&2
  MG_MAC_VM_TESTING=1
  mg_mac_vm_ssh "{ mkdir -p $(mg_mac_vm_q "$dir") && rm -rf $(mg_mac_vm_q "$prefix.xcresult") &&
    env ${envs[0]+$(mg_mac_vm_q "${envs[@]}")} xcodebuild test-without-building -xctestrun $(mg_mac_vm_q "$xctestrun") \
    -destination $(mg_mac_vm_q "$dest") -resultBundlePath $(mg_mac_vm_q "$prefix.xcresult") -collect-test-diagnostics never \
    ${1+$(mg_mac_vm_q "$@")}; }; rc=\$?; echo \"$MG_MAC_VM_MARK \$rc\"; exit \$rc" >"$prefix.log" 2>&1
  rc=$?
  code="$(sed -n "s/^$MG_MAC_VM_MARK \([0-9][0-9]*\)\$/\1/p" "$prefix.log" | tail -n 1)"
  if [ -z "$code" ]; then
    mg_failure mac-vm-ssh "the SSH session to the VM $vm ended before xcodebuild did (exit $rc); see $prefix.log"
    MG_MAC_VM_RUN_ERROR=1
    # xcodebuild may still be running there: it must end before any other run
    # starts in the VM's one login session.
    echo "mac-vm: stopping the test run that lost its SSH session" >&2
    if mg__mac_vm_stop_tests; then
      echo "mac-vm: $([ -n "$MG_MAC_VM_STOPPED" ] && echo "stopped it" || echo "it had already ended")" >&2
    else
      MG_MAC_VM_LOST="the test run that lost its SSH session may still be running in the VM $vm ($MG_MAC_VM_STOP_REASON)"
      mg_failure mac-vm-stop "$MG_MAC_VM_LOST; this job runs nothing more there, and the next job that takes $vm stops it first"
    fi
  fi
  MG_MAC_VM_TESTING=""
  # The result bundle comes back into the run directory; the VM keeps no copy.
  # A run that reported its exit code needs it: without it the result is
  # unknown. After a lost session the copy is only for the record.
  rm -rf "$prefix.xcresult"
  out="$(mg_mac_vm_ssh "test -d $(mg_mac_vm_q "$prefix.xcresult")" 2>&1)"
  probe=$?
  case "$probe" in
    0)
      mg_mac_vm_ssh "tar -C $(mg_mac_vm_q "$dir") -cf - $(mg_mac_vm_q "$base.xcresult")" | tar -C "$dir" -xf -
      rcs="${PIPESTATUS[*]}"
      if [ "$rcs" != "0 0" ]; then
        copy_error="copying $prefix.xcresult back from the VM $vm failed (ssh and tar exit codes: $rcs)"
      elif [ ! -d "$prefix.xcresult" ]; then
        copy_error="copying $prefix.xcresult back from the VM $vm brought no result bundle back"
      fi
      mg_mac_vm_ssh "rm -rf $(mg_mac_vm_q "$prefix.xcresult")" >/dev/null 2>&1
      ;;
    1)
      # xcodebuild that failed before testing writes no bundle: the caller
      # reports it as a run that could not start.
      [ "$code" != 0 ] || copy_error="xcodebuild passed in the VM $vm but left no result bundle at $prefix.xcresult"
      ;;
    *) copy_error="could not check for $prefix.xcresult in the VM $vm (ssh exit $probe: $(tail -n 1 <<<"$out"))" ;;
  esac
  if [ -n "$copy_error" ] && [ -n "$code" ]; then
    mg_failure mac-vm-copy "$copy_error"
    MG_MAC_VM_RUN_ERROR=1
  fi
  [ -z "$MG_MAC_VM_RUN_ERROR" ] || return 1
  return "$code"
}

# mg__mac_vm_stop_tests: stops every UI test run in the leased VM (xcodebuild
# test-without-building and the test runner it drives: SIGTERM, then SIGKILL
# halfway through MEGURI_MAC_VM_STOP_SECONDS) and confirms none is left. Only
# the lease holder runs tests in the VM, so any such process is a run left
# behind, and two runs would share the VM's one login session. Returns 0 when
# none is left (MG_MAC_VM_STOPPED=1 when it stopped one); 1 when one still
# runs or the VM did not answer, with MG_MAC_VM_STOP_REASON.
mg__mac_vm_stop_tests() {
  local t="${MEGURI_MAC_VM_STOP_SECONDS:-20}" out rc
  MG_MAC_VM_STOPPED=""
  MG_MAC_VM_STOP_REASON=""
  # The brackets keep the shell that runs this command from matching itself.
  out="$(mg_mac_vm_ssh "p=$(mg_mac_vm_q 'xcodebuild[ ]test-without-building|UITests-Runner[.]app/Contents/MacOS/')
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
if pgrep -f \"\$p\" >/dev/null; then echo 'mg-mac-vm: still running'; else echo 'mg-mac-vm: none'; fi" 2>&1)"
  rc=$?
  if grep -qx 'mg-mac-vm: none' <<<"$out"; then
    ! grep -qx 'mg-mac-vm: stopping' <<<"$out" || MG_MAC_VM_STOPPED=1
    return 0
  fi
  if grep -qx 'mg-mac-vm: still running' <<<"$out"; then
    MG_MAC_VM_STOP_REASON="it was still running $t s after SIGTERM and SIGKILL"
  else
    MG_MAC_VM_STOP_REASON="the VM did not answer (ssh exit $rc: $(tail -n 1 <<<"$out"))"
  fi
  return 1
}

# mg__mac_vm_stop_orphans: when this job started a test run in the VM and
# did not see it end (it was cancelled), stops it before the lease goes to the
# next job. Runs at exit, before the lease is released. A run it cannot stop
# is stopped by the next job that takes the VM (mg_mac_vm_take).
mg__mac_vm_stop_orphans() {
  [ -n "$MG_MAC_VM_TESTING" ] || return 0
  MG_MAC_VM_TESTING=""
  echo "mac-vm: stopping the test run this job left in the VM" >&2
  mg__mac_vm_stop_tests ||
    echo "mac-vm: could not stop it ($MG_MAC_VM_STOP_REASON); the next job that takes the VM stops it first" >&2
}

# mg_mac_vm_log_show <since> <predicate>: the VM's unified log (`log show`
# needs sudo there to see testmanagerd and WindowServer).
mg_mac_vm_log_show() {
  mg_mac_vm_ssh "$(mg_mac_vm_q sudo log show --style compact --start "$1" --predicate "$2")"
}

# mg_mac_vm_now: the VM's local time, as `log show --start` takes it.
mg_mac_vm_now() {
  mg_mac_vm_ssh "date '+%Y-%m-%d %H:%M:%S'"
}
