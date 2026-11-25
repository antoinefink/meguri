# Jobs in the macOS test VM (U8b: Mac UI suites; U8c: simulators). Sourced
# by tools/mg.
#
# Every Mac XCUITest run (mac-ui, a11y --platform mac, drive --platform mac)
# and every simulator job (sim, sim-setup, run, drive, screenshot, ui-tree,
# record, a11y, snapshots, xcode-check, perf-ui, metrics) builds on the host
# and runs its tests and simulators in a macOS VM on this Mac
# (docs/process/mac-ui-vm.md, tools/vm/AGENTS.md), so it never takes over the
# host's screen, boots no simulator on the host, needs no host session and
# runs as often as needed.
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
# Mac's pool. A VM that cannot be reached is queued too (U19c, mac-vm, with
# the reason): no logged-in session over SSH in time, an SSH call that
# fails as ssh (255) or whose command could not start there (exec request
# refused, no fork: mvl_unreachable), or too few free processes there after a
# bounded wait (mvl_wait_procs, checked once the job holds the VM's lease and
# before it copies or runs anything). A VM that tart cannot start, a run that
# cannot be stopped, a copy or a result the VM answered wrongly stay errors.
# None is ever a reason to use the host's screen or simulators. The config,
# state, start, copy, reachability and process helpers are
# tools/scripts/mac-vm-lib.sh, shared with scripts that run outside tools/mg.
#
# Overrides (self-tests): MEGURI_MAC_VM_CONFIG, MEGURI_MAC_VM_POLL_SECONDS (3),
# MEGURI_MAC_VM_BOOT_SECONDS (240), MEGURI_MAC_VM_LEASE_WAIT_SECONDS (3600),
# MEGURI_MAC_VM_STOP_SECONDS (20), MEGURI_MAC_VM_MIN_FREE_PROCS (400),
# MEGURI_MAC_VM_PROCS_WAIT_SECONDS (600), MEGURI_MAC_VM_PROCS_POLL_SECONDS (10).

# shellcheck source=tools/scripts/mac-vm-lib.sh
. "$MG_TOOLS/scripts/mac-vm-lib.sh"

MG_MAC_RUNNER=""
MG_MAC_VM=""
MG_MAC_VM_DETAIL='{}'
MG_MAC_VM_RUN_ERROR=""
MG_MAC_VM_TESTING=""
# Set (to the reason) when a test run that lost its SSH session could not be
# stopped in the VM: this job starts no further run there. _UNREACHABLE: it
# could not be stopped because the VM did not answer (the runs left are
# queued, not errors).
MG_MAC_VM_LOST=""
MG_MAC_VM_LOST_UNREACHABLE=""
# Set (to the reason) once the VM could not be reached for this job
# (mg__mac_vm_unreachable); MG_MAC_VM_RUN_QUEUED: the last remote test run.
MG_MAC_VM_QUEUED=""
MG_MAC_VM_RUN_QUEUED=""
MG_MAC_VM_STEP=""
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
  mg_user_step mac-vm missing true "Make the macOS test VM available" \
    "Simulators and Mac UI suites run only in the macOS test VM, never on this Mac: make its local or external storage available and name the VM in $(mg_mac_vm_config_path) (docs/process/mac-ui-vm.md). Until then they are queued."
}

# mg__mac_vm_unavailable_step <reason>: the step behind a VM that is there
# but cannot be reached (U19c).
mg__mac_vm_unavailable_step() {
  mg_user_step mac-vm missing true "Let the macOS test VM answer again" \
    "The test VM is there but cannot take jobs now: $1. Simulator jobs and Mac UI suites are queued until it answers over SSH in a logged-in session with enough free processes; tools/mg catch-up runs them again once tools/mg doctor sees that. It usually recovers once its load drops; if it does not, look at it (docs/process/mac-ui-vm.md). Jobs never stop or restart it."
}

