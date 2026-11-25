#!/bin/bash
# tools/vm/duo-fold.sh: changes the iPhone Duo's fold position in the macOS test
# VM. Only Device Hub's buttons can (not simctl, not XCUITest), so this gives the
# headless VM a screen and input over Tart's VNC server.
#
# usage: tools/vm/duo-fold.sh start [--vm <name>]   prepare/reuse VNC (manual alias)
#        tools/vm/duo-fold.sh prepare|status      persistent session (no repeated restart)
#        tools/vm/duo-fold.sh maintenance-prepare  operator declares BOTH schedulers paused/drained
#        tools/vm/duo-fold.sh deploy|preflight    private hosted component readiness
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
# Use master's claim admission/keeper, with only its reporting hooks adapted.
MG_TOOLS="$ROOT/tools"
. "$MG_TOOLS/mg.d/mac-vm.sh"
mg__mac_vm_detail() { :; }
mg__mac_vm_fail() { log "$2"; }
mg__mac_vm_unreachable() { log "$1; no guest work admitted"; }

MAIN="$(dirname "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)")"
CANONICAL="$MAIN/build/vm-fold"
STATE="$CANONICAL"
VENV="$CANONICAL/venv"
VNCDOTOOL_VERSION=1.4.2

log() { printf 'duo-fold: %s\n' "$*" >&2; }
usage() { sed -n '6,14p' "$0" | sed 's/^# *//' >&2; exit 2; }

SESSION="$ROOT/tools/vm/vnc-session.py"
HOSTED="$ROOT/tools/vm/vnc-hosted.py"
BOUNDARY="$ROOT/tools/vm/fold-claim.py"
RECOVER="" LEASE="" LEASE_WRITING="" PRIVATE_RUN_DIR="" DRAIN_BEFORE=""

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
case "$COMMAND" in start|prepare|maintenance-prepare|stop|inspect|open|closed|calibrate|deploy|preflight)
  [ "$VM" = "$PRIMARY" ] || { log "VNC lifecycle supports only configured primary VM $PRIMARY"; exit 1; }
esac

# The deployed native kit is a resident, not another folding scheduler. A
# standalone development Tart/config without that kit remains supported.
case "$COMMAND" in status|receipt|preflight) ;;
  *)
    if [ ! -e "$(dirname "$TART")/remote.json" ] &&
       [ -e "$(dirname "$TART")/../vnc-owner-v1" ]; then
      log "fold requests run on M5 through its remote wrappers; this M3 kit already owns the persistent VNC client. M3 may schedule ordinary MG jobs"
      exit 1
    fi ;;
esac

select_state() {
  STATE="$(python3 "$SESSION" route "$CANONICAL" "$VM" "$TART")" || exit 1
  python3 "$SESSION" primary "$STATE" "$VM" || exit 1
  FIFO="$STATE/commands"
}

recheck_state() {
  local selected
  selected="$(python3 "$SESSION" route "$CANONICAL" "$VM" "$TART")" || return 1
  [ "$selected" = "$STATE" ] || { log "session route changed; retain evidence"; return 1; }
  python3 "$SESSION" primary "$STATE" "$VM" || return 1
}

drain_markers() {
  python3 - "$STATE" <<'PY'
import os, sys
records = []
for name in ("drain-precommit", "shutdown-pending"):
    try:
        info = os.lstat(os.path.join(sys.argv[1], name))
    except FileNotFoundError:
        continue
    records.append((name, info.st_dev, info.st_ino, info.st_mtime_ns, info.st_ctime_ns))
print(records)
PY
}

release() {
  if [ -n "$LEASE" ] && host_lease_lock "$LEASE"; then
    if [ "$LEASE_WRITING" = 1 ] || [ "$(jq -r .pid "$LEASE/holder.json" 2>/dev/null)" = "$$" ]; then
      # Cover an unjoined drain before Python arms its stop fence, but never
      # promote unchanged evidence that this invocation only queried/refused.
      if [ -n "$DRAIN_BEFORE" ] && [ "$(drain_markers)" != "$DRAIN_BEFORE" ] &&
         { [ -e "$STATE/drain-precommit" ] || [ -e "$STATE/shutdown-pending" ]; }; then
        if mkdir "$LEASE/vm-shutdown-pending" 2>/dev/null; then
          jq -nc --arg state "$STATE" '{state: $state, reason: "unconfirmed fold drain/stop"}' \
            >"$LEASE/vm-shutdown-pending/intent.json"
        fi
      fi
      host_lease_drop "$LEASE"
    fi
    host_lease_unlock
  fi
  [ -z "$LEASE" ] || mg__mac_vm_claim_nudge "$LEASE.claim"
}

