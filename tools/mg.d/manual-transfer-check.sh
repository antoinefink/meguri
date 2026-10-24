# Explicit RF4 evidence utility; ordinary gates never start this workflow.
mg_job manual-transfer-check host mg_manual_transfer_check \
  "Run RF4 in the pool VM: --platform mac|iphone|ipad; or explicit --archive-stale-fixture-state lossless VM repair"
mg_job manual-transfer-data docker:dev mg_manual_transfer_data \
  "RF4 controller data helper: seed|verify <controller build directory>"

. "$MG_TOOLS/scripts/manual-transfer-route.sh"

mg_mt_error() {
  mg_failure "manual-transfer/$1" "$2"
  mg_status "${3:-error}"
}

mg_mt_pending() {
  mg_detail native_transfer_complete false
  mg_detail native_transfer_setup "$(jq -nc --arg reason "$1" '{reason: $reason}')"
  mg_message "RF4 needs more evidence: $1"
  mg_status warn
}

# Child results belong to this checkout and this invocation, never latest.
mg_mt_child() {
  local result
  result="$("$MG_TOOLS/mg" "$@")" || return 1
  case "$result" in
    "$MG_ROOT"/build/mg/*/result.json) ;;
    *) return 1 ;;
  esac
  if ! jq -e --arg checkout "$MG_ROOT" \
      '.root == $checkout and .dry_run == false and .status == "pass"' \
      "$result" >/dev/null 2>&1; then
    return 1
  fi
  mg_artifact child-receipt "$result" "completed $1 child"
}

mg_manual_transfer_data() {
  local operation="${1:-}" relative="${2:-}"
  if [ "$#" -ne 2 ] || ! mg_word_in "$operation" "seed verify"; then
    mg_mt_error usage "data helper requires seed or verify and an owned run directory"
    return 0
  fi
  case "$relative" in
    build/mg/manual-transfer-check/*) ;;
    *) mg_mt_error usage "data helper requires a manual-transfer-check run"; return 0 ;;
  esac
  local run_name="${relative#build/mg/manual-transfer-check/}"
  case "$run_name" in
    ''|*[!A-Za-z0-9._-]*) mg_mt_error usage "invalid controller run name"; return 0 ;;
  esac
  export MEGURI_MANUAL_TRANSFER_DATA_RUN="$MG_ROOT/$relative"
  export MEGURI_MANUAL_TRANSFER_DATA_MODE="$operation"
  mg_core_test -- --test manual_transfer_data --run-ignored only \
    -E 'package(meguri-host) & test(=rig_manual_transfer_data)'
}

mg_mt_prepare_data() {
  local container
  if ! mg_mt_child sync-rig up; then
    mg_mt_error rig "could not establish this checkout's rig"
    return 1
  fi
  MG_MT_HOST_PORT="$(mg_sync_rig__port 8080)"
  case "$MG_MT_HOST_PORT" in
    127.0.0.1:[0-9]*) MG_MT_HOST_PORT="${MG_MT_HOST_PORT#127.0.0.1:}" ;;
    *) mg_mt_error rig-scope "the rig must bind workstation loopback"; return 1 ;;
  esac
  container="$(mg_sync_rig__running syncserver)"
  if [ -z "$container" ]; then
    mg_mt_error accounts "no running rig container"
    return 1
  fi
  # Inspect only through the configuration helper; never emit the inventory.
  if ! docker inspect "$container" | python3 "$MG_TOOLS/scripts/manual-transfer-config.py" \
      check-users "$MG_ROOT/docker/syncserver.env.example"; then
    mg_mt_error accounts "rig accounts differ from the two reserved examples"
    return 1
  fi
  container="$(mg_sync_rig__running sync-edge)"
  if [ -z "$container" ] || ! docker inspect "$container" | \
      python3 "$MG_TOOLS/scripts/manual-transfer-config.py" check-bind "$MG_MT_HOST_PORT"; then
    mg_mt_error rig-scope "owned rig edge must publish exactly the selected loopback port"
    return 1
  fi
  jq -nc --argjson port "$MG_MT_HOST_PORT" '{port: $port}' >"$MG_RUN_DIR/rig-input.json" || return 1
  if ! mg_mt_child manual-transfer-data seed "${MG_RUN_DIR#"$MG_ROOT"/}"; then
    mg_mt_error seed "data preparation child did not pass"
    return 1
  fi
  if ! jq -e '.distinct_accounts == true and .empty_nondefault_b == true and .deckID == 1
      and .notes == 1 and .cards == 1 and .reviews == 0 and .media_seeded == true
      and .media_names == ["rf4-server.bin", "rf4-device.bin", "rf4-zero.bin"]' \
      "$MG_RUN_DIR/seed.json" >/dev/null; then
    mg_mt_error seed-proof "generated account facts are missing or invalid"
    return 1
  fi
  mg_artifact seed-proof "$MG_RUN_DIR/seed.json" "generated account counts and identity facts"
}

mg_mt_write_manifest() {
  local script
  MG_MT_MANIFEST="$MG_ROOT/app/Fixtures/ManualTransfer/controlled-rig.json"
  script="umask 077; mkdir -p $(mvl_q "$(dirname "$MG_MT_MANIFEST")"); cat >$(mvl_q "$MG_MT_MANIFEST")"
  # Only a pipe and a VM file carry credentials. Neither is a host artifact.
  printf '%s' "$MG_MT_WIRE_CONFIG" | python3 "$MG_TOOLS/scripts/manual-transfer-config.py" manifest \
    "$MG_ROOT/docker/syncserver.env.example" "$MG_MT_VM_PORT" "$MG_RUN_DIR/device-fixture" "$MG_MT_SUPPORT" |
    mg_mac_vm_ssh_in "$script" >/dev/null 2>&1
  case "${PIPESTATUS[*]}" in
    '0 0 0') return 0 ;;
    *) mg_mt_error manifest "VM manifest staging failed"; return 1 ;;
  esac
}

mg_mt_set_native_support() {
  local app_home
  case "$1" in
    mac)
      MG_MT_SUPPORT="$(mg_mac_vm_ssh 'printf "%s/Library/Containers/com.antoinefink.meguri.dev/Data/Library/Application Support" "$HOME"')" || return 1
      ;;
    *)
      # Select the actual container through the existing owned VM app-ID seam.
      # This never launches or imports a collection.
      mg_simctl install "$2" "$(mg_app_products ios-sim Debug)/Meguri.app" >&2 || return 1
      app_home="$(mg_simctl get_app_container "$2" com.antoinefink.meguri.dev data)" || return 1
      case "$app_home" in /*/Containers/Data/Application/*) ;; *) return 1 ;; esac
      MG_MT_SUPPORT="$app_home/Library/Application Support"
      ;;
  esac
}

mg_mt_cleanup_media() {
  # Only controller-owned VM inputs are removed; never app receipts or rig data.
  rm -rf "$MG_RUN_DIR/device-fixture" "$MG_RUN_DIR/media-proof-config.py"
  rm -f "$MG_RUN_DIR/native-reader-command.json"
  if [ -n "${MG_MT_MEDIA_STAGED:-}" ]; then
    if ! mg_mac_vm_ssh "rm -rf $(mvl_q "$MG_RUN_DIR/device-fixture" "$MG_RUN_DIR/media-proof-config.py" "$MG_RUN_DIR/native-reader-input.json")" >/dev/null 2>&1; then
      mg_mt_error cleanup "media fixture teardown is unconfirmed"
      return 1
    fi
    MG_MT_MEDIA_STAGED=""
  fi
}

mg_mt_stage_media() {
  # The fixture launch hook replaces Collection. Stage ONLY reserved media,
  # through the selected container seam and under the retained pool leases.
  mvl_lease_held "$MG_LEASE_DIR" || return 1
  [ "$(jq -r .name <<<"$MG_MAC_VM")" = meguri-mac ] || return 1
  cp "$MG_TOOLS/scripts/manual-transfer-config.py" "$MG_RUN_DIR/media-proof-config.py" || return 1
  MG_MT_MEDIA_STAGED=1
  mg_on_exit mg_mt_cleanup
  mg_mac_vm_copy_in "$MG_RUN_DIR/media-proof-config.py" || return $?
  mg_mac_vm_ssh "/usr/bin/python3 $(mvl_q "$MG_RUN_DIR/media-proof-config.py") stage-media $(mvl_q "$MG_MT_SUPPORT" "${MG_RUN_DIR##*/}")" >/dev/null 2>&1
}

