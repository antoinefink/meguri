# Explicit guest-only Device Hub operation; the caller retains its simulator.
mg_job sim-posture host mg_sim_posture \
  "Set a leased pool iPhone Duo's display posture in the running test VM: <udid> open|closed"

mg_posture_target() {
  local pool rc v envs=(MEGURI_SIM_POOL_IN_VM=1 "MEGURI_SIM_DEVICES_JSON=$MG_RUN_DIR/sim-devices.json")
  # Query only the leased guest. Re-entering pool routing could select/start
  # another VM or honor an ambient host fallback after our selection.
  for v in MEGURI_SIM_POOL_DIR MEGURI_SIM_POOL_MAX MEGURI_SIM_DEVICES_DIR; do
    [ -z "${!v:-}" ] || envs[${#envs[@]}]="$v=${!v}"
  done
  pool="$(mg_mac_vm_ssh "env $(mvl_q "${envs[@]}") /bin/bash $(mvl_q "$MG_ROOT/tools/scripts/sim-pool.sh") status" 2>"$MG_RUN_DIR/pool.err")"; rc=$?
  if mvl_unreachable "$rc" "$(cat "$MG_RUN_DIR/pool.err")"; then
    mg__mac_vm_unreachable 'selected guest could not query the simulator pool' 'Duo posture'
    return 1
  fi
  MG_POSTURE_DEVICE="$(jq -ce --arg u "$1" '[.devices[] | select(.udid == $u)] | select(length == 1) | .[0]' <<<"$pool")"
  if [ "$rc" -ne 0 ] || ! jq -e --arg root "$MG_ROOT" --arg vm "$(jq -r .name <<<"$MG_MAC_VM")" \
    --argjson now "$(date +%s)" --argjson margin "$2" '
      .kind == "foldable" and (.name | test("^Meguri-Pool-foldable-[0-9]+$")) and
      .state == "Booted" and .available == true and .current_runtime == true and
      .leased == true and .lease.worktree == $root and .lease.vm == $vm and
      .lease.kind == "foldable" and .lease.udid == .udid and .lease.expires_epoch > ($now + $margin)' \
      <<<"${MG_POSTURE_DEVICE:-null}" >/dev/null; then
    mg_failure posture-target 'requires a booted current-runtime pool Duo with a live checkout-owned lease in this VM'
    mg_status error
    return 1
  fi
}

mg_posture_receipt() {
  local out="$MG_RUN_DIR/posture-attachments" method='SimulatorPostureTests/testRequestedPosture()' file
  xcrun xcresulttool export attachments --path "$MG_RUN_DIR/posture.xcresult" --output-path "$out" \
    --test-id "$method" --filter 'mg-posture_0_*.json' >/dev/null 2>"$MG_RUN_DIR/attachment.err" || return 1
  file="$(jq -er --arg m "$method" '
    select(length == 1 and .[0].testIdentifier == $m) | .[0].attachments | select(length == 1) | .[0] |
    select(.isAssociatedWithFailure == false and
      (.suggestedHumanReadableName | test("^mg-posture_0_[A-Fa-f0-9-]{36}\\.json$")) and
      (.exportedFileName | test("^[A-Fa-f0-9-]{36}\\.json$"))) | .exportedFileName' "$out/manifest.json")" || return 1
  [ -f "$out/$file" ] && [ ! -L "$out/$file" ] || return 1
  jq -e --argjson r "$TEST_RUNNER_MEGURI_POSTURE_REQUEST" '
    .udid == $r.udid and .name == $r.name and .nonce == $r.nonce and .posture == $r.posture and
    (.status == "pass" or .status == "unavailable" or .status == "fail") and
    (.reason | type == "string") and (.diagnostics | type == "object") and (.changed | type == "boolean")' \
    "$out/$file" >/dev/null || return 1
  cp "$out/$file" "$MG_RUN_DIR/posture.json"
}

mg_posture_native_failure() {
  local rc="$1" receipt="$2" stage message
  stage="$(sed -n 's/^sim-posture: xcui-stage=\([a-z-]*\)$/\1/p' "$MG_RUN_DIR/posture.log" | tail -n 1)"
  stage="${stage:-not-recorded}"
  # Preserve XCTest's own failure (including activation/query failures), rather
  # than replacing it with a missing-attachment diagnosis. The result/log stay
  # authoritative when XCTest produced no readable failing-test record.
  message="$(xcrun xcresulttool get test-results tests --path "$MG_RUN_DIR/posture.xcresult" 2>/dev/null |
    jq -r '[.. | objects | select(.nodeType? == "Test Case" and .result? == "Failed") |
      .children[]? | select(.nodeType == "Failure Message") | .name | strings] | join("; ") | .[0:800]')"
  mg_failure posture-runner "native XCTest failed (exit $rc; last recorded preflight stage: $stage; valid final receipt: $receipt): ${message:-failure text unavailable; see posture.log and posture.xcresult}"
  mg_status fail
}

mg_posture_displays() {
  local u="$1" p="$2" sample displays rc png1="$MG_RUN_DIR/outer.png" png3="$MG_RUN_DIR/inner.png"
  for sample in 1 2; do
    rc=0
    mg_simctl io "$u" screenshot --display=1 "$png1" >&2 &&
      mg_simctl io "$u" screenshot --display=3 "$png3" >&2 &&
      displays="$(mg_mac_vm_ssh "osascript -l JavaScript $(mvl_q "$MG_ROOT/tools/vm/posture-display.js" "$png1" "$png3")")" || rc=1
    mg_mac_vm_ssh "rm -f $(mvl_q "$png1" "$png3")" >/dev/null || return 1
    [ "$rc" -eq 0 ] || return 1
    mg_detail displays "$displays"
    jq -e --arg p "$p" 'length == 2 and .[0].id == 1 and .[1].id == 3 and
      all(.[]; .width > 32 and .height > 32) and .[0].lit == ($p == "closed") and .[1].lit == ($p == "open")' \
      <<<"$displays" >/dev/null || return 1
  done
  mg_detail display_verified true
}

mg_posture_hub() {
  mg_mac_vm_ssh 'developer=$(xcode-select -p) && case "$developer" in /*/Contents/Developer) ;; *) exit 1 ;; esac && hub="${developer%/Developer}/Applications/DeviceHub.app" && id=$(/usr/libexec/PlistBuddy -c "Print CFBundleIdentifier" "$hub/Contents/Info.plist") && [ "$id" = com.apple.dt.Devices ] && /usr/bin/codesign --verify --strict "$hub" && open -a "$hub" && printf "%s\n" "$hub"'
}

mg_sim_posture() {
  local udid="${1:-}" posture="${2:-}" rc original inventory types hub report="$MG_RUN_DIR/posture.json"
  if [ "$#" -ne 2 ] || ! [[ "$udid" =~ ^[A-Fa-f0-9]{8}(-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$ ]] ||
    { [ "$posture" != open ] && [ "$posture" != closed ]; }; then
    mg_failure usage 'usage: tools/mg sim-posture <leased-pool-udid> open|closed'
    mg_status error; return 0
  fi
  # Pin the dedicated pool VM, never start it or honor a host fallback.
  mg_mac_vm_probe; rc=$?
  case "$rc" in
    0) ;;
    2) mg__mac_vm_config_error; return 0 ;;
    *) mg__mac_vm_queued 'sim-posture requires the running pool VM' 'Duo posture'; return 0 ;;
  esac
  MG_MAC_VMS="$(jq -c --arg n "$(jq -r '.[0].name' <<<"$MVL_ALL")" '[.[] | select(.name == $n)]' <<<"$MG_MAC_VMS")"
  if [ "$(jq length <<<"$MG_MAC_VMS")" -ne 1 ]; then
    mg__mac_vm_queued 'dedicated pool VM is unavailable' 'Duo posture'; return 0
  fi
  MG_MAC_VM="$(jq -c '.[0]' <<<"$MG_MAC_VMS")"
  MG_SIM_RUNNER=vm
  mg_detail sim_runner '"vm"'
  mg_app_build mac Debug || return 0
  mg_mac_vm_take --no-copy --running-only || return 0
  cp "$MG_ROOT/tools/sim-devices.json" "$MG_RUN_DIR/sim-devices.json" || return 1
  mg_mac_vm_copy_in "$MG_ROOT/tools/scripts/sim-pool.sh" "$MG_RUN_DIR/sim-devices.json" || return 0
  mg_posture_target "$udid" 120 || return 0
  original="$MG_POSTURE_DEVICE"
  mg_sim_check_runtime "$udid" "$MG_RUN_DIR/sim-devices.json" || return 0
  inventory="$(mg_simctl list devices --json)"
  types="$(mg_simctl list devicetypes --json)"
  if ! jq -e --argjson d "$original" --argjson t "$types" '
    [.devices[][] | select(.name == $d.name)] as $names |
    ($names | length) == 1 and
    any(.devices[$d.runtime][]; .udid == $d.udid and .state == "Booted" and .isAvailable == true and
      (.deviceTypeIdentifier as $id | any($t.devicetypes[]; .identifier == $id and .name == "iPhone Duo" and .productFamily == "iPhone")))' \
      <<<"$inventory" >/dev/null; then
    mg_failure posture-target 'unique named booted iPhone Duo inventory required'
    mg_status error; return 0
  fi
  mg_mac_vm_copy_in "$MG_ROOT/$MG_APP_DD/Build/Products" "$MG_ROOT/tools/vm/posture-display.js" || return 0
  # This runs outside the sandboxed test runner, under MG's guest lease and
  # existing logged-in-session admission. Only the selected Hub bundle path
  # crosses into XCTest; no checkout/lease/report paths do.
  hub="$(mg_posture_hub)"
  if [ "$?" -ne 0 ] || [[ "$hub" != /*/Contents/Applications/DeviceHub.app ]]; then
    mg__mac_vm_queued 'selected Xcode Device Hub unavailable' 'Duo posture'; return 0
  fi
  mg_posture_target "$udid" 120 || return 0
  if [ "$(jq -c .lease <<<"$MG_POSTURE_DEVICE")" != "$(jq -c .lease <<<"$original")" ]; then
    mg_failure posture-target 'simulator lease changed during preparation'; mg_status error; return 0
  fi
  local TEST_RUNNER_MEGURI_POSTURE_REQUEST
  TEST_RUNNER_MEGURI_POSTURE_REQUEST="$(jq -nc --argjson d "$original" --arg p "$posture" --arg b "$hub" \
    --arg nonce "$MG_RUN_ID" --argjson end "$(($(date +%s) + 90))" \
    '{udid:$d.udid,name:$d.name,posture:$p,bundlePath:$b,nonce:$nonce,mutationUntil:$end}')"
  export TEST_RUNNER_MEGURI_POSTURE_REQUEST
  jq -nc --argjson d "$original" --arg root "$MG_ROOT" --arg vm "$(jq -r .name <<<"$MG_MAC_VM")" \
    '{device:$d,root:$root,vm:$vm}' >"$MG_RUN_DIR/posture-context.json" || return 1
  mg_mac_vm_copy_in "$MG_ROOT/tools/vm/sim-posture-transaction.sh" "$MG_RUN_DIR/posture-context.json" || return 0
  mg_mac_vm_xcodebuild_test "$MG_RUN_DIR/posture" 'platform=macOS,arch=arm64' Debug \
    -only-testing:MeguriUITests/SimulatorPostureTests/testRequestedPosture \
    -parallel-testing-enabled NO -destination-timeout 15 \
    -maximum-test-execution-time-allowance 30 -test-timeouts-enabled YES
  rc=$?
  [ -z "${MG_MAC_VM_RUN_QUEUED:-}${MG_MAC_VM_RUN_ERROR:-}" ] || return 0
  if [ "$rc" -eq 65 ] && grep -q '^sim-posture: invalid-target-or-lease$' "$MG_RUN_DIR/posture.log"; then
    mg_failure posture-target 'simulator target or acquisition changed before the transaction, or its TTL is insufficient'
    mg_status error; return 0
  fi
  mg_artifact xcresult "$MG_RUN_DIR/posture.xcresult" 'Device Hub control receipt'
  if ! mg_posture_receipt; then
    if [ "$rc" -ne 0 ]; then
      mg_posture_native_failure "$rc" missing; return 0
    fi
    mg_failure posture-report 'expected one target-bound XCTest posture attachment'; mg_status error; return 0
  fi
  mg_detail posture "$(cat "$report")"
  mg_posture_target "$udid" 0 || return 0
  if [ "$(jq -c .lease <<<"$MG_POSTURE_DEVICE")" != "$(jq -c .lease <<<"$original")" ]; then
    mg_failure posture-target 'simulator lease changed during XCTest'; mg_status error; return 0
  fi
  if [ "$rc" -ne 0 ]; then
    mg_posture_native_failure "$rc" present; return 0
  fi
  case "$(jq -r .status "$report")" in
    unavailable) mg__mac_vm_queued "Duo posture: $(jq -r .reason "$report")" 'Duo posture' ;;
    pass)
      if ! jq -e '.control_verified == true' "$report" >/dev/null || ! mg_posture_displays "$udid" "$posture"; then
        mg_failure posture-proof 'control or independent display observations did not verify requested posture'; mg_status fail
      else
        mg_posture_target "$udid" 0 || return 0
        if [ "$(jq -c .lease <<<"$MG_POSTURE_DEVICE")" != "$(jq -c .lease <<<"$original")" ]; then
          mg_failure posture-target 'simulator lease changed during display verification'; mg_status error; return 0
        fi
        mg_message "sim-posture: $udid is $posture (control and target displays verified)"; mg_status pass
      fi ;;
    *) mg_failure posture-verification "$(jq -r .reason "$report")"; mg_status fail ;;
  esac
}