claim_check() { python3 "$BOUNDARY" check "$LEASE" "$SSH"; }

maintenance_instruction() {
  log "resident preparation requires scheduled maintenance: an operator must suspend dispatch on BOTH M5 and M3 and drain outstanding jobs/leases, then invoke tools/mg sim-posture maintenance-prepare. MG cannot detect that administrative pause; retain all fences"
}

initial_claim() {
  local current
  if [ "$STATE" = "$MAIN/build/vmh3" ]; then
    if [ "$COMMAND" = maintenance-prepare ]; then
      # Host-only evidence may admit this exact stopped reservation. There is
      # no guest to claim yet; every running/unknown guest takes normal admission.
      if [ "$(mvl_state "$TART" "$VM")" = stopped ]; then
        python3 -I "$BOUNDARY" resume-check "$LEASE" "$SSH" "$STATE" "$TART" "$VM"
        return $?
      fi
    elif [ "$COMMAND" = prepare ] || [ "$COMMAND" = start ]; then
      current="$(session_status)" || return 1
      if [ "$(jq -r .connected <<<"$current")" != true ]; then
        maintenance_instruction
        return 1
      fi
    fi
  fi
  claim_take
}

claim_take() {
  # Never honor the general opt-out for folding: every guest access needs the
  # same guest-wide claim, even for a connected resident's cleanup/inspection.
  MG_LEASE_DIR="$LEASE" MG_MAC_VM="$SELECTED" MG_ROOT="$ROOT"
  if [ -z "${MG_RUN_DIR:-}" ]; then
    PRIVATE_RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/meguri-fold.XXXXXX")" || return 1
    MG_RUN_DIR="$PRIVATE_RUN_DIR"
  fi
  MG_RUN_ID="${MG_RUN_ID:-fold.$$}" MG_JOB="${MG_JOB:-$COMMAND}"
  MEGURI_MAC_VM_SHARED_CLAIM=1
  mkdir -p "$MG_RUN_DIR" || return 1
  mg__mac_vm_claim || return $?
  claim_check
}

fold_python() {
  local interpreter=python3 rc
  if [ "$1" = "$VENV/bin/python" ]; then interpreter="$1"; shift; fi
  "$interpreter" -I "$BOUNDARY" run "$LEASE" "$SSH" "$@" &
  ACTIVE_CHILD=$!
  wait "$ACTIVE_CHILD"; rc=$?
  ACTIVE_CHILD=""
  return "$rc"
}

fold_start() {
  local rc
  if [ "$STATE" = "$MAIN/build/vmh3" ] && [ "$COMMAND" != maintenance-prepare ]; then
    maintenance_instruction; return 1
  fi
  # Starts run while the owned, drained guest is positively stopped. There is
  # no live guest keeper to prove authority then. Track the controller so Bash
  # can forward cancellation while waiting, including before publication.
  # Keep recovery disarmed through cancellation or an unprovable start. Only
  # an ordinary local result followed by fresh admission may rearm it.
  RECOVER=""
  python3 -I "$BOUNDARY" start "$LEASE" "$SSH" "$@" &
  ACTIVE_CHILD=$!
  wait "$ACTIVE_CHILD"; rc=$?
  ACTIVE_CHILD=""
  return "$rc"
}

fold_ssh() { claim_check && "$SSH" "$@"; }

cancel() {
  trap '' HUP INT TERM
  RECOVER=""
  if [ -n "${ACTIVE_CHILD:-}" ]; then
    kill -TERM "$ACTIVE_CHILD" 2>/dev/null || :
    wait "$ACTIVE_CHILD" || :
    ACTIVE_CHILD=""
  fi
  exit "$1"
}

postboot_claim() {
  # The restart kills the claim. Keep the original keeper/lease; it asks again
  # after SSH returns. No readiness read or drain cleanup precedes admission.
  claim_take
}