mg_mt_cleanup_repair() {
  # Only the transient helper. The persistent archive is never a trap target.
  rm -f "$MG_RUN_DIR/repair-config.py"
  if [ -n "${MG_MT_REPAIR_STAGED:-}" ]; then
    mg_mac_vm_ssh "rm -f $(mvl_q "$MG_RUN_DIR/repair-config.py")" >/dev/null 2>&1 || \
      mg_mt_error repair-cleanup "transient helper cleanup is unconfirmed"
    MG_MT_REPAIR_STAGED=""
  fi
}

mg_mt_archive_stale_fixture_state() {
  # Dedicated environment repair: source provenance BEFORE any VM lease.
  local evidence="$MG_RUN_DIR/repair-source.json" receipt="$MG_RUN_DIR/repair-receipt.json" rc expected_status
  mg_detail purpose '"environment-repair-not-M8"'
  if [ "$#" -ne 5 ] || [ "$1" != --archive-stale-fixture-state ] || \
      [ "$2" != --platform ] || [ "$3" != mac ] || [ "$4" != --source-run ] || \
      [ "$5" != 20261023T234838Z-84101 ]; then
    mg_mt_error repair-usage "use --archive-stale-fixture-state --platform mac --source-run 20261023T234838Z-84101"
    return 0
  fi
  if ! python3 "$MG_TOOLS/scripts/manual-transfer-config.py" repair-source "$MG_ROOT" "$5" >"$evidence"; then
    mg_detail repair_status '"blocked"'
    mg_mt_error repair-source "owned failed-run result and synthetic seed provenance could not be established"
    return 0
  fi
  unset MEGURI_SIM_ALLOW_HOST MEGURI_MAC_UI_ALLOW_HOST
  mg_mac_ui_begin || return 0
  if [ "$MG_MAC_RUNNER" != vm ]; then
    mg_detail repair_status '"blocked"'
    mg_mt_error repair-runner "repair requires the dedicated VM"
    return 0
  fi
  MG_MAC_VMS="$(jq -c '[.[] | select(.name == "meguri-mac")]' <<<"$MG_MAC_VMS")"
  if [ "$(jq length <<<"$MG_MAC_VMS")" != 1 ]; then
    mg_detail repair_status '"blocked"'
    mg_mt_error repair-runner "repair requires exactly one dedicated VM"
    return 0
  fi
  mg_mac_vm_take --no-copy || return 0
  if [ "$(jq -r .name <<<"$MG_MAC_VM")" != meguri-mac ] || ! mvl_lease_held "$MG_LEASE_DIR"; then
    mg_detail repair_status '"blocked"'
    mg_mt_error repair-lease "dedicated VM lease is required"
    return 0
  fi
  if ! cp "$MG_TOOLS/scripts/manual-transfer-config.py" "$MG_RUN_DIR/repair-config.py"; then
    mg_mt_error repair-helper "repair helper could not be prepared"
    return 0
  fi
  mg_on_exit mg_mt_cleanup_repair
  MG_MT_REPAIR_STAGED=1
  mg_mac_vm_copy_in "$MG_RUN_DIR/repair-config.py" || return 0
  # The host owns this lease throughout the remote operation; no host execution
  # fallback. Only the aggregate receipt crosses SSH, never archive contents.
  if ! mvl_lease_held "$MG_LEASE_DIR"; then
    mg_detail repair_status '"blocked"'
    mg_mt_error repair-lease "dedicated VM lease was lost before repair"
    return 0
  fi
  mg_mac_vm_ssh_in "/usr/bin/python3 $(mvl_q "$MG_RUN_DIR/repair-config.py") archive-stale-fixture-state" \
    <"$evidence" >"$receipt" 2>/dev/null
  rc=$?
  mg_detail repair_ssh_exit_code "$rc"
  case "$rc" in
    0) expected_status=archived ;;
    3) expected_status=blocked ;;
    4) expected_status=preservation-unconfirmed ;;
  esac
  case "$rc" in
    0|3|4)
      if ! jq -e --arg status "$expected_status" \
          '.purpose == "environment-repair-not-M8" and .source_run == "20261023T234838Z-84101"
          and .status == $status and (.archived == ($status == "archived"))' "$receipt" >/dev/null 2>&1; then
        rc=255
      fi
      ;;
  esac
  if [ "$rc" = 255 ] || ! mg_word_in "$rc" "0 3 4"; then
    # SSH loss can follow the rename. Never claim the source was untouched.
    mg_detail repair_status '"preservation-unknown"'
    mg_mt_error repair-uncertain "repair outcome unknown; preserve ~/MeguriTestArchives/ManualTransferSupport-20261023T234838Z-84101 in the VM"
    return 0
  fi
  mg_detail environment_repair "$(cat "$receipt")"
  mg_artifact repair-receipt "$receipt" "environment repair only; aggregate preservation receipt"
  case "$rc" in
    0) mg_status pass ;;
    3) mg_mt_error repair-blocked "archive preconditions blocked; support was not moved" ;;
    4) mg_mt_error repair-preservation "support moved; preservation unconfirmed at ~/MeguriTestArchives/ManualTransferSupport-20261023T234838Z-84101 in the VM" ;;
  esac
}

