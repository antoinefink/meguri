# Host lease, session check and the Mac UI gate (U8, U8b). Sourced by tools/mg.
#
# Host lease: every job that installs or launches the app, drives it or
# records it on this Mac takes one lease shared by every checkout of this
# repository (the main checkout's build/mg/host.lock, a mkdir lock) and
# queues while another job holds it. Holding it, the job checks that the
# macOS session is unlocked and a display awake, and keeps the display awake
# (caffeinate) until it ends; a locked or sleeping Mac makes the job report
# `blocked: host` (status error, user step host-session), never fail.
#
# Mac UI suites (Mac XCUITest) run in the macOS test VM (mac-vm.sh), as often
# as needed: they take the VM's lease, not the host lease, and never drive
# this Mac's cursor or keyboard (root AGENTS.md). Without the VM they are
# queued. MEGURI_MAC_UI_ALLOW_HOST=1 runs them on this Mac instead, for a run
# Antoine starts himself, through the Mac UI gate: one run at a time, and none
# within MEGURI_MAC_UI_INTERVAL_MINUTES (60) of the previous run's start; a
# refused run is reported as queued (status warn).
#
# Overrides (self-tests): MEGURI_HOST_LEASE_DIR, MEGURI_HOST_LEASE_WAIT_SECONDS
# (3600), MEGURI_HOST_LEASE_POLL_SECONDS (5), MEGURI_MAC_UI_DIR,
# MEGURI_MAC_UI_INTERVAL_MINUTES.

mg_job mac-ui host mg_mac_ui \
  "Mac UI-driving suites in the macOS test VM, unthrottled (docs/process/mac-ui-vm.md): [--suite test-hooks|agent-driver|a11y|drive:<script>]... (default: TestHooksTests, the agent driver's resize test, the accessibility suites (U10) and every example drive script for the Mac; scripts that mark a transition are recorded); without the VM they are queued (warn, user step mac-vm), or with MEGURI_MAC_UI_ALLOW_HOST=1 run on this Mac through the hourly gate"

# ---------------------------------------------------------------------------
# Exit actions: a job runs in its own subshell; these run once when it ends,
# also when tools/mg cancels it (SIGTERM), last registered first.

MG_EXIT_CMDS=()

