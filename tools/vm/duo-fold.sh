#!/bin/bash
# tools/vm/duo-fold.sh: changes the iPhone Duo's fold position in the macOS test
# VM. Only Device Hub's buttons can (not simctl, not XCUITest), so this gives the
# headless VM a screen and input over Tart's VNC server.
#
# usage: tools/vm/duo-fold.sh start [--vm <name>]   restart the VM with VNC, connect
#        tools/vm/duo-fold.sh hub <udid>            open Device Hub, boot the simulator
#        tools/vm/duo-fold.sh fold closed|half|open click Device Hub's fold button
#        tools/vm/duo-fold.sh click <x> <y>         click a point of the VM's screen
#        tools/vm/duo-fold.sh screen <file.png>     save the VM's whole screen
#        tools/vm/duo-fold.sh shot <udid> <display> <file.png>   1 is outer, 3 is inner
#        tools/vm/duo-fold.sh stop                  disconnect, restart the VM headless
#
# Get the simulator with `tools/mg sim acquire foldable` after `start`: restarting
# the VM shuts its simulators down and voids their leases. `start` and `stop` wait
# for the VM's lease and restart only that VM, so run them with no job you care
# about in it. Between them, tools/mg jobs still work. Only this script's one
# connection may reach VNC: reconnecting crashes Tart and takes running jobs with
# it (docs/process/mac-ui-vm.md).
#
# The fold buttons are screen positions, right for Device Hub's window where it
# opened on 2026-11-01. Check with `screen`, and set DUO_FOLD_Y and
# DUO_FOLD_CLOSED_X, DUO_FOLD_HALF_X and DUO_FOLD_OPEN_X if the window moved.
# `start` and `stop` also take the VM's claim before the restart and refuse
# while a job of another Mac holds it. A VM that runs but does not answer is
# not restarted unless DUO_FOLD_FORCE=1.
# Exit codes: 0 done, 1 failed, 2 usage, 3 the lease stayed busy.

set -u

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
# shellcheck source=/dev/null
. "$ROOT/tools/scripts/mac-vm-lib.sh"

MAIN="$(dirname "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)")"
STATE="$MAIN/build/vm-fold"
FIFO="$STATE/commands"
VNCDOTOOL_VERSION=1.4.2
CONFIG="${MEGURI_MAC_VM_CONFIG:-$HOME/Library/Application Support/meguri/mac-vm.json}"

log() { printf 'duo-fold: %s\n' "$*" >&2; }
usage() { sed -n '7,13p' "$0" | sed 's/^# *//' >&2; exit 2; }

[ -f "$CONFIG" ] || { log "no VM config at $CONFIG"; exit 1; }
TART="$(jq -r .tart "$CONFIG")"
SSH="$(jq -r .ssh "$CONFIG")"
VM="$(jq -r '.vms[0] | if type == "object" then .name else . end' "$CONFIG")"

lease() {
  local i
  LEASE="$MAIN/build/mg/mac-vm.$VM.lock"
  for i in $(seq 1 120); do
    if mkdir "$LEASE" 2>/dev/null; then
      jq -nc --argjson pid $$ --arg job "$1" '{pid: $pid, job: $job, holder: "duo-fold"}' >"$LEASE/holder.json"
      trap 'rm -rf "$LEASE"' EXIT
      return 0
    fi
    sleep 5
  done
  log "the lease of $VM stayed busy"
  cat "$LEASE/holder.json" >&2
  exit 3
}

send() {
  [ -p "$FIFO" ] && [ -f "$STATE/holder.pid" ] && kill -0 "$(cat "$STATE/holder.pid")" 2>/dev/null ||
    { log "not connected: run start first"; exit 1; }
  printf '%s\n' "$1" >"$FIFO"
}

wait_for_vm() {
  mvl_wait_ready "$SSH" "$TART" "$VM" true "$1" "$1.ssh" ||
    { log "$VM did not answer: ${MVL_REASON:-unknown}"; exit 1; }
}

