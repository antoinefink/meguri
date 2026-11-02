# Signed-agent J2.1 admission. This job cannot yet run the app-hosted agent.
mg_job cloudkit-signed host mg_cloudkit_signed \
  "Signed CloudKit agent: exact receipt and independent product admission; VM Mac/reserved simulator scope; missing install/runtime proof fails"

mg_cloudkit_signed() {
  local header="" results="$MG_ROOT/build/mg" report="$MG_RUN_DIR/input-admission.json" rc outcome
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --header|--results-directory)
        if [ "$#" -lt 2 ]; then
          mg_failure usage "cloudkit-signed requires an input path"
          mg_status error
          return 0
        fi
        if [ "$1" = --header ]; then header="$2"; else results="$2"; fi
        shift 2
        ;;
      *)
        mg_failure usage "usage: tools/mg cloudkit-signed --header <run.json> [--results-directory <directory>]"
        mg_status error
        return 0
        ;;
    esac
  done
  if [ -z "$header" ]; then
    mg_failure implementation_missing "cloudkit-agent-install, VM/pool destination checks and app-hosted execution are not implemented"
    mg_detail outcome '"implementation_missing"'
    mg_detail lane '"cloud-signed"'
    mg_status fail
    return 0
  fi
  # The verifier prints only fixed categories. Never copy inputs or private
  # command diagnostics into log.txt or the public report.
  python3 -B -I "$MG_TOOLS/scripts/cloud-agent-verify.py" \
    --root "$MG_ROOT" --run-id "$MG_RUN_ID" --header "$header" \
    --results-directory "$results" >"$report"
  rc=$?
  mg_artifact json "$report" "INPUT-VALIDATION only; no execution evidence"
  mg_detail lane '"cloud-signed"'
  outcome="$(jq -r '.outcome | select(. == "implementation_missing" or . == "failed_evidence")' "$report" 2>/dev/null)"
  if [ "$rc" -eq 1 ] && [ "$outcome" = failed_evidence ]; then
    mg_detail outcome '"failed_evidence"'
    mg_failure input-evidence "signed-agent input admission failed; see fixed categories in input-admission.json"
    mg_status fail
  else
    mg_detail outcome '"implementation_missing"'
    # There is deliberately no passing branch, including an unexpected exit 0.
    mg_failure implementation_missing "signed-agent deployment and runtime verification are not implemented"
    mg_status fail
  fi
  mg_message "cloudkit-signed: incomplete signed-agent implementation"
}
