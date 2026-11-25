#!/bin/bash
# tools/vm/duo-fold.sh: changes the iPhone Duo's fold position in the macOS test
# VM. Only Device Hub's buttons can (not simctl, not XCUITest), so this gives the
# headless VM a screen and input over Tart's VNC server.
#
# usage: tools/vm/duo-fold.sh start [--vm <name>]   prepare/reuse VNC (manual alias)
#        tools/vm/duo-fold.sh prepare|status      persistent session (no repeated restart)
#        tools/vm/duo-fold.sh inspect <udid>       private hardware calibration capture
#        tools/vm/duo-fold.sh open|closed|calibrate <udid>  guarded leased posture
#        tools/vm/duo-fold.sh screen <file.png>     save the VM's whole screen
#        tools/vm/duo-fold.sh shot <udid> <display> <file.png>   1 is outer, 3 is inner
#        tools/vm/duo-fold.sh stop                  disconnect, restart the VM headless
#
# Get the simulator with `tools/mg sim acquire foldable` after `prepare`.
# Release it with `tools/mg sim release <udid>` before `stop`.
# Preparation reuses an existing session. A new session and `stop` require no
# active acquisitions or guest test jobs, under the shared VM scheduler lease.
# Between them, tools/mg jobs still work. Only this script's one
# connection may reach VNC: reconnecting crashes Tart and takes running jobs with
# it (docs/process/mac-ui-vm.md).
#
# Production posture uses private calibration and fresh visual control evidence.
# No fixed-coordinate fold or unleased generic click/boot route is provided.
# Exit codes: 0 done, 1 failed, 2 usage, 3 the lease stayed busy.

set -u
umask 077

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
# shellcheck source=/dev/null
. "$ROOT/tools/scripts/mac-vm-lib.sh"
. "$ROOT/tools/scripts/host-lease-mutex.sh"

MAIN="$(dirname "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)")"
CANONICAL="$MAIN/build/vm-fold"
STATE="$CANONICAL"
VENV="$CANONICAL/venv"
VNCDOTOOL_VERSION=1.4.2

log() { printf 'duo-fold: %s\n' "$*" >&2; }
usage() { sed -n '6,13p' "$0" | sed 's/^# *//' >&2; exit 2; }

SESSION="$ROOT/tools/vm/vnc-session.py"
RECOVER="" LEASE="" LEASE_WRITING=""

mvl_read_config || { log "$MVL_REASON"; exit 1; }
PRIMARY="$(jq -r '.[0].name' <<<"$MVL_ALL")"
VM="$PRIMARY"
COMMAND="${1:-}"
[ $# -eq 0 ] || shift
if [ "${1:-}" = --vm ] && [ $# -eq 2 ]; then VM="$2"; shift 2; fi
SELECTED="$(jq -c --arg n "$VM" 'first(.[] | select(.name == $n)) // empty' <<<"$MVL_ALL")"
[ -n "$SELECTED" ] || { log "VM is not configured"; exit 1; }
TART="$(jq -r .tart <<<"$SELECTED")"
SSH="$(jq -r .ssh <<<"$SELECTED")"
# The persistent state and holder venv are global, not per VM. Never stop or
# start a secondary guest under the primary guest's state/scheduler lease.
case "$COMMAND" in start|prepare|stop|inspect|open|closed|calibrate)
  [ "$VM" = "$PRIMARY" ] || { log "VNC lifecycle supports only configured primary VM $PRIMARY"; exit 1; }
esac

select_state() {
  STATE="$(python3 "$SESSION" route "$CANONICAL" "$VM")" || exit 1
  python3 "$SESSION" primary "$STATE" "$VM" || exit 1
  FIFO="$STATE/commands"
}

recheck_state() {
  local selected
  selected="$(python3 "$SESSION" route "$CANONICAL" "$VM")" || return 1
  [ "$selected" = "$STATE" ] || { log "session route changed; retain evidence"; return 1; }
  python3 "$SESSION" primary "$STATE" "$VM" || return 1
}

release() {
  if [ -n "$LEASE" ] && host_lease_lock "$LEASE"; then
    if [ "$LEASE_WRITING" = 1 ] || [ "$(jq -r .pid "$LEASE/holder.json" 2>/dev/null)" = "$$" ]; then
      host_lease_drop "$LEASE"
    fi
    host_lease_unlock
  fi
}

headless() {
  recheck_state || return 1
  [ ! -e "$STATE/shutdown-pending" ] || {
    log "remote stop completion uncertain; evidence retained; installed wrapper cannot prove recovery"; return 1;
  }
  python3 "$SESSION" owned "$STATE" || return 1
  case "$(mvl_state "$TART" "$VM")" in
    stopped) ;;
    running) shutdown_guest || return 1 ;;
    *) log "VM shutdown status uncertain; evidence retained"; return 1 ;;
  esac
  python3 "$SESSION" stop "$STATE" || return 1
  mvl_start "$TART" "$VM" "$STATE/headless.log" || return 1
  mvl_wait_ready "$SSH" "$TART" "$VM" true "$STATE/headless.log" "$STATE/headless.ssh" >"$STATE/readiness.log" 2>&1 || return 1
  python3 "$SESSION" clean-drain "$STATE" "$SSH" || return 1
  rm -f "$STATE/recovery-pending"
}

