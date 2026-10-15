# Jobs in the macOS test VM (U8b: Mac UI suites; U8c: simulators). Sourced
# by tools/mg.
#
# Every Mac XCUITest run (mac-ui, a11y --platform mac, drive --platform mac)
# and every simulator job (sim, sim-setup, run, drive, screenshot, ui-tree,
# record, a11y, snapshots, xcode-check) builds on the host and runs its tests
# and simulators in a macOS VM on this Mac (docs/process/mac-ui-vm.md,
# tools/vm/AGENTS.md), so it never takes over the host's screen, boots no
# simulator on the host, needs no host session and runs as often as needed.
# The VMs are named by a per-host config, never by the repository:
# $MEGURI_MAC_VM_CONFIG, else ~/Library/Application Support/meguri/mac-vm.json:
#
#   {"tart": "/Volumes/T9/meguri-vm/bin/tart",
#    "ssh": "/Volumes/T9/meguri-vm/bin/vm-ssh",
#    "vms": ["meguri-mac", {"name": "meguri-mac-2", "ssh": "<its wrapper>"}]}
#
# `ssh` is a wrapper that runs its arguments as a shell command in that VM;
# a VM entry may name its own (and its own tart). A Mac UI run takes the
# first VM whose lease is free (mac-vm.<name>.lock beside the host lease,
# shared by every checkout); a simulator job takes the lease of the pool VM,
# the first VM that can run, where the simulator pool lives. The job starts
# the VM headless when it is not running, waits for a logged-in session,
# copies this checkout's test products and app/Fixtures to the same absolute
# paths in the VM, runs xcodebuild test-without-building (and simctl) there
# and copies the result bundle and captures back. Without the config, the
# drive or the VM, the job is queued (warn, user step mac-vm); with
# MEGURI_MAC_UI_ALLOW_HOST=1 Mac suites take the host path (host lease and
# hourly gate, host.sh), with MEGURI_SIM_ALLOW_HOST=1 simulator jobs use this
# Mac's pool. A VM that does not start or answer is an error, never a reason
# to use the host's screen or simulators. The config, state, start and copy
# helpers are tools/scripts/mac-vm-lib.sh, shared with scripts that run
# outside tools/mg.
#
# Overrides (self-tests): MEGURI_MAC_VM_CONFIG, MEGURI_MAC_VM_POLL_SECONDS (3),
# MEGURI_MAC_VM_BOOT_SECONDS (240), MEGURI_MAC_VM_LEASE_WAIT_SECONDS (3600),
# MEGURI_MAC_VM_STOP_SECONDS (20).

# shellcheck source=tools/scripts/mac-vm-lib.sh
. "$MG_TOOLS/scripts/mac-vm-lib.sh"

MG_MAC_RUNNER=""
MG_MAC_VM=""
MG_MAC_VM_DETAIL='{}'
MG_MAC_VM_RUN_ERROR=""
MG_MAC_VM_TESTING=""
# Set (to the reason) when a test run that lost its SSH session could not be
# stopped in the VM: this job starts no further run there.
MG_MAC_VM_LOST=""
MG_MAC_VM_MARK="mg-mac-vm: remote exit"

mg_mac_vm_config_path() { mvl_config_path; }

# mg__mac_vm_read_config: sets MG_MAC_VM_ALL to the configured VMs, a JSON
# array of {name, tart, ssh}. Returns 1 without a config and 2 for one that
# is not valid, with MG_MAC_VM_REASON.
mg__mac_vm_read_config() {
  local rc
  mvl_read_config
  rc=$?
  MG_MAC_VM_ALL="$MVL_ALL"
  MG_MAC_VM_REASON="${MVL_REASON:-}"
  return "$rc"
}

# mg__mac_vm_state <tart> <name>: running, stopped (or tart's other state),
# or missing; nothing when tart cannot list its VMs.
mg__mac_vm_state() { mvl_state "$@"; }

# mg_mac_vm_probe: the configured VMs that can run now: their wrappers are
# there (the drive that holds them is connected) and tart lists them. Sets
# MG_MAC_VMS (a JSON array of {name, tart, ssh, state}) and returns 0, or
# sets MG_MAC_VM_REASON and returns 1 (none can) or 2 (the config is not
# valid). Reads only.
mg_mac_vm_probe() {
  local rc
  MVL_REASON=""
  mvl_probe
  rc=$?
  MG_MAC_VMS="${MVL_VMS:-[]}"
  MG_MAC_VM_REASON="$MVL_REASON"
  return "$rc"
}

# mg_mac_vm_user_step: the one-time step behind a missing VM.
mg_mac_vm_user_step() {
  mg_user_step mac-vm missing true "Connect the drive with the macOS test VM" \
    "Simulators and Mac UI suites run only in the macOS test VM, never on this Mac: connect the T9 drive, or set the VM up and name it in $(mg_mac_vm_config_path) (docs/process/mac-ui-vm.md). Until then they are queued."
}