mg_mt_stage_reader() {
  local context command wrapper
  mvl_lease_held "$MG_LEASE_DIR" || return 1
  # The selected app-ID seam above alone chooses the root. HTTP supplies nothing.
  context="$(mg_mac_vm_ssh "/usr/bin/python3 $(mvl_q "$MG_RUN_DIR/media-proof-config.py") native-context $(mvl_q "$MG_MT_SUPPORT")" 2>/dev/null)" || return 1
  printf '%s' "$context" | mg_mac_vm_ssh_in "umask 077; cat >$(mvl_q "$MG_RUN_DIR/native-reader-input.json")" >/dev/null 2>&1 || return 1
  wrapper="$(jq -r .ssh <<<"$MG_MAC_VM")"
  # SSH stdin stays open for the read. EOF/SSH loss terminates and reaps the
  # remote reader; its own six-second alarm is independent of the host deadline.
  command="$(mvl_q /bin/bash -c 'exec 3<&0
worker=""; guard=""
cleanup() {
  [ -z "$worker" ] || kill -KILL "$worker" 2>/dev/null || true
  [ -z "$guard" ] || kill -TERM "$guard" 2>/dev/null || true
  [ -z "$worker" ] || wait "$worker" 2>/dev/null || true
  [ -z "$guard" ] || wait "$guard" 2>/dev/null || true
}
trap cleanup EXIT
trap '\''exit 143'\'' TERM HUP
/usr/bin/python3 "$1" native-state "$2" < /dev/null &
worker=$!
(IFS= read -r ignored <&3; kill -KILL "$worker" 2>/dev/null || true) &
guard=$!
wait "$worker"; result=$?
exit "$result"' manual-transfer-reader "$MG_RUN_DIR/media-proof-config.py" "$MG_RUN_DIR/native-reader-input.json")"
  printf '%s' "$context" | python3 "$MG_TOOLS/scripts/manual-transfer-config.py" reader-config "$wrapper" "$command" \
    >"$MG_RUN_DIR/native-reader-command.json"
}

