# Explicit dedicated-VM maintenance; never app acceptance or signed execution.
mg_job vm-xcode-stage host mg_vm_xcode_stage \
  "Stage the complete selected host Xcode in the running dedicated VM; activate only with existing prompt-free readiness and sudo permission"

mg_vm_xcode_join() {
  local remaining=120
  # Cleanup cannot be interrupted into releasing an unconfirmed guest operation.
  trap '' TERM HUP INT
  if [ -n "${MG_VM_XCODE_WORKER:-}" ]; then
    kill -TERM "$MG_VM_XCODE_WORKER" 2>/dev/null
    while kill -0 "$MG_VM_XCODE_WORKER" 2>/dev/null && [ "$remaining" -gt 0 ]; do
      sleep 1
      remaining=$((remaining - 1))
    done
    if kill -0 "$MG_VM_XCODE_WORKER" 2>/dev/null; then
      echo "vm-xcode-stage: original worker shutdown unconfirmed; retaining VM lease and private evidence" >&2
      exit 1
    fi
    wait "$MG_VM_XCODE_WORKER" 2>/dev/null
    MG_VM_XCODE_WORKER=""
    MG_VM_XCODE_FINISHED=1
  fi
  if [ "${MG_VM_XCODE_STARTED:-}" = 1 ] && [ "${MG_VM_XCODE_FINISHED:-}" != 1 ]; then
    echo "vm-xcode-stage: original worker launch/join uncertain; retaining VM lease" >&2
    exit 1
  fi
  if { [ "${MG_VM_XCODE_STARTED:-}" = 1 ] || [ -e "$private/receiver-pending.json" ]; } &&
    [ ! -f "$private/lease-safe.json" ]; then
    # Exit this private subshell/EXIT trap before the later lease-release action.
    # SSH exit and an earlier receiver join cannot prove activation stopped.
    echo "vm-xcode-stage: original guest operation stop unconfirmed; retaining VM lease and private evidence" >&2
    exit 1
  fi
  # Remove only our empty guard after original worker/guest completion is
  # confirmed (or before the worker started). Unknown state survives holder death.
  if [ "${MG_VM_XCODE_GUARD:-}" = 1 ]; then
    rmdir "$MG_LEASE_DIR/vm-xcode-pending" 2>/dev/null || :
  fi
  return 0
}

mg_vm_xcode_stop_exclusive() {
  # A recording may offer a nested slot. Maintenance must not drain that
  # other holder's runs, even when both jobs belong to this checkout.
  if [ "$MG_LEASE_SLOT" != "$MG_LEASE_DIR" ]; then
    MG_MAC_VM_STOP_REASON="Xcode maintenance requires an exclusive VM lease"
    MG_MAC_VM_STOP_UNREACHABLE=""
    return 1
  fi
  local rc=0
  mvl_stop_tests "$(jq -r .ssh <<<"$MG_MAC_VM")" || rc=1
  MG_MAC_VM_STOPPED="$MVL_STOPPED"
  MG_MAC_VM_STOP_REASON="$MVL_REASON"
  MG_MAC_VM_STOP_UNREACHABLE="$MVL_UNREACHABLE"
  return "$rc"
}

mg_vm_xcode_stage_private() (
  local private="$1" report="$2" rc
  MG_EXIT_CMDS=()
  MG_STATE="$private/state"
  MG_RUN_DIR="$private"
  mkdir -m 700 "$MG_STATE" || exit 1
  mg_mac_vm_probe
  rc=$?
  case "$rc" in 0) ;; 1) exit 75 ;; *) exit 1 ;; esac
  # The dedicated pool VM only. Never choose a second free VM as fallback.
  MG_MAC_VMS="$(jq -c --argjson dedicated "$(jq -c '.[0]' <<<"$MVL_ALL")" \
    '[.[] | select(.name == $dedicated.name and .ssh == $dedicated.ssh and .tart == $dedicated.tart)]' \
    <<<"$MG_MAC_VMS")" || exit 1
  [ "$(jq length <<<"$MG_MAC_VMS")" -eq 1 ] || exit 75
  MG_SIM_RUNNER=vm MG_MAC_RUNNER=vm
  # This override exists only in the private subshell, before take's drain.
  mg__mac_vm_stop_tests() { mg_vm_xcode_stop_exclusive; }
  mg_mac_vm_take --no-copy --running-only
  rc=$?
  case "$rc" in 0) ;; 2) exit 75 ;; *) exit 1 ;; esac
  mvl_lease_held "$MG_LEASE_DIR" || exit 1
  mg_on_exit mg_vm_xcode_join
  mkdir -m 700 "$MG_LEASE_DIR/vm-xcode-pending" || exit 1
  MG_VM_XCODE_GUARD=1
  jq -nc --argjson selection "$MG_MAC_VM" --arg slot "$MG_LEASE_SLOT" \
    --arg root "$MG_ROOT" --arg run "$MG_RUN_ID" --arg private "$private" \
    '{selection:$selection,lease_slot:$slot,root:$root,run:$run,private:$private}' >"$private/context.json" || exit 1
  # Async wait is intentional: cancellation cleanup joins this ORIGINAL child
  # before the take's existing orphan cleanup and lease release (LIFO).
  MG_VM_XCODE_STARTED=1 MG_VM_XCODE_FINISHED=""
  python3 -B -I "$MG_TOOLS/scripts/vm-xcode-stage.py" \
    3<"$private/context.json" >"$report" &
  MG_VM_XCODE_WORKER=$!
  printf '%s\n' "$MG_VM_XCODE_WORKER" >"$private/worker.pid"
  wait "$MG_VM_XCODE_WORKER"
  rc=$?
  MG_VM_XCODE_FINISHED=1
  MG_VM_XCODE_WORKER=""
  mvl_lease_held "$MG_LEASE_DIR" || exit 1
  exit "$rc"
)

