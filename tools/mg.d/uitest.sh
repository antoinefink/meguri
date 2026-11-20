# UI test suites (U150): any MeguriUITests class on a pool simulator or for
# the Mac, both in the macOS test VM (mac-vm.sh; queued, warn, without it),
# as a11y runs its suites. An explicit physical iOS target runs through
# host Xcode only, with no screen or simulator fallback.

mg_job ui-test host mg_ui_test \
  "XCUITest suites of MeguriUITests: --suite <class>[/<test>]... [--platform iphone|ipad|mac] [--device <leased udid> | --physical-device <explicit identifier>] [--config Debug|Profile]; --owned-meguri-sync-receipt <exact-result.json> opts the single signed Mac Debug setup case into disposable owned sync; --list-physical-devices inventories connected iOS devices; functional assertions only, no performance receipt"

mg_ui_test__usage() {
  mg_failure usage "ui-test: $1" "tools/mg --list"
  mg_message "ui-test: $1"
  mg_status error
}

# xcdevice's JSON array is kept in memory; only these public destination
# fields enter the receipt. Names, errors and non-iOS identifiers stay out.
mg_ui_test__physical_inventory() {
  local raw
  raw="$(xcrun xcdevice list --timeout 20 2>/dev/null)" &&
    MG_UI_PHYSICAL_DEVICES="$(jq -cse '
      if length != 1 or (.[0] | type) != "array" then error("expected device array") else .[0] end
      | if all(.[]; type == "object" and (.identifier | type) == "string"
          and (.platform | type) == "string" and (.simulator | type) == "boolean")
        then . else error("invalid device entry") end
      | map(select(.platform == "com.apple.platform.iphoneos" and .simulator == false)
          | {identifier, modelCode, operatingSystemVersion, available})
      | if all(.[]; (.modelCode | type) == "string" and (.operatingSystemVersion | type) == "string"
          and (.available | type) == "boolean")
        then . else error("invalid physical device entry") end' <<<"$raw" 2>/dev/null)" || {
      mg_failure physical-device-inventory "xcdevice did not return a readable device JSON array"
      mg_status error
      return 1
    }
  MG_UI_PHYSICAL_REJECTED="$(jq -r --arg id "${1:-}" 'any(.[];
    .identifier == $id and (.simulator != false or .platform != "com.apple.platform.iphoneos"))' <<<"$raw")"
  mg_detail devices "$MG_UI_PHYSICAL_DEVICES"
}

mg_ui_test__physical_check() {
  local rec
  mg_ui_test__physical_inventory "$1" || return 1
  if [ "$MG_UI_PHYSICAL_REJECTED" = true ]; then
    mg_ui_test__usage "--physical-device must identify a real iOS device, never a simulator or Mac"
    return 1
  fi
  rec="$(jq -c --arg id "$1" 'first(.[] | select(.identifier == $id)) // null' <<<"$MG_UI_PHYSICAL_DEVICES")"
  if [ "$rec" != null ] && ! jq -e --arg axis "$2" '
      .modelCode | if $axis == "iphone" then test("^iPhone")
      elif $axis == "ipad" then test("^iPad") else false end' <<<"$rec" >/dev/null; then
    mg_ui_test__usage "the physical device model must match --platform iphone or ipad"
    return 1
  fi
  if [ "$rec" = null ] || [ "$(jq -r .available <<<"$rec")" != true ]; then
    mg_user_step device-developer-mode missing true "Connect, unlock and pair the explicitly selected iOS device" \
      "Connect and unlock the phone, pair it in Xcode, and enable Developer Mode on it; then rerun the same explicit --physical-device command."
    mg_queued_part device-developer-mode ui-test "the selected physical iOS device is missing or unavailable"
    mg_message "ui-test: queued: connect, unlock and Xcode-pair the selected phone with Developer Mode enabled"
    mg_status warn
    mg_queued_settle
    return 1
  fi
  if ! jq -e 'try (.operatingSystemVersion | capture("^(?<major>[0-9]+)(\\.| |$)").major | tonumber >= 27)
      catch false' <<<"$rec" >/dev/null 2>&1; then
    mg_ui_test__usage "the physical device must run iOS 27 or later"
    return 1
  fi
  MG_UI_PHYSICAL_DEVICE="$1"
}

# This one case (or its containing class) needs the generated media fixture
# and a native destination capture. No other selector acquires this evidence.
mg_ui_test__media_selected() {
  local selector
  for selector in $1; do
    case "$selector" in
      CollectionEntryJourneyTests|CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace) return 0 ;;
    esac
  done
  return 1
}

# Keep VM reachability queued; a refused capture or missing package is an error.
mg_ui_test__media_command() {
  local rc
  MG_UI_MEDIA_COMMAND_OUTPUT="$("$@" 2>&1)"
  rc=$?
  [ "$rc" -ne 0 ] || return 0
  if mvl_unreachable "$rc" "$MG_UI_MEDIA_COMMAND_OUTPUT"; then
    mg__mac_vm_unreachable "the VM could not run native media destination capture" native-media-export
    return 2
  fi
  return 1
}

mg_ui_test__media_prepare() {
  local runner="$1" rc helper="$MG_ROOT/tools/scripts/native-media-export-capture.py"
  if [ "$runner" != vm ]; then
    mg_failure native-media-export "native media destination capture requires the leased test VM"
    mg_status error
    return 1
  fi
  # Source guard and stable per-run output; never honor an ambient handoff.
  case "$MG_RUN_DIR" in "$MG_ROOT/build/mg/ui-test/"*) ;; *) return 1 ;; esac
  # Copy by retained descriptors before Docker sees anything. The pinned
  # decoder gets only this run's parent copy and fresh derived output.
  if ! python3 "$helper" stage "$MG_RUN_DIR" ||
      ! mg_compose --progress plain build package-interop-decoder >&2 </dev/null ||
      ! mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
        -u "$(id -u):$(id -g)" \
        -v "$MG_ROOT/tools/scripts/package-interop-check.py:/decoder.py:ro" \
        -v "$MG_RUN_DIR/native-media-parent:/in:ro" \
        -v "$MG_RUN_DIR/native-media-baseline:/out" package-interop-decoder \
        python3 /decoder.py prepare-native-media --expected /in --output /out </dev/null; then
    mg_failure native-media-source "closed generated native media baseline preparation failed"
    mg_status error
    return 1
  fi
  MG_UI_MEDIA_NAME="$(python3 "$helper" identity "$MG_RUN_DIR")" || {
    mg_failure native-media-source "requires the frozen prepared typical-1 baseline and its generated parent"
    mg_status error
    return 1
  }
  [[ "$MG_UI_MEDIA_NAME" =~ ^Meguri-Media-[a-f0-9]{32}\.colpkg$ ]] || return 1
  mg_vm_fixture "$MG_RUN_DIR/native-media-baseline/fixture"
  rc=$?
  [ "$rc" -eq 0 ] || { [ "$rc" -ne 2 ] || mg_queued_settle; return "$rc"; }
  mg_mac_vm_copy_in "$MG_RUN_DIR/expected-source.json"
  rc=$?
  [ "$rc" -eq 0 ] || { [ "$rc" -ne 2 ] || mg_queued_settle; return "$rc"; }
  export TEST_RUNNER_MEGURI_NATIVE_MEDIA_FIXTURE_RECORD="$MG_RUN_DIR/expected-source.json"
  if [ "$MG_UI_MEDIA_PLATFORM" != mac ]; then
    mg_mac_vm_copy_in "$MG_ROOT/tools/scripts/native-media-export-capture.py" "$MG_RUN_DIR/native-media-export" || return $?
    export TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT="$MG_RUN_DIR/native-media-export"
  fi
  export TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME="$MG_UI_MEDIA_NAME"
  MG_UI_MEDIA_READY=1
}