# A restart ends every claim on the VM (tools/mg.d/mac-vm.sh), so it takes
# the claim first and keeps it up to the restart: another Mac's job can then
# neither be running in the VM nor start there meanwhile. The keeper holds
# it while this script's lease lasts and takes it again after the restart.
# A VM that Tart does not list as running runs no job and has no claim. One
# that runs but does not answer has an unknown claim: no restart, unless
# DUO_FOLD_FORCE=1 says the VM is to be recovered whatever runs in it.
claim_take() {
  local state="$LEASE.claim" token="duo-fold.$$" record st="" i
  [ "${MEGURI_MAC_VM_SHARED_CLAIM:-1}" != 0 ] || return 0
  [ "$(mvl_state "$TART" "$VM")" = running ] || return 0
  # A keeper of the job that just gave up the lease is leaving.
  for i in $(seq 1 40); do [ -e "$state" ] || break; sleep 0.5; done
  rm -f "$state.nudge"
  (
    cd / && exec perl -MPOSIX=setsid -e 'fork and exit 0; setsid(); exec @ARGV or die "exec $ARGV[0]: $!\n"' \
      /usr/bin/env python3 "$ROOT/tools/scripts/vm-claim.py" --ssh "$SSH" --lease "$LEASE" --state "$state" --token "$token" \
      --holder "$(jq -nc --arg m "$(hostname -s 2>/dev/null || hostname)" --arg wt "$ROOT" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        '{machine: $m, job: "duo-fold", worktree: $wt, started_at: $at}')" \
      --path "${MEGURI_MAC_VM_CLAIM_PATH:-/tmp/meguri-vm-claim.lock}" \
      </dev/null >>"$STATE/claim.log" 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
  )
  for i in $(seq 1 180); do
    record="$(cat "$state" 2>/dev/null)"
    if [ "$(jq -r '.token // empty' <<<"$record" 2>/dev/null)" = "$token" ]; then
      st="$(jq -r '.state // empty' <<<"$record" 2>/dev/null)"
      [ "$st" = starting ] || break
    fi
    sleep 0.5
  done
  case "$st" in
    taken) return 0 ;;
    busy)
      log "$VM is in use by another Mac's job ($(jq -r '.holder // "" | (fromjson? // {}) | "\(.job // "a job") on \(.machine // "?") in \(.worktree // "?") since \(.started_at // "?")"' <<<"$record" 2>/dev/null || echo unknown)); not restarting it"
      exit 1
      ;;
  esac
  if [ "${DUO_FOLD_FORCE:-}" = 1 ]; then
    log "$VM did not answer, so whether a job holds it is unknown; restarting it anyway (DUO_FOLD_FORCE=1)"
    return 0
  fi
  log "$VM runs but did not answer, so whether another Mac's job holds it is unknown; not restarting it (DUO_FOLD_FORCE=1 restarts it anyway)"
  exit 1
}

start() {
  local url i
  [ "${1:-}" = --vm ] && [ -n "${2:-}" ] && VM="$2"
  mkdir -p "$STATE"
  lease vm-fold-start
  [ -x "$STATE/venv/bin/python" ] || {
    python3 -m venv "$STATE/venv" &&
      "$STATE/venv/bin/pip" install --quiet "vncdotool==$VNCDOTOOL_VERSION"
  } || { log "could not install vncdotool"; exit 1; }
  claim_take
  "$TART" stop "$VM" --timeout 60 2>/dev/null
  : >"$STATE/tart.log"
  (
    cd / && exec perl -MPOSIX=setsid -e 'fork and exit 0; setsid(); exec @ARGV or die "exec $ARGV[0]: $!\n"' \
      "$TART" run --no-graphics --vnc-experimental "$VM" </dev/null >"$STATE/tart.log" 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
  )
  wait_for_vm "$STATE/tart.log"
  url="$(grep -o 'vnc://[^ ]*' "$STATE/tart.log" | tail -1)"
  [ -n "$url" ] || { log "Tart printed no VNC address"; exit 1; }
  rm -f "$FIFO"
  mkfifo "$FIFO"
  # The address holds the session's password: pass it in the environment, not as an argument.
  (
    DUO_FOLD_VNC_URL="$url" exec perl -MPOSIX=setsid -e 'fork and exit 0; setsid(); exec @ARGV or die "exec $ARGV[0]: $!\n"' \
      "$STATE/venv/bin/python" -I "$ROOT/tools/vm/vnc-holder.py" "$FIFO" "$STATE/holder.pid" </dev/null >"$STATE/holder.log" 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
  )
  for i in $(seq 1 30); do
    grep -q '^connected' "$STATE/holder.log" 2>/dev/null && { log "connected to $VM"; return 0; }
    sleep 1
  done
  log "could not connect:"
  tail -3 "$STATE/holder.log" >&2
  exit 1
}

stop() {
  if [ -p "$FIFO" ] && [ -f "$STATE/holder.pid" ] && kill -0 "$(cat "$STATE/holder.pid")" 2>/dev/null; then
    printf 'quit\n' >"$FIFO"
    sleep 2
  fi
  rm -f "$FIFO" "$STATE/holder.pid"
  mkdir -p "$STATE"
  lease vm-fold-stop
  claim_take
  "$TART" stop "$VM" --timeout 60 || exit 1
  : >"$STATE/tart.log"
  mvl_start "$TART" "$VM" "$STATE/tart.log"
  wait_for_vm "$STATE/tart.log"
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

case "${1:-}" in
  start) shift; start "$@" ;;
  stop) stop ;;
  hub) hub "${2:-}" ;;
  fold) set_fold "${2:-}" ;;
  click)
    [ $# -eq 3 ] || usage
    send "move $2 $3"
    sleep 0.4
    send click
    ;;
  screen) screen "${2:-}" ;;
  shot) shift; shot "$@" ;;
  *) usage ;;
esac
