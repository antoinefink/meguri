# Snapshot matrix runs (U10, R1970-R1975). Sourced by tools/mg; a host job.
#
# MeguriSnapshotTests renders every registered subject (MatrixRegistry) in
# its matrix cells inside the app, compares each image with its baseline in
# app/Snapshots/<model>-<OS build>/ and writes results.jsonl (and new images)
# to the run directory; the tests never write baselines. A device class
# renders on the pool simulator whose screen it is (`--platform iphone`
# leases the iphone and the iphone-small kinds: the small class needs a
# small screen's safe area), with its own baselines. On a simulator the
# job runs the tests once per settings group (Reduce Transparency, Reduce
# Motion, Liquid Glass opacity), set with devicectl before each run (spike
# S5) and reset afterwards; a plan run lists the groups first. Every group
# renders the settings probe, whatever the filter, and the job fails when a
# probe is missing or a setting left it unchanged. Simulators run in the
# macOS test VM (U8c, mac-vm.sh): the products, app/Snapshots (the baselines
# the tests read at their source path) and the inventory go there, and each
# run's output directory comes back. The Mac renders in-process on this Mac,
# off-screen; its test host is sandboxed, so its results are written in its
# container and copied here. `--record <filter>` then copies the images of
# failing or missing snapshots that match the filter into app/Snapshots/,
# leaving the others byte for byte; `--rebaseline` records everything for
# the current OS build and drops the baselines of other builds of the same
# model (R1974).
# Approved signature baselines (docs/design/approved-baselines.json) that
# changed beyond the tolerance fail the run as needs-human-review; an
# approved image whose OS build was dropped is compared with its successor
# (same model and cell, the build that replaced it), and approving the
# successor in the gallery retires the old entry.

mg_job snapshots host mg_snapshots \
  "Snapshot matrix (R1970-R1975): [--platform iphone|ipad|mac[,...]] [--device <leased udid>]... [--filter <glob>[,...]] [--record <glob>[,...]] [--rebaseline]; app-hosted renders compared with app/Snapshots/<model>-<os build>/ at perceptual precision 0.98, diff PNGs for failures, the approved-baseline guard (needs-human-review)"

MG_SNAP_DEFAULT_GROUP=rt0-rm0-t50
MG_SNAP_SUITES="GalleryComponentSnapshots ScreenSnapshots"

# mg_snapshots__kinds <platform>: the pool kinds (tools/sim-devices.json) a
# platform's device classes render on (SnapshotTarget in SnapshotMatrix.swift).
mg_snapshots__kinds() {
  case "$1" in
    iphone) echo "iphone iphone-small" ;;
    *) echo "$1" ;;
  esac
}

mg_snapshots__usage() {
  mg_failure usage "snapshots: $1" "tools/mg --list"
  mg_message "snapshots: $1"
  mg_status error
}

