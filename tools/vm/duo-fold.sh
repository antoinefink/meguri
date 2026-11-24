#!/bin/bash
# tools/vm/duo-fold.sh: changes the iPhone Duo's fold position in the macOS test
# VM. Only Device Hub's buttons can (not simctl, not XCUITest), so this gives the
# headless VM a screen and input over Tart's VNC server.
#
# usage: tools/vm/duo-fold.sh start [--vm <name>]   prepare/reuse VNC (manual alias)
#        tools/vm/duo-fold.sh prepare|status      persistent session (no repeated restart)
#        tools/vm/duo-fold.sh inspect <udid>       private hardware calibration capture
#        tools/vm/duo-fold.sh hub <udid>            open Device Hub, boot the simulator
#        tools/vm/duo-fold.sh fold closed|half|open click Device Hub's fold button
#        tools/vm/duo-fold.sh click <x> <y>         click a point of the VM's screen
#        tools/vm/duo-fold.sh screen <file.png>     save the VM's whole screen
#        tools/vm/duo-fold.sh shot <udid> <display> <file.png>   1 is outer, 3 is inner
#        tools/vm/duo-fold.sh stop                  disconnect, restart the VM headless
#
# Get the simulator with `tools/mg sim acquire foldable` after `prepare`.
# Preparation reuses an existing session. A new session and `stop` require no
# active acquisitions or guest test jobs, under the shared VM scheduler lease.
# Between them, tools/mg jobs still work. Only this script's one
# connection may reach VNC: reconnecting crashes Tart and takes running jobs with
# it (docs/process/mac-ui-vm.md).
#
# The fold buttons are screen positions, right for Device Hub's window where it
# opened on 2026-11-01. Check with `screen`, and set DUO_FOLD_Y and
# DUO_FOLD_CLOSED_X, DUO_FOLD_HALF_X and DUO_FOLD_OPEN_X if the window moved.
# Exit codes: 0 done, 1 failed, 2 usage, 3 the lease stayed busy.

set -u
umask 077

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
# shellcheck source=/dev/null
. "$ROOT/tools/scripts/mac-vm-lib.sh"
. "$ROOT/tools/scripts/host-lease-mutex.sh"

MAIN="$(dirname "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)")"
STATE="$MAIN/build/vm-fold"
FIFO="$STATE/commands"
VNCDOTOOL_VERSION=1.4.2

log() { printf 'duo-fold: %s\n' "$*" >&2; }
usage() { sed -n '6,13p' "$0" | sed 's/^# *//' >&2; exit 2; }

SESSION="$ROOT/tools/vm/vnc-session.py"
RECOVER="" LEASE=""

