#!/bin/bash
# Isolated controller doubles: no network services, Apple products or VM access.
set -u
mg_job() { :; }

initialize_manual_transfer_fakes() {
  MG_ROOT="$FX_ROOT"
  MG_TOOLS="$FX_REPO/tools"
  MG_RUN_DIR="$MG_ROOT/build/mg/manual-transfer-check/20261024T010101Z-17"
  MG_STATE="$MG_RUN_DIR/.mg"
  mkdir -p "$MG_STATE" "$FX_VM"
  # Retain real exit handling and VM quoting; replace external operations below.
  # Job registration stays inert while the controller loads its route helper.
  . "$MG_TOOLS/mg.d/host.sh"
  . "$MG_TOOLS/scripts/mac-vm-lib.sh"
  . "$MG_TOOLS/mg.d/manual-transfer-check.sh"
}
initialize_manual_transfer_fakes

mg_status() { printf '%s\n' "$1" >"$MG_STATE/status"; }
mg_failure() { printf 'failure %s\n' "$1" >>"$FX_EVENTS"; }
mg_message() { :; }
mg_detail() { printf '%s %s\n' "$1" "$2" >>"$FX_EVENTS"; }
mg_count() { :; }
mg_artifact() { printf 'artifact %s\n' "$1" >>"$FX_EVENTS"; }
mg_word_in() { local word; for word in $2; do [ "$1" != "$word" ] || return 0; done; return 1; }
mg_mac_ui_begin() {
  [ "${FX_MISSING_VM:-0}" = 0 ] || { mg_status warn; return 1; }
  MG_MAC_RUNNER="${FX_RUNNER:-vm}"
  MG_MAC_VMS="$(jq -nc --arg ssh "$FX_WRAPPER" '[{name:"meguri-mac",ssh:$ssh}]')"
}
mg_mt_prepare_data() {
  printf 'seed\n' >>"$FX_EVENTS"
  MG_MT_HOST_PORT=19090
  mkdir -p "$MG_RUN_DIR/device-fixture/collection.media"
}
# Controller-order doubles only. Real request counts come exclusively from
# manual-transfer-wire.py; its socket tests never derive counts from UI state.
mg_mt_start_wire() {
  [ -f "$MG_RUN_DIR/native-reader-command.json" ] || return 1
  printf 'start-wire\n' >>"$FX_EVENTS"
  MG_MT_OBSERVER_PORT=19090
  MG_MT_WIRE_CONFIG='{"port":19090,"token":"0000000000000000000000000000000000000000000000000000000000000000","nativeContext":"1111111111111111111111111111111111111111111111111111111111111111"}'
}
mg_mt_wire_proof() {
  printf 'wire-proof\n' >>"$FX_EVENTS"
  if [ "${FX_WIRE_FAIL:-0}" = 1 ]; then
    mg_mt_error wire-proof "contract double: missing native wire proof" fail
    return 1
  fi
}
mg_mac_vm_copy_in() {
  local input destination
  printf 'stage-media\n' >>"$FX_EVENTS"
  [ "${FX_MEDIA_STAGE_FAIL:-0}" = 0 ] || return 1
  for input in "$@"; do
    destination="${input/"$MG_ROOT"/"$FX_VM"}"
    mkdir -p "$(dirname "$destination")"
    cp -R "$input" "$destination"
  done
}
mg_app_build() { printf 'build %s\n' "$*" >>"$FX_EVENTS"; }
mg_mac_vm_take() {
  printf 'lease\n' >>"$FX_EVENTS"
  [ "${1:-}" != --no-copy ] || printf 'lease-no-copy\n' >>"$FX_EVENTS"
  MG_LEASE_DIR="$MG_ROOT/fake-lease"
  MG_MAC_VM="$(jq -c '.[0]' <<<"$MG_MAC_VMS")"
  mg_on_exit 'printf "release\n" >>"$FX_EVENTS"'
}
mvl_lease_held() { [ "${FX_NO_LEASE:-0}" = 0 ]; }
mg_app_log_now() { printf '2026-10-23 12:00:00\n'; }
mg_app_mac_input_refused() { [ "${FX_INPUT_DROP:-0}" = 1 ]; }
mg_app_mac_input_result() { return "${FX_INPUT_RESULT:-3}"; }
mg__mac_vm_stop_orphans() { printf 'stop-orphans\n' >>"$FX_EVENTS"; }
mg_mac_vm_ssh() {
  case "$1" in
    *' stage-media '*)
      printf 'install-reserved-media\n' >>"$FX_EVENTS"
      [ "${FX_RESERVED_COLLISION:-0}" = 0 ]
      ;;
    *lsof*)
      printf 'loopback-probe\n' >>"$FX_EVENTS"
      [ "${FX_BAD_ROUTE:-0}" = 0 ]
      ;;
    *) /bin/bash -c "${1//"$MG_ROOT"/"$FX_VM"}" ;;
  esac
}
mg_mac_vm_ssh_in() {
  case "$1" in
    *' archive-stale-fixture-state')
      printf 'archive-helper\n' >>"$FX_EVENTS"
      [ "${FX_REPAIR_SSH_FAIL:-0}" = 0 ] || return 255
      # Explicit nonmutating transport doubles; never run/retry the utility.
      if [ -n "${FX_REPAIR_REPLY:-}" ]; then
        local repair_rc=3 repair_status=blocked repair_archived=false
        case "$FX_REPAIR_REPLY" in
          blocked) ;;
          mismatched) repair_status=archived; repair_archived=true ;;
          missing) return 3 ;;
          ssh-loss-blocked) repair_rc=255 ;;
          *) return 97 ;;
        esac
        jq -nc --arg status "$repair_status" --argjson archived "$repair_archived" \
          '{purpose:"environment-repair-not-M8",source_run:"20261023T234838Z-84101",status:$status,archived:$archived}'
        return "$repair_rc"
      fi
      # Run the real utility on synthetic data, replacing ONLY VM metadata.
      # stdin is still the real controller's bounded source provenance proof.
      python3 -c '