finish() {
  local rc=$?
  trap - EXIT HUP INT TERM
  if [ "$RECOVER" = 1 ] && [ -e "$STATE/recovery-pending" ]; then
    log "preparation failed; checking guarded headless restoration"
    if headless; then log "$VM restored headless"; else
      log "headless restoration unconfirmed; retain private $STATE evidence for operator recovery. Lost remote exits cannot be resolved by rerunning the wrapper; do not clear records or prepare again"
      rc=1
    fi
  fi
  release
  exit "$rc"
}

lease() {
  local dir start
  dir="$(mvl_lease_dir "$ROOT" "$PRIMARY")"
  mkdir -p "$(dirname "$dir")" || exit 1
  start="$(mvl_epoch)"
  trap finish EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM
  while :; do
    host_lease_lock "$dir" || exit 1
    if mkdir "$dir" 2>/dev/null; then
      LEASE="$dir" LEASE_WRITING=1
      # Keep stale-holder reclamation excluded until metadata is complete.
      if ! jq -nc --argjson pid $$ --arg wt "$ROOT" --arg job "$1" \
        '{pid: $pid, worktree: $wt, job: $job, holder: "duo-fold"}' >"$LEASE/holder.json"; then
        host_lease_drop "$LEASE"
        LEASE="" LEASE_WRITING=""
        host_lease_unlock
        exit 1
      fi
      LEASE_WRITING=""
      host_lease_unlock
      return 0
    fi
    host_lease_unlock
    if [ $(($(mvl_epoch) - start)) -ge "${MEGURI_MAC_VM_LEASE_WAIT_SECONDS:-600}" ]; then
      log "the lease of $VM stayed busy"; exit 3
    fi
    sleep 1
  done
}

# Scheduler excludes acquisitions/jobs; the guest pool mutex also excludes
# legacy renewals from final inventory through shutdown initiation.
shutdown_guest() {
  recheck_state || return 1
  mvl_tests_idle "$SSH" || { log "guest tests running or unknown; wait for jobs to finish"; return 1; }
  python3 "$SESSION" shutdown "$STATE" "$SSH" "$TART" "$VM" "$LEASE" || return 1
  [ "$(mvl_state "$TART" "$VM")" = stopped ] || {
    log "shutdown command completed but stopped state unconfirmed; evidence retained"; return 1;
  }
}

send() {
  [ "$(session_status | jq -r .connected)" = true ] ||
    { log "existing holder unavailable"; exit 1; }
  printf '%s\n' "$1" >"$FIFO"
}

session_status() { python3 "$SESSION" status "$STATE" "$VM" "$TART"; }

pending_cleanup() {
  local name
  for name in drain-token recovery-pending shutdown-pending drain-precommit; do
    if [ -e "$STATE/$name" ] || [ -L "$STATE/$name" ]; then return 0; fi
  done
  return 1
}

poststart_cleanup() {
  recheck_state || return 1
  python3 "$SESSION" poststart-cleanup "$STATE" "$SSH" "$VM" "$LEASE" "$$" "$1"
}

