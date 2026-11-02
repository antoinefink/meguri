# Explicit read-only retained diagnostic utility. Never an acceptance lane.
mg_job vm-crash-info host mg_vm_crash_info \
  "Read bounded xcodebuild crash taxonomy for an admitted original Mac UI run and suite; diagnostic only"

mg_vm_crash_join() {
  # Reuse only the existing original-worker join/fence cleanup. The guard's
  # existing name is recognized by stale-holder and mutex-protected release;
  # it is not permission to run Xcode, drain jobs or perform maintenance.
  local MG_VM_XCODE_WORKER="${MG_VM_CRASH_WORKER:-}"
  local MG_VM_XCODE_STARTED="${MG_VM_CRASH_STARTED:-}"
  local MG_VM_XCODE_FINISHED="${MG_VM_CRASH_FINISHED:-}"
  local MG_VM_XCODE_GUARD="${MG_VM_CRASH_GUARD:-}"
  mg_vm_xcode_join
}

mg_vm_crash_private() (
  local private="$1" report="$2" run="$3" suite="$4" rc
  MG_EXIT_CMDS=()
  MG_STATE="$private/state"
  MG_RUN_DIR="$private"
  mkdir -m 700 "$MG_STATE" || exit 1
  # All admission and pinning precede config, lease and guest access.
  python3 -B -I "$MG_TOOLS/scripts/vm-crash-info.py" --admit "$MG_ROOT" "$run" "$suite" \
    >"$private/admission.json" || exit 1
  mvl_read_config || exit 75
  MG_MAC_VMS="$(jq -c '[.[0]]' <<<"$MVL_ALL")" || exit 1
  [ "$(jq -r '.[0].name' <<<"$MG_MAC_VMS")" = "$(jq -r .vm "$private/admission.json")" ] || exit 1
  # Existing lease-only seam: no take/start/readiness/copy/drain helpers.
  MEGURI_MAC_VM_LEASE_WAIT_SECONDS=0
  mg__mac_vm_lease || exit 75
  [ "$MG_LEASE_SLOT" = "$MG_LEASE_DIR" ] || exit 75
  mvl_lease_held "$MG_LEASE_DIR" || exit 1
  [ "$(mvl_state "$(jq -r .tart <<<"$MG_MAC_VM")" "$(jq -r .name <<<"$MG_MAC_VM")")" = running ] || exit 75
  MG_VM_CRASH_STARTED="" MG_VM_CRASH_FINISHED="" MG_VM_CRASH_WORKER="" MG_VM_CRASH_GUARD=""
  mg_on_exit mg_vm_crash_join
  mkdir -m 700 "$MG_LEASE_DIR/vm-xcode-pending" || exit 1
  MG_VM_CRASH_GUARD=1
  jq -nc --slurpfile admission "$private/admission.json" --argjson selection "$MG_MAC_VM" \
    --arg private "$private" \
    '{admission:$admission[0],selection:$selection,private:$private}' >"$private/context.json" || exit 1
  MG_VM_CRASH_STARTED=1
  python3 -B -I "$MG_TOOLS/scripts/vm-crash-info.py" --collect "$MG_ROOT" "$run" "$suite" \
    3<"$private/context.json" >"$report" &
  MG_VM_CRASH_WORKER=$!
  wait "$MG_VM_CRASH_WORKER"
  rc=$?
  MG_VM_CRASH_FINISHED=1 MG_VM_CRASH_WORKER=""
  mvl_lease_held "$MG_LEASE_DIR" || exit 1
  exit "$rc"
)

mg_vm_crash_info() {
  local run suite private base report="$MG_RUN_DIR/crash-info.json" rc=0 category
  if [ "$#" -ne 4 ] || [ "$1" != --mac-ui-run ] || [ "$3" != --suite ]; then
    mg_failure usage "usage: tools/mg vm-crash-info --mac-ui-run <run-id> --suite <drive-suite>"
    mg_status error; return 0
  fi
  run="$2" suite="$4"
  if ! printf '%s\n' "$run" | LC_ALL=C grep -Eq '^[0-9]{8}T[0-9]{6}Z-[0-9]+$' ||
     ! printf '%s\n' "$suite" | LC_ALL=C grep -Eq '^drive-[a-z0-9]+(-[a-z0-9]+)*$'; then
    mg_failure input "Invalid retained run or suite identifier"
    mg_status error; return 0
  fi
  base="$(cd -P "${TMPDIR:-/tmp}" && pwd -P)" || return 1
  private="$(mktemp -d "$base/meguri-vm-crash.XXXXXX")" || return 1
  chmod 700 "$private" || return 1
  # Transport/config/lease paths and helper messages stay private. Retain evidence.
  (umask 077; mg_vm_crash_private "$private" "$report" "$run" "$suite") \
    >"$private/helpers.log" 2>&1 || rc=$?
  if [ ! -s "$report" ]; then
    category=source
    [ "$rc" -ne 75 ] || category=vm-unavailable
    jq -nc --arg run "$run" --arg suite "$suite" --arg category "$category" \
      '{scope:"MAC-UI-CRASH-DIAGNOSTIC-ONLY",source_job:"mac-ui",source:"original-result-and-suite-log",run:$run,suite:$suite,outcome:"refused",category:$category,counts:{candidates:0,matched:0,pid_mismatch:0,time_mismatch:0,link:0,oversize:0,corrupt:0,changed:0,limit:0},reports:[]}' >"$report"
  fi
  mg_artifact json "$report" "Bounded crash taxonomy only; no app or gate acceptance"
  mg_detail scope '"MAC-UI-CRASH-DIAGNOSTIC-ONLY"'
  mg_message "vm-crash-info: diagnostic only; no crash cause, repair or acceptance claim"
  # Even readable/absent reports are never a passing app/gate result.
  if [ "$rc" -eq 0 ]; then
    mg_status warn
  else
    mg_failure diagnostic-refused "Diagnostic refused or unavailable; inspect fixed report category"
    mg_status error
  fi
}