mg_snapshots() {
  local platforms="" devices=() filter="" mode=verify rebaseline=false p d kind kinds
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --platform)
        [ "$#" -ge 2 ] || { mg_snapshots__usage "--platform takes iphone, ipad or mac (comma-separated)"; return 0; }
        for p in $(printf '%s' "$2" | tr ',' ' '); do
          mg_word_in "$p" "iphone ipad mac" || { mg_snapshots__usage "--platform takes iphone, ipad or mac, not '$p'"; return 0; }
          mg_word_in "$p" "$platforms" || platforms="$platforms $p"
        done
        shift 2
        ;;
      --device)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_snapshots__usage "--device needs a UDID (tools/mg sim acquire <kind>)"; return 0; }
        devices[${#devices[@]}]="$2" && shift 2
        ;;
      --filter)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_snapshots__usage "--filter needs a glob over subject/state/cell names"; return 0; }
        filter="$2" && shift 2
        ;;
      --record)
        [ "$#" -ge 2 ] && [ -n "$2" ] || { mg_snapshots__usage "--record needs a glob over subject/state/cell names (e.g. 'designsystem.toast/*')"; return 0; }
        mode=record filter="$2" && shift 2
        ;;
      --rebaseline) rebaseline=true mode=record filter="" && shift ;;
      *) mg_snapshots__usage "unknown argument '$1'"; return 0 ;;
    esac
  done
  [ -n "$platforms" ] || platforms="iphone ipad mac"
  platforms="${platforms# }"
  # Simulators: where they run is decided before anything else touches them.
  [ "$platforms" = mac ] || mg_sim_begin || return 0
  # Each --device stands in for the pool kind it is; the job leases the others.
  kinds=""
  for p in $platforms; do [ "$p" = mac ] || kinds="$kinds $(mg_snapshots__kinds "$p")"; done
  MG_SNAP_DEVICES=""
  for d in ${devices[0]+"${devices[@]}"}; do
    kind="$("$MG_TOOLS/scripts/sim-pool.sh" status 2>/dev/null | jq -r --arg u "$d" 'first(.devices[] | select(.udid == $u) | .kind) // empty')"
    if [ -z "$kind" ] || ! mg_word_in "$kind" "$kinds"; then
      mg_snapshots__usage "--device $d is ${kind:+a $kind simulator, }not one of the kinds --platform $(printf '%s' "$platforms" | tr ' ' ',') renders on (${kinds# })"
      return 0
    fi
    case " $MG_SNAP_DEVICES " in *" $kind="*) mg_snapshots__usage "two --device options for the $kind kind"; return 0 ;; esac
    mg_sim_check_given "$d" || return 0
    MG_SNAP_DEVICES="$MG_SNAP_DEVICES $kind=$d"
  done
  mg_detail snapshots "$(jq -nc --arg m "$mode" --arg f "$filter" --argjson r "$rebaseline" --arg p "$platforms" \
    '{mode: $m, filter: $f, rebaseline: $r, platforms: ($p | split(" "))}')"
  # The app sources this run renders (tools/mg critic matches a11y runs to it).
  mg_detail input "$(jq -n --arg d "$(mg_app_input_digest)" 'if $d == "" then null else $d end')"

  mg_lint_ui_first || return 0
  if ! mg_image_tools python3 tools/scripts/screen-status/screen_status.py inventory-json \
    --out "$(mg_container_path "$MG_RUN_DIR/inventory.json")" >&2; then
    mg_failure inventory "could not read docs/design/screen-inventory.yaml; see log.txt" "tools/mg design-docs"
    mg_status error
    return 0
  fi
  case " $platforms " in *" iphone "* | *" ipad "*) mg_app_build ios-sim Debug Snapshot || return 0 ;; esac
  case " $platforms " in *" mac "*) mg_app_build mac Debug Snapshot || return 0 ;; esac

  MG_SNAP_MODE="$mode" MG_SNAP_FILTER="$filter" MG_SNAP_BROKEN=0
  # The simulators' lease (the test VM's) comes before the Mac's host lease,
  # whatever the platform order: every job that takes both takes them so.
  case " $platforms " in *" iphone "* | *" ipad "*) mg_snapshots__sim_lease || return 0 ;; esac
  for p in $platforms; do
    if [ "$p" = mac ]; then
      mg_snapshots__mac || return 0
      continue
    fi
    for kind in $(mg_snapshots__kinds "$p"); do
      d="$(printf '%s\n' $MG_SNAP_DEVICES | sed -n "s/^$kind=//p")"
      mg_snapshots__simulator "$kind" "$d" || return 0
    done
  done
  mg_snapshots__finish "$platforms" "$rebaseline"
}

# The host lease, taken once for the whole job (mg_host_lease is not
# re-entrant: a second call would wait for this job itself).
mg_snapshots__lease() {
  [ -z "${MG_SNAP_LEASED:-}" ] || return 0
  mg_host_lease || return 1
  MG_SNAP_LEASED=1
}

# The simulators' lease, taken once: the test VM's (with the products,
# app/Snapshots and the inventory copied in), or the host lease with
# MEGURI_SIM_ALLOW_HOST=1.
mg_snapshots__sim_lease() {
  [ -z "${MG_SNAP_SIM_LEASED:-}" ] || return 0
  if [ "$MG_SIM_RUNNER" = vm ]; then
    mg_sim_take || return 1
    if ! mg_mac_vm_copy_in "$MG_ROOT/app/Snapshots" "$MG_RUN_DIR/inventory.json"; then
      mg_message "snapshots: the test VM could not run the simulators: the baselines could not be copied into it"
      mg_status error
      return 1
    fi
  else
    mg_snapshots__lease || return 1
  fi
  MG_SNAP_SIM_LEASED=1
}