mg_mt_read_results() {
  local result_path="$1.xcresult" summary tree
  summary="$(xcrun xcresulttool get test-results summary --path "$result_path" 2>/dev/null)" || return 1
  tree="$(xcrun xcresulttool get test-results tests --path "$result_path" 2>/dev/null)" || return 1
  if ! jq -e '[.result, .passedTests, .failedTests, .skippedTests] == ["Passed", 1, 0, 0]' \
      <<<"$summary" >/dev/null; then
    return 1
  fi
  # Count the leaf cases independently of the aggregate summary.
  if ! jq -e '[.. | objects | select(.nodeType? == "Test Case")] as $cases |
      ($cases | length) == 1 and $cases[0].result == "Passed" and
      ($cases[0].nodeIdentifier |
        test("^(MeguriUITests/)?ManualTransferJourneyTests/testDistinctAccountRoundTripPreservesImportCooldownAfterRestart(\\(\\))?$"))' \
      <<<"$tree" >/dev/null; then
    return 1
  fi
  jq '{passed: .passedTests, skipped: .skippedTests, failed: .failedTests}' \
    <<<"$summary" >"$MG_RUN_DIR/native-tests.json" || return 1
  mg_artifact test-counts "$MG_RUN_DIR/native-tests.json" "exact native case: one pass, zero skips or failures"
}