# mg__mac_vm_unreachable <reason> [part]: the VM cannot take this job's work
# now (U19c): no logged-in session over SSH in time, an SSH call that did not
# reach it or could not start its command (mvl_unreachable), or too few free
# processes after the wait (mvl_wait_procs). <part> (default: the job's
# simulator runs or Mac UI suites) is queued with the mac-vm step and the
# reason, never an error; the run is queued unless a part that ran failed or
# erred (mg_queued_settle). Sets MG_MAC_VM_QUEUED and MG_MAC_VM_REASON.
mg__mac_vm_unreachable() {
  local part="${2:-${MG_VM_PART:-}}"
  if [ -z "$part" ]; then
    if [ "$MG_SIM_RUNNER" = vm ]; then part="the simulator runs"; else part="the Mac UI suites"; fi
  fi
  MG_MAC_VM_QUEUED="$1"
  MG_MAC_VM_REASON="$1"
  echo "$MG_JOB: the test VM cannot be reached: $1; $part queued (user step mac-vm)" >&2
  mg__mac_vm_detail "$(jq -nc --arg r "$1" '{reason: $r}')"
  if [ -z "$MG_MAC_VM_STEP" ]; then
    mg__mac_vm_unavailable_step "$1"
    MG_MAC_VM_STEP=1
  fi
  mg_queued_part mac-vm "$part" "$1"
  case "$(cat "$MG_STATE/status" 2>/dev/null)" in
    fail | error) ;;
    *) mg_message "$MG_JOB: queued: $1; $part did not run (user step mac-vm); tools/mg catch-up runs them again once the VM answers" ;;
  esac
  mg_queued_settle
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
  mg__mac_vm_queued "Mac UI suites run only in the test VM (docs/process/mac-ui-vm.md), never on this Mac's screen" "the Mac UI suites"
  return 1
}

# mg__mac_vm_config_error: a config that is not valid (error).
mg__mac_vm_config_error() {
  mg_failure mac-vm-config "$MG_MAC_VM_REASON" "tools/mg doctor"
  mg_message "$MG_JOB: $MG_MAC_VM_REASON"
  mg_status error
}

# mg__mac_vm_queued <what runs only in the VM> <part>: no VM can run the job
# now: <part> is queued (mg_queued_part), and so is the run (warn), with the
# mac-vm user step.
mg__mac_vm_queued() {
  mg_detail queued true
  mg_detail mac_vm "$(jq -nc --arg r "$MG_MAC_VM_REASON" --arg c "$(mg_mac_vm_config_path)" '{reason: $r, config: $c}')"
  mg_mac_vm_user_step
  mg_queued_part mac-vm "$2" "$MG_MAC_VM_REASON"
  mg_message "$MG_JOB: queued: $MG_MAC_VM_REASON; $1"
  mg_status warn
}

# mg_mac_ui_take: after the build, what the runner needs: the VM's lease,
# start and copy (mg_mac_vm_take), or on the host its lease, the .dev app
# guard and the hourly gate. Records the result and returns 1 when the job
# cannot go on (2: the VM could not be reached, the suites are queued).
mg_mac_ui_take() {
  if [ "$MG_MAC_RUNNER" = vm ]; then
    mg_mac_vm_take
  else
    mg_host_lease && mg_app_mac_guard && mg_mac_ui_gate
  fi
}

# mg_mac_vm_take [--no-copy] [--running-only]: a VM lease, the VM running with a logged-in
# session, no test run left in it and enough free processes (mvl_wait_procs:
# it waits for them), and (unless --no-copy) this checkout's test products
# and fixtures in it. Sets MG_MAC_VM. Returns 1 when the job cannot go on
# (recorded), 2 when the VM could not be reached (queued,
# mg__mac_vm_unreachable): the lease is then released at once, since the job
# runs nothing more there.
# --running-only queues a stopped VM without starting it (delivery-only jobs).
mg_mac_vm_take() {
  local name rc no_copy="" running_only="" option
  for option in ${1+"$@"}; do
    case "$option" in
      --no-copy) no_copy=1 ;;
      --running-only) running_only=1 ;;
      *) return 1 ;;
    esac
  done
  mg__mac_vm_lease || return 1
  mg_on_exit mg__mac_vm_stop_orphans
  if [ -n "$running_only" ]; then
    # Delivery must never start/restart a VM. Recheck under its lease and
    # use the existing read-only readiness wait, without ensure_ready/start.
    name="$(jq -r .name <<<"$MG_MAC_VM")"
    if [ "$(mvl_state "$(jq -r .tart <<<"$MG_MAC_VM")" "$name")" != running ]; then
      mg__mac_vm_unreachable "selected VM is not running"
      mg__mac_vm_release_queued 2
      return 2
    fi
    mvl_wait_ready "$(jq -r .ssh <<<"$MG_MAC_VM")" "$(jq -r .tart <<<"$MG_MAC_VM")" \
      "$name" false "" "$MG_RUN_DIR/mac-vm-ssh.err"
    rc=$?
    if [ "$rc" -ne 0 ]; then
      mg__mac_vm_unreachable "selected VM is not ready"
      rc=2
    fi
  else
    mg__mac_vm_boot
    rc=$?
  fi
  [ "$rc" -eq 0 ] || { mg__mac_vm_release_queued "$rc"; return "$rc"; }
  # Another Mac may schedule jobs in the same VM: its lease is not this
  # Mac's, so nothing is stopped or started there before the claim is ours.
  mg__mac_vm_claim
  rc=$?
  [ "$rc" -eq 0 ] || { mg__mac_vm_release_queued "$rc"; return "$rc"; }
  # A run that an earlier job could not stop (its SSH session was lost, or the
  # job was killed) ends before this job's run starts: until then the VM gets
  # no new run.
  name="$(jq -r .name <<<"$MG_MAC_VM")"
  if ! mg__mac_vm_stop_tests; then
    if [ -n "$MG_MAC_VM_STOP_UNREACHABLE" ]; then
      mg__mac_vm_unreachable "$name did not answer when this job looked for a UI test run an earlier job left there: $MG_MAC_VM_STOP_REASON"
      mg__mac_vm_release_queued 2
      return 2
    fi
    mg__mac_vm_fail mac-vm-stop "a UI test run that an earlier job left in $name could not be stopped: $MG_MAC_VM_STOP_REASON; no new run starts there until it is"
    return 1
  fi
  if [ -n "$MG_MAC_VM_STOPPED" ]; then
    echo "mac-vm: stopped a UI test run that an earlier job left in $name" >&2
    mg__mac_vm_detail '{"stopped_left_run": true}'
  fi
  if ! mg__mac_vm_procs; then
    mg__mac_vm_release_queued 2
    return 2
  fi
  [ -z "$no_copy" ] || return 0
  mg_mac_vm_copy_in "$MG_ROOT/$MG_APP_DD/Build/Products" "$MG_ROOT/app/Fixtures"
  rc=$?
  case "$rc" in
    0) return 0 ;;
    2)
      mg__mac_vm_release_queued 2
      return 2
      ;;
  esac
  mg_message "$MG_JOB: the test VM could not run the $([ "$MG_SIM_RUNNER" = vm ] && echo simulators || echo suites): the test products could not be copied into it"
  mg_status error
  return 1
}