# Called only after a successful run and checked summary. The exact media case
# must also be present and Passed; Saved/hash alone is no content/client proof.
mg_ui_test__media_capture() {
  local prefix="$1" platform="$2" device="$3" tree container diagnostic
  MG_UI_MEDIA_PHASE=tree
  tree="$(xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null)" || return 1
  jq -e '[.. | objects | select(.nodeType? == "Test Case")
      | select(.nodeIdentifier? | strings | test("^(MeguriUITests/)?CollectionEntryJourneyTests/testNativeMediaExportCancelSaveReplace(\\(\\))?$"))] as $cases
      | ($cases | length) == 1 and $cases[0].result == "Passed"' <<<"$tree" >/dev/null 2>&1 || return 1
  MG_UI_MEDIA_PHASE=copy
  if [ "$platform" != mac ]; then
    mg_ui_test__media_command mg_simctl get_app_container "$device" com.antoinefink.meguri.dev data || return $?
    container="$MG_UI_MEDIA_COMMAND_OUTPUT"
    # Validate before passing the response to any file-reading VM command.
    [[ "$device" =~ ^[A-Fa-f0-9]{8}(-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$ ]] || return 1
    [[ "$container" =~ ^/Users/[A-Za-z0-9._-]+/Library/Developer/CoreSimulator/Devices/[A-Fa-f0-9-]+/data/Containers/Data/Application/[A-Fa-f0-9]{8}(-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$ ]] || return 1
    case "$container" in */Devices/"$device"/data/Containers/Data/Application/*) ;; *) return 1 ;; esac
    mg_ui_test__media_command mg_mac_vm_ssh "python3 $(mvl_q "$MG_ROOT/tools/scripts/native-media-export-capture.py" simulator "$MG_RUN_DIR" "$container" "$device" "$MG_UI_MEDIA_NAME")" || return $?
    mg_sim_fetch "$MG_RUN_DIR/native-media-export/$MG_UI_MEDIA_NAME" || return $?
  else
    MG_UI_MEDIA_PHASE=export
    diagnostic="$(python3 "$MG_ROOT/tools/scripts/native-media-export-capture.py" mac \
      "$MG_RUN_DIR" "$MG_UI_MEDIA_NAME" "$prefix.xcresult" 2>&1)" || {
      case "$diagnostic" in
        FIXED-PHASE:export) MG_UI_MEDIA_PHASE=export ;;
        FIXED-PHASE:manifest) MG_UI_MEDIA_PHASE=manifest ;;
        FIXED-PHASE:copy) MG_UI_MEDIA_PHASE=copy ;;
      esac
      return 1
    }
  fi
  MG_UI_MEDIA_PHASE=receipt
  python3 "$MG_ROOT/tools/scripts/native-media-export-capture.py" receipt "$MG_RUN_DIR" "$MG_UI_MEDIA_NAME" >/dev/null 2>&1 || return 1
  mg_artifact native-media-export "$MG_RUN_DIR/native-media-export/$MG_UI_MEDIA_NAME" "file received by native destination; content/client proof pending"
  mg_artifact native-media-source "$MG_RUN_DIR/expected-source.json" "frozen prepared fixture hashes; separate generated parent manifest and dump provenance"
  mg_artifact native-media-receipt "$MG_RUN_DIR/native-media-export.json" "destination SHA256 and size; E7.4 acceptance pending"
  mg_detail native_media_export "$(cat "$MG_RUN_DIR/native-media-export.json")"
}

# F11.2 owns one exact native method, separate from F11.1's baseline/capture.
mg_ui_test__review_selected() {
  local selector
  for selector in $1; do
    case "$selector" in
      CollectionEntryJourneyTests|CollectionEntryJourneyTests/testNativeMediaImportReviewReopenAndExport) return 0 ;;
    esac
  done
  return 1
}

mg_ui_test__review_prepare() {
  local runner="$1" app_platform="$2" helper="$MG_ROOT/tools/scripts/native-media-review-capture.py"
  local decoder="$MG_ROOT/tools/scripts/package-interop-check.py" path paths rc revision case_name container_run
  [ "$runner" = vm ] || return 1
  revision="$(git -C "$MG_ROOT" rev-parse HEAD)" || return 1
  [[ "$revision" =~ ^[a-f0-9]{40}$ ]] || return 1
  case_name="${MG_RUN_DIR##*/}"
  [[ "$case_name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,119}$ ]] || return 1
  [ "$MG_RUN_DIR" = "$MG_ROOT/build/mg/ui-test/$case_name" ] || return 1
  container_run="/work/build/mg/ui-test/$case_name"
  # Generate only the unchanged own parent if absent. No host SQLite/cargo.
  if [ ! -d "$MG_ROOT/build/fixtures/recipes/local-review-1" ]; then
    "$MG_TOOLS/mg" fixtures local-review --seed 1 --media files || return 1
  fi
  python3 "$helper" decoder-root "$MG_RUN_DIR" || return 1
  mg_compose --progress plain build package-interop-decoder >&2 </dev/null || return 1
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
    -u "$(id -u):$(id -g)" \
    -v "$decoder:/decoder.py:ro" \
    -v "$MG_RUN_DIR/decoder-root:/work:ro" \
    -v "$MG_ROOT/build/fixtures/recipes/local-review-1:/work/build/fixtures/recipes/local-review-1:ro" \
    -v "$MG_RUN_DIR/source-preparation:$container_run/source-preparation" package-interop-decoder \
    python3 /decoder.py prepare-native-review --root /work --run "$container_run" \
    --expected /work/build/fixtures/recipes/local-review-1 --output "$container_run/source-preparation" \
    --platform "$MG_UI_REVIEW_PLATFORM" </dev/null || return 1
  # Build with the existing root/cache authority, then publish and execute only
  # a run-owned single-link copy as the host owner. No cargo in the replay step;
  # hide its home and mount the compiler output volume read-only.
  mg_conf__dev -- "cd core && cargo build -q -p meguri-cli" || return 1
  mg_conf__dev -u "$(id -u):$(id -g)" -v "$MG_RUN_DIR/empty-cargo-home:/cargo-home:ro" \
    -v "${MEGURI_DOCKER_CARGO_TARGET:-$(mg_compose_project)_cargo-target}:/cargo-target:ro" -- \
    "python3 /work/tools/scripts/native-media-review-capture.py derive '$container_run' '$MG_UI_REVIEW_PLATFORM' '$revision'" || return 1
  python3 "$helper" bind "$MG_RUN_DIR" "$(mg_app_products "$app_platform" Debug)/Meguri.app" Debug || return 1
  paths="$(python3 "$helper" stage-paths "$MG_RUN_DIR")" || return 1
  while IFS= read -r path; do
    [ -n "$path" ] || return 1
    mg_vm_fixture "$path"
    rc=$?
    [ "$rc" -eq 0 ] || return "$rc"
  done <<<"$paths"
  # Sibling helper import and fresh output are needed for guarded VM capture.
  mg_mac_vm_copy_in "$helper" "$MG_ROOT/tools/scripts/native-media-export-capture.py" "$MG_RUN_DIR/native-review-export" || return $?
  export TEST_RUNNER_MEGURI_NATIVE_REVIEW_CASE="$case_name"
  export TEST_RUNNER_MEGURI_NATIVE_REVIEW_PLATFORM="$MG_UI_REVIEW_PLATFORM"
  export TEST_RUNNER_MEGURI_NATIVE_REVIEW_FREEZE_SHA256="$(jq -er .freeze.sha256 "$MG_RUN_DIR/native-review-binding.json")"
  export TEST_RUNNER_MEGURI_NATIVE_REVIEW_FREEZE_BYTES="$(jq -er .freeze.bytes "$MG_RUN_DIR/native-review-binding.json")"
  MG_UI_REVIEW_READY=1
}

mg_ui_test__review_capture() {
  local prefix="$1" platform="$2" device="$3" tree container proof result_path phase
  local helper="$MG_ROOT/tools/scripts/native-media-review-capture.py" filename
  tree="$(xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null)" || return 1
  jq -e '[.. | objects | select(.nodeType? == "Test Case")
      | select(.nodeIdentifier? | strings | test("^(MeguriUITests/)?CollectionEntryJourneyTests/testNativeMediaImportReviewReopenAndExport(\\(\\))?$"))] as $cases
      | ($cases | length) == 1 and $cases[0].result == "Passed"' <<<"$tree" >/dev/null 2>&1 || return 1
  filename="$(jq -er .native_filename "$MG_RUN_DIR/native-review-binding.json")" || return 1
  if [ "$platform" = mac ]; then
    python3 "$helper" mac "$MG_RUN_DIR" "$prefix.xcresult" || return 1
  else
    mg_ui_test__media_command mg_simctl get_app_container "$device" com.antoinefink.meguri.dev data || return 1
    container="$MG_UI_MEDIA_COMMAND_OUTPUT"
    [[ "$device" =~ ^[A-Fa-f0-9]{8}(-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$ ]] || return 1
    [[ "$container" =~ ^/Users/[A-Za-z0-9._-]+/Library/Developer/CoreSimulator/Devices/[A-Fa-f0-9-]+/data/Containers/Data/Application/[A-Fa-f0-9]{8}(-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$ ]] || return 1
    case "$container" in */Devices/"$device"/data/Containers/Data/Application/*) ;; *) return 1 ;; esac
    mg_ui_test__media_command mg_mac_vm_ssh "python3 $(mvl_q "$helper" simulator "$MG_RUN_DIR" "$container" "$device")" || return 1
    mg_sim_fetch "$MG_RUN_DIR/native-review-export/$filename" || return 1
    for phase in import accepted closed reopened; do
      mg_sim_fetch "$MG_RUN_DIR/native-review-export/native-review-$phase.json" || return 1
    done
    python3 "$helper" attestation "$MG_RUN_DIR" "$prefix.xcresult" || return 1
  fi
  python3 "$helper" receipts "$MG_RUN_DIR" || return 1
  filename="$(jq -er .native_filename "$MG_RUN_DIR/native-review-binding.json")" || return 1
  # Keep the full existing strict decoder and compatible-client check intact.
  proof="$("$MG_TOOLS/mg" package-interop-check --package "$MG_RUN_DIR/native-review-export/$filename" \
    --expected "$MG_RUN_DIR/expected-replay/review" --now 2030-01-15T10:00:00+01:00 --tz Europe/Paris)" || return 1
  result_path="${proof##*$'\n'}"
  python3 "$helper" proof "$MG_RUN_DIR" "$result_path" || return 1
  mg_artifact native-review-package "$MG_RUN_DIR/native-review-export/$filename" "actual native all-media destination"
  mg_artifact native-review-proof "$MG_RUN_DIR/native-review-proof.json" "frozen native chain and strict independent package/client proof; milestone acceptance remains separate"
  mg_detail native_review "$(cat "$MG_RUN_DIR/native-review-proof.json")"
}

mg_ui_test() {
  if [ "${TEST_RUNNER_MEGURI_OWNED_PROVIDER_JOURNEY_RUN_ID+set}" = set ] ||
      [ "${TEST_RUNNER_MEGURI_OWNED_PROVIDER_JOURNEY_CREDENTIALS_SHA256+set}" = set ]; then
    mg_failure owned-provider-ambient "Owned journey inputs require the explicit receipt option"
    mg_status error; return 0
  fi
  local MG_UI_MAC_PROFILE_SELECTOR=""
  local platform=iphone device="" config=Debug suites="" suite_count=0 only=() build_args=() s result since udid
  local physical="" inventory="" run_args="" MG_UI_PHYSICAL_DEVICE=""
  local owned="" key
  local review="" MG_UI_REVIEW_READY="" MG_UI_REVIEW_PLATFORM=""
  local TEST_RUNNER_MEGURI_NATIVE_REVIEW_CASE TEST_RUNNER_MEGURI_NATIVE_REVIEW_PLATFORM
  local TEST_RUNNER_MEGURI_NATIVE_REVIEW_FREEZE_SHA256 TEST_RUNNER_MEGURI_NATIVE_REVIEW_FREEZE_BYTES
  unset TEST_RUNNER_MEGURI_NATIVE_REVIEW_CASE TEST_RUNNER_MEGURI_NATIVE_REVIEW_PLATFORM
  unset TEST_RUNNER_MEGURI_NATIVE_REVIEW_FREEZE_SHA256 TEST_RUNNER_MEGURI_NATIVE_REVIEW_FREEZE_BYTES
  local media="" MG_UI_MEDIA_READY="" MG_UI_MEDIA_PLATFORM="" MG_UI_MEDIA_DEVICE="" MG_UI_MEDIA_NAME="" MG_UI_MEDIA_PHASE=""
  local TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME TEST_RUNNER_MEGURI_NATIVE_MEDIA_FIXTURE_RECORD
  unset TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_OUT TEST_RUNNER_MEGURI_NATIVE_MEDIA_EXPORT_NAME TEST_RUNNER_MEGURI_NATIVE_MEDIA_FIXTURE_RECORD
  while [ "$#" -gt 0 ]; do
    [ "$1" = --list-physical-devices ] || run_args=1
    case "$1" in
      --owned-meguri-sync-receipt)
        [ "$#" -ge 2 ] && [ -z "$owned" ] && [ -n "$2" ] && [[ "$2" != --* ]] ||
          { mg_ui_test__usage "--owned-meguri-sync-receipt requires one exact package receipt"; return 0; }
        owned="$2" && shift 2
        ;;
      --list-physical-devices)
        [ -z "$inventory" ] || { mg_ui_test__usage "duplicate --list-physical-devices"; return 0; }
        inventory=1 && shift
        ;;
      --physical-device)
        [ "$#" -ge 2 ] && [ -z "$physical" ] && [[ "$2" =~ ^([A-Fa-f0-9]{8}-[A-Fa-f0-9]{16}|[A-Fa-f0-9]{8}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{12})$ ]] ||
          { mg_ui_test__usage "--physical-device needs one explicit Xcode device identifier"; return 0; }
        physical="$2" && shift 2
        ;;
      --platform)
        [ "$#" -ge 2 ] && mg_word_in "$2" "iphone ipad mac" || { mg_ui_test__usage "--platform takes iphone, ipad or mac"; return 0; }
        platform="$2" && shift 2
        ;;
      --device)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_ui_test__usage "--device needs a UDID (tools/mg sim acquire <kind>)"; return 0; }
        device="$2" && shift 2
        ;;
      --config)
        [ "$#" -ge 2 ] && mg_word_in "$2" "Debug Profile" || { mg_ui_test__usage "--config takes Debug or Profile"; return 0; }
        config="$2" && shift 2
        ;;
      --suite)
        [ "$#" -ge 2 ] && printf '%s' "$2" | grep -Eq '^[A-Z][A-Za-z0-9]*(/[A-Za-z0-9_]+)?$' ||
          { mg_ui_test__usage "--suite takes a test class of MeguriUITests, or Class/testMethod, e.g. NavigationLayoutTests"; return 0; }
        suite_count=$((suite_count + 1))
        suites="$suites $2" && shift 2
        ;;
      *) mg_ui_test__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  if [ -n "$owned" ]; then
    case "$-" in *x*|*v*)
      mg_failure owned-provider-trace "Owned provider mode refuses shell tracing"; mg_status error; return 0 ;;
    esac
    [ "$platform/$config" = mac/Debug ] && [ -z "$device$physical$inventory" ] &&
      [ "$suite_count" -eq 1 ] &&
      [ "$suites" = " MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch" ] &&
      [ "${MEGURI_APP_SIGNING:-}" = team ] &&
      [[ "$MG_RUN_ID" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,95}$ ]] || {
        mg_ui_test__usage "owned sync requires team-signed Mac Debug, the single exact setup method and no device or inventory"; return 0;
      }
    mg_native_route__ambient || {
      mg_failure owned-provider-ambient "Owned provider ambient authority refused"; mg_status error; return 0;
    }
    for key in $(compgen -e); do
      case "$key" in TEST_RUNNER_*|MEGURI_OWNED_PROVIDER_*|MEGURI_MAC_UI_ALLOW_HOST)
        mg_failure owned-provider-ambient "Owned provider ambient authority refused"; mg_status error; return 0 ;;
      esac
    done
    local MG_APP_TEST_SCHEME=Meguri MG_APP_TEST_PLAN=UI
    export MG_APP_TEST_SCHEME MG_APP_TEST_PLAN
  fi
  if [ -n "$inventory" ]; then
    [ -z "$run_args" ] || { mg_ui_test__usage "--list-physical-devices cannot be combined with run arguments"; return 0; }
    mg_ui_test__physical_inventory || return 0
    mg_message "ui-test: physical iOS device inventory"
    mg_status pass
    return 0
  fi
  if [ -n "$physical" ]; then
    [ -z "$device" ] && [ "$platform" != mac ] || { mg_ui_test__usage "--physical-device conflicts with --device and --platform mac"; return 0; }
  fi
  [ -n "$suites" ] || { mg_ui_test__usage "name at least one --suite"; return 0; }
  if mg_ui_test__media_selected "$suites"; then
    media=1
    [ -z "$physical" ] || { mg_ui_test__usage "native media export capture is unsupported on physical devices; use the leased VM"; return 0; }
    # Its generated parent with media files, made once per checkout (a new
    # worktree has none) before any build or lease.
    if [ ! -d "$MG_ROOT/build/fixtures/recipes/typical-1" ] &&
        ! "$MG_TOOLS/mg" fixtures typical --seed 1 --media files >&2 </dev/null; then
      mg_failure native-media-source "the media parent build/fixtures/recipes/typical-1 is missing and could not be generated: tools/mg fixtures typical --seed 1 --media files"
      mg_status error
      return 0
    fi
  fi
  if mg_ui_test__review_selected "$suites"; then
    review=1
    [ -z "$physical" ] && [ "$config" = Debug ] || { mg_ui_test__usage "native media review requires Debug and the leased VM"; return 0; }
    python3 "$MG_ROOT/tools/scripts/native-media-review-capture.py" start "$MG_RUN_DIR" "$platform" || {
      mg_failure native-review "fresh bounded source capture refused"; mg_status error; return 0;
    }
  fi
  MG_UI_REVIEW_PLATFORM="$platform"
  MG_UI_MEDIA_PLATFORM="$platform"
  for s in $suites; do only[${#only[@]}]="-only-testing:MeguriUITests/$s"; done
  if [ "$platform" = mac ] && [ "$config" = Debug ] && [ -z "$device" ] &&
      [ -z "$physical" ] && [ -z "$inventory" ] && [ "$suite_count" -eq 1 ] &&
      [ "$suites" = " MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch" ]; then
    MG_UI_MAC_PROFILE_SELECTOR=MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch
  fi
  if [ -n "$owned" ]; then
    mg_ui_test__owned "$owned"
    return 0
  fi
  mg_detail suites "$(printf '%s\n' $suites | jq -R . | jq -sc .)"
  mg_detail platform "$(jq -n --arg p "$platform" '$p')"
  mg_detail config "$(jq -n --arg c "$config" '$c')"
  if [ "$config" = Profile ]; then
    export MG_APP_TEST_SCHEME=Meguri-Profile MG_APP_TEST_PLAN=Performance
    build_args=(Performance Meguri-Profile)
  fi
  if [ -n "$physical" ]; then
    mg_detail destination_platform '"ios-device"'
    mg_ui_test__physical_check "$physical" "$platform" || return 0
    mg_host_lease --no-session || return 0
    mg_ui_test__physical_check "$physical" "$platform" || return 0
    if [ "$config" = Debug ]; then
      export MG_APP_TEST_SCHEME=Meguri MG_APP_TEST_PLAN=UI
    fi
    mg_app_build ios-device "$config" ${build_args[0]+"${build_args[@]}"} || return 0
    mg_ui_test__physical_check "$physical" "$platform" || return 0
    mg_app_xcodebuild_test "$MG_RUN_DIR/ios-device" ios-device "platform=iOS,id=$physical" "$config" "${only[@]}"
    result=$?
    mg_ui_test__report "$MG_RUN_DIR/ios-device" "$result" physical "$suites"
    return 0
  fi
  if [ "$platform" = mac ]; then
    [ -z "$device" ] || { mg_ui_test__usage "--device is for simulators"; return 0; }
    mg_ui_suite_fixtures $suites || { mg_status error; return 0; }
    mg_mac_ui_begin || return 0
    mg_app_build mac "$config" ${build_args[0]+"${build_args[@]}"} || return 0
    mg_mac_ui_take || return 0
    mg_ui_suite_fixtures_stage "$MG_MAC_RUNNER"
    result=$?
    if [ "$result" -ne 0 ]; then
      if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
      return 0
    fi
    if [ -n "$media" ]; then
      mg_ui_test__media_prepare "$MG_MAC_RUNNER"
      result=$?
      if [ "$result" -ne 0 ]; then
        if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
        return 0
      fi
    fi
    if [ -n "$review" ]; then
      mg_ui_test__review_prepare "$MG_MAC_RUNNER" mac
      result=$?
      if [ "$result" -ne 0 ]; then
        mg_failure native-review "prelaunch frozen review preparation/staging failed; artifacts retained"
        mg_status error; return 0
      fi
    fi
    since="$(mg_app_log_now)"
    mg_app_xcodebuild_test "$MG_RUN_DIR/mac" mac "platform=macOS,arch=arm64" "$config" "${only[@]}"
    result=$?
    if [ "$result" -le 1 ] && mg_app_mac_input_refused "$since" "$(printf '%s' "${suites# }" | tr ' ' ',')"; then
      mg_app_mac_input_result "$MG_RUN_DIR/mac.xcresult" "$result" test
      result=$?
    fi
    mg_ui_test__report "$MG_RUN_DIR/mac" "$result"
    return 0
  fi
  mg_ui_suite_fixtures $suites || { mg_status error; return 0; }
  mg_sim_begin || return 0
  if [ -n "$device" ]; then
    mg_sim_check_given "$device" || return 0
  fi
  mg_app_build ios-sim "$config" ${build_args[0]+"${build_args[@]}"} || return 0
  mg_sim_take || return 0
  mg_ui_suite_fixtures_stage "$MG_SIM_RUNNER"
  result=$?
  if [ "$result" -ne 0 ]; then
    if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
    return 0
  fi
  if [ -n "$device" ]; then
    mg_sim_check_given "$device" || return 0
    udid="$device"
  else
    mg_sim_lease "$platform" release || return 0
    udid="$MG_SIM_UDID"
    mg_sim_check_runtime "$udid" || return 0
  fi
  MG_UI_MEDIA_DEVICE="$udid"
  if [ -n "$media" ]; then
    mg_ui_test__media_prepare "$MG_SIM_RUNNER"
    result=$?
    if [ "$result" -ne 0 ]; then
      if [ "$result" -eq 2 ]; then mg_queued_settle; else mg_status error; fi
      return 0
    fi
  fi
  if [ -n "$review" ]; then
    mg_ui_test__review_prepare "$MG_SIM_RUNNER" ios-sim
    result=$?
    if [ "$result" -ne 0 ]; then
      mg_failure native-review "prelaunch frozen review preparation/staging failed; artifacts retained"
      mg_status error; return 0
    fi
  fi
  mg_sim_prepare "$udid" || return 0
  mg_app_xcodebuild_test "$MG_RUN_DIR/$platform" "$platform" "id=$udid" "$config" "${only[@]}"
  result=$?
  mg_ui_test__report "$MG_RUN_DIR/$platform" "$result"
}

# All existing build/VM helpers retain their diagnostics in private state for
# this explicit mode. Only fixed failure IDs and checked case counts escape.
mg_ui_test__owned() {
  local receipt="$1" private="$MG_RUN_DIR/private" previous_umask
  previous_umask="$(umask)"; umask 077
  case "$MG_RUN_DIR" in "$MG_ROOT/build/mg/ui-test/$MG_RUN_ID") ;; *)
    mg_failure owned-provider-run "Owned provider run identity refused"; mg_status fail; umask "$previous_umask"; return ;;
  esac
  if ! mkdir -m 700 "$private"; then
    mg_failure owned-provider-private "Owned provider private storage refused"; mg_status fail; umask "$previous_umask"; return
  fi
  MG_NR_CASE_TOTAL=0; MG_NR_CASE_PASSED=0; MG_NR_CASE_FAILED=0; MG_NR_CASE_SKIPPED=0
  MG_NR_UI_ATTEMPT=0; MG_NR_UI_OK=0; MG_NR_FIRST=; MG_NR_STAGE=preparation
  MG_NR_CLEAN_PENDING=0; MG_NR_REVIEW_OK=0; MG_NR_PRIVATE_RC=1
  if mg_ui_test__owned_private "$private" "$receipt" >"$private/control.log" 2>&1; then
    MG_NR_PRIVATE_RC=0
  else
    MG_NR_PRIVATE_RC=$?
    MG_NR_FIRST="${MG_NR_FIRST:-$MG_NR_STAGE}"
  fi
  if ! printf '%s\n' "$MG_NR_PRIVATE_RC" >"$private/return-code" 2>>"$private/control.log"; then
    MG_NR_PRIVATE_RC=1; MG_NR_FIRST="${MG_NR_FIRST:-private-return}"
  fi
  # Drain the unchanged VM/profile/lease exit actions while their raw output
  # and MG state still have this private boundary, including early failures.
  mg_ui_test__owned_finish "$private" >>"$private/control.log" 2>&1
  umask "$previous_umask"
  mg_count total "$MG_NR_CASE_TOTAL"; mg_count passed "$MG_NR_CASE_PASSED"
  mg_count failed "$MG_NR_CASE_FAILED"; mg_count skipped "$MG_NR_CASE_SKIPPED"
  if [ "$MG_NR_PRIVATE_RC" = 0 ] && [ -z "$MG_NR_FIRST" ] &&
      [ "$MG_NR_REVIEW_OK/$MG_NR_UI_OK/$MG_NR_CLEAN_OK/$MG_NR_CLEAN_PENDING" = 1/1/1/0 ] &&
      [ "$MG_NR_CASE_TOTAL/$MG_NR_CASE_PASSED/$MG_NR_CASE_FAILED/$MG_NR_CASE_SKIPPED" = 1/1/0/0 ] &&
      [ -z "${MG_MAC_VM_TESTING:-}${MG_MAC_VM_LOST:-}" ]; then
    mg_detail committed_review '"owned-synthetic-card-good-committed"'
    mg_status pass
    return 0
  fi
  mg_failure "owned-provider-${MG_NR_FIRST:-$MG_NR_STAGE}" "Owned provider journey failed; private evidence retained"
  if [ "$MG_NR_CLEAN_PENDING" = 1 ] || { [ "$MG_NR_UI_ATTEMPT" = 1 ] && [ "$MG_NR_UI_OK" != 1 ]; }; then
    mg_failure owned-provider-cleanup-pending "Owned provider cleanup pending; no credential cleanup assertion"
  fi
  mg_status fail
}

mg_ui_test__owned_finish() {
  local MG_RUN_DIR="$1" MG_STATE="$1/state"
  mg__run_exit_cmds
}

# Fixed synthetic input only. Both hosts run the same bounded admission and
# immutable queries; only private files receive IDs, hashes or diagnostics.
mg_ui_test__owned_review_fixture() {
  local phase="$1" script binding="$MG_NR_PRIVATE/review-$1.binding"
  case "$phase" in prepare|delivered|pre-ui|after) ;; *) return 1 ;; esac
  script="$(cat <<'PY'
import hashlib, json, os, pathlib, signal, sqlite3, stat, sys
signal.alarm(10)
try:
    root = sys.argv[1]
    if root != os.path.abspath(root) or root == '/': raise ValueError()
    parts = pathlib.PurePath(root).parts[1:] + ('build', 'fixtures', 'recipes', 'owned-sync-review-1')
    parents = [(os.open('/', os.O_RDONLY | os.O_DIRECTORY), '/')]
    for part in parts:
        fd, path = parents[-1]
        parents.append((os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                dir_fd=fd), os.path.join(path, part)))
    directory = parents[-1][0]
    name = 'collection.anki2'
    path = os.path.join(parents[-1][1], name)
    def identity(s):
        return (s.st_dev, s.st_ino, s.st_mode, s.st_nlink, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    def closed_bytes():
        for suffix in ('-wal', '-shm', '-journal'):
            try: os.stat(name + suffix, dir_fd=directory, follow_symlinks=False)
            except FileNotFoundError: pass
            else: raise ValueError()
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        with os.fdopen(fd, 'rb') as source:
            before = os.fstat(source.fileno())
            if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or not 0 < before.st_size <= 8*1024*1024:
                raise ValueError()
            data = source.read(8*1024*1024 + 1)
            if len(data) != before.st_size or identity(os.fstat(source.fileno())) != identity(before):
                raise ValueError()
        if identity(os.stat(name, dir_fd=directory, follow_symlinks=False)) != identity(before):
            raise ValueError()
        return identity(before), hashlib.sha256(data).hexdigest()
    before = closed_bytes()
    db = sqlite3.connect(pathlib.Path(path).as_uri() + '?mode=ro&immutable=1', uri=True, timeout=0)
    try:
        db.execute('PRAGMA query_only=ON')
        db.execute('PRAGMA trusted_schema=OFF')
        db.execute('PRAGMA temp_store=MEMORY')
        db.set_progress_handler(lambda: 1, 100000)
        tables = db.execute("SELECT type,sql FROM sqlite_master WHERE name IN ('cards','notes','revlog')").fetchall()
        if len(tables) != 3 or any(t != 'table' or not sql.lstrip().upper().startswith('CREATE TABLE ') for t,sql in tables):
            raise ValueError()
        if db.execute('SELECT (SELECT count(*) FROM cards),(SELECT count(*) FROM notes),(SELECT count(*) FROM revlog)').fetchone() != (1,1,0):
            raise ValueError()
        rows = db.execute("""SELECT c.id,n.id FROM cards c JOIN notes n ON n.id=c.nid
            WHERE typeof(c.id)='integer' AND c.id>0 AND typeof(c.nid)='integer'
              AND typeof(n.id)='integer' AND n.id>0
              AND typeof(c.queue)='integer' AND c.queue=0
              AND typeof(c.type)='integer' AND c.type=0
              AND typeof(c.reps)='integer' AND c.reps=0""").fetchall()
        if len(rows) != 1 or any(type(v) is not int or v <= 0 for v in rows[0]): raise ValueError()
    finally:
        db.close()
    if closed_bytes() != before: raise ValueError()
    for fd, path in parents:
        s, current = os.fstat(fd), os.stat(path, follow_symlinks=False)
        if (s.st_dev,s.st_ino,s.st_mode) != (current.st_dev,current.st_ino,current.st_mode): raise ValueError()
    for fd, path in reversed(parents): os.close(fd)
    print(json.dumps(dict(zip(('card_id','note_id'), rows[0])), separators=(',',':')))
    print(before[1])
except Exception:
    raise SystemExit(1) from None
PY
)" || return 1
  python3 -B -I - "$MG_ROOT" <<<"$script" >"$binding" || return 1
  if [ "$phase" = prepare ]; then
    sed -n '1p' "$binding" >"$MG_NR_PRIVATE/review-expected.json" &&
      sed -n '2p' "$binding" >"$MG_NR_PRIVATE/review-source.sha256" || return 1
    MG_NR_REVIEW_SOURCE_SHA="$(cat "$MG_NR_PRIVATE/review-source.sha256")"
    [[ "$MG_NR_REVIEW_SOURCE_SHA" =~ ^[0-9a-f]{64}$ ]] || return 1
  else
    cmp -s "$MG_NR_PRIVATE/review-prepare.binding" "$binding" || return 1
    (set -o pipefail
      sed -n '1p' "$binding" | cmp -s "$MG_NR_PRIVATE/review-expected.json" - &&
      sed -n '2p' "$binding" | cmp -s "$MG_NR_PRIVATE/review-source.sha256" -) || return 1
  fi
  case "$phase" in delivered|pre-ui)
    [ "$MG_VM_FIXTURE" = "$MG_ROOT/build/fixtures/recipes/owned-sync-review-1" ] || return 1
    mg_mac_vm_ssh_in "python3 -B -I - $(mg_mac_vm_q "$MG_ROOT")" <<<"$script" \
      >"$MG_NR_PRIVATE/review-$phase-guest.binding" || return 1
    cmp -s "$binding" "$MG_NR_PRIVATE/review-$phase-guest.binding" || return 1
    ;;
  esac
}

mg_ui_test__owned_private() {
  local MG_RUN_DIR="$1" MG_STATE="$1/state" receipt="$2" input rc=0 app
  mkdir -m 700 "$MG_STATE" || return 1
  MG_NR_PHASE=native-owned-journey-fixture; MG_NR_RECEIPT="$receipt"
  MG_NR_CLEANED=0; MG_NR_CLEAN_OK=0; MG_NR_GUEST_ATTEMPT=0; MG_NR_GUEST_ID=
  MG_NR_NETWORK_ATTEMPT=0; MG_NR_INIT_ATTEMPT=0; MG_NR_RUNTIME_ATTEMPT=0; MG_NR_FIXTURE_ATTEMPT=0
  MG_NR_NETWORK_ID=; MG_NR_RUNTIME_ID=; MG_NR_FIXTURE_ID=; MG_NR_CA_SHA=; MG_NR_CA_BYTES=
  MG_NR_INGRESS_ATTEMPT=0; MG_NR_RELAY_ATTEMPT=0; MG_NR_INGRESS_ID=; MG_NR_RELAY_ID=; MG_NR_TARGET=
  MG_NR_INGRESS=; MG_NR_RELAY=
  MG_NR_HOST_STAGE_ATTEMPT=0; MG_NR_GUEST_STAGE_ATTEMPT=0; MG_NR_TRANSPORT_SHA=; MG_NR_CREDENTIALS_SHA=
  MG_NR_PRIVATE="$MG_RUN_DIR/native-route"; MG_NR_STAGING="$MG_NR_PRIVATE/guest"
  mg_native_route__stage source-before
  MG_NR_COMMIT="$(mg_meguri_provider__commit)" &&
    MG_NR_SCRIPT_SHA="$(shasum -a 256 "$MG_ROOT/server/tools/package-smoke.mjs" | awk '{print $1}')" &&
    mg_native_route__source || return 1
  input="$(mg_app_input_digest)"; [[ "$input" =~ ^[0-9a-f]{64}$ ]] || return 1
  MG_NR_REVIEW_APP_INPUT="$input"
  mg_native_route__stage package-receipt
  mg_sync_package__receipt "$receipt" || return 1
  # The shared receipt guard also resolves canonical parents. Here the caller
  # must spell the exact run itself, never latest, a symlink or another alias.
  [ "$receipt" = "$MG_ROOT/build/mg/sync-server-package/$(jq -er .run_id "$receipt")/result.json" ] &&
    [ ! -L "$receipt" ] && [ ! -L "$(dirname "$receipt")" ] || return 1
  MG_NR_RUNTIME_IMAGE="$(jq -er .details.package.image_id "$receipt")" &&
    MG_NR_TOOL_IMAGE="$(jq -er .details.package.client_image_id "$receipt")" || return 1
  mg_native_route__stage vm-selection
  # Reuse the normal Mac build/profile and its one VM lease. No host admission,
  # inventory, recursive job or second lease acquisition exists in this mode.
  mg_mac_vm_probe || return 1
  MG_MAC_RUNNER=vm
  mg_native_route__stage build
  mg_app_build mac Debug || return 1
  mg_native_route__stage vm-delivery
  mg_mac_ui_take || return 1
  mg_app_mac_agent_profile_check && [ "$input" = "$(mg_app_input_digest)" ] || return 1
  app="$(mg_app_products mac Debug)/Meguri.app"
  codesign --verify --strict "$app" || return 1
  mg_native_route__stage ownership-plan
  MG_NR_OWNER="$(mg_meguri_provider__nonce)"; [[ "$MG_NR_OWNER" =~ ^[0-9a-f]{32}$ ]] || return 1
  MG_NR_NETWORK="meguri-native-net-$MG_RUN_ID-$MG_NR_OWNER"
  MG_NR_GUEST="/tmp/meguri-native-route-$MG_RUN_ID-$MG_NR_OWNER"
  MG_NR_RUNTIME="meguri-native-runtime-$MG_RUN_ID"; MG_NR_FIXTURE="meguri-native-fixture-$MG_RUN_ID"
  mkdir -m 700 "$MG_NR_PRIVATE" "$MG_NR_STAGING" "$MG_NR_PRIVATE/ui" || return 1
  mg_on_exit mg_ui_test__owned_cleanup
  mg_native_route__stage review-fixture-prepare
  mg_ui_test__owned_review_fixture prepare || return 1
  mg_native_route__stage review-fixture-delivery
  mg_vm_fixture "$MG_ROOT/build/fixtures/recipes/owned-sync-review-1" &&
    mg_ui_test__owned_review_fixture delivered && mg_native_route__source &&
    [ "$input" = "$(mg_app_input_digest)" ] || return 1
  if ! mg_native_route__execute; then
    mg_native_route__failure; rc=1
  elif [ -n "$MG_NR_FIRST" ] || [ "$MG_NR_UI_OK" != 1 ] ||
      [ "$MG_NR_CASE_TOTAL/$MG_NR_CASE_PASSED/$MG_NR_CASE_FAILED/$MG_NR_CASE_SKIPPED" != 1/1/0/0 ]; then
    mg_native_route__stage review-ui-result
    mg_native_route__failure; rc=1
  else
    mg_native_route__stage review-arrival
    if mg_ui_test__owned_review_fixture after && mg_native_route__source &&
        [ "$input" = "$(mg_app_input_digest)" ] &&
        [ -z "${MG_MAC_VM_TESTING:-}${MG_MAC_VM_LOST:-}" ] &&
        mg_native_route__journey_verify_review "$MG_NR_PRIVATE/review-expected.json"; then
      MG_NR_REVIEW_OK=1
    else
      mg_native_route__failure; rc=1
    fi
  fi
  mg_ui_test__owned_cleanup || { MG_NR_CLEAN_PENDING=1; rc=1; }
  mg_native_route__stage source-after
  if ! mg_native_route__source || [ "$input" != "$(mg_app_input_digest)" ] ||
      ! mg_ui_test__owned_review_fixture after; then
    mg_native_route__failure; rc=1
  fi
  [ "$rc" = 0 ] && [ -z "$MG_NR_FIRST" ] &&
    [ "$MG_NR_REVIEW_OK/$MG_NR_CLEAN_OK/$MG_NR_CLEAN_PENDING" = 1/1/0 ] &&
    [ -z "${MG_MAC_VM_TESTING:-}${MG_MAC_VM_LOST:-}" ]
}

mg_ui_test__owned_cleanup() {
  local MG_STATE="$MG_NR_PRIVATE/../state"
  mg_native_route__stage owned-cleanup
  # An unjoined original UI worker still owns its inputs and server. Preserve
  # the VM lease fence and everything else; do not guess that it has exited.
  if [ -n "${MG_MAC_VM_TESTING:-}${MG_MAC_VM_LOST:-}" ]; then
    MG_NR_CLEAN_PENDING=1
    mg_native_route__failure
    return 1
  fi
  mg_native_route__cleanup >>"$MG_NR_PRIVATE/cleanup.log" 2>&1 || {
    MG_NR_CLEAN_PENDING=1; mg_native_route__failure; return 1;
  }
}

mg_ui_test__owned_run() {
  local script prefix="$MG_NR_PRIVATE/ui/case" result key summary
  mg_native_route__stage review-fixture-pre-ui
  mg_ui_test__owned_review_fixture pre-ui && mg_native_route__source &&
    [ "$MG_NR_REVIEW_APP_INPUT" = "$(mg_app_input_digest)" ] || return 1
  script="$(mg_native_route__journey_file_script)" || return 1
  mg_mac_vm_ssh_in "python3 -B -I - $(mg_mac_vm_q output "$MG_ROOT" "$MG_RUN_ID" "$MG_NR_OWNER")" <<<"$script" \
    >"$MG_NR_PRIVATE/ui/guest-output.json" || return 1
  local TEST_RUNNER_MEGURI_OWNED_PROVIDER_JOURNEY_RUN_ID="$MG_RUN_ID"
  local TEST_RUNNER_MEGURI_OWNED_PROVIDER_JOURNEY_CREDENTIALS_SHA256="$MG_NR_CREDENTIALS_SHA"
  export TEST_RUNNER_MEGURI_OWNED_PROVIDER_JOURNEY_RUN_ID TEST_RUNNER_MEGURI_OWNED_PROVIDER_JOURNEY_CREDENTIALS_SHA256
  for key in $(compgen -e); do
    case "$key" in
      TEST_RUNNER_MEGURI_OWNED_PROVIDER_JOURNEY_RUN_ID|TEST_RUNNER_MEGURI_OWNED_PROVIDER_JOURNEY_CREDENTIALS_SHA256) ;;
      TEST_RUNNER_*) return 1 ;;
    esac
  done
  MG_NR_UI_ATTEMPT=1
  mg_vm_xcodebuild_test "$prefix" UI macosx "platform=macOS,arch=arm64" Debug \
    -parallel-testing-enabled NO \
    -only-testing:MeguriUITests/MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch
  result=$?
  # The helper's errors and all raw XCTest content stay in private state/logs.
  # Its original exit/result/uncertain-worker guards remain authoritative.
  [ -z "${MG_MAC_VM_RUN_ERROR:-}${MG_MAC_VM_RUN_QUEUED:-}${MG_MAC_VM_LOST:-}" ] || return 1
  xcrun xcresulttool get test-results summary --path "$prefix.xcresult" >"$MG_NR_PRIVATE/ui/summary.json" 2>/dev/null || return 1
  xcrun xcresulttool get test-results tests --path "$prefix.xcresult" >"$MG_NR_PRIVATE/ui/tests.json" 2>/dev/null || return 1
  summary="$(jq -cse 'if length!=1 then error("summary") else .[0] end |
    select(all(.passedTests,.failedTests,.skippedTests;
      type=="number" and .>=0 and .==floor)) |
    {total:(.passedTests+.failedTests+.skippedTests),passed:.passedTests,failed:.failedTests,skipped:.skippedTests}' \
    "$MG_NR_PRIVATE/ui/summary.json")" || return 1
  MG_NR_CASE_TOTAL="$(jq -r .total <<<"$summary")"; MG_NR_CASE_PASSED="$(jq -r .passed <<<"$summary")"
  MG_NR_CASE_FAILED="$(jq -r .failed <<<"$summary")"; MG_NR_CASE_SKIPPED="$(jq -r .skipped <<<"$summary")"
  [ "$result" = 0 ] && jq -se 'length==1 and (.[0] |
    .result=="Passed" and .passedTests==1 and .failedTests==0 and .skippedTests==0)' \
    "$MG_NR_PRIVATE/ui/summary.json" >/dev/null || return 1
  jq -se 'length==1 and (.[0] |
    [.. | objects | select(.nodeType?=="Test Case")] as $cases |
    ($cases|length)==1 and $cases[0].result=="Passed" and
    ($cases[0].nodeIdentifier | test("^(MeguriUITests/)?MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch(\\(\\))?$")))' \
    "$MG_NR_PRIVATE/ui/tests.json" >/dev/null || return 1
  MG_NR_UI_OK=1
}

# mg_ui_test__report <prefix> <test result 0|1|2|3|4> [physical <selectors>]: counts and failing tests
# from the result bundle, then the status. 3: dropped input (queued,
# mac-input; independent failures were already recorded). 4: the test VM
# could not be reached (queued, mac-vm).
mg_ui_test__report() {
  local prefix="$1" result="$2" summary f cases capture_result
  if { [ "$result" -eq 3 ] || [ "$result" -eq 4 ]; } && [ -n "${MG_UI_REVIEW_READY:-}" ]; then
    mg_failure native-review "native review did not execute; no proof captured"
    [ ! -d "$prefix.xcresult" ] || mg_artifact xcresult "$prefix.xcresult" "unexecuted native review result bundle"
    mg_status error
    return 0
  fi
  if [ "$result" -eq 4 ] && [ "${3:-}" != physical ]; then
    mg_status pass
    mg_queued_settle
    return 0
  fi
  summary="$(
    set -o pipefail
    xcrun xcresulttool get test-results summary --path "$prefix.xcresult" 2>/dev/null |
    jq -c '{passed: .passedTests, failed: .failedTests, skipped: .skippedTests, outcome: .result}')" || summary=""
  if [ "${3:-}" = physical ]; then
    case "$result" in 0|1|2) ;; *) result=2 ;; esac
    if ! jq -se 'length == 1 and (.[0] | type == "object" and all(.passed, .failed, .skipped;
        type == "number" and . >= 0 and . == floor))' <<<"$summary" >/dev/null 2>&1; then
      mg_failure physical-device-results "physical UI tests require a readable summary with integer test counts"
      mg_status error
      [ ! -d "$prefix.xcresult" ] || mg_artifact xcresult "$prefix.xcresult" "result bundle"
      return 0
    fi
    if ! jq -e '.outcome == "Passed" and .passed > 0 and .failed == 0 and .skipped == 0' <<<"$summary" >/dev/null; then
      mg_failure physical-device-results "physical UI tests require passed tests, zero failures and zero skipped tests"
      result=1
    fi
    cases="$(set -o pipefail
      xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null |
      jq -cse '
        if length != 1 or (.[0] | type) != "object" or (.[0].testNodes | type) != "array"
        then error("invalid test tree") else .[0] end
        | if all(.testNodes[]; type == "object") and all(.testNodes[] | .. | objects;
            all(.children?, .subTestNodes?; . == null or (type == "array" and all(.[]; type == "object"))))
          then . else error("invalid test nodes") end
        | [.testNodes[] | .. | objects | select(.nodeType? == "Test Case")
          | select(([.children[]?, .subTestNodes[]? | .. | objects | select(.nodeType? == "Test Case")] | length) == 0)
          | if (.nodeIdentifier | type) != "string" or (.result | type) != "string"
            then error("invalid test case") else . end
          | if (.nodeIdentifier | test("^(MeguriUITests/)?[A-Z][A-Za-z0-9]*/[A-Za-z0-9_]+(\\(\\))?$"))
            then . else error("invalid method identifier") end
          | {id: (.nodeIdentifier | capture("^(MeguriUITests/)?(?<id>[A-Z][A-Za-z0-9]*/[A-Za-z0-9_]+)(\\(\\))?$").id), result}]' 2>/dev/null)" || cases=""
    if [ -z "$cases" ]; then
      mg_failure physical-device-results "physical UI tests require a readable test tree"
      result=2
    elif ! jq -e --arg selectors "${4:-}" --argjson count "$(jq .passed <<<"$summary")" '
        . as $cases | ($selectors | split(" ") | map(select(length > 0))) as $requested
        | length > 0 and length == $count and all(.[]; .result == "Passed") and ($requested | length > 0)
          and all($requested[]; . as $s | any($cases[];
            if ($s | contains("/")) then .id == $s else (.id | split("/")[0]) == $s end))' <<<"$cases" >/dev/null; then
      mg_failure physical-device-results "every physical selector must have a passed test method"
      [ "$result" -eq 2 ] || result=1
    fi
  elif [ "$result" -eq 0 ]; then
    if ! jq -se 'length == 1 and (.[0] | type == "object" and all(.passed, .failed, .skipped;
        type == "number" and . >= 0 and . == floor))' <<<"$summary" >/dev/null 2>&1; then
      mg_failure ui-test-results "UI tests require a readable summary with integer test counts"
      summary=""
      result=2
    elif ! jq -e '.outcome == "Passed" and .passed > 0 and .failed == 0' <<<"$summary" >/dev/null; then
      mg_failure ui-test-results "UI tests require a Passed outcome, at least one passed test and zero failed tests"
      result=1
    fi
  fi
  if [ "$result" -eq 0 ] && [ -n "${MG_UI_MEDIA_READY:-}" ]; then
    if ! jq -e '.skipped == 0' <<<"$summary" >/dev/null; then
      mg_failure native-media-export "native media destination capture requires a non-skipped run"
      result=1
    else
      mg_ui_test__media_capture "$prefix" "$MG_UI_MEDIA_PLATFORM" "$MG_UI_MEDIA_DEVICE"
      capture_result=$?
      if [ "$capture_result" -eq 2 ]; then
        if [ -n "${MG_UI_REVIEW_READY:-}" ]; then
          mg_failure native-review "native review capture did not finish; no proof captured"
          result=2
        else
          mg_status pass
          mg_queued_settle
          return 0
        fi
      elif [ "$capture_result" -ne 0 ]; then
        mg_detail native_media_capture_failure "$(jq -nc --arg phase "$MG_UI_MEDIA_PHASE" '{phase: $phase}')"
        case "$MG_UI_MEDIA_PHASE" in
          tree) mg_failure native-media-export "FIXED-PHASE:tree: exact media method missing, duplicate or not Passed; no export evidence registered" ;;
          export) mg_failure native-media-export "FIXED-PHASE:export: own-run selective attachment export refused; no export evidence registered" ;;
          manifest) mg_failure native-media-export "FIXED-PHASE:manifest: expected one safe matching nonfailure package attachment; no export evidence registered" ;;
          copy) mg_failure native-media-export "FIXED-PHASE:copy: retained native package copy refused; no export evidence registered" ;;
          receipt) mg_failure native-media-export "FIXED-PHASE:receipt: fixture or destination receipt validation failed; no export evidence registered" ;;
        esac
        result=2
      fi
    fi
  fi
  if [ "$result" -eq 0 ] && [ -n "${MG_UI_REVIEW_READY:-}" ]; then
    if ! jq -e '.skipped == 0' <<<"$summary" >/dev/null ||
        ! mg_ui_test__review_capture "$prefix" "$MG_UI_REVIEW_PLATFORM" "$MG_UI_MEDIA_DEVICE"; then
      mg_failure native-review "exact native review/capture/frozen-input/strict package proof refused; artifacts retained"
      result=2
    fi
  fi
  if [ -n "$summary" ]; then
    mg_count passed "$(jq .passed <<<"$summary")"
    mg_count failed "$(jq .failed <<<"$summary")"
    mg_count skipped "$(jq .skipped <<<"$summary")"
    mg_count total "$(jq '.passed + .failed + .skipped' <<<"$summary")"
    [ "$result" -eq 3 ] || [ -n "${MG_MAC_INPUT_REFUSED:-}" ] || xcrun xcresulttool get test-results tests --path "$prefix.xcresult" 2>/dev/null | jq -c '
      [.. | objects | select(.nodeType? == "Test Case" and .result? == "Failed")
        | {id: .nodeIdentifier, message: ([.children[]? | select(.nodeType == "Failure Message") | .name] | join("; ") | .[0:800])}] | .[]' |
      while IFS= read -r f; do mg_failure "test/$(jq -r .id <<<"$f")" "$(jq -r .message <<<"$f")"; done
  fi
  [ ! -d "$prefix.xcresult" ] || mg_artifact xcresult "$prefix.xcresult" "result bundle"
  case "$result" in
    0)
      mg_message "ui-test: $(jq -r '"\(.passed) passed, \(.skipped) skipped"' <<<"${summary:-{\"passed\":0,\"skipped\":0\}}")"
      mg_status pass
      ;;
    1)
      mg_message "ui-test: $(jq -r '.failed' <<<"${summary:-{\"failed\":\"some\"\}}") test(s) failed"
      mg_status fail
      mg_queued_settle
      ;;
    3)
      mg_message "ui-test: queued: macOS dropped the input XCUITest synthesized in $(mg_queued_parts_text); the suites did not test the app"
      mg_status pass
      mg_queued_settle
      ;;
    *)
      mg_message "ui-test: the suites could not run; see log.txt"
      mg_status error
      ;;
  esac
}
