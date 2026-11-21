# Xcode project checks (U7). Sourced by tools/mg; they run on the host
# (xcodebuild, codesign), with the simulator pool in the macOS test VM (U8c).

mg_job xcode-check host mg_xcode_check \
  "Xcode project checks after lint-ui: build settings, zero-warning builds of every configuration (Mac, iPhone and iPad simulators), core stamp, entitlements, arm64, hook code, hidden Mac launches of every route, synced folders, test plans, DriveScriptTests, TestHooksTests (simulators in the macOS test VM, queued without it; on the Mac only with MEGURI_ALLOW_MAC_UI=1, from tools/mg mac-ui); all launches and test runs use the VM's lease; args go to tools/tests/xcode_project_check.sh (--iphone UDID, --ipad UDID, else leased from the simulator pool; --no-ui); --mac-unit-only alone focuses the whole Mac Unit Debug plan in the dedicated VM, without full matrix/gate coverage; --settings-only alone checks the project, resolved build settings of every configuration (app name and icon per identity included) and test plans; it builds only the core XCFramework, no app product, and uses no VM"

# Reuse the real Swift producers, with source authority retained by this MG
# run. This is ordinary Unit input routing, not signed-agent evidence.
mg__xcode_unit_inputs() {
  local script="$MG_ROOT/tools/scripts/xcode-unit-inputs.py"
  unset MEGURI_INITIAL_ADOPTION_FIXTURE MEGURI_INITIAL_ADOPTION_RESULT MEGURI_CLOUD_SERVICE_CLI
  if ! python3 "$script" start "$MG_RUN_DIR"; then
    mg_failure xcode-unit-inputs "Mac Unit input source admission failed"
    mg_status fail
    return 1
  fi
  mg__swift_initial_adoption_fixture || return 1
  mg__swift_cloud_service_helper || return 1
  if ! python3 "$script" verify-inputs "$MG_RUN_DIR" "$MEGURI_INITIAL_ADOPTION_RESULT"; then
    mg_failure xcode-unit-inputs "Mac Unit input publication verification failed"
    mg_status fail
    return 1
  fi
  mg_artifact json "$MG_RUN_DIR/xcode-unit-source.json" "Mac Unit input source authority"
  mg_artifact json "$MG_RUN_DIR/xcode-unit-inputs.json" "Mac Unit input identities"
}