# mg__mac_vm_detail <JSON object>: merged into details.mac_vm. `started`
# stays true once a step of the job started the VM.
mg__mac_vm_detail() {
  MG_MAC_VM_DETAIL="$(jq -c --argjson d "$1" '(.started // false) as $s | . + $d
    | if has("started") then .started = ($s or .started) else . end' <<<"$MG_MAC_VM_DETAIL")"
  mg_detail mac_vm "$MG_MAC_VM_DETAIL"
}

# mg__mac_vm_fail <id> <reason>: the VM could not run the job: an error,
# never fail, and never a run on the host instead.
mg__mac_vm_fail() {
  mg_failure "$1" "$2"
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg_message "$MG_JOB: the test VM could not run the simulators: $2"
  else
    mg_message "$MG_JOB: the test VM could not run the suites: $2"
  fi
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
      mg__mac_vm_config_error
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
  mg__mac_vm_queued "Mac UI suites run only in the test VM (docs/process/mac-ui-vm.md), never on this Mac's screen"
  return 1
}

# mg__mac_vm_config_error: a config that is not valid (error).
mg__mac_vm_config_error() {
  mg_failure mac-vm-config "$MG_MAC_VM_REASON" "tools/mg doctor"
  mg_message "$MG_JOB: $MG_MAC_VM_REASON"
  mg_status error
}

# mg__mac_vm_queued <what runs only in the VM>: no VM can run the job now:
# queued (warn), with the mac-vm user step.
mg__mac_vm_queued() {
  mg_detail queued true
  mg_detail mac_vm "$(jq -nc --arg r "$MG_MAC_VM_REASON" --arg c "$(mg_mac_vm_config_path)" '{reason: $r, config: $c}')"
  mg_mac_vm_user_step
  mg_message "$MG_JOB: queued: $MG_MAC_VM_REASON; $1"
  mg_status warn
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

# mg_mac_vm_take [--no-copy]: a VM lease, the VM running with a logged-in
# session and no test run left in it, and (unless --no-copy) this checkout's
# test products and fixtures in it. Sets MG_MAC_VM.
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
  [ "${1:-}" != --no-copy ] || return 0
  if ! mg_mac_vm_copy_in "$MG_ROOT/$MG_APP_DD/Build/Products" "$MG_ROOT/app/Fixtures"; then
    mg_message "$MG_JOB: the test VM could not run the $([ "$MG_SIM_RUNNER" = vm ] && echo simulators || echo suites): the test products could not be copied into it"
    mg_status error
    return 1
  fi
  return 0
}

# ---------------------------------------------------------------------------
# Simulators in the VM (U8c)
#
# The pool's simulators live in the first configured VM that can run (the
# pool VM): tools/scripts/sim-pool.sh runs there over SSH by itself. Jobs
# that use a simulator call mg_sim_begin before they build and mg_sim_take
# after, then reach the simulators through mg_simctl and mg_devicectl, and
# test with mg_vm_xcodebuild_test (mg_app_xcodebuild_test).

MG_SIM_RUNNER=""

# mg_sim_begin: where this job's simulators are, decided before it builds:
# the pool VM (MG_SIM_RUNNER=vm; MG_MAC_VM names it, started headless when
# it was not running, and the job's VM lease will be that VM's); without one,
# this Mac's simulators with MEGURI_SIM_ALLOW_HOST=1 (host); otherwise the job
# is recorded as queued (warn) and this returns 2. A config that is not
# valid, or a VM that does not start or answer, is an error (returns 1),
# never a reason to boot a simulator on this Mac.
mg_sim_begin() {
  local rc
  MG_SIM_RUNNER=""
  mg_mac_vm_probe
  rc=$?
  case "$rc" in
    0)
      MG_SIM_RUNNER=vm
      MG_MAC_VMS="$(jq -c '[.[0]]' <<<"$MG_MAC_VMS")"
      MG_MAC_VM="$(jq -c '.[0]' <<<"$MG_MAC_VMS")"
      mg_detail sim_runner '"vm"'
      mg__mac_vm_detail "$(jq -c '{name}' <<<"$MG_MAC_VM")"
      mg__mac_vm_boot || return 1
      return 0
      ;;
    2)
      mg__mac_vm_config_error
      return 1
      ;;
  esac
  if [ "${MEGURI_SIM_ALLOW_HOST:-}" = 1 ]; then
    echo "$MG_JOB: no test VM ($MG_MAC_VM_REASON); MEGURI_SIM_ALLOW_HOST=1: using this Mac's simulators" >&2
    MG_SIM_RUNNER=host
    mg_detail sim_runner '"host"'
    return 0
  fi
  mg__mac_vm_queued "simulators run only in the test VM (docs/process/mac-ui-vm.md), never on this Mac"
  return 2
}