mg_on_exit() {
  MG_EXIT_CMDS[${#MG_EXIT_CMDS[@]}]="$1"
  trap mg__run_exit_cmds EXIT
  trap 'exit 143' TERM
  trap 'exit 129' HUP
}

mg__run_exit_cmds() {
  local i=${#MG_EXIT_CMDS[@]}
  while [ "$i" -gt 0 ]; do
    i=$((i - 1))
    eval "${MG_EXIT_CMDS[$i]}"
  done
  MG_EXIT_CMDS=()
}

# A sleep that a TERM trap interrupts at once.
mg_sleep() {
  sleep "$1" &
  wait "$!"
}

mg__epoch() { date -u +%s; }
mg__iso() { date -u -r "$1" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ; }

# ---------------------------------------------------------------------------
# Host lease

# The lease directory, shared by every checkout of the repository.
mg_host_lease_dir() {
  if [ -n "${MEGURI_HOST_LEASE_DIR:-}" ]; then
    printf '%s\n' "$MEGURI_HOST_LEASE_DIR"
    return 0
  fi
  local common
  common="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || common=""
  if [ -n "$common" ] && [ "$(basename "$common")" = .git ]; then
    printf '%s/build/mg/host.lock\n' "$(dirname "$common")"
  else
    printf '%s/build/mg/host.lock\n' "$MG_ROOT"
  fi
}

# mg_host_session_state: ok, locked or display-asleep.
mg_host_session_state() {
  if ioreg -n Root -d1 2>/dev/null | grep -q '"CGSSessionScreenIsLocked"=Yes'; then
    echo locked
    return 0
  fi
  if ! system_profiler SPDisplaysDataType -json 2>/dev/null | jq -e '
      [.SPDisplaysDataType[]?.spdisplays_ndrvs[]?] as $d
      | ($d | length) > 0 and ($d | all(.spdisplays_asleep == "spdisplays_yes") | not)' >/dev/null 2>&1; then
    echo display-asleep
    return 0
  fi
  echo ok
}

# Every lease change but taking a free lease runs under its mutex.
# shellcheck source=/dev/null
. "$MG_TOOLS/scripts/host-lease-mutex.sh"

# mg__lease_stale <lock dir> <holder JSON>: whether the holder died, or died
# between mkdir and writing holder.json.
mg__lease_stale() {
  local pid
  pid="$(jq -r '.pid // empty' <<<"$2" 2>/dev/null)"
  { [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null; } ||
    { [ -z "$pid" ] && [ -n "$(find "$1" -maxdepth 0 -mmin +2 2>/dev/null)" ]; }
}

# mg__lease_remove_stale <lease dir> <holder JSON read>: under the mutex, ends
# the lease if it still holds what was read and is still stale; a live job in
# its nested slot (a recording worker died under it) inherits it. Another
# waiter may have removed it and taken the lease meanwhile.
mg__lease_remove_stale() {
  host_lease_lock "$1" || return 1
  if [ "$(cat "$1/holder.json" 2>/dev/null)" = "$2" ] && mg__lease_stale "$1" "$2"; then
    echo "host lease: removing a stale lease ($(jq -r '"\(.job) in \(.worktree), pid \(.pid)"' <<<"$2" 2>/dev/null || echo unknown))" >&2
    host_lease_drop "$1"
  fi
  host_lease_unlock
}

# mg__lease_write <slot dir> <nested true|false>: this job's holder.json.
mg__lease_write() {
  jq -n --arg job "$MG_JOB" --arg run "${MG_RUN_ID:-}" --argjson pid "$$" --arg wt "$MG_ROOT" --arg at "$(mg__iso "$(mg__epoch)")" \
    --argjson nested "$2" \
    '{job: $job, run_id: $run, pid: $pid, worktree: $wt, started_at: $at} + (if $nested then {nested: true} else {} end)' >"$1/holder.json.tmp" &&
    mv -f "$1/holder.json.tmp" "$1/holder.json"
}

# mg_host_lease [--no-session]: takes the host lease, waiting while another
# job holds it, then checks the session. Returns 0 when the job may go on
# (the lease is released when the job ends); otherwise records the result
# and returns 1. --no-session is for jobs that use neither the screen nor a
# simulator (bench-mac): they run on a locked Mac too, and keep it from idle
# sleep but let the display sleep.
#
# A recording (`tools/mg record start`) keeps the lease until it stops. While
# it holds it for this checkout, this checkout's jobs take the one nested slot
# <lease>/inner instead, so that the recording can film them; other checkouts
# keep queueing. A nested job still running when the recording stops inherits
# the lease (tools/scripts/sim-record.sh). The nested slot is taken, and the
# recording's lease released, under the lease's mutex
# (tools/scripts/host-lease-mutex.sh).
mg_host_lease() {
  local dir wait="${MEGURI_HOST_LEASE_WAIT_SECONDS:-3600}" poll="${MEGURI_HOST_LEASE_POLL_SECONDS:-5}"
  local start holder inner announced="" lock session=check
  [ "${1:-}" != --no-session ] || session=skip
  dir="$(mg_host_lease_dir)"
  mkdir -p "$(dirname "$dir")" || {
    mg_failure host-lease "cannot create $(dirname "$dir")"
    mg_status error
    return 1
  }
  start="$(mg__epoch)"
  while :; do
    if mkdir "$dir" 2>/dev/null; then
      lock="$dir"
      mg__lease_write "$lock" false
      break
    fi
    holder="$(cat "$dir/holder.json" 2>/dev/null)"
    if mg__lease_stale "$dir" "$holder"; then
      mg__lease_remove_stale "$dir" "$holder" || mg_sleep "$poll"
      continue
    fi
    if [ "$(jq -r '"\(.kind // "")|\(.worktree // "")"' <<<"$holder" 2>/dev/null)" = "recording|$MG_ROOT" ]; then
      # The nested slot is taken under the mutex, so the recording cannot
      # release or hand over the lease between its mkdir and holder.json.
      if ! host_lease_lock "$dir"; then
        mg_sleep "$poll"
        continue
      fi
      if [ "$(cat "$dir/holder.json" 2>/dev/null)" != "$holder" ]; then
        # The recording released or handed over the lease since it was read.
        host_lease_unlock
        continue
      fi
      if mkdir "$dir/inner" 2>/dev/null; then
        lock="$dir/inner"
        mg__lease_write "$lock" true
        host_lease_unlock
        break
      fi
      inner="$(cat "$dir/inner/holder.json" 2>/dev/null)"
      if mg__lease_stale "$dir/inner" "$inner"; then
        rm -rf "$dir/inner"
        host_lease_unlock
        continue
      fi
      host_lease_unlock
      [ -z "$inner" ] || holder="$inner"
    fi
    if [ $(($(mg__epoch) - start)) -ge "$wait" ]; then
      mg_failure host-busy "the host lease stayed held for $wait s by $(jq -r '"\(.job) in \(.worktree) since \(.started_at)"' <<<"$holder" 2>/dev/null || echo "another job")" \
        "$MG_DEFAULT_REPRO"
      mg_message "$MG_JOB: the host lease stayed busy for $wait s"
      mg_status error
      return 1
    fi
    if [ -z "$announced" ]; then
      echo "host lease: waiting for $(jq -r '"\(.job) in \(.worktree) (since \(.started_at))"' <<<"$holder" 2>/dev/null || echo "another job")" >&2
      announced=1
    fi
    mg_sleep "$poll"
  done
  mg_on_exit "mg__host_lease_release '$dir' $$"
  mg_detail host_lease "$(jq -c --argjson waited "$(($(mg__epoch) - start))" '. + {waited_s: $waited}' "$lock/holder.json")"
  [ -z "$announced" ] || echo "host lease: taken after $(($(mg__epoch) - start)) s" >&2

  local state
  if [ "$session" = skip ]; then
    if command -v caffeinate >/dev/null 2>&1; then
      caffeinate -i -w "$$" >/dev/null 2>&1 &
      mg_on_exit "kill $! 2>/dev/null"
    fi
    return 0
  fi
  state="$(mg_host_session_state)"
  if [ "$state" != ok ]; then
    mg_user_step host-session missing true "Unlock the Mac and keep a display awake" \
      "App, simulator and Mac UI jobs wait while the Mac is locked or its display sleeps; unlock it, or run them later."
    mg_failure blocked-host "blocked: host ($state); the job did not run" "$MG_DEFAULT_REPRO"
    mg_detail blocked '"host"'
    mg_detail host_session "$(jq -n --arg s "$state" '$s')"
    mg_message "$MG_JOB: blocked: host ($state); run it again once the Mac is unlocked and awake"
    mg_status error
    return 1
  fi
  if command -v caffeinate >/dev/null 2>&1; then
    caffeinate -d -i -w "$$" >/dev/null 2>&1 &
    mg_on_exit "kill $! 2>/dev/null"
  fi
  return 0
}

# mg__host_lease_release <lease dir> <pid>: releases the slot <pid> holds (the
# nested one or the lease), and nothing else: a lease handed to a recording
# (mg_host_lease_transfer) or inherited from one stays with its holder, and a
# live nested job inherits the lease it releases (record stop of a worker
# that died or would not stop).
mg__host_lease_release() {
  local locked=0
  host_lease_lock "$1" && locked=1
  if [ "$(jq -r '.pid // empty' "$1/inner/holder.json" 2>/dev/null)" = "$2" ]; then
    rm -rf "$1/inner"
  elif [ "$(jq -r '.pid // empty' "$1/holder.json" 2>/dev/null)" = "$2" ]; then
    host_lease_drop "$1"
  fi
  [ "$locked" = 0 ] || host_lease_unlock
}

# mg_host_lease_transfer <pid> <JSON object>: hands the slot this job holds
# (the lease, or the nested slot) to a process that outlives the job (a
# recording worker), merging the object into its holder.json. That process
# releases it; until then other jobs wait for it, and the slot turns stale
# when it dies. Returns 1 when this job holds no slot.
mg_host_lease_transfer() {
  local dir slot rc=0
  dir="$(mg_host_lease_dir)"
  host_lease_lock "$dir" || return 1
  if [ "$(jq -r '.pid // empty' "$dir/inner/holder.json" 2>/dev/null)" = "$$" ]; then
    slot="$dir/inner"
  elif [ "$(jq -r '.pid // empty' "$dir/holder.json" 2>/dev/null)" = "$$" ]; then
    slot="$dir"
  else
    host_lease_unlock
    return 1
  fi
  jq --argjson pid "$1" --argjson extra "$2" '. + $extra + {pid: $pid}' "$slot/holder.json" >"$slot/holder.json.tmp" &&
    mv -f "$slot/holder.json.tmp" "$slot/holder.json" || rc=1
  host_lease_unlock
  return "$rc"
}

# ---------------------------------------------------------------------------
# Mac UI gate (host runs only, MEGURI_MAC_UI_ALLOW_HOST=1)

mg__mac_ui_dir() { printf '%s\n' "${MEGURI_MAC_UI_DIR:-$HOME/Library/Caches/meguri/mac-ui}"; }

# mg__mac_ui_queued <reason>: records a refused Mac UI run.
mg__mac_ui_queued() {
  mg_detail queued true
  local last
  last="$(jq -c . "$(mg__mac_ui_dir)/last-start.json" 2>/dev/null)" || last=null
  mg_detail mac_ui "$(jq -n --arg r "$1" --argjson last "${last:-null}" '{reason: $r, last_start: $last}')"
  mg_message "$MG_JOB: queued by the hourly Mac UI gate (runs on this Mac): $1"
  mg_status warn
}

# mg__mac_ui_throttled: whether the previous run started too recently;
# prints the reason.
mg__mac_ui_throttled() {
  local last now interval="${MEGURI_MAC_UI_INTERVAL_MINUTES:-60}"
  last="$(jq -r '.epoch // empty' "$(mg__mac_ui_dir)/last-start.json" 2>/dev/null)"
  now="$(mg__epoch)"
  if [ -n "$last" ] && [ $((now - last)) -lt $((interval * 60)) ]; then
    echo "the previous Mac UI run started at $(mg__iso "$last"); the next may start at $(mg__iso $((last + interval * 60)))"
    return 0
  fi
  return 1
}

# mg_mac_ui_gate_open: whether a Mac UI run could start now (reads only).
# Prints the reason when it could not.
mg_mac_ui_gate_open() {
  local dir holder pid
  dir="$(mg__mac_ui_dir)"
  if [ -d "$dir/lock" ]; then
    holder="$(cat "$dir/lock/holder.json" 2>/dev/null)"
    pid="$(jq -r '.pid // empty' <<<"$holder" 2>/dev/null)"
    if { [ -z "$pid" ] && [ -z "$(find "$dir/lock" -maxdepth 0 -mmin +2 2>/dev/null)" ]; } ||
      { [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; }; then
      echo "another Mac UI run is in progress ($(jq -r '"\(.job) in \(.worktree)"' <<<"$holder" 2>/dev/null || echo unknown))"
      return 1
    fi
  fi
  ! mg__mac_ui_throttled
}

# mg_mac_ui_gate: takes the gate for this job, or records the run as queued
# and returns 1. Taking it stamps the start, holds the gate until the job
# ends and exports MEGURI_ALLOW_MAC_UI=1.
mg_mac_ui_gate() {
  local dir reason holder pid
  dir="$(mg__mac_ui_dir)"
  mkdir -p "$dir"
  if ! mkdir "$dir/lock" 2>/dev/null; then
    holder="$(cat "$dir/lock/holder.json" 2>/dev/null)"
    pid="$(jq -r '.pid // empty' <<<"$holder" 2>/dev/null)"
    # A run that died, or died between mkdir and writing holder.json.
    if { [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null; } ||
      { [ -z "$pid" ] && [ -n "$(find "$dir/lock" -maxdepth 0 -mmin +2 2>/dev/null)" ]; }; then
      rm -rf "$dir/lock"
    fi
    if ! mkdir "$dir/lock" 2>/dev/null; then
      mg__mac_ui_queued "another Mac UI run is in progress ($(jq -r '"\(.job) in \(.worktree)"' <<<"$holder" 2>/dev/null || echo unknown))"
      return 1
    fi
  fi
  jq -n --arg job "$MG_JOB" --argjson pid "$$" --arg wt "$MG_ROOT" '{job: $job, pid: $pid, worktree: $wt}' >"$dir/lock/holder.json"
  if reason="$(mg__mac_ui_throttled)"; then
    rm -rf "$dir/lock"
    mg__mac_ui_queued "$reason"
    return 1
  fi
  local now
  now="$(mg__epoch)"
  jq -n --argjson e "$now" --arg at "$(mg__iso "$now")" --arg job "$MG_JOB" --arg run "${MG_RUN_ID:-}" \
    --arg wt "$MG_ROOT" --arg sha "$(git -C "$MG_ROOT" rev-parse --verify -q HEAD 2>/dev/null)" \
    '{epoch: $e, started_at: $at, job: $job, run_id: $run, worktree: $wt, commit: $sha}' >"$dir/last-start.json.tmp" &&
    mv -f "$dir/last-start.json.tmp" "$dir/last-start.json"
  mg_on_exit "rm -rf '$dir/lock'"
  mg_detail mac_ui "$(jq -c '{started: .}' "$dir/last-start.json")"
  export MEGURI_ALLOW_MAC_UI=1
  return 0
}

# ---------------------------------------------------------------------------
# tools/mg mac-ui

mg_mac_ui() {
  local suites=() s
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --suite)
        [ "$#" -ge 2 ] || { mg_failure usage "--suite needs test-hooks, agent-driver, a11y or drive:<script>"; mg_status error; return 0; }
        case "$2" in
          test-hooks | agent-driver | a11y | drive:?*) suites[${#suites[@]}]="$2" ;;
          *) mg_failure usage "unknown suite '$2' (test-hooks, agent-driver, a11y, drive:<script>)"; mg_status error; return 0 ;;
        esac
        shift 2
        ;;
      *) mg_failure usage "usage: tools/mg mac-ui [--suite test-hooks|agent-driver|a11y|drive:<script>]..."; mg_status error; return 0 ;;
    esac
  done
  if [ "${#suites[@]}" -eq 0 ]; then
    suites=(test-hooks agent-driver a11y)
    for s in tools/drive-scripts/examples/*.json; do
      [ -f "$s" ] || continue
      jq -e '(.platforms // ["ios", "mac"]) | index("mac")' "$s" >/dev/null 2>&1 && suites[${#suites[@]}]="drive:$s"
    done
  fi
  mg_detail suites "$(mg_json_array "${suites[@]}")"
  mg_mac_ui_begin || return 0
  mg_app_build mac Debug || return 0
  mg_mac_ui_take || return 0

  local passed=0 failed=0 errors=0 name result record since only c
  for s in "${suites[@]}"; do
    case "$s" in
      drive:*) name="drive-$(basename "${s#drive:}" .json)" ;;
      *) name="$s" ;;
    esac
    # A lost run the VM could not stop: no run starts beside it.
    if [ -n "$MG_MAC_VM_LOST" ]; then
      errors=$((errors + 1))
      mg_failure "suite/$name" "Mac UI suite $name was not run: $MG_MAC_VM_LOST" "tools/mg mac-ui --suite $s"
      continue
    fi
    case "$s" in
      test-hooks | agent-driver | a11y)
        since="$(mg_app_log_now)"
        case "$s" in
          test-hooks) only=TestHooksTests ;;
          agent-driver) only=AgentDriverTests/testResizeKeepsTheRunningAppAndItsState ;;
          a11y) only="$MG_A11Y_SUITES" ;;
        esac
        # shellcheck disable=SC2046 # one test class per word
        mg_app_xcodebuild_test "$MG_RUN_DIR/$name" mac "platform=macOS,arch=arm64" Debug \
          $(for c in $only; do printf -- '-only-testing:MeguriUITests/%s\n' "$c"; done)
        result=$?
        ! mg_app_mac_input_refused "$since" || result=2
        ;;
      drive:*)
        # A script that marks a transition also records it (motion capture).
        record=no
        jq -e 'any(.steps[]; has("transition"))' "${s#drive:}" >/dev/null 2>&1 && record=yes
        mg_drive_run "${s#drive:}" mac "" Debug "$record" "$MG_RUN_DIR/$name"
        result=$?
        ;;
    esac
    case "$result" in
      0) passed=$((passed + 1)) ;;
      1) failed=$((failed + 1)) && mg_failure "suite/$name" "Mac UI suite $name failed; see log.txt" "tools/mg mac-ui --suite $s" ;;
      *) errors=$((errors + 1)) && mg_failure "suite/$name" "Mac UI suite $name could not run; see log.txt" "tools/mg mac-ui --suite $s" ;;
    esac
  done
  mg_count total "${#suites[@]}"
  mg_count passed "$passed"
  mg_count failed "$failed"
  mg_count errors "$errors"
  if [ "$failed" -gt 0 ]; then
    mg_message "mac-ui: $failed of ${#suites[@]} suite(s) failed"
    mg_status fail
  elif [ "$errors" -gt 0 ]; then
    if [ -n "${MG_MAC_INPUT_REFUSED:-}" ]; then
      mg_message "mac-ui: blocked: macOS dropped the input XCUITest synthesized (user step mac-input); $errors of ${#suites[@]} suite(s) did not test the app"
    else
      mg_message "mac-ui: $errors of ${#suites[@]} suite(s) could not run"
    fi
    mg_status error
  else
    mg_message "mac-ui: ${#suites[@]} suite(s) passed"
    mg_status pass
  fi
}
