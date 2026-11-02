# Signed-agent J2.1 admission and delivery. Neither launches the agent.
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
    mg_failure implementation_missing "signed command, journal, account and generated-data coordination are not implemented"
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
    mg_failure implementation_missing "signed command, journal, account and generated-data coordination are not implemented"
    mg_status fail
  fi
  mg_message "cloudkit-signed: incomplete signed-agent implementation"
}

mg_job cloudkit-agent-install host mg_cloudkit_agent_install \
  "J2.1 delivery only to one frozen configured VM/pool destination; no agent launch or signed acceptance"

# All existing VM helpers may report private routes in their diagnostics or
# MG details. Run them with private state, discard it, and translate ONLY the
# fixed outcome. Keep the selected VM's lease through the Python transport.
mg_cloudkit_agent_install_private() (
  local original_dir="$MG_RUN_DIR" private="$1" parent="$2" lease_run="$3" rc
  MG_EXIT_CMDS=()
  MG_STATE="$private/state"
  MG_RUN_DIR="$private"
  MG_DEFAULT_REPRO="tools/mg cloudkit-agent-install"
  mkdir -m 700 "$MG_STATE" || exit 1
  mg_mac_vm_probe
  rc=$?
  case "$rc" in 0) ;; 1) exit 75 ;; *) exit 1 ;; esac
  # The pool uses the first configured available VM. This cut freezes exactly
  # one destination; it never expands an existing map or falls back to host.
  MG_MAC_VMS="$(jq -c '[.[0]]' <<<"$MG_MAC_VMS")"
  MG_SIM_RUNNER=vm
  MG_MAC_RUNNER=vm
  mg_mac_vm_take --no-copy --running-only
  rc=$?
  case "$rc" in 0) ;; 2) exit 75 ;; *) exit 1 ;; esac
  printf '%s\n' "$MG_MAC_VM" >"$private/selection.json"
  local lease_args=()
  [ -z "$lease_run" ] || lease_args=(--lease-run "$lease_run")
  python3 -B -I "$MG_TOOLS/scripts/cloud-agent-install.py" --phase deliver \
    --root "$MG_ROOT" --run-directory "$original_dir" --run-id "$MG_RUN_ID" \
    --parent-run "$parent" --source-commit "$MG_COMMIT_SHA" ${lease_args[0]+"${lease_args[@]}"} \
    3<"$private/selection.json" >"$original_dir/install-report.json"
)

mg_cloudkit_agent_install() {
  local template="" parent="" lease_run="" private rc reference path report="$MG_RUN_DIR/install-report.json"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --header-template|--parent-run|--lease-run)
        if [ "$#" -lt 2 ]; then
          mg_failure usage "cloudkit-agent-install requires an option value"
          mg_status error
          return 0
        fi
        case "$1" in
          --header-template) template="$2" ;;
          --parent-run) parent="$2" ;;
          --lease-run) lease_run="$2" ;;
        esac
        shift 2
        ;;
      *)
        mg_failure usage "usage: tools/mg cloudkit-agent-install --parent-run <coordinator-id> --header-template <JSON without deployment_receipt> [--lease-run <pool-receipt-id>]; --lease-run required for simulators"
        mg_status error
        return 0
        ;;
    esac
  done
  if [ -z "$template" ] || [ -z "$parent" ]; then
    mg_failure usage "cloudkit-agent-install needs --header-template <coordinator JSON> and --parent-run <coordinator-id>"
    mg_status error
    return 0
  fi
  if [ "${MG_COMMIT_DIRTY:-true}" != false ]; then
    mg_failure delivery-source "delivery requires a clean MG starting source"
    mg_status fail
    return 0
  fi
  private="$(mktemp -d "${TMPDIR:-/tmp}/meguri-cloud-delivery.XXXXXX")" || {
    mg_failure delivery-private "private delivery workspace unavailable"
    mg_status error
    return 0
  }
  chmod 700 "$private"
  # Quote cleanup using the same helper as the SSH/copy paths.
  mg_on_exit "rm -rf $(mvl_q "$private")"
  (umask 077
    python3 -B -I "$MG_TOOLS/scripts/cloud-agent-install.py" --phase prepare \
      --root "$MG_ROOT" --run-directory "$MG_RUN_DIR" --run-id "$MG_RUN_ID" \
      --parent-run "$parent" --source-commit "$MG_COMMIT_SHA" --header-template "$template" >"$report" 2>"$private/prepare.err"
  )
  rc=$?
  if [ "$rc" -eq 0 ]; then
    (umask 077; mg_cloudkit_agent_install_private "$private" "$parent" "$lease_run") \
      >"$private/helpers.log" 2>&1
    rc=$?
  fi
  # A private VM preflight can end before the Python deliver phase starts.
  # Replace input-only preparation metadata with an honest delivery outcome.
  if [ "$rc" -eq 75 ] && ! jq -e '.outcome == "queued"' "$report" >/dev/null 2>&1; then
    jq -nc '{scope:"DELIVERY-ONLY",outcome:"queued",category:"vm-unavailable",signed_execution:false}' >"$report"
  elif [ "$rc" -ne 0 ] && [ "$rc" -ne 75 ] && ! jq -e '.outcome == "failed_evidence"' "$report" >/dev/null 2>&1; then
    jq -nc '{scope:"DELIVERY-ONLY",outcome:"failed_evidence",category:"vm-preflight",signed_execution:false}' >"$report"
  fi
  mg_detail lane '"cloud-signed"'
  mg_detail scope '"DELIVERY-ONLY"'
  mg_artifact json "$report" "Fixed delivery outcome; no signed execution"
  case "$rc" in
    0)
      if ! jq -e '.outcome == "delivery_only" and .scope == "DELIVERY-ONLY" and .signed_execution == false' "$report" >/dev/null 2>&1; then
        mg_failure delivery-report "delivery did not produce a complete transport handoff"
        mg_status fail
        return 0
      fi
      reference="$(jq -c .agent_deployment "$report")"
      mg_detail agent_deployment "$reference"
      mg_detail outcome '"delivery_only"'
      mg_detail destination_scope '"single-device-frozen"'
      mg_detail product_evidence '"implementation_missing-finalized-receipt-handoff"'
      while IFS= read -r path; do
        case "$path" in
          *.xctestrun) mg_artifact file "$path" "Exact frozen CloudAgent xctestrun; no launch" ;;
          *) mg_artifact json "$path" "J2.1 exact delivery input or observation" ;;
        esac
      done < <(jq -r '.artifacts[].path' "$report")
      mg_message "cloudkit-agent-install: delivery only; finalized product-evidence handoff and signed runtime remain incomplete"
      mg_status pass
      ;;
    75)
      # No VM route, probe value or raw command diagnostic enters this result.
      mg_detail outcome '"queued"'
      mg_user_step mac-vm missing true "Make the configured test VM ready for delivery" \
        "The running configured VM must answer and use the same Xcode toolchain as the host; delivery stays queued until then. No host fallback."
      mg_queued_part mac-vm "CloudAgent delivery" "configured VM or matching guest toolchain unavailable"
      mg_queued_settle
      mg_message "cloudkit-agent-install: queued; actual delivery has not completed"
      ;;
    *)
      mg_detail outcome '"failed_evidence"'
      mg_failure delivery-evidence "CloudAgent delivery refused; only fixed categories appear in install-report.json"
      mg_status fail
      ;;
  esac
}