# mg_sim_take [--no-copy]: after the build, the lease a simulator job holds
# while it uses its device: the pool VM's lease (mg_mac_vm_take, which also
# copies the products in unless --no-copy), or this Mac's host lease.
mg_sim_take() {
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg_mac_vm_take "$@"
  else
    mg_host_lease
  fi
}

# mg_simctl <args>: `xcrun simctl` where the job's simulators are.
mg_simctl() {
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg_mac_vm_ssh "xcrun simctl $(mvl_q "$@")"
  else
    xcrun simctl "$@"
  fi
}

# mg_devicectl <args>: `xcrun devicectl` where the job's simulators are.
mg_devicectl() {
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg_mac_vm_ssh "xcrun devicectl $(mvl_q "$@")"
  else
    xcrun devicectl "$@"
  fi
}

# mg_sim_mkdir <absolute dir>: makes a directory where the simulators write
# (in the VM, at the same path), for captures that come back afterwards.
mg_sim_mkdir() {
  mkdir -p "$1" || return 1
  [ "$MG_SIM_RUNNER" != vm ] || mg_mac_vm_ssh "mkdir -p $(mvl_q "$1")"
}

# mg_sim_fetch <absolute path>...: in the VM, copies captures the simulators
# wrote there back to the same paths here and removes them from the VM;
# nothing to do on this Mac. Records a failure (mac-vm-copy) and returns 1
# when one does not come back.
mg_sim_fetch() {
  [ "$MG_SIM_RUNNER" = vm ] || return 0
  mg_mac_vm_copy_out "$@" || return 1
  mg_mac_vm_ssh "rm -rf $(mvl_q "$@")" >/dev/null 2>&1
  return 0
}

# ---------------------------------------------------------------------------
# VM lease: one job at a time per VM, across every checkout. A recording of
# this checkout keeps the lease between jobs (record start); this checkout's
# jobs then run in its nested slot (mg__lease_try, host.sh).

mg__mac_vm_lease_dir() { printf '%s/mac-vm.%s.lock\n' "$(dirname "$(mg_host_lease_dir)")" "$1"; }

# mg__mac_vm_lease: takes the first free VM of MG_MAC_VMS, waiting while all
# are leased; a lease whose holder died is removed (under the lease's mutex,
# tools/scripts/host-lease-mutex.sh, on the VM lease's own mutex file). Sets
# MG_LEASE_DIR. Released when the job ends.
mg__mac_vm_lease() {
  local wait="${MEGURI_MAC_VM_LEASE_WAIT_SECONDS:-3600}" poll="${MEGURI_MAC_VM_POLL_SECONDS:-3}"
  local start n i vm name dir holders announced=""
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
      mg__lease_try "$dir" "VM lease"
      case $? in
        0)
          MG_LEASE_DIR="$dir"
          mg_on_exit "mg__host_lease_release '$dir' $$"
          MG_MAC_VM="$vm"
          mg__mac_vm_detail "$(jq -c --arg n "$name" --argjson w "$(($(mg__epoch) - start))" '{name: $n, lease: (. + {waited_s: $w})}' "$MG_LEASE_SLOT/holder.json")"
          [ -z "$announced" ] || echo "VM lease: took $name after $(($(mg__epoch) - start)) s" >&2
          return 0
          ;;
        2) continue ;;
        3) mg_sleep "$poll" && continue ;;
      esac
      holders="$holders; $name: $(jq -r '"\(.job) in \(.worktree) since \(.started_at)"' <<<"$MG_LEASE_HOLDER" 2>/dev/null || echo "another job")"
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

# ---------------------------------------------------------------------------
# Starting the VM

# mg__mac_vm_boot: starts the VM MG_MAC_VM names headless when it is not
# running (mvl_start: in a session of its own, so it outlives this job; jobs
# never stop it), then waits until SSH answers in a logged-in session. On
# failure records an error and returns 1.
mg__mac_vm_boot() {
  local name rc=0
  name="$(jq -r .name <<<"$MG_MAC_VM")"
  mvl_ensure_ready "$MG_MAC_VM" "$MG_RUN_DIR" || rc=1
  [ -z "$MVL_TART_LOG" ] || mg_artifact log "$MVL_TART_LOG" "tart run $name"
  mg__mac_vm_detail "$(jq -nc --argjson s "$MVL_STARTED" '{started: $s}')"
  [ "$rc" -eq 0 ] && return 0
  mg__mac_vm_fail "mac-vm-$MVL_FAIL" "$MVL_REASON"
  return 1
}

# ---------------------------------------------------------------------------
# Commands and files in the leased VM

