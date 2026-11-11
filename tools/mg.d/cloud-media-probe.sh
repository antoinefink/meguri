# Explicit normal-app diagnostic; never a signed-agent or gate acceptance run.
mg_job cloud-media-probe host mg_cloud_media_probe \
  "NORMAL-APP-MEDIA-PROBE: stage --preview-build RUN_ID | verify-phone --stage RUN_ID --preparation RUN_ID; staged-only or peer-byte-match, never signed acceptance"

mg_cloud_media_probe() {
  local report="$MG_RUN_DIR/probe-report.json" rc
  local admitted=false run_pattern='^[0-9]{8}T[0-9]{6}Z-[0-9]+$'
  # Admit the bounded public grammar before Python can parse duplicate internal
  # globals. Both verify flag orders are supported; values must be run IDs.
  case "${1:-}" in
    stage)
      if [ "$#" -eq 3 ] && [ "$2" = "--preview-build" ] && [[ "$3" =~ $run_pattern ]]; then
        admitted=true
      fi
      ;;
    verify-phone)
      if [ "$#" -eq 5 ] && [[ "$3" =~ $run_pattern ]] && [[ "$5" =~ $run_pattern ]]; then
        case "$2:$4" in
          --stage:--preparation|--preparation:--stage) admitted=true ;;
        esac
      fi
      ;;
  esac
  if [ "$admitted" != true ]; then
    mg_failure usage "usage: tools/mg cloud-media-probe stage --preview-build RUN_ID | verify-phone --stage RUN_ID --preparation RUN_ID" "tools/mg --list"
    mg_status fail
    return 0
  fi
  # The helper emits a closed public report only. Native diagnostics and the
  # retained synthetic source live outside MG artifacts in private OS-user storage.
  python3 -B -I "$MG_TOOLS/scripts/cloud-media-probe.py" --root "$MG_ROOT" --run-id "$MG_RUN_ID" "$@" >"$report"
  rc=$?
  if ! jq -e 'type == "object" and .scope == "NORMAL-APP-MEDIA-PROBE" and .signed_lane_acceptance == false' "$report" >/dev/null 2>&1; then
    mg_failure probe-report "normal media probe report refused"
    mg_status fail
    return 0
  fi
  mg_detail probe "$(cat "$report")"
  mg_artifact json "$report" "normal app synthetic media diagnostic only"
  if [ "$rc" -eq 0 ] && jq -e '.outcome == "staged-only" or .outcome == "peer-byte-match"' "$report" >/dev/null; then
    mg_message "normal media probe: $(jq -r .outcome "$report"); signed-lane acceptance false"
    mg_status pass
  else
    mg_failure media-probe "normal media probe failed; see fixed category in probe report"
    mg_status fail
  fi
}