mg_vm_xcode_check_source() {
  local private temp_base report="$MG_RUN_DIR/stage-report.json" rc=0
  temp_base="$(CDPATH= cd -- "${TMPDIR:-/tmp}" 2>/dev/null && pwd -P 2>/dev/null)" || return 1
  private="$(mktemp -d "${temp_base%/}/meguri-vm-xcode.XXXXXX" 2>/dev/null)" || return 1
  chmod 700 "$private" 2>/dev/null || return 1
  # No probe, lease, receiver or guest context in this diagnostic path.
  (
    umask 077
    exec >"$private/helpers.log" 2>&1
    jq -nc --arg private "$private" '{private:$private}' >"$private/context.json" || exit 1
    python3 -B -I "$MG_TOOLS/scripts/vm-xcode-stage.py" --check-source \
      3<"$private/context.json" >"$report"
  ) || rc=$?
  if [ ! -s "$report" ]; then
    jq -nc '{scope:"SOURCE-CHECK-ONLY",outcome:"refused",checks:{}}' >"$report"
  fi
  mg_artifact json "$report" "SOURCE-CHECK-ONLY; selected host source signature and facts"
  if [ "$rc" -eq 0 ] && jq -e \
    '.scope == "SOURCE-CHECK-ONLY" and .outcome == "source-checked"' "$report" >/dev/null 2>&1; then
    mg_message "vm-xcode-stage: SOURCE-CHECK-ONLY; selected host source check completed"
    mg_status pass
  else
    mg_failure vm-xcode-source "SOURCE-CHECK-ONLY; selected host source check failed"
    mg_status error
  fi
}

mg_vm_xcode_stage() {
  local private temp_base report="$MG_RUN_DIR/stage-report.json" rc=0 outcome
  if [ "$#" -eq 1 ] && [ "$1" = --check-source ]; then
    mg_vm_xcode_check_source
    return $?
  fi
  if [ "$#" -ne 0 ]; then
    mg_failure usage "usage: tools/mg vm-xcode-stage [--check-source]"
    return 1
  fi
  # Resolve only the OS temporary base, before creating our owned directory.
  # Directory admission stays nofollow for every subsequent candidate path.
  temp_base="$(CDPATH= cd -- "${TMPDIR:-/tmp}" && pwd -P)" || return 1
  private="$(mktemp -d "${temp_base%/}/meguri-vm-xcode.XXXXXX")" || return 1
  chmod 700 "$private" || return 1
  # Retain private diagnostics, never export them or delete a substituted path.
  (umask 077; mg_vm_xcode_stage_private "$private" "$report") >"$private/helpers.log" 2>&1 || rc=$?
  if [ ! -s "$report" ]; then
    jq -nc --arg outcome "$([ "$rc" -eq 75 ] && echo queued || echo refused)" \
      '{scope:"VM-XCODE-MAINTENANCE-ONLY",outcome:$outcome,checks:{}}' >"$report"
  fi
  outcome="$(jq -r .outcome "$report")"
  mg_artifact json "$report" "Xcode maintenance only; no SDK execution or app acceptance"
  case "$rc:$outcome" in
    0:activated|0:already-selected)
      mg_message "vm-xcode-stage: verified staged Xcode and selected version/build match"
      mg_status pass ;;
    75:queued|75:staged)
      mg_user_step mac-vm missing true "Finish Xcode activation in the dedicated VM" \
        "In the VM, inspect ~/Applications/MeguriXcode/Xcode-<version>-<build>.app using the version/build in stage-report.json. Complete any Apple license, first-launch or administrator action yourself for that bundle. In the VM only, select it with: sudo xcode-select --switch ~/Applications/MeguriXcode/Xcode-<version>-<build>.app/Contents/Developer (substitute the report version/build). Alternatively rerun tools/mg vm-xcode-stage after readiness and prompt-free sudo permission already exist. Do not rerun setup-guest.sh. Staging proves no activation, SDK execution or app acceptance."
      mg_queued_part mac-vm "VM Xcode activation" "VM unavailable or activation requires a user-owned step"
      mg_message "vm-xcode-stage: queued; staging alone proves no activation or app acceptance"
      mg_status warn ;;
    *)
      mg_failure vm-xcode-stage "Xcode maintenance refused; no version/build match is claimed"
      mg_status error ;;
  esac
}