# XCTest teardown stops the app before the SQLite backup includes its WAL.
mg_mt_copy_export() {
  local platform="$1" device="$2" app_home collection backup support
  case "$platform" in
    mac)
      collection='"${HOME}/Library/Containers/com.antoinefink.meguri.dev/Data/Library/Application Support/Collection/collection.anki2"'
      support='"${HOME}/Library/Containers/com.antoinefink.meguri.dev/Data/Library/Application Support"'
      ;;
    *)
      app_home="$(mg_simctl get_app_container "$device" com.antoinefink.meguri.dev data)" || return 1
      case "$app_home" in /*/Containers/Data/Application/*) ;; *) return 1 ;; esac
      collection="$(mvl_q "$app_home/Library/Application Support/Collection/collection.anki2")"
      support="$(mvl_q "$app_home/Library/Application Support")"
      ;;
  esac
  case "$MG_RUN_DIR" in *"'"*|*$'\n'*) return 1 ;; esac
  backup=".backup '$MG_RUN_DIR/native.anki2'"
  if ! mg_mac_vm_ssh "mkdir -p $(mvl_q "$MG_RUN_DIR") && test -f $collection && /usr/bin/sqlite3 $collection $(mvl_q "$backup")" >/dev/null 2>&1; then
    return 1
  fi
  mg_mac_vm_copy_out "$MG_RUN_DIR/native.anki2" || return 1
  # Export only aggregates, fixed synthetic names and SHA256; account identities,
  # operation records, collection rows and credentials stay in the VM.
  if ! mg_mac_vm_ssh "/usr/bin/python3 $(mvl_q "$MG_RUN_DIR/media-proof-config.py") native-proof $support" \
      >"$MG_RUN_DIR/native-media.json" 2>/dev/null; then
    return 1
  fi
  mg_artifact native-media "$MG_RUN_DIR/native-media.json" "native durable media stages and actual file hashes"
  mg_mac_vm_ssh "rm -f $(mvl_q "$MG_RUN_DIR/native.anki2")" >/dev/null 2>&1
}

mg_mt_compare_peer() {
  local evidence="$MG_RUN_DIR/peer-proof.json"
  if ! mg_mt_child manual-transfer-data verify "${MG_RUN_DIR#"$MG_ROOT"/}"; then
    mg_mt_error peer "independent B download and comparison failed" fail
    return 1
  fi
  if ! jq -e '.complete == true and [.notes, .cards, .reviews, .easy_reviews] == [1,1,1,1]
      and .identity_content == true and .scheduling == true
      and .all_logical_cells == true and .declared_download_transforms == true
      and .media.complete == true and .media.native_bytes == true and .media.server_bytes == true
      and (.media.typed_stages | length) == 2 and (.media.peers | length) == 4' \
      "$evidence" >/dev/null; then
    mg_mt_error peer-proof "required peer comparison facts were not established" fail
    return 1
  fi
  mg_detail peer_proof "$(cat "$evidence")"
  mg_artifact peer-proof "$evidence" "B data checked against review replay and activated Export"
}

mg_manual_transfer_check() {
  if [ "${1:-}" = --archive-stale-fixture-state ]; then
    mg_mt_archive_stale_fixture_state "$@"
    return 0
  fi
  local selected_platform="${2:-}" build_platform test_destination leased_device="" test_status started_at media_stage_status
  if [ "$#" -ne 2 ] || [ "$1" != --platform ] || ! mg_word_in "$selected_platform" "mac iphone ipad"; then
    mg_mt_error usage "use manual-transfer-check --platform mac|iphone|ipad"
    return 0
  fi
  mg_mt_pending "native execution, peer comparison and teardown need proof"
  mg_detail platform "$(jq -nc --arg value "$selected_platform" '$value')"
  unset MEGURI_SIM_ALLOW_HOST MEGURI_MAC_UI_ALLOW_HOST
  mg_mac_ui_begin || return 0
  if [ "$MG_MAC_RUNNER" != vm ]; then
    mg_mt_error runner "run RF4 in the pool VM"
    return 0
  fi
  MG_MAC_VMS="$(jq -c '[.[0]]' <<<"$MG_MAC_VMS")"
  # This attests the selected wrapper's argument contract, not a live route.
  # Forward startup must still establish listener scope and HTTP reachability.
  case "${MEGURI_MANUAL_TRANSFER_SSH_OPTION_PASSTHROUGH:-}" in
    1) ;;
    *)
      mg_mt_pending "manual-transfer-check requires attested SSH option passthrough; see tools/vm/manual-transfer-check.md"
      return 0
      ;;
  esac
  MG_MT_LOCK="$MG_ROOT/build/manual-transfer-check.lock"
  mkdir -p "$MG_ROOT/build"
  if ! mkdir "$MG_MT_LOCK" 2>/dev/null; then
    mg_mt_error busy "reserved accounts are held by another controller in this checkout"
    return 0
  fi
  mg_on_exit 'rmdir "$MG_MT_LOCK" 2>/dev/null || true'
  umask 077
  mg_on_exit mg_mt_cleanup_media
  mg_mt_prepare_data || return 0
  case "$selected_platform" in mac) build_platform=mac ;; *) build_platform=ios-sim ;; esac
  export MG_APP_TEST_SCHEME=Meguri MG_APP_TEST_PLAN=UI
  mg_app_build "$build_platform" Debug || return 0
  MG_SIM_RUNNER=vm
  mg_mac_vm_take || return 0
  mg_on_exit mg_mt_cleanup
  MG_MT_MANIFEST="$MG_ROOT/app/Fixtures/ManualTransfer/controlled-rig.json"
  MG_MT_VM_PORT=18765
  if [ "$selected_platform" = mac ]; then
    test_destination='platform=macOS,arch=arm64'
    started_at="$(mg_app_log_now)"
  else
    mg_sim_lease "$selected_platform" release || return 0
    leased_device="$MG_SIM_UDID"
    mg_on_exit mg_mt_cleanup
    mg_sim_check_runtime "$leased_device" || return 0
    mg_sim_prepare "$leased_device" || return 0
    test_destination="id=$leased_device"
  fi
  mg_on_exit mg_mt_cleanup
  if ! mg_mt_set_native_support "$selected_platform" "$leased_device"; then
    mg_mt_error native-container "the VM development app container could not be established"
    return 0
  fi
  mg_mt_stage_media
  media_stage_status=$?
  case "$media_stage_status" in
    0) ;;
    2) mg_queued_settle; return 0 ;;
    *) mg_mt_error media-fixture "generated device media could not be staged in the VM"; return 0 ;;
  esac
  if ! mg_mt_stage_reader; then
    mg_mt_error native-reader "the fixed VM evidence reader could not be staged"
    return 0
  fi
  mg_mt_start_wire || return 0
  mg_mt_open_forward || return 0
  mg_mt_write_manifest || return 0
  # Callbacks run in reverse order. Remove inputs before releasing leases.
  mg_on_exit mg_mt_cleanup
  mg_app_xcodebuild_test "$MG_RUN_DIR/$selected_platform" "$selected_platform" "$test_destination" Debug \
    -only-testing:MeguriUITests/ManualTransferJourneyTests/testDistinctAccountRoundTripPreservesImportCooldownAfterRestart
  test_status=$?
  mg_artifact xcresult "$MG_RUN_DIR/$selected_platform.xcresult" "VM native receipt; check credential redaction before sharing"
  if [ "$selected_platform" = mac ] && [ "$test_status" -le 1 ] && \
      mg_app_mac_input_refused "$started_at" ManualTransferJourneyTests; then
    mg_app_mac_input_result "$MG_RUN_DIR/$selected_platform.xcresult" "$test_status" test
    test_status=$?
    if [ "$test_status" -eq 3 ]; then
      mg_mt_pending "lost VM input leaves the native case and peer comparison incomplete"
    else
      mg_mt_error execution "native failure is independent of the lost VM input" fail
    fi
    mg_queued_settle
    return 0
  fi
  case "$test_status" in
    4) mg_queued_settle; return 0 ;;
    0) ;;
    *) mg_mt_error execution "the selected native test did not pass" fail; return 0 ;;
  esac
  if ! mg_mt_read_results "$MG_RUN_DIR/$selected_platform"; then
    mg_mt_error execution "require one pass for the selected case, zero skips and zero failures" fail
    return 0
  fi
  mg_mt_wire_proof || return 0
  if ! mg_mt_copy_export "$selected_platform" "$leased_device"; then
    mg_mt_error native-copy "backup of the development app's activated Export failed"
    return 0
  fi
  mg_mt_compare_peer || return 0
  mg_mt_cleanup || return 0
  mg_count total 1
  mg_count passed 1
  mg_count failed 0
  mg_count skipped 0
  mg_detail native_transfer_complete true
  mg_message "RF4 has a passing native case, matching peer data and confirmed teardown"
  mg_status pass
}