# Static checks and builds stay on the host. One pool-VM lease covers the
# devices phase: hidden Mac launches, Mac plans and simulator tests. Without
# the VM only those executions queue; independent build failures stand.
mg_xcode_check() {
  # Jobs have their own shell. Keep the pool's host override suppressed for
  # child scripts and exit cleanup even if the VM later becomes unavailable.
  MEGURI_SIM_ALLOW_HOST=0
  export MEGURI_SIM_ALLOW_HOST
  [ "${1:-}" = "--" ] && shift
  local tap="$MG_RUN_DIR/xcode-check.tap" rc devices_rc sims=() queued="" begin lease=0 take part reason
  local vmq=build/xcode-check/vm-queued.tsv arg unit_inputs=1 focused=0
  for arg in "$@"; do case "$arg" in --mac-unit-only*) focused=1 ;; esac; done
  if [ "$focused" -eq 1 ]; then
    if [ "$#" -ne 1 ] || [ "$1" != --mac-unit-only ]; then
      mg_failure arguments "--mac-unit-only accepts no other arguments"
      mg_status error
      return 0
    fi
    mg_detail scope '"focusedMacUnit"'
    mg_detail focusedMacUnit '{"scheme":"Meguri","plan":"Unit","configuration":"Debug","destination":"platform=macOS,arch=arm64","runner":"vm"}'
  fi
  for arg in "$@"; do [ "$arg" != --no-ui ] || unit_inputs=0; done
  for arg in "$@"; do
    [ "$arg" = --settings-only ] || continue
    if [ "$#" -ne 1 ]; then
      mg_failure arguments "--settings-only accepts no other arguments"
      mg_status error
      return 0
    fi
    mg_detail scope '"settingsOnly"'
  done
  mg_lint_ui_first || return 0
  if ! mg__xcframework "$MG_RUN_DIR/xcframework.json"; then
    mg_failure xcframework "building the core XCFramework failed; see log.txt" "tools/mg core-xcframework"
    mg_status fail
    return 0
  fi
  if [ "${1:-}" = --settings-only ]; then
    # Settings resolution only: no product, no Release Mac app to register,
    # no VM lease.
    /bin/bash tools/tests/xcode_project_check.sh build/xcode-check --phase builds --settings-only | tee "$tap"
    rc=${PIPESTATUS[0]}
    mg_artifact tap "$tap"
    mg_tap_report "$tap" "$rc" ""
    return 0
  fi
  MG_VM_PART="the devices phase (hidden Mac launches, simulators, test runs)"
  [ "$focused" -eq 0 ] || MG_VM_PART="focusedMacUnit"
  mg_sim_begin
  begin=$?
  case "$begin" in
    0) [ "$MG_SIM_RUNNER" != vm ] || sims=(--vm-ssh "$(jq -r .ssh <<<"$MG_MAC_VM")" --vm-name "$(jq -r .name <<<"$MG_MAC_VM")") ;;
    2) queued=1 sims=(--no-sims) ;;
    *) return 0 ;;
  esac
  rc=0
  if [ "$focused" -eq 0 ]; then
    /bin/bash tools/tests/xcode_project_check.sh build/xcode-check --phase builds ${1+"$@"} | tee "$tap"
    rc=${PIPESTATUS[0]}
  else
    : >"$tap"
    [ "$MG_SIM_RUNNER" = vm ] || { queued=1 sims=(--no-sims); }
  fi
  mg_artifact tap "$tap"
  mg_artifact logs build/xcode-check/logs "xcodebuild logs"
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg_sim_take --no-copy
    take=$?
    case "$take" in
      0) ;;
      # The test VM could not be reached (queued, its lease given back): the
      # devices phase builds on the host and queues all app execution.
      2) queued=1 sims=(--no-sims) MG_SIM_RUNNER="" ;;
      *) lease=1 ;;
    esac
  fi
  if [ "$lease" -ne 0 ]; then
    # The builds' results stand; the launches and test runs did not run.
    mg_tap_report "$tap" "$rc" ""
    if [ "$focused" -eq 1 ]; then
      mg_message "xcode-check: focusedMacUnit did not run (see the failures)"
    else
      mg_message "xcode-check: the builds were checked; the launches, simulators and test runs did not run (see the failures)"
    fi
    mg_status error
    return 0
  fi
  if [ "$unit_inputs" -eq 1 ] && [ "$MG_SIM_RUNNER" = vm ]; then
    if ! mg__xcode_unit_inputs; then
      unset MEGURI_INITIAL_ADOPTION_FIXTURE MEGURI_INITIAL_ADOPTION_RESULT MEGURI_CLOUD_SERVICE_CLI
      if [ "$focused" -eq 1 ]; then
        mg_failure xcode-unit-inputs "focusedMacUnit input preparation failed; Unit did not launch"
        mg_status fail
        return 0
      fi
      mg_failure xcode-unit-inputs "Mac Unit input preparation failed; independent device checks continue"
      mg_status fail
      rc=1
    fi
  fi
  # The script's simulator runs in the VM outlive it when the job is
  # cancelled: the job then stops them before it releases the VM's lease
  # (mg__mac_vm_stop_orphans). A run whose SSH session was lost the script
  # stops itself.
  [ "$MG_SIM_RUNNER" != vm ] || MG_MAC_VM_TESTING=1
  /bin/bash tools/tests/xcode_project_check.sh build/xcode-check --phase devices \
    --continue-from "$(grep -cE '^(not )?ok ' "$tap")" ${sims[0]+"${sims[@]}"} ${1+"$@"} | tee -a "$tap"
  devices_rc=${PIPESTATUS[0]}
  # A script that ended abnormally may have left a remote app or test alive.
  [ "$devices_rc" -eq 0 ] || mg__mac_vm_stop_orphans
  MG_MAC_VM_TESTING=""
  [ "$devices_rc" -eq 0 ] || rc="$devices_rc"
  mg_tap_report "$tap" "$rc" ""
  # Leases and runs a VM that stopped answering kept from happening.
  if [ -s "$vmq" ]; then
    mg_artifact tsv "$vmq" "leases and runs the test VM could not take"
    while IFS="$(printf '\t')" read -r part reason; do
      [ -z "$part" ] || mg__mac_vm_unreachable "$reason" "$part"
    done <"$vmq"
  fi
  case "$(cat "$MG_STATE/status"):$focused" in
    pass:0 | warn:0)
      if [ -n "$queued" ]; then
        mg_message "xcode-check: builds checked; VM execution queued: $MG_MAC_VM_REASON (user step mac-vm)"
      elif [ "$MG_QUEUED_PARTS" != '[]' ]; then
        mg_message "xcode-check: what ran passed; queued: $(mg_queued_parts_text): $MG_MAC_VM_REASON"
      fi
      ;;
  esac
  mg_queued_settle
  if [ "$focused" -eq 1 ]; then
    [ "$MG_QUEUED_PARTS" = '[]' ] || queued=1
    mg_message "xcode-check: focusedMacUnit, whole Meguri Unit Debug in the dedicated VM: $(cat "$MG_STATE/status")${queued:+; VM execution queued}; full Xcode matrix, T0/gate and broader-platform coverage not evaluated"
  fi
}