# mg_snapshots__xcodebuild <out dir> <destination> <only-testing suites|all> <env...>:
# test-without-building of the Snapshot plan with TEST_RUNNER_ variables;
# writes <out>.log and <out>.xcresult. A simulator run in the test VM writes
# its output directory there (TEST_RUNNER_MEGURI_SNAPSHOT_OUT, the same path)
# and it comes back here. Records tests that failed outside the snapshot
# suites (their snapshot failures come from results.jsonl).
mg_snapshots__xcodebuild() {
  local out="$1" dest="$2" suites="$3" only=() s rc summary e
  shift 3
  if [ "$suites" != all ]; then
    for s in $suites; do only[${#only[@]}]="-only-testing:MeguriSnapshotTests/$s"; done
  fi
  rm -rf "$out.xcresult"
  mkdir -p "$(dirname "$out")"
  if [ "$dest" != "platform=macOS,arch=arm64" ] && [ "$MG_SIM_RUNNER" = vm ]; then
    # mg_vm_xcodebuild_test passes the exported TEST_RUNNER_ variables.
    for e in "$@"; do export "${e?}"; done
    rm -rf "$out"
    mg_vm_xcodebuild_test "$out" Snapshot iphonesimulator "$dest" Debug ${only[0]+"${only[@]}"}
    rc=$?
    for e in "$@"; do unset "${e%%=*}"; done
    if [ -n "$MG_MAC_VM_RUN_ERROR" ]; then
      mg_artifact log "$out.log" "xcodebuild test $(basename "$(dirname "$out")")/$(basename "$out") (in the test VM)"
      MG_SNAP_BROKEN=1
      return 1
    fi
    # The run's output directory comes back (a run that wrote none has none).
    if mg_mac_vm_ssh "test -d $(mvl_q "$out")" >/dev/null 2>&1; then
      mg_sim_fetch "$out" || { MG_SNAP_BROKEN=1; return 1; }
    fi
  else
    env "$@" xcodebuild test-without-building -project app/Meguri.xcodeproj -scheme Meguri -testPlan Snapshot -configuration Debug \
      -destination "$dest" -derivedDataPath "$MG_APP_DD" -resultBundlePath "$out.xcresult" -collect-test-diagnostics never \
      ${only[0]+"${only[@]}"} >"$out.log" 2>&1
    rc=$?
  fi
  mg_artifact log "$out.log" "xcodebuild test $(basename "$(dirname "$out")")/$(basename "$out")"
  summary="$(xcrun xcresulttool get test-results summary --path "$out.xcresult" 2>/dev/null |
    jq -c '{result, passed: .passedTests, failed: .failedTests, skipped: .skippedTests}')" || summary=""
  echo "snapshots: $(basename "$out"): exit $rc ${summary:-no result}" >&2
  if [ -z "$summary" ]; then
    mg_failure xcodebuild-test "xcodebuild test-without-building could not run (exit $rc); see $out.log"
    MG_SNAP_BROKEN=1
    return 1
  fi
  xcrun xcresulttool get test-results tests --path "$out.xcresult" 2>/dev/null | jq -c '
    [.. | objects | select(.nodeType? == "Test Case" and .result? == "Failed")
      | select((.nodeIdentifier // "") | test("^(GalleryComponentSnapshots|ScreenSnapshots)/") | not)
      | {id: .nodeIdentifier, message: ([.children[]? | select(.nodeType == "Failure Message") | .name] | join("; ") | .[0:600])}] | .[]' |
    while IFS= read -r f; do
      mg_failure "test/$(jq -r .id <<<"$f")" "$(jq -r .message <<<"$f")"
    done
  return 0
}

# mg_snapshots__settings <udid> <group>: the simulator's Reduce Transparency,
# Reduce Motion and Liquid Glass opacity for a settings group (rt1-rm0-t50),
# everything else at its default.
mg_snapshots__settings() {
  local udid="$1" group="$2" rt rm t onoff
  onoff() { [ "$1" = 1 ] && echo on || echo off; }
  rt="$(printf '%s' "$group" | sed -E 's/^rt([01])-rm([01])-t([0-9]+)$/\1/')"
  rm="$(printf '%s' "$group" | sed -E 's/^rt([01])-rm([01])-t([0-9]+)$/\2/')"
  t="$(printf '%s' "$group" | sed -E 's/^rt([01])-rm([01])-t([0-9]+)$/\3/')"
  printf '%s' "$group" | grep -Eq '^rt[01]-rm[01]-t[0-9]+$' || { echo "snapshots: bad settings group '$group'" >&2; return 1; }
  mg_devicectl device settings appearance --device "$udid" --mode light --text-size large --larger-accessibility-sizes off \
    --increase-contrast off --reduce-transparency "$(onoff "$rt")" --reduce-motion "$(onoff "$rm")" \
    --liquid-glass-opacity "$(awk -v t="$t" 'BEGIN { printf "%.2f", t / 100 }')" >&2
}

# mg_snapshots__simulator <pool kind> [udid]: plan, then one run per group.
# The test process renders only the device classes of its kind
# (TEST_RUNNER_MEGURI_SNAPSHOT_TARGET).
mg_snapshots__simulator() {
  local kind="$1" udid="$2" dir="$MG_RUN_DIR/$1" groups g dest
  mg_snapshots__sim_lease || return 1
  if [ -z "$udid" ]; then
    mg_sim_lease "$kind" release || return 1
    udid="$MG_SIM_UDID"
    mg_sim_check_runtime "$udid" || return 1
  fi
  [ -z "$2" ] || mg_sim_check_given "$udid" || return 1
  dest="id=$udid"
  mg_on_exit "mg_snapshots__settings '$udid' '$MG_SNAP_DEFAULT_GROUP' >/dev/null 2>&1"
  if ! mg_snapshots__settings "$udid" "$MG_SNAP_DEFAULT_GROUP"; then
    mg_failure settings "devicectl could not set $udid's appearance settings (spike S5)"
    mg_status error
    return 1
  fi
  mg_snapshots__xcodebuild "$dir/plan" "$dest" "$MG_SNAP_SUITES" TEST_RUNNER_MEGURI_SNAPSHOT_MODE=plan \
    TEST_RUNNER_MEGURI_SNAPSHOT_FILTER="$MG_SNAP_FILTER" TEST_RUNNER_MEGURI_SNAPSHOT_GROUP="$MG_SNAP_DEFAULT_GROUP" \
    TEST_RUNNER_MEGURI_SNAPSHOT_TARGET="$kind" TEST_RUNNER_MEGURI_SNAPSHOT_OUT="$dir/plan" || { mg_status error; return 1; }
  groups="$( (echo "$MG_SNAP_DEFAULT_GROUP"; [ ! -f "$dir/plan/plan.jsonl" ] || jq -r 'select(.deferred == null) | .group' "$dir/plan/plan.jsonl" | sort -u) |
    awk '!seen[$0]++')"
  mg_detail "groups_$(printf '%s' "$kind" | tr '-' '_')" "$(printf '%s\n' $groups | jq -R . | jq -sc .)"
  for g in $groups; do
    if ! mg_snapshots__settings "$udid" "$g"; then
      mg_failure settings "devicectl could not set settings group $g on $udid (spike S5)"
      mg_status error
      return 1
    fi
    # The default group runs the whole bundle (matrix, mechanism, state
    # coverage and settings checks); the others only what that group renders.
    local suites="$MG_SNAP_SUITES SystemSettingsTests"
    [ "$g" != "$MG_SNAP_DEFAULT_GROUP" ] || suites=all
    mg_snapshots__xcodebuild "$dir/$g" "$dest" "$suites" TEST_RUNNER_MEGURI_SNAPSHOT_MODE="$MG_SNAP_MODE" \
      TEST_RUNNER_MEGURI_SNAPSHOT_FILTER="$MG_SNAP_FILTER" TEST_RUNNER_MEGURI_SNAPSHOT_GROUP="$g" TEST_RUNNER_MEGURI_SNAPSHOT_TARGET="$kind" \
      TEST_RUNNER_MEGURI_SNAPSHOT_OUT="$dir/$g" TEST_RUNNER_MEGURI_INVENTORY_JSON="$MG_RUN_DIR/inventory.json" || { mg_status error; return 1; }
  done
  mg_snapshots__settings "$udid" "$MG_SNAP_DEFAULT_GROUP" >/dev/null 2>&1
  return 0
}

# The Mac: one in-process run, off-screen windows only. The sandboxed test
# host writes in its own container; the results are moved here.
mg_snapshots__mac() {
  local dir="$MG_RUN_DIR/mac/mac" box
  box="$HOME/Library/Containers/$MG_APP_DEV_ID/Data/tmp/meguri-snapshots/$MG_RUN_ID"
  mg_snapshots__lease || return 1
  mg_app_mac_guard || return 1
  rm -rf "$box"
  mg_snapshots__xcodebuild "$dir" "platform=macOS,arch=arm64" all TEST_RUNNER_MEGURI_SNAPSHOT_MODE="$MG_SNAP_MODE" \
    TEST_RUNNER_MEGURI_SNAPSHOT_FILTER="$MG_SNAP_FILTER" TEST_RUNNER_MEGURI_SNAPSHOT_OUT="$box" \
    TEST_RUNNER_MEGURI_INVENTORY_JSON="$MG_RUN_DIR/inventory.json" || { mg_status error; return 1; }
  mkdir -p "$dir"
  if [ -d "$box" ]; then
    cp -R "$box/." "$dir/"
    rm -rf "$box"
    [ ! -f "$dir/results.jsonl" ] ||
      jq -c --arg from "$box" --arg to "$dir" 'with_entries(if (.value | type) == "string" and (.value | startswith($from)) then .value |= ($to + .[($from | length):]) else . end)' \
        "$dir/results.jsonl" >"$dir/results.jsonl.tmp" && mv "$dir/results.jsonl.tmp" "$dir/results.jsonl"
  fi
  return 0
}

# Collects the runs' results, records, guards and reports.
mg_snapshots__finish() {
  local platforms="$1" rebaseline="$2" results="$MG_RUN_DIR/results.jsonl" e name baseline actual n p f
  : >"$results"
  for f in "$MG_RUN_DIR"/*/*/results.jsonl; do
    [ -f "$f" ] && cat "$f" >>"$results"
  done
  mg_artifact results "$results" "snapshot results (one JSON object per snapshot)"

  # State coverage ran in each platform's default run.
  for p in $platforms; do
    f="$MG_RUN_DIR/$p/$([ "$p" = mac ] && echo mac || echo "$MG_SNAP_DEFAULT_GROUP")/state-coverage.json"
    if [ ! -f "$f" ]; then
      mg_failure "state-coverage/$p" "StateCoverageTests did not run on $p (no state-coverage.json): a skipped check is no check"
    elif [ "$(jq '.problems | length' "$f")" -gt 0 ]; then
      jq -r '.problems[]' "$f" | while IFS= read -r line; do mg_failure "state-coverage/$p" "$line"; done
    fi
  done
  # Where the matrix registers each screen and the cells it renders there,
  # whatever the filter (every state-coverage report lists all platforms):
  # tools/mg critic grades a screen only on all of them.
  for f in "$MG_RUN_DIR"/*/*/state-coverage.json; do
    [ -f "$f" ] || continue
    jq '{screens: ([.screens[] | {(.id): {platforms: (.platforms // []), states, cells}}] | add // {})}' "$f" >"$MG_RUN_DIR/registry.json"
    mg_artifact registry "$MG_RUN_DIR/registry.json" "where the snapshot matrix registers each screen"
    break
  done

  # Snapshot outcomes.
  jq -c 'select(.status == "fail" or .status == "new")' "$results" | while IFS= read -r e; do
    name="$(jq -r .name <<<"$e")"
    mg_failure "snapshot/$name" "$(jq -r '"\(.message // "")\(if .diff then "; diff: \(.diff)" else "" end)"' <<<"$e")" \
      "tools/mg snapshots --platform $(jq -r .platform <<<"$e") --record '$name'"
    [ "$(jq -r '.diff // empty' <<<"$e")" = "" ] || mg_artifact diff "$(jq -r .diff <<<"$e")" "$name"
  done

  # Recording: only images that failed the comparison or had no baseline.
  local recorded=0 deleted=0
  if [ "$MG_SNAP_MODE" = record ]; then
    while IFS="$(printf '\t')" read -r baseline actual; do
      [ -n "$baseline" ] || continue
      mkdir -p "$(dirname "$MG_ROOT/$baseline")"
      cp "$actual" "$MG_ROOT/$baseline"
      recorded=$((recorded + 1))
    done < <(jq -r 'select(.status == "recorded") | [.baseline, .actual] | @tsv' "$results")
    if [ "$rebaseline" = true ] && [ "$MG_SNAP_BROKEN" = 0 ]; then
      deleted="$(mg_snapshots__prune "$results")"
    fi
  fi

  mg_snapshots__probe "$results"
  mg_snapshots__guard

  local counts
  counts="$(jq -sc 'group_by(.status) | map({(.[0].status): length}) | add // {}' "$results")"
  mg_detail counts "$counts"
  mg_detail recorded "$recorded"
  mg_detail pruned "$deleted"
  mg_detail keys "$(jq -sc '[.[].key] | unique' "$results")"
  n="$(jq -s '[.[] | select(.status != "captured")] | length' "$results")"
  mg_count total "$n"
  mg_count passed "$(jq -s '[.[] | select(.status == "pass" or .status == "recorded")] | length' "$results")"
  mg_count failed "$(jq -s '[.[] | select(.status == "fail" or .status == "new")] | length' "$results")"
  mg_count skipped "$(jq -s '[.[] | select(.status == "deferred")] | length' "$results")"
  if [ "$MG_SNAP_BROKEN" != 0 ]; then
    mg_status error
  elif [ -s "$MG_STATE/failures.jsonl" ]; then
    mg_status fail
  else
    mg_status pass
  fi
  mg_message "snapshots: $(jq -r 'to_entries | map("\(.value) \(.key)") | join(", ")' <<<"$counts")$([ "$recorded" = 0 ] || echo "; $recorded recorded into app/Snapshots/")$([ "${MG_SNAP_VERDICT:-pass}" = pass ] || echo "; needs-human-review")"
}

# mg_snapshots__prune <results>: after a rebaseline, deletes baselines this
# run no longer produces under its keys, and other OS builds' baselines of
# the same models. Prints the number of files deleted.
mg_snapshots__prune() {
  local results="$1" key model keep f dir n=0
  keep="$MG_RUN_DIR/keep.txt"
  jq -r 'select(.baseline) | .baseline' "$results" | sort -u >"$keep"
  for key in $(jq -r '.key' "$results" | sort -u); do
    while IFS= read -r f; do
      grep -qxF "${f#"$MG_ROOT"/}" "$keep" || { rm -f "$f" && n=$((n + 1)); }
    done < <(find "$MG_ROOT/app/Snapshots/$key" -name '*.png' -type f 2>/dev/null)
    model="${key%-*}"
    for dir in "$MG_ROOT/app/Snapshots/$model"-*; do
      [ -d "$dir" ] && [ "$(basename "$dir")" != "$key" ] || continue
      n=$((n + $(find "$dir" -name '*.png' | wc -l)))
      rm -rf "$dir"
    done
  done
  find "$MG_ROOT/app/Snapshots" -type d -empty -delete 2>/dev/null
  echo "$n"
}

# The settings self-test (spike S5): every settings group of every simulator
# kind renders the probe (system glass over content), whatever the filter
# (SystemSettingsTests captures it outside the filter); a group without its
# probe image fails the run, and so does one that changes Reduce
# Transparency or the glass opacity but renders like the default group's
# (its setting was not applied). Reduce Motion leaves a still frame
# unchanged: SystemSettingsTests checks it.
mg_snapshots__probe() {
  local results="$1" dir kind group reference others out probe missing
  for dir in "$MG_RUN_DIR"/*/; do
    dir="${dir%/}"
    kind="$(basename "$dir")"
    [ "$kind" != mac ] && [ -d "$dir/plan" ] || continue
    reference="" others="" missing=""
    # Group directories only: the runs' logs and result bundles sit beside them.
    for group in $(cd "$dir" && ls 2>/dev/null | grep -E '^rt[01]-rm[01]-t[0-9]+$'); do
      probe="$(jq -r --arg d "$dir/$group/" 'select(.subject == "matrix.probe" and ((.actual // "") | startswith($d))) | .actual' "$results" | head -n 1)"
      if [ -z "$probe" ] || [ ! -f "$probe" ]; then
        missing="$missing $group"
      elif [ "$group" = "$MG_SNAP_DEFAULT_GROUP" ]; then
        reference="$probe"
      elif printf '%s' "$group" | grep -Eq '^rt1|-t(0|100)$'; then
        others="$others $probe"
      fi
    done
    for group in $missing; do
      mg_failure "probe/$kind/$group" "settings group $group on $kind has no probe image: the settings self-test could not check it (spike S5)"
    done
    [ -n "$reference" ] && [ -n "$others" ] || continue
    out="$MG_RUN_DIR/probe-$kind.json"
    # shellcheck disable=SC2046 # one path per word
    if ! mg_image_tools python3 tools/scripts/image-diff/image_diff.py differs "$(mg_container_path "$reference")" \
      $(for f in $others; do mg_container_path "$f"; done) --report "$(mg_container_path "$out")" >&2; then
      jq -r '.unchanged[]?' "$out" | while IFS= read -r f; do
        mg_failure "setting-not-applied/$kind" "the probe in ${f#*"/$kind/"} renders like the default settings group: devicectl did not apply that setting (spike S5)"
      done
      [ -s "$out" ] || mg_failure "probe/$kind" "the settings self-test could not compare the probes; see log.txt"
    fi
    mg_artifact probe "$out" "settings self-test ($kind)"
  done
}

# The baseline guard (R1974): an approved signature baseline whose file
# differs from the approved image is compared with it at the snapshot
# tolerance; a mismatch needs human review (gallery approval, U254). An
# approved path whose OS build was re-recorded away (--rebaseline) is
# compared through its successor: the same model's same cell under another
# build. Approving the successor in the gallery retires the old entry.
mg_snapshots__guard() {
  local approved="$MG_ROOT/docs/design/approved-baselines.json" pairs="$MG_RUN_DIR/guard-pairs.json" dir="$MG_RUN_DIR/guard" e path current sha blob
  MG_SNAP_VERDICT=pass
  [ -f "$approved" ] || return 0
  mkdir -p "$dir"
  echo "[]" >"$pairs"
  while IFS= read -r e; do
    path="$(jq -r .path <<<"$e")"
    current="$path"
    [ -f "$MG_ROOT/$path" ] || current="$(mg_snapshots__successor "$path")"
    if [ -n "$current" ] && [ -f "$MG_ROOT/$current" ]; then
      sha="$(shasum -a 256 "$MG_ROOT/$current" | cut -d' ' -f1)"
      [ "$sha" != "$(jq -r .sha256 <<<"$e")" ] || continue
    else
      current="$path"
    fi
    blob="$dir/approved-$(jq -r .sha256 <<<"$e").png"
    git -C "$MG_ROOT" cat-file blob "$(jq -r .blob <<<"$e")" >"$blob" 2>/dev/null || rm -f "$blob"
    jq --arg p "$current" --arg old "$path" --arg c "$(mg_container_path "$MG_ROOT/$current")" --arg a "$( [ -f "$blob" ] && mg_container_path "$blob" || echo /nonexistent)" \
      '. + [{path: $p, current: $c, approved: $a} + (if $p == $old then {} else {supersedes: $old} end)]' "$pairs" >"$pairs.tmp" && mv "$pairs.tmp" "$pairs"
  done < <(jq -c '.baselines[] | select(.signature == true)' "$approved")
  [ "$(jq length "$pairs")" -gt 0 ] || { mg_detail guard '{"verdict": "pass", "checked": 0}'; return 0; }
  if ! mg_image_tools python3 tools/scripts/image-diff/image_diff.py guard "$(mg_container_path "$pairs")" \
    --report "$(mg_container_path "$dir/report.json")" --diff-dir "$(mg_container_path "$dir")" >&2; then
    mg_failure guard "the baseline guard could not run; see log.txt"
    MG_SNAP_BROKEN=1
    return 0
  fi
  mg_artifact guard "$dir/report.json" "approved-baseline guard"
  MG_SNAP_VERDICT="$(jq -r .verdict "$dir/report.json")"
  mg_detail guard "$(jq -c '{verdict, checked: (.checked | length), needs_human_review: [.needs_human_review[].path],
    supersedes: ([.checked[] | select(.supersedes) | {(.path): .supersedes}] | add // {})}' "$dir/report.json")"
  jq -c '.needs_human_review[]' "$dir/report.json" | while IFS= read -r e; do
    mg_failure needs-human-review "$(jq -r '"\(.path): the approved signature baseline\(if .supersedes then " (approved as \(.supersedes))" else "" end) changed beyond the tolerance (\(.reason // "")); approve it in the gallery (tools/mg gallery) or restore it"' <<<"$e")" \
      "tools/mg gallery --pending $MG_ROOT"
  done
}

# mg_snapshots__successor <approved path>: the same model's same cell under
# another OS build (app/Snapshots/<model>-<build>/<cell path>), if one exists.
mg_snapshots__successor() {
  local key rest model f
  key="${1#app/Snapshots/}"
  rest="${key#*/}"
  key="${key%%/*}"
  model="${key%-*}"
  for f in "$MG_ROOT/app/Snapshots/$model"-*/"$rest"; do
    [ -f "$f" ] && [ "$f" != "$MG_ROOT/$1" ] || continue
    printf '%s\n' "${f#"$MG_ROOT"/}"
    return 0
  done
}