# mg_mac_vm_ssh <command>: runs a shell command in the leased VM (its login
# shell is zsh; quote words with mg_mac_vm_q), stdin from /dev/null.
# mg_mac_vm_ssh_in keeps stdin.
mg_mac_vm_ssh() { "$(jq -r .ssh <<<"$MG_MAC_VM")" "$1" </dev/null; }
mg_mac_vm_ssh_in() { "$(jq -r .ssh <<<"$MG_MAC_VM")" "$1"; }

# mg_mac_vm_q <word>...: the words, each single-quoted for the VM's shell.
mg_mac_vm_q() { mvl_q "$@"; }

# mg_mac_vm_copy_in <absolute path>...: copies files and directories to the
# same absolute paths in the leased VM (tar over SSH), replacing what is
# there; paths that do not exist here are skipped. The UI tests read the app
# bundle and fixtures at their build paths. Only this checkout's directories
# are replaced. Records a failure (mac-vm-copy) and returns 1 when a copy
# fails.
mg_mac_vm_copy_in() {
  mvl_copy_in "$(jq -r .ssh <<<"$MG_MAC_VM")" "$(jq -r .name <<<"$MG_MAC_VM")" "$MG_ROOT" "$@" && return 0
  mg_failure mac-vm-copy "$MVL_REASON"
  return 1
}

# mg_mac_vm_copy_out <absolute path>...: copies files and directories back
# from the same absolute paths in the leased VM, replacing what is here.
# Records a failure (mac-vm-copy) and returns 1 when one does not come back.
mg_mac_vm_copy_out() {
  mvl_copy_out "$(jq -r .ssh <<<"$MG_MAC_VM")" "$(jq -r .name <<<"$MG_MAC_VM")" "$@" && return 0
  mg_failure mac-vm-copy "$MVL_REASON"
  return 1
}

# mg__vm_xctestrun <plan> <sdk> <config>: this checkout's xctestrun of the
# test plan built for the SDK (macosx, iphonesimulator) and configuration
# (the newest, when several SDK versions left one). Debug and Profile builds
# write the same file name; its test root says which.
mg__vm_xctestrun() {
  local f root="$3"
  [ "$2" = macosx ] || root="$3-$2"
  # shellcheck disable=SC2012 # newest first; the names are Xcode's
  ls -t "$MG_ROOT/$MG_APP_DD/Build/Products/Meguri_$1_$2"*.xctestrun 2>/dev/null | while IFS= read -r f; do
    if grep -q "__TESTROOT__/$root/" "$f"; then
      printf '%s\n' "$f"
      break
    fi
  done
}

# mg_mac_vm_xcodebuild_test <prefix> <destination> <config> [xcodebuild args]:
# the Mac UI plan's tests in the leased VM (mg_vm_xcodebuild_test).
mg_mac_vm_xcodebuild_test() {
  local prefix="$1"
  shift
  mg_vm_xcodebuild_test "$prefix" UI macosx "$@"
}

# mg_vm_xcodebuild_test <prefix> <plan> <sdk> <destination> <config> [xcodebuild args]:
# a test plan's tests in the leased VM, from the products copied there
# (test-without-building -xctestrun), with this job's TEST_RUNNER_ variables:
# the Mac's (sdk macosx) or a simulator's in the VM (iphonesimulator,
# destination id=<udid>). Writes <prefix>.log here and copies
# <prefix>.xcresult back. Returns xcodebuild's exit code; when the VM could
# not run the tests (no xctestrun, a lost SSH session, a result bundle that
# did not come back) records the failure, sets MG_MAC_VM_RUN_ERROR and
# returns 1. A run whose SSH session was lost may go on in the VM: it is
# stopped before this returns, or when it cannot be, MG_MAC_VM_LOST is set.
mg_vm_xcodebuild_test() {
  local prefix="$1" plan="$2" sdk="$3" dest="$4" config="$5" xctestrun n envs=() rc code dir base vm rcs probe out copy_error=""
  shift 5
  MG_MAC_VM_RUN_ERROR=""
  vm="$(jq -r .name <<<"$MG_MAC_VM")"
  dir="$(dirname "$prefix")"
  base="$(basename "$prefix")"
  xctestrun="$(mg__vm_xctestrun "$plan" "$sdk" "$config")"
  if [ -z "$xctestrun" ]; then
    echo "no $config $plan-plan xctestrun for $sdk in $MG_APP_DD/Build/Products" >"$prefix.log"
    mg_failure mac-vm-xctestrun "no $plan-plan xctestrun for $config ($sdk) in $MG_APP_DD/Build/Products; build it first" \
      "tools/mg build-app --platform $([ "$sdk" = macosx ] && echo mac || echo ios-sim) --config $config"
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