headless() {
  recheck_state || return 1
  [ ! -e "$STATE/shutdown-pending" ] || {
    log "remote stop completion uncertain; evidence retained; installed wrapper cannot prove recovery"; return 1;
  }
  python3 "$SESSION" owned "$STATE" || return 1
  case "$(mvl_state "$TART" "$VM")" in
    stopped)
      # Before VNC publication our completed drain authorizes restoration.
      # An existing VNC owner must not bypass lost admission just because
      # the guest subsequently stopped.
      [ ! -e "$STATE/owner" ] || claim_check || return 1 ;;
    running) shutdown_guest || return 1 ;;
    *) log "VM shutdown status uncertain; evidence retained"; return 1 ;;
  esac
  python3 "$SESSION" stop "$STATE" || return 1
  mvl_start "$TART" "$VM" "$STATE/headless.log" || return 1
  postboot_claim || return 1
  mvl_wait_ready fold_ssh "$TART" "$VM" true "$STATE/headless.log" "$STATE/headless.ssh" >"$STATE/readiness.log" 2>&1 || return 1
  fold_python "$SESSION" clean-drain "$STATE" "$SSH" || return 1
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
  [ -z "$PRIVATE_RUN_DIR" ] || rm -rf "$PRIVATE_RUN_DIR"
  exit "$rc"
}

lease() {
  local dir start
  dir="$(mvl_lease_dir "$ROOT" "$PRIMARY")"
  mkdir -p "$(dirname "$dir")" || exit 1
  start="$(mvl_epoch)"
  trap finish EXIT
  trap 'cancel 129' HUP
  trap 'cancel 130' INT
  trap 'cancel 143' TERM
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
      export MEGURI_FOLD_SCHEDULER_PID="$$"
      initial_claim || exit 1
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
  claim_check || return 1
  recheck_state || return 1
  mvl_tests_idle fold_ssh || { log "guest tests running or unknown; wait for jobs to finish"; return 1; }
  DRAIN_BEFORE="$(drain_markers)" || return 1
  fold_python "$SESSION" shutdown "$STATE" "$SSH" "$TART" "$VM" "$LEASE" || return 1
  [ "$(mvl_state "$TART" "$VM")" = stopped ] || {
    log "shutdown command completed but stopped state unconfirmed; evidence retained"; return 1;
  }
}

send() {
  lease vm-fold-screen
  claim_check || exit 1
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
  fold_python "$SESSION" poststart-cleanup "$STATE" "$SSH" "$VM" "$LEASE" "$$" "$1"
}