import importlib.util, sys
from pathlib import Path
from unittest import mock
spec = importlib.util.spec_from_file_location("repair", sys.argv[1])
module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
with mock.patch.object(module, "dedicated_fixture_vm", return_value=Path(sys.argv[2])):
    sys.argv = [sys.argv[1], "archive-stale-fixture-state"]
    module.main()
' "$MG_TOOLS/scripts/manual-transfer-config.py" "$FX_VM"
      return $?
      ;;
  esac
  printf 'stage-manifest\n' >>"$FX_EVENTS"
  /bin/bash -c "${1//"$MG_ROOT"/"$FX_VM"}"
}
mg_sim_lease() { printf 'sim-lease %s\n' "$*" >>"$FX_EVENTS"; MG_SIM_UDID=fake-udid; }
mg_sim_check_runtime() { :; }
mg_sim_prepare() { :; }
mg_mt_set_native_support() {
  printf 'native-container\n' >>"$FX_EVENTS"
  MG_MT_SUPPORT="$FX_VM/native-support"
}
mg_mt_stage_reader() {
  printf 'stage-reader\n' >>"$FX_EVENTS"
  [ "${FX_READER_STAGE_FAIL:-0}" = 0 ] || return 1
  printf '{}' >"$MG_RUN_DIR/native-reader-command.json"
  mkdir -p "${MG_RUN_DIR/"$MG_ROOT"/"$FX_VM"}"
  printf '{}' >"${MG_RUN_DIR/"$MG_ROOT"/"$FX_VM"}/native-reader-input.json"
  MG_MT_MEDIA_STAGED=1
}
mg_app_xcodebuild_test() {
  local selection='-only-testing:MeguriUITests/ManualTransferJourneyTests/testDistinctAccountRoundTripPreservesImportCooldownAfterRestart'
  [ "$#" -eq 5 ] && [ "$5" = "$selection" ] || return 2
  printf 'selector %s\n' "$5" >>"$FX_EVENTS"
  printf 'test %s\n' "$2" >>"$FX_EVENTS"
  [ -f "$FX_VM/app/Fixtures/ManualTransfer/controlled-rig.json" ] || return 2
  mkdir -p "$1.xcresult"
}
xcrun() {
  case "$*" in
    *summary*)
      jq -nc --argjson omitted "${FX_SKIP:-0}" --argjson passed "${FX_PASSED:-1}" \
        '{result:"Passed",passedTests:$passed,failedTests:0,skippedTests:$omitted}'
      ;;
    *tests*)
      jq -nc --arg method "${FX_TEST_ID:-ManualTransferJourneyTests/testDistinctAccountRoundTripPreservesImportCooldownAfterRestart()}" \
        '{testNodes:[{nodeType:"Test Case",result:"Passed",nodeIdentifier:$method}]}'
      ;;
    *) return 97 ;;
  esac
}
mg_mt_copy_export() { printf 'capture\n' >>"$FX_EVENTS"; }
mg_mt_child() {
  printf 'peer\n' >>"$FX_EVENTS"
  jq -nc --argjson cells "${FX_PEER_MATCH:-true}" --argjson media "${FX_MEDIA_MATCH:-true}" '{complete:true,notes:1,cards:1,reviews:1,
    easy_reviews:1,identity_content:true,scheduling:true,all_logical_cells:$cells,
    declared_download_transforms:true,media:{complete:$media,native_bytes:$media,server_bytes:$media,
      typed_stages:[{},{}],peers:[{},{},{},{}]}}' >"$MG_RUN_DIR/peer-proof.json"
}
mg_queued_settle() { :; }

case "${1:-}" in
  check)
    shift
    mg_manual_transfer_check "$@"
    ;;
  forward|terminate)
    operation="$1"
    mg_mac_vm_take
    MG_MT_VM_PORT=18765
    MG_MT_HOST_PORT=19090
    MG_MT_MANIFEST="$MG_ROOT/app/Fixtures/ManualTransfer/controlled-rig.json"
    MG_MT_SUPPORT="$FX_VM/native-support"
    mg_on_exit mg_mt_cleanup
    mg_mt_stage_reader
    mg_mt_start_wire
    if [ "$operation" = forward ]; then
      mg_mt_open_forward
    else
      mg_mt_open_forward && mg_mt_write_manifest
      kill -TERM $$
    fi
    ;;
  *) exit 97 ;;
esac