# mg__mac_vm_release_queued <code>: for code 2 (the VM could not be reached,
# the job's VM part is queued), gives the VM's lease back at once: the job
# runs nothing more there, and others need not wait for its other parts.
mg__mac_vm_release_queued() {
  [ "$1" -eq 2 ] && [ -n "${MG_LEASE_DIR:-}" ] || return 0
  mg__host_lease_release "$MG_LEASE_DIR" $$
}

# mg__mac_vm_procs: waits while the leased VM has too few free processes for
# a job, or does not answer the check (mvl_wait_procs, bounded), and records
# the last check in details.mac_vm.procs. Returns 1 when it stayed so: queued
# (mg__mac_vm_unreachable).
mg__mac_vm_procs() {
  local rc=0
  mvl_wait_procs "$(jq -r .ssh <<<"$MG_MAC_VM")" "$(jq -r .name <<<"$MG_MAC_VM")" || rc=1
  mg__mac_vm_detail "$(jq -nc --argjson p "${MVL_PROCS:-null}" --argjson w "${MVL_PROCS_WAITED:-0}" \
    --argjson m "${MEGURI_MAC_VM_MIN_FREE_PROCS:-400}" '{procs: (($p // {}) + {needed: $m, waited_s: $w})}')"
  [ "$rc" -eq 0 ] && return 0
  mg__mac_vm_unreachable "$MVL_REASON"
  return 1
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
# is recorded as queued (warn) and this returns 2, as when the VM does not
# answer (mg__mac_vm_unreachable; MG_SIM_RUNNER is then empty: nothing more
# goes to the VM). A config that is not valid, or a VM that does not start,
# is an error (returns 1), never a reason to boot a simulator on this Mac.
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
      mg__mac_vm_boot
      rc=$?
      [ "$rc" -ne 2 ] || MG_SIM_RUNNER=""
      return "$rc"
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
  mg__mac_vm_queued "simulators run only in the test VM (docs/process/mac-ui-vm.md), never on this Mac" "${MG_VM_PART:-the simulator runs}"
  return 2
}

# mg_sim_take [--no-copy]: after the build, the lease a simulator job holds
# while it uses its device: the pool VM's lease (mg_mac_vm_take, which also
# copies the products in unless --no-copy; 2: the VM could not be reached,
# the job's simulator runs are queued), or this Mac's host lease.
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
# when one does not come back; 2 when the VM could not be reached (queued).
mg_sim_fetch() {
  [ "$MG_SIM_RUNNER" = vm ] || return 0
  mg_mac_vm_copy_out "$@" || return
  mg_mac_vm_ssh "rm -rf $(mvl_q "$@")" >/dev/null 2>&1
  return 0
}

