# Explicit host SDK observation; never included in gates or VM routing.
mg_job cloud-fetch-probe-host host mg_cloud_fetch_probe_host \
  "No arguments; explicit MEGURI_APP_SIGNING=team only. Mac Debug Snapshot native SDK diagnostic on this host; diagnostic-only warn, never signed-lane or full-suite acceptance"
mg_job cloud-record-read-host host mg_cloud_record_read_host \
  "No arguments; explicit MEGURI_CLOUD_RECORD_READ_INPUT build JSON and MEGURI_APP_SIGNING=team required; development private exact-record read only; on-demand diagnostic warn"

mg_cloud_fetch_probe_host() {
  if [ "$#" -ne 0 ]; then
    mg_failure usage "cloud-fetch-probe-host requires zero arguments"
    mg_status error
    return 0
  fi
  mg_cloud_fetch_probe__entry scope
}

mg_cloud_record_read_host() {
  if [ "$#" -ne 0 ]; then
    mg_failure usage "cloud-record-read-host requires zero arguments"
    mg_status error
    return 0
  fi
  if [ -z "${MEGURI_CLOUD_RECORD_READ_INPUT:-}" ]; then
    mg_failure usage "cloud-record-read-host requires explicit MEGURI_CLOUD_RECORD_READ_INPUT"
    mg_status error
    return 0
  fi
  mg_cloud_fetch_probe__entry record-read "$MEGURI_CLOUD_RECORD_READ_INPUT"
}

mg_cloud_fetch_probe__entry() {
  local private rc report key previous_umask
  local mode="$1" record_input="${2:-}"
  local MG_CFP_FAILURE=private-storage
  if [ "${MEGURI_APP_SIGNING:-}" != team ] || [ "$(uname -s)" != Darwin ]; then
    mg_failure usage "Host SDK diagnostic requires explicit MEGURI_APP_SIGNING=team and Darwin"
    mg_status error
    return 0
  fi
  previous_umask="$(umask)"
  umask 077
  private="$MG_RUN_DIR/private"
  if ! mkdir -m 700 "$private"; then
    umask "$previous_umask"
    mg_failure private-storage "Host SDK diagnostic private storage refused"
    mg_status fail
    return 0
  fi
  mg_cloud_fetch_probe__private "$private" "$mode" "$record_input" >"$private/workflow.log" 2>&1
  rc=$?
  umask "$previous_umask"
  mg_detail scope '"HOST-CK-SDK-DIAGNOSTIC-ONLY"'
  mg_detail full_suite false
  if [ -s "$private/report.json" ]; then
    report="$(cat "$private/report.json")"
    mg_detail observation "$report"
    if [ "$mode" = scope ]; then
      for key in total executed passed failed skipped; do
        mg_count "$key" "$(jq -r ".counts.$key" <<<"$report")"
      done
    fi
  fi
  if [ "$rc" -eq 0 ]; then
    mg_status warn
    mg_message "Host SDK observation only; no app, sync, unit, milestone, release or signed-lane acceptance"
  else
    mg_failure "$MG_CFP_FAILURE" "Host SDK diagnostic failed; native evidence remains private"
    mg_status fail
  fi
}

mg_cloud_fetch_probe__private() {
  # Existing helpers can record paths, IDs and native errors. Give all their
  # state, artifacts and console a run-owned private boundary, never public MG.
  local MG_RUN_DIR="$1" MG_STATE="$1/state" input xctestrun rc marker proof
  local mode="$2" record_input="$3"
  local -a record_args=()
  if [ "$mode" = record-read ]; then
    # Canonicalize only inside private logging; Python performs nofollow/bounds
    # and verifies the input stays under build/.
    case "$record_input" in /*) ;; *) record_input="$MG_ROOT/$record_input" ;; esac
    record_args=(--record-read "$record_input")
  fi
  mkdir -m 700 "$MG_STATE" || return 1
  MG_CFP_FAILURE=host-lease
  mg_host_lease || return 1
  mg_cloud_fetch_probe__root_lease || return 1
  [ ! -e "$MG_LEASE_DIR/vm-xcode-pending" ] || return 1
  MG_CFP_FAILURE=app-in-use
  mg_app_mac_guard || return 1
  MG_CFP_FAILURE=source-binding
  input="$(mg_app_input_digest)"
  [[ "$input" =~ ^[0-9a-f]{64}$ ]] || return 1
  touch "$MG_RUN_DIR/build-start" || return 1
  MG_CFP_FAILURE=build
  mg_app_build mac Debug Snapshot || return 1
  MG_CFP_FAILURE=source-binding
  [ "$input" = "$(mg_app_input_digest)" ] || return 1
  MG_CFP_FAILURE=product
  xctestrun="$(MG_APP_TEST_SCHEME=Meguri mg__vm_xctestrun Snapshot macosx Debug)"
  [ -n "$xctestrun" ] || return 1
  MG_CFP_FAILURE=app-in-use
  mg_app_mac_guard || return 1
  MG_CFP_FAILURE=host-lease
  mg_cloud_fetch_probe__root_lease || return 1
  marker="$MG_LEASE_DIR/vm-xcode-pending"
  proof="$MG_RUN_DIR/originals-joined"
  [ ! -e "$proof" ] && [ ! -L "$proof" ] || return 1
  # Historical marker name; this job never accesses a VM. No EXIT cleanup:
  # Python exit alone says nothing about its original native groups.
  mkdir -m 700 "$marker" || return 1
  MG_CFP_FAILURE=diagnostic
  python3 -B -I "$MG_TOOLS/scripts/cloud-fetch-probe-host.py" "$MG_RUN_DIR" "$xctestrun" \
    "$(mg_app_products mac Debug)/Meguri.app" "${record_args[@]}" >"$MG_RUN_DIR/report.json"
  rc=$?
  MG_CFP_FAILURE=join-uncertain
  mg_cloud_fetch_probe__root_lease &&
    [ -f "$proof" ] && [ ! -L "$proof" ] && [ -O "$proof" ] &&
    cmp -s "$proof" <(printf 'HOST-CK-SDK-ORIGINALS-JOINED-v1\n') || return 1
  rmdir "$marker" || return 1
  MG_CFP_FAILURE=source-binding
  [ "$input" = "$(mg_app_input_digest)" ] || return 1
  MG_CFP_FAILURE=diagnostic
  return "$rc"
}

mg_cloud_fetch_probe__root_lease() {
  # Refuse a recording's nested slot before any guarded work; only this job's
  # root exclusive lease may carry and later release the completion fence.
  [ -n "${MG_RUN_ID:-}" ] && [ -n "${MG_LEASE_DIR:-}" ] &&
    [ "${MG_LEASE_SLOT:-}" = "$MG_LEASE_DIR" ] &&
    [ "$MG_LEASE_DIR" = "$(mg_host_lease_dir)" ] &&
    [ ! -e "$MG_LEASE_DIR/inner" ] &&
    jq -e --argjson pid "$$" --arg run "$MG_RUN_ID" --arg wt "$MG_ROOT" --arg job "$MG_JOB" '
      .pid == $pid and .run_id == $run and .worktree == $wt and
      .job == $job and ($job == "cloud-fetch-probe-host" or $job == "cloud-record-read-host") and
      .nested == null and .kind == null
    ' "$MG_LEASE_DIR/holder.json" >/dev/null
}