start() {
  local host current
  [ $# -eq 0 ] || usage
  lease vm-fold-prepare
  recheck_state || exit 1
  python3 "$SESSION" init "$STATE" || exit 1
  current="$(session_status)" || exit 1
  if [ "$(jq -r .vm <<<"$current")" != null ] && [ "$(jq -r .vm <<<"$current")" != "$VM" ]; then
    log "global VNC state belongs to another VM; retain evidence for operator recovery"; exit 1
  fi
  if [ "$(jq -r .connected <<<"$current")" = true ]; then
    if pending_cleanup; then
      poststart_cleanup check || exit 1
      # This is an owned, already booted session. Never arm rollback, reconnect
      # VNC or issue another stop/start when its cleanup SSH is unavailable.
      mvl_wait_ready "$SSH" "$TART" "$VM" false "$STATE/tart.log" "$STATE/guest.ssh" >"$STATE/readiness.log" 2>&1 || {
        log "post-start cleanup cannot reach the logged-in guest; keep VNC connected and retry prepare when SSH is available; private evidence retained"; exit 1;
      }
      poststart_cleanup finish || exit 1
    fi
    log "connected to $VM (reused)"; return 0
  fi
  # Never replace an existing or incomplete session on a regular prepare call.
  python3 "$SESSION" vacant "$STATE" || exit 1
  recheck_state || exit 1
  host="$(python3 "$SESSION" host "$TART")" || exit 1
  if [ -n "$host" ] || [ "$STATE" != "$CANONICAL" ]; then
    [ -x "$VENV/bin/python" ] && "$VENV/bin/python" -I -c 'from vncdotool import api' >"$STATE/dependency.log" 2>&1 || {
      log "existing holder venv missing/unusable at $VENV; restore it on the coordinator before preparing"; exit 1;
    }
  else
    [ -x "$VENV/bin/python" ] || {
      python3 -m venv "$VENV" && "$VENV/bin/pip" install --quiet "vncdotool==$VNCDOTOOL_VERSION"
    } || { log "could not install vncdotool"; exit 1; }
  fi
  python3 "$SESSION" preflight "$STATE" || exit 1
  # The shutdown helper arms recovery only once drain succeeds. Keep it armed
  # even if a completed shutdown's subsequent state query fails.
  RECOVER=1
  shutdown_guest || exit 1
  recheck_state || exit 1
  python3 "$SESSION" start "$STATE" "$TART" "$VM" "$VENV/bin/python" || exit 1
  mvl_wait_ready "$SSH" "$TART" "$VM" true "$STATE/tart.log" "$STATE/guest.ssh" >"$STATE/readiness.log" 2>&1 || {
    log "VNC guest did not become accessible; inspect private $STATE logs"; exit 1;
  }
  # GUI login after our successful start confirms a working post-start session.
  # Disarm before any further status read: a lost response is not proof of a
  # failed start. Only a positively authenticated disconnect permits rollback.
  RECOVER=""
  current="$(session_status)" || exit 1
  if [ "$(jq -r .connected <<<"$current")" != true ]; then
    if [ "$(jq -r .conclusive <<<"$current")" = true ]; then
      RECOVER=1
      log "VNC transport disconnected before post-start confirmation"
    else
      log "post-start status inconclusive; retain evidence and retry prepare"
    fi
    exit 1
  fi
  poststart_cleanup finish || exit 1
  [ "$(session_status | jq -r .connected)" = true ] || {
    log "VNC transport disconnected after preparation"; exit 1;
  }
  log "connected to $VM"
}

stop() {
  [ $# -eq 0 ] || usage
  lease vm-fold-stop
  recheck_state || exit 1
  python3 "$SESSION" init "$STATE" || exit 1
  # One recovery attempt only; an uncertain result keeps all session evidence.
  headless || exit 1
  log "$VM is back in plain headless mode"
}

screen() {
  [ -n "${1:-}" ] || usage
  send "shot $1"
  sleep 2
}

shot() {
  [ $# -eq 3 ] || usage
  "$SSH" "xcrun simctl io $(mvl_q "$1") screenshot --display=$(mvl_q "$2") /tmp/duo-fold.png >/dev/null 2>&1 && cat /tmp/duo-fold.png" </dev/null >"$3"
}

# Read-only inspection. Never calls start, headless, boot, reset or a controller.
# Keep the scheduler lease through guest validation, launch and capture join.
inspect() {
  [ $# -eq 1 ] || usage
  lease vm-fold-inspect
  recheck_state || exit 1
  python3 "$ROOT/tools/vm/sim-posture-inspect.py" "$STATE" "$SSH" "$VM" "$ROOT" "$1" "$TART"
}

posture() {
  [ $# -eq 2 ] || usage
  [ "$VM" = "$PRIMARY" ] || { log "posture requires the primary pool VM"; exit 1; }
  lease vm-fold-posture
  recheck_state || exit 1
  # Use the existing operator environment, including Pillow. Never install,
  # prepare VNC, reconnect, start the VM or fall back to host simulators here.
  [ -x "$VENV/bin/python" ] || { log "existing operator venv required"; return 1; }
  "$VENV/bin/python" -I -c 'from PIL import Image, ImageChops' >/dev/null 2>&1 || {
    log "restore Pillow in the existing operator venv before posture"; return 1;
  }
  "$VENV/bin/python" -I "$ROOT/tools/vm/sim-posture.py" \
    "$STATE" "$SSH" "$VM" "$ROOT" "$2" "$TART" "$1" "$LEASE"
}

select_state

case "$COMMAND" in
  start|prepare) start "$@" ;;
  status) [ $# -eq 0 ] || usage; session_status ;;
  inspect) inspect "$@" ;;
  open|closed|calibrate) posture "$COMMAND" "$@" ;;
  stop) stop "$@" ;;
  screen) screen "${1:-}" ;;
  shot) shot "$@" ;;
  *) usage ;;
esac