# mg_sim_pool_take: before the pool's own changes (sim acquire and gc,
# sim-setup), the pool VM's lease (mg_sim_take --no-copy), so that no
# simulator is created, booted or deleted in the VM while another job runs
# there. Nothing to take when this process runs under the job that holds it
# (mvl_lease_held), nor for this Mac's pool (MEGURI_SIM_ALLOW_HOST=1, as
# before U8c). Returns 1 when the job cannot go on (recorded).
mg_sim_pool_take() {
  [ "$MG_SIM_RUNNER" = vm ] || return 0
  if mvl_lease_held "$(mg__mac_vm_lease_dir "$(jq -r .name <<<"$MG_MAC_VM")")"; then
    echo "$MG_JOB: runs under the job that holds the test VM's lease" >&2
    return 0
  fi
  mg_sim_take --no-copy
}

# mg_ui_suite_fixtures <suite>...: the generated recipe fixtures
# (build/fixtures/recipes/<name>) that the selected MeguriUITests classes
# launch with, read from the source files that declare those classes, into
# MG_UI_SUITE_FIXTURES (absolute paths, one per line). Call it before
# building. A fixture this checkout has not generated (a new worktree) is
# made with the nested fixtures job; when that fails it records a failure
# naming the command and returns 1. owned-sync-review-1 is left out: the
# owned journey prepares and stages it itself.
mg_ui_suite_fixtures() {
  local s class files names name recipe seed missing=""
  MG_UI_SUITE_FIXTURES=""
  names=""
  for s in "$@"; do
    class="${s%%/*}"
    files="$(grep -rlE "(class|struct|extension) $class([^A-Za-z0-9_]|$)" "$MG_ROOT/app/MeguriUITests" 2>/dev/null)" || continue
    while IFS= read -r name; do
      [ -n "$name" ] || continue
      names="$names $name"
    done < <(printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 grep -ohE 'build/fixtures/recipes/[A-Za-z0-9._-]+' 2>/dev/null |
      sed 's|^build/fixtures/recipes/||' | sort -u)
  done
  for name in $(printf '%s\n' $names | sort -u); do
    [ "$name" != owned-sync-review-1 ] || continue
    recipe="${name%-*}"; seed="${name##*-}"
    if [ ! -d "$MG_ROOT/build/fixtures/recipes/$name" ]; then
      echo "$MG_JOB: generating the suites' fixture $name: tools/mg fixtures $recipe --seed $seed" >&2
      "$MG_TOOLS/mg" fixtures "$recipe" --seed "$seed" >&2 </dev/null || true
    fi
    if [ -d "$MG_ROOT/build/fixtures/recipes/$name" ]; then
      MG_UI_SUITE_FIXTURES="$MG_UI_SUITE_FIXTURES$MG_ROOT/build/fixtures/recipes/$name"$'\n'
      continue
    fi
    missing="$missing; tools/mg fixtures $recipe --seed $seed"
  done
  [ -n "$missing" ] || return 0
  mg_failure fixture "the selected suites launch with recipe fixtures that could not be generated in this checkout; nothing was built or launched; see: ${missing#; }"
  mg_message "$MG_JOB: the suites' fixtures are missing and could not be generated: ${missing#; }"
  return 1
}

# mg_ui_suite_fixtures_stage <runner>: puts MG_UI_SUITE_FIXTURES at their own
# paths in the leased VM (runner vm; nothing to do on this Mac). Returns as
# mg_vm_fixture does: 1 recorded, 2 queued.
mg_ui_suite_fixtures_stage() {
  local path rc
  [ "$1" = vm ] || return 0
  while IFS= read -r path; do
    [ -n "$path" ] || continue
    mg_vm_fixture "$path"
    rc=$?
    [ "$rc" -eq 0 ] || return "$rc"
  done <<<"$MG_UI_SUITE_FIXTURES"
}