start() {
  local host current rc
  [ $# -eq 0 ] || usage
  if [ "$COMMAND" = maintenance-prepare ]; then
    [ "$STATE" = "$MAIN/build/vmh3" ] || {
      log "maintenance-prepare supports only the hosted M3 route"; return 1;
    }
    log "operator maintenance declaration for this invocation: dispatch on BOTH Macs is suspended and outstanding jobs/leases drained externally; MG cannot verify that pause"
  fi
  lease vm-fold-prepare
  recheck_state || exit 1
  python3 "$SESSION" init "$STATE" || exit 1
  if [ "$STATE" = "$MAIN/build/vmh3" ]; then hosted_start; return $?; fi
  current="$(session_status)" || exit 1
  if [ "$(jq -r .vm <<<"$current")" != null ] && [ "$(jq -r .vm <<<"$current")" != "$VM" ]; then
    log "global VNC state belongs to another VM; retain evidence for operator recovery"; exit 1
  fi
  if [ "$(jq -r .connected <<<"$current")" = true ]; then
    if pending_cleanup; then
      poststart_cleanup check || exit 1
      # This is an owned, already booted session. Never arm rollback, reconnect
      # VNC or issue another stop/start when its cleanup SSH is unavailable.
      claim_check || exit 1
      mvl_wait_ready fold_ssh "$TART" "$VM" false "$STATE/tart.log" "$STATE/guest.ssh" >"$STATE/readiness.log" 2>&1 || {
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
  fold_start "$SESSION" start "$STATE" "$TART" "$VM" "$VENV/bin/python"; rc=$?
  case "$rc" in 0|1) ;; *) exit 1 ;; esac
  # Even a normal VNC failure may have booted the guest. Recover only after
  # bounded admission by the original keeper; lost/unprovable claims retain
  # the transaction. headless still requires owned state and a guarded stop.
  postboot_claim || exit 1
  RECOVER=1
  [ "$rc" = 0 ] || exit 1
  mvl_wait_ready fold_ssh "$TART" "$VM" true "$STATE/tart.log" "$STATE/guest.ssh" >"$STATE/readiness.log" 2>&1 || {
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

hosted_start() {
  local current
  python3 "$HOSTED" bind "$STATE" "$TART" "$VM" || return 1
  current="$(session_status)" || return 1
  if [ "$(jq -r .connected <<<"$current")" = true ]; then
    fold_python "$HOSTED" cleanup "$STATE" "$TART" "$VM" "$SSH" "$LEASE" || return 1
    log "connected to $VM (reused; resident holder)"; return 0
  fi
  [ "$COMMAND" = maintenance-prepare ] || { maintenance_instruction; return 1; }
  if pending_cleanup; then
    [ -f "$STATE/recovery-pending" ] && [ ! -e "$STATE/shutdown-pending" ] && [ ! -e "$STATE/drain-precommit" ] || {
      log "prior drain/stop unconfirmed; retain fences; no restart issued"; return 1;
    }
    # Resume only this stopped/start reservation. Never repeat the drained stop.
  else
    # An uncertain/starting owner is never permission to drain or reconnect.
    if [ "$(jq -r .vacant <<<"$current")" != true ]; then
      log "resident owner unavailable or starting; query status; no restart issued"; return 1
    fi
    python3 "$HOSTED" prepare "$STATE" "$TART" "$VM" "$CANONICAL" admit || return 1
    # No automatic headless rollback: exactly one authorized stop.
    shutdown_guest || return 1
  fi
  fold_start "$HOSTED" prepare "$STATE" "$TART" "$VM" "$CANONICAL" start || return 1
  postboot_claim || return 1
  mvl_wait_ready fold_ssh "$TART" "$VM" false "$STATE/tart.log" "$STATE/guest.ssh" >"$STATE/readiness.log" 2>&1 || {
    log "resident started; guest readiness unconfirmed; retain evidence and query status"; return 1;
  }
  fold_python "$HOSTED" cleanup "$STATE" "$TART" "$VM" "$SSH" "$LEASE" || return 1
  log "connected to $VM (resident holder)"
}

stop() {
  [ $# -eq 0 ] || usage
  lease vm-fold-stop
  recheck_state || exit 1
  [ "$STATE" != "$MAIN/build/vmh3" ] || {
    log "resident holder retained; this route grants no further restart"; return 1;
  }
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
  lease vm-fold-shot
  claim_check || exit 1
  fold_ssh "xcrun simctl io $(mvl_q "$1") screenshot --display=$(mvl_q "$2") /tmp/duo-fold.png >/dev/null 2>&1 && cat /tmp/duo-fold.png" </dev/null >"$3"
}

# Read-only inspection. Never calls start, headless, boot, reset or a controller.
# Keep the scheduler lease through guest validation, launch and capture join.
inspect() {
  [ $# -eq 1 ] || usage
  lease vm-fold-inspect
  recheck_state || exit 1
  fold_python "$ROOT/tools/vm/sim-posture-inspect.py" "$STATE" "$SSH" "$VM" "$ROOT" "$1" "$TART"
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
  fold_python "$VENV/bin/python" "$ROOT/tools/vm/sim-posture.py" \
    "$STATE" "$SSH" "$VM" "$ROOT" "$2" "$TART" "$1" "$LEASE"
}

select_state

case "$COMMAND" in
  deploy|preflight) [ $# -eq 0 ] || usage; python3 "$HOSTED" "$COMMAND" "$TART" "$VM" ;;
  receipt) [ $# -eq 0 ] || usage; python3 "$HOSTED" receipt "$STATE" "$TART" "$VM" "$(mvl_lease_dir "$ROOT" "$PRIMARY")" ;;
  start|prepare|maintenance-prepare) start "$@" ;;
  status) [ $# -eq 0 ] || usage; session_status ;;
  inspect) inspect "$@" ;;
  open|closed|calibrate) posture "$COMMAND" "$@" ;;
  stop) stop "$@" ;;
  screen) screen "${1:-}" ;;
  shot) shot "$@" ;;
  *) usage ;;
esac