mvl_read_config || { log "$MVL_REASON"; exit 1; }
VM="$(jq -r '.[0].name' <<<"$MVL_ALL")"
COMMAND="${1:-}"
[ $# -eq 0 ] || shift
if [ "${1:-}" = --vm ] && [ $# -eq 2 ]; then VM="$2"; shift 2; fi
SELECTED="$(jq -c --arg n "$VM" 'first(.[] | select(.name == $n)) // empty' <<<"$MVL_ALL")"
[ -n "$SELECTED" ] || { log "VM is not configured"; exit 1; }
TART="$(jq -r .tart <<<"$SELECTED")"
SSH="$(jq -r .ssh <<<"$SELECTED")"

release() {
  if [ -n "$LEASE" ] && host_lease_lock "$LEASE"; then
    [ "$(jq -r .pid "$LEASE/holder.json" 2>/dev/null)" != "$$" ] || host_lease_drop "$LEASE"
    host_lease_unlock
  fi
}

headless() {
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
    log "preparation failed; restoring normal headless service"
    if headless; then log "$VM restored headless"; else
      log "headless restoration failed; inspect private $STATE logs; restore with the configured Tart wrapper"
      rc=1
    fi
  fi
  release
  exit "$rc"
}

lease() {
  local dir start
  dir="$(mvl_lease_dir "$ROOT" "$VM")"
  mkdir -p "$(dirname "$dir")" || exit 1
  start="$(mvl_epoch)"
  until mkdir "$dir" 2>/dev/null; do
    if [ $(($(mvl_epoch) - start)) -ge "${MEGURI_MAC_VM_LEASE_WAIT_SECONDS:-600}" ]; then
      log "the lease of $VM stayed busy"; exit 3
    fi
    sleep 1
  done
  LEASE="$dir"
  trap finish EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM
  jq -nc --argjson pid $$ --arg wt "$ROOT" --arg job "$1" \
    '{pid: $pid, worktree: $wt, job: $job, holder: "duo-fold"}' >"$LEASE/holder.json" || exit 1
}

# Scheduler excludes acquisitions/jobs; the guest pool mutex also excludes
# legacy renewals from final inventory through shutdown initiation.
shutdown_guest() {
  mvl_tests_idle "$SSH" || { log "guest tests running or unknown; wait for jobs to finish"; return 1; }
  python3 "$SESSION" shutdown "$STATE" "$SSH" "$TART" "$VM" || return 1
  [ "$(mvl_state "$TART" "$VM")" = stopped ] || {
    log "VM shutdown unconfirmed; evidence retained"; return 1;
  }
}

send() {
  [ "$(session_status | jq -r .connected)" = true ] ||
    { log "existing holder unavailable"; exit 1; }
  printf '%s\n' "$1" >"$FIFO"
}

session_status() { python3 "$SESSION" status "$STATE" "$VM" "$TART"; }

start() {
  local host current
  [ $# -eq 0 ] || usage
  lease vm-fold-prepare
  mkdir -p "$STATE" || exit 1
  current="$(session_status)" || exit 1
  if [ "$(jq -r .connected <<<"$current")" = true ]; then
    log "connected to $VM (reused)"; return 0
  fi
  # Never replace an existing or incomplete session on a regular prepare call.
  if [ -e "$STATE/vm" ] || [ -e "$STATE/commands" ] || [ -e "$STATE/holder.pid" ] || [ -e "$STATE/tart.pid" ] || [ -e "$STATE/tunnel.pid" ] || [ -e "$STATE/owner" ] || [ -e "$STATE/control.sock" ] || [ -e "$STATE/recovery-pending" ]; then
    log "existing session is unavailable; run duo-fold.sh stop once drained, then prepare"; exit 1
  fi
  chmod 700 "$STATE" || exit 1
  host="$(python3 "$SESSION" host "$TART")" || exit 1
  if [ -n "$host" ]; then
    [ -x "$STATE/venv/bin/python" ] && "$STATE/venv/bin/python" -I -c 'from vncdotool import api' >"$STATE/dependency.log" 2>&1 || {
      log "existing holder venv missing/unusable at $STATE/venv; restore it on the coordinator before preparing"; exit 1;
    }
  else
    [ -x "$STATE/venv/bin/python" ] || {
      python3 -m venv "$STATE/venv" && "$STATE/venv/bin/pip" install --quiet "vncdotool==$VNCDOTOOL_VERSION"
    } || { log "could not install vncdotool"; exit 1; }
  fi
  # Set before stop: transport failure may mean that stop reached the host.
  RECOVER=stopping
  shutdown_guest || exit 1
  RECOVER=1
  python3 "$SESSION" start "$STATE" "$TART" "$VM" || exit 1
  mvl_wait_ready "$SSH" "$TART" "$VM" true "$STATE/tart.log" "$STATE/guest.ssh" >"$STATE/readiness.log" 2>&1 || {
    log "VNC guest did not become accessible; inspect private $STATE logs"; exit 1;
  }
  python3 "$SESSION" clean-drain "$STATE" "$SSH" || exit 1
  RECOVER=""
  rm -f "$STATE/recovery-pending"
  log "connected to $VM"
}

stop() {
  [ $# -eq 0 ] || usage
  lease vm-fold-stop
  mkdir -p "$STATE" || exit 1
  if [ -e "$STATE/vm" ] && [ "$(cat "$STATE/vm")" != "$VM" ]; then
    log "session belongs to another VM; select it with --vm"; exit 1
  fi
  # One recovery attempt only; an uncertain result keeps all session evidence.
  headless || exit 1
  log "$VM is back in plain headless mode"
}

hub() {
  [ -n "${1:-}" ] || usage
  "$SSH" "open -a /Applications/Xcode.app/Contents/Applications/DeviceHub.app; xcrun simctl boot $(mvl_q "$1") 2>/dev/null; xcrun simctl bootstatus $(mvl_q "$1") -b >/dev/null" </dev/null
}

set_fold() {
  local x y="${DUO_FOLD_Y:-1889}"
  case "${1:-}" in
    closed) x="${DUO_FOLD_CLOSED_X:-2245}" ;;
    half) x="${DUO_FOLD_HALF_X:-2313}" ;;
    open) x="${DUO_FOLD_OPEN_X:-2381}" ;;
    *) usage ;;
  esac
  send "move $x $y"
  send click
  sleep 6
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

# Hardware pilot only. Never calls start, headless, boot, reset or a controller.
# Keep the scheduler lease through guest validation, launch and capture join.
inspect() {
  [ $# -eq 1 ] || usage
  lease vm-fold-inspect
  python3 "$ROOT/tools/vm/sim-posture-inspect.py" "$STATE" "$SSH" "$VM" "$ROOT" "$1" "$TART"
}

case "$COMMAND" in
  start|prepare) start "$@" ;;
  status) [ $# -eq 0 ] || usage; session_status ;;
  inspect) inspect "$@" ;;
  stop) stop "$@" ;;
  hub) hub "${1:-}" ;;
  fold) set_fold "${1:-}" ;;
  click)
    [ $# -eq 2 ] || usage
    send "move $1 $2"
    sleep 0.4
    send click
    ;;
  screen) screen "${1:-}" ;;
  shot) shot "$@" ;;
  *) usage ;;
esac