# mg_vm_fixture <path>: puts a launch fixture (a -MeguriFixture value, or a
# drive script's launch.fixture) where the app in the leased VM reads it,
# and sets MG_VM_FIXTURE to the path the launch must name there: a fixture in
# this checkout goes to its own path; one outside it goes under the run
# directory (vm-fixtures/<n>/, left there: an app launched by `run` may read
# it after the job ends), because the VM's paths outside a checkout are not
# this Mac's. A value that is not absolute stays as it is (the app
# refuses it, as on this Mac). Records a failure and returns 1 when the
# fixture is not here or cannot be copied; 2 when the VM could not be reached
# (queued).
mg_vm_fixture() {
  local dir
  MG_VM_FIXTURE="$1"
  case "$1" in /*) ;; *) return 0 ;; esac
  if [ ! -e "$1" ]; then
    mg_failure fixture "the launch fixture $1 does not exist; nothing was launched with it"
    return 1
  fi
  case "$1" in
    "$MG_ROOT"/?*)
      mg_mac_vm_copy_in "$1"
      return
      ;;
  esac
  MG_VM_FIXTURES=$((${MG_VM_FIXTURES:-0} + 1))
  dir="$MG_RUN_DIR/vm-fixtures/$MG_VM_FIXTURES"
  if ! mvl_copy_to "$(jq -r .ssh <<<"$MG_MAC_VM")" "$(jq -r .name <<<"$MG_MAC_VM")" "$1" "$dir"; then
    if [ -n "$MVL_UNREACHABLE" ]; then
      mg__mac_vm_unreachable "$MVL_REASON"
      return 2
    fi
    mg_failure mac-vm-copy "$MVL_REASON"
    return 1
  fi
  MG_VM_FIXTURE="$dir/$(basename "$1")"
  echo "$MG_JOB: the fixture $1 is $MG_VM_FIXTURE in the test VM" >&2
}

# mg_vm_launch_args <launch argument>...: the app's launch arguments for the
# leased VM, in MG_VM_ARGS: each -MeguriFixture value put there and named by
# its path there (mg_vm_fixture). Read in pairs as the app reads them
# (LaunchOptions.swift): a -Meguri<Name> argument takes the next one as its
# value. Returns 1 when a fixture could not be put there (recorded), 2 when
# the VM could not be reached (queued).
mg_vm_launch_args() {
  MG_VM_ARGS=()
  while [ "$#" -gt 0 ]; do
    MG_VM_ARGS[${#MG_VM_ARGS[@]}]="$1"
    case "$1" in
      -Meguri?*)
        [ "$#" -ge 2 ] || break
        if [ "$1" = -MeguriFixture ]; then
          mg_vm_fixture "$2" || return
          MG_VM_ARGS[${#MG_VM_ARGS[@]}]="$MG_VM_FIXTURE"
        else
          MG_VM_ARGS[${#MG_VM_ARGS[@]}]="$2"
        fi
        shift
        ;;
    esac
    shift
  done
}

# ---------------------------------------------------------------------------
# VM lease: one job at a time per VM, across every checkout. A recording of
# this checkout keeps the lease between jobs (record start); this checkout's
# jobs then run in its nested slot (mg__lease_try, host.sh).

mg__mac_vm_lease_dir() { mvl_lease_dir "$MG_ROOT" "$1"; }

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
# VM claim: one Mac at a time per VM. The lease above lives on the Mac that
# runs the job, so two Macs that reach one VM (the Mac that hosts it, and a
# coordinator over SSH) each see only their own. The claim is a lock inside
# the VM (tools/scripts/vm-claim.py): a keeper started here holds it for as
# long as a live job holds this Mac's lease, across hand-overs to a recording
# or a nested job. MEGURI_MAC_VM_SHARED_CLAIM=0 turns it off.

# mg__mac_vm_claim: with the lease held and the VM answering, takes the
# VM's claim, waiting while another Mac holds it. A keeper of this Mac that
# still holds it (the lease changed hands between two of its jobs) is kept.
# Returns 0; 2 when the VM did not answer (queued); 1 when the claim stayed
# with another Mac for MEGURI_MAC_VM_LEASE_WAIT_SECONDS, or could not be
# taken (recorded).
mg__mac_vm_claim() {
  [ "${MEGURI_MAC_VM_SHARED_CLAIM:-1}" != 0 ] || return 0
  local wait="${MEGURI_MAC_VM_LEASE_WAIT_SECONDS:-3600}" poll="${MEGURI_MAC_VM_CLAIM_POLL_SECONDS:-5}"
  local beat="${MEGURI_MAC_VM_CLAIM_BEAT_SECONDS:-5}" path="${MEGURI_MAC_VM_CLAIM_PATH:-/tmp/meguri-vm-claim.lock}"
  local state="$MG_LEASE_DIR.claim" log="$MG_RUN_DIR/mac-vm-claim.log" name ssh start holder token
  local record st pid n=0 i announced="" who
  name="$(jq -r .name <<<"$MG_MAC_VM")"
  ssh="$(jq -r .ssh <<<"$MG_MAC_VM")"
  start="$(mg__epoch)"
  mg_on_exit "mg__mac_vm_claim_nudge '$state'"
  holder="$(jq -nc --arg m "$(hostname -s 2>/dev/null || hostname)" --arg job "$MG_JOB" --arg run "$MG_RUN_ID" \
    --arg wt "$MG_ROOT" --arg at "$(mg__iso "$start")" '{machine: $m, job: $job, run_id: $run, worktree: $wt, started_at: $at}')"
  while :; do
    record="$(cat "$state" 2>/dev/null)"
    st="$(jq -r '.state // empty' <<<"$record" 2>/dev/null)"
    pid="$(jq -r '.pid // empty' <<<"$record" 2>/dev/null)"
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      if [ "$st" = taken ]; then
        # It looks at the lease every $beat s: one that is still there after
        # that has seen this job hold it, and keeps the claim for it.
        mg_sleep $((${beat%.*} + 1))
        if [ "$(cat "$state" 2>/dev/null)" = "$record" ] && kill -0 "$pid" 2>/dev/null; then
          mg__mac_vm_detail "$(jq -nc --argjson w "$(($(mg__epoch) - start))" '{claim: {kept: true, waited_s: $w}}')"
          return 0
        fi
      else
        mg_sleep 1
      fi
      continue
    fi
    n=$((n + 1))
    token="$MG_RUN_ID.$$.$n"
    (
      cd / && exec perl -MPOSIX=setsid -e 'fork and exit 0; setsid(); exec @ARGV or die "exec $ARGV[0]: $!\n"' \
        /usr/bin/env python3 "$MG_TOOLS/scripts/vm-claim.py" --ssh "$ssh" --lease "$MG_LEASE_DIR" --state "$state" \
        --token "$token" --holder "$holder" --path "$path" --beat "$beat" \
        </dev/null >>"$log" 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
    )
    st=""
    i=0
    while [ "$i" -lt 180 ]; do
      record="$(cat "$state" 2>/dev/null)"
      if [ "$(jq -r '.token // empty' <<<"$record" 2>/dev/null)" = "$token" ]; then
        st="$(jq -r '.state // empty' <<<"$record" 2>/dev/null)"
        [ "$st" = starting ] || break
      fi
      mg_sleep 0.5
      i=$((i + 1))
    done
    case "$st" in
      taken)
        mg__mac_vm_detail "$(jq -nc --argjson w "$(($(mg__epoch) - start))" '{claim: {waited_s: $w}}')"
        [ -z "$announced" ] || echo "VM claim: took $name after $(($(mg__epoch) - start)) s" >&2
        return 0
        ;;
      busy)
        who="$(jq -r '.holder // "" | (fromjson? // {}) | "\(.job // "a job") on \(.machine // "another Mac") in \(.worktree // "?") since \(.started_at // "?")"' <<<"$record" 2>/dev/null)"
        [ -n "$who" ] || who="a job on another Mac"
        ;;
      error)
        mg__mac_vm_fail mac-vm-claim "the claim on $name could not be taken: $(jq -r '.reason // "unknown"' <<<"$record" 2>/dev/null)"
        return 1
        ;;
      *)
        mg__mac_vm_unreachable "$name did not answer when this job asked for its claim${record:+: $(jq -r '.reason // "no answer"' <<<"$record" 2>/dev/null)}"
        return 2
        ;;
    esac
    if [ $(($(mg__epoch) - start)) -ge "$wait" ]; then
      mg__mac_vm_fail mac-vm-busy "$name stayed claimed for $wait s ($who)"
      return 1
    fi
    if [ -z "$announced" ]; then
      echo "VM claim: waiting for $name ($who)" >&2
      announced=1
    fi
    mg_sleep "$poll"
  done
}

# mg__mac_vm_claim_nudge <state file>: as the job ends, tells this Mac's
# keeper to look at the lease now, so the claim goes with the lease and not
# a beat later.
mg__mac_vm_claim_nudge() {
  local pid
  pid="$(jq -r '.pid // empty' "$1" 2>/dev/null)"
  [ -z "$pid" ] || kill -USR1 "$pid" 2>/dev/null || :
}

# ---------------------------------------------------------------------------
# Starting the VM

# mg__mac_vm_boot: starts the VM MG_MAC_VM names headless when it is not
# running (mvl_start: in a session of its own, so it outlives this job; jobs
# never stop it), then waits until SSH answers in a logged-in session.
# Returns 2 when it gave none in time (queued, mg__mac_vm_unreachable);
# when tart could not start it records an error and returns 1.
mg__mac_vm_boot() {
  local name rc=0
  name="$(jq -r .name <<<"$MG_MAC_VM")"
  mvl_ensure_ready "$MG_MAC_VM" "$MG_RUN_DIR" || rc=1
  [ -z "$MVL_TART_LOG" ] || mg_artifact log "$MVL_TART_LOG" "tart run $name"
  mg__mac_vm_detail "$(jq -nc --argjson s "$MVL_STARTED" '{started: $s}')"
  [ "$rc" -eq 0 ] && return 0
  if [ "$MVL_FAIL" = ssh ]; then
    mg__mac_vm_unreachable "$MVL_REASON"
    return 2
  fi
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
# fails; 2 when the VM could not be reached (queued, mg__mac_vm_unreachable).
mg_mac_vm_copy_in() {
  mvl_copy_in "$(jq -r .ssh <<<"$MG_MAC_VM")" "$(jq -r .name <<<"$MG_MAC_VM")" "$MG_ROOT" "$@" && return 0
  if [ -n "$MVL_UNREACHABLE" ]; then
    mg__mac_vm_unreachable "$MVL_REASON"
    return 2
  fi
  mg_failure mac-vm-copy "$MVL_REASON"
  return 1
}

# mg_mac_vm_copy_out <absolute path>...: copies files and directories back
# from the same absolute paths in the leased VM, replacing what is here.
# Records a failure (mac-vm-copy) and returns 1 when one does not come back;
# 2 when the VM could not be reached (queued, mg__mac_vm_unreachable).
mg_mac_vm_copy_out() {
  mvl_copy_out "$(jq -r .ssh <<<"$MG_MAC_VM")" "$(jq -r .name <<<"$MG_MAC_VM")" "$@" && return 0
  if [ -n "$MVL_UNREACHABLE" ]; then
    mg__mac_vm_unreachable "$MVL_REASON"
    return 2
  fi
  mg_failure mac-vm-copy "$MVL_REASON"
  return 1
}

# mg__vm_xctestrun <plan> <sdk> <config>: this checkout's xctestrun of the
# test plan (built with the scheme MG_APP_TEST_SCHEME, Meguri by default) for
# the SDK (macosx, iphonesimulator) and configuration (the newest, when
# several SDK versions left one). Debug and Profile builds write the same
# file name; its test root says which.
mg__vm_xctestrun() {
  local f root="$3"
  [ "$2" = macosx ] || root="$3-$2"
  # shellcheck disable=SC2012 # newest first; the names are Xcode's
  ls -t "$MG_ROOT/$MG_APP_DD/Build/Products/${MG_APP_TEST_SCHEME:-Meguri}_$1_$2"*.xctestrun 2>/dev/null | while IFS= read -r f; do
    if grep -q "__TESTROOT__/$root/" "$f"; then
      printf '%s\n' "$f"
      break
    fi
  done
}

# mg_mac_vm_xcodebuild_test <prefix> <destination> <config> [xcodebuild args]:
# the Mac UI plan's (or MG_APP_TEST_PLAN's) tests in the leased VM
# (mg_vm_xcodebuild_test).
mg_mac_vm_xcodebuild_test() {
  local prefix="$1"
  shift
  mg_vm_xcodebuild_test "$prefix" "${MG_APP_TEST_PLAN:-UI}" macosx "$@"
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
# When the VM could not be reached (U19c: the session refused, a fork that
# failed there before the run made a result bundle, a lost session after
# which the VM does not answer, a result bundle that could not be fetched
# because SSH failed) the run is queued instead: MG_VM_PART (default: the
# job's runs) is a queued part (mg__mac_vm_unreachable), MG_MAC_VM_RUN_QUEUED
# is set and this returns 1.
mg_vm_xcodebuild_test() {
  local prefix="$1" plan="$2" sdk="$3" dest="$4" config="$5" xctestrun n envs=() rc code dir base vm rcs probe out copy_error="" queued="" err
  shift 5
  MG_MAC_VM_RUN_ERROR=""
  MG_MAC_VM_RUN_QUEUED=""
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
    # xcodebuild may still be running there: it must end before any other run
    # starts in the VM's one login session.
    echo "mac-vm: stopping the test run that lost its SSH session" >&2
    if mg__mac_vm_stop_tests; then
      echo "mac-vm: $([ -n "$MG_MAC_VM_STOPPED" ] && echo "stopped it" || echo "it had already ended")" >&2
      if mvl_unreachable "$rc" "$(cat "$prefix.log")" && mvl_refused "$(cat "$prefix.log")"; then
        queued="$vm refused the test run's SSH session (ssh exit $rc$(mvl_last "$prefix.log"))"
      else
        mg_failure mac-vm-ssh "the SSH session to the VM $vm ended before xcodebuild did (exit $rc); see $prefix.log"
        MG_MAC_VM_RUN_ERROR=1
      fi
    elif [ -n "$MG_MAC_VM_STOP_UNREACHABLE" ]; then
      MG_MAC_VM_LOST="the test run that lost its SSH session may still be running in the VM $vm, which did not answer ($MG_MAC_VM_STOP_REASON)"
      MG_MAC_VM_LOST_UNREACHABLE=1
      queued="the SSH session to $vm ended before xcodebuild did (exit $rc), and $vm did not answer after it ($MG_MAC_VM_STOP_REASON)"
    else
      mg_failure mac-vm-ssh "the SSH session to the VM $vm ended before xcodebuild did (exit $rc); see $prefix.log"
      MG_MAC_VM_RUN_ERROR=1
      MG_MAC_VM_LOST="the test run that lost its SSH session may still be running in the VM $vm ($MG_MAC_VM_STOP_REASON)"
      mg_failure mac-vm-stop "$MG_MAC_VM_LOST; this job runs nothing more there, and the next job that takes $vm stops it first"
    fi
  fi
  MG_MAC_VM_TESTING=""
  # The result bundle comes back into the run directory; the VM keeps no copy.
  # A run that reported its exit code needs it: without it the result is
  # unknown. After a lost session the copy is only for the record.
  rm -rf "$prefix.xcresult"
  if [ -z "$queued" ]; then
    out="$(mg_mac_vm_ssh "test -d $(mg_mac_vm_q "$prefix.xcresult")" 2>&1)"
    probe=$?
    [ "$probe" -ne 1 ] || ! mvl_refused "$out" || probe=255
    case "$probe" in
      0)
        err="$(mvl_tmp)" || err=/dev/null
        mg_mac_vm_ssh "tar -C $(mg_mac_vm_q "$dir") -cf - $(mg_mac_vm_q "$base.xcresult")" 2>"$err" | tar -C "$dir" -xf -
        rcs="${PIPESTATUS[*]}"
        [ "$err" = /dev/null ] || cat "$err" >&2
        if [ "$rcs" != "0 0" ]; then
          if [ -n "$code" ] && [ "$err" != /dev/null ] && mvl_unreachable "${rcs%% *}" "$(cat "$err")"; then
            queued="copying $prefix.xcresult back from the VM $vm failed (ssh and tar exit codes: $rcs$(mvl_last "$err"))"
          else
            copy_error="copying $prefix.xcresult back from the VM $vm failed (ssh and tar exit codes: $rcs)"
          fi
        elif [ ! -d "$prefix.xcresult" ]; then
          copy_error="copying $prefix.xcresult back from the VM $vm brought no result bundle back"
        fi
        [ "$err" = /dev/null ] || rm -f "$err"
        mg_mac_vm_ssh "rm -rf $(mg_mac_vm_q "$prefix.xcresult")" >/dev/null 2>&1
        ;;
      1)
        # xcodebuild that failed before testing writes no bundle: the caller
        # reports it as a run that could not start, unless what failed was a
        # fork in the VM (its process limit).
        if [ -n "$code" ] && [ "$code" != 0 ] && mvl_refused "$(cat "$prefix.log")"; then
          queued="the test run could not start processes in the VM $vm (exit $code$(grep -Ei 'fork|exec request' "$prefix.log" | tail -n 1 | sed 's/^/: /' | tr -d '\r'))"
        fi
        [ "$code" != 0 ] || copy_error="xcodebuild passed in the VM $vm but left no result bundle at $prefix.xcresult"
        ;;
      *)
        if [ -n "$code" ] && mvl_unreachable "$probe" "$out"; then
          queued="could not check for $prefix.xcresult in the VM $vm (ssh exit $probe: $(tail -n 1 <<<"$out" | tr -d '\r'))"
        else
          copy_error="could not check for $prefix.xcresult in the VM $vm (ssh exit $probe: $(tail -n 1 <<<"$out"))"
        fi
        ;;
    esac
  fi
  if [ -n "$queued" ]; then
    mg__mac_vm_unreachable "$queued" "${MG_VM_PART:-}"
    MG_MAC_VM_RUN_QUEUED=1
    return 1
  fi
  if [ -n "$copy_error" ] && [ -n "$code" ]; then
    mg_failure mac-vm-copy "$copy_error"
    MG_MAC_VM_RUN_ERROR=1
  fi
  [ -z "$MG_MAC_VM_RUN_ERROR" ] || return 1
  return "$code"
}

# mg__mac_vm_stop_tests: stops every UI test run in the leased VM and
# confirms none is left (mvl_stop_tests). Returns 0 when none is left
# (MG_MAC_VM_STOPPED=1 when it stopped one); 1 when one still runs or the VM
# did not answer, with MG_MAC_VM_STOP_REASON (MG_MAC_VM_STOP_UNREACHABLE=1
# when the VM could not be reached: mvl_unreachable).
mg__mac_vm_stop_tests() {
  local rc=0
  mvl_stop_tests "$(jq -r .ssh <<<"$MG_MAC_VM")" || rc=1
  MG_MAC_VM_STOPPED="$MVL_STOPPED"
  MG_MAC_VM_STOP_REASON="$MVL_REASON"
  MG_MAC_VM_STOP_UNREACHABLE="$MVL_UNREACHABLE"
  return "$rc"
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
