# OWNER ONLY. Never called by gates, catch-up, doctor or another producer.
mg_job cloudkit-account-setup host mg_cloudkit_account_setup \
  "OWNER ONLY J2.2.1: --label acct1|acct2; interactive setup-only source publication; --dry-run is inert"

mg_cloudkit_account_setup_private() (
  local private="$1" report="$2" label="$3" rc
  MG_EXIT_CMDS=()
  MG_STATE="$private/state"
  MG_RUN_DIR="$private"
  mkdir -m 700 "$MG_STATE" || exit 1
  mg_mac_vm_probe
  rc=$?
  case "$rc" in 0) ;; 1) exit 75 ;; *) exit 1 ;; esac
  MG_MAC_VMS="$(jq -c '[.[0]]' <<<"$MG_MAC_VMS")" || exit 1
  MG_SIM_RUNNER=vm
  MG_MAC_RUNNER=vm
  mg_mac_vm_take --no-copy --running-only
  rc=$?
  case "$rc" in 0) ;; 2) exit 75 ;; *) exit 1 ;; esac
  mvl_lease_held "$MG_LEASE_DIR" || exit 1
  jq -nc --argjson selection "$MG_MAC_VM" --arg slot "$MG_LEASE_SLOT" \
    '{selection:$selection,lease_slot:$slot}' >"$private/context.json" || exit 1
  MG_MAC_VM_TESTING=1
  python3 -B -I "$MG_TOOLS/scripts/cloud-account-setup.py" --root "$MG_ROOT" \
    --run-id "$MG_RUN_ID" --label "$label" 3<"$private/context.json" >"$report"
  rc=$?
  # Original Python app/transport groups have joined before this checked drain.
  mvl_lease_held "$MG_LEASE_DIR" || exit 1
  mg__mac_vm_stop_orphans
  if [ -n "$MG_MAC_VM_STOP_REASON" ]; then
    # An operation was attempted: a failed drain is failure, never queued.
    if [ "$rc" -eq 0 ]; then
      jq -nc --arg label "$label" '{scope:"SETUP-ONLY",outcome:"failed_evidence",category:"publication-uncertain",label:$label,signed_execution:false}' >"$report"
    fi
    exit 1
  fi
  exit "$rc"
)

mg_cloudkit_account_setup() {
  local label="" private base rc report="$MG_RUN_DIR/setup-report.json" outcome category
  if [ "$#" -eq 2 ] && [ "$1" = --label ]; then label="$2"; fi
  if ! mg_word_in "$label" "acct1 acct2"; then
    mg_failure usage "usage: tools/mg cloudkit-account-setup --label acct1|acct2 (OWNER ONLY)"
    mg_status error; return 0
  fi
  if [ "${MG_JOB:-}" != cloudkit-account-setup ] || [ "${MG_COMMIT_DIRTY:-true}" != false ]; then
    mg_failure setup-source "Owner setup requires its own clean MG invocation"
    mg_status fail; return 0
  fi
  if [ ! -t 0 ]; then
    mg_failure setup-terminal "Owner setup requires the owner's interactive terminal"
    mg_status fail; return 0
  fi
  # Canonicalize the trusted OS temporary BASE, not a private descendant.
  base="$(/usr/bin/getconf DARWIN_USER_TEMP_DIR)" || return 1
  base="$(cd -P "$base" && pwd -P)" || return 1
  private="$(mktemp -d "$base/meguri-account-owner.XXXXXX")" || return 1
  chmod 700 "$private" || return 1
  # Retain private failure/publication evidence. No recursive path cleanup,
  # candidate file, attachment, raw diagnostic or private path becomes an export.
  (umask 077; mg_cloudkit_account_setup_private "$private" "$report" "$label") >"$private/helpers.log" 2>&1
  rc=$?
  mg_detail scope '"SETUP-ONLY"'
  case "$rc" in
    0)
      if jq -e --arg label "$label" '. == {scope:"SETUP-ONLY",outcome:"setup_completed",label:$label,signed_execution:false}' "$report" >/dev/null 2>&1; then
        mg_detail outcome '"setup_completed"'; mg_status pass
      else rc=1; fi
      ;;
    75)
      jq -nc --arg label "$label" '{scope:"SETUP-ONLY",outcome:"queued",category:"vm-unavailable",label:$label,signed_execution:false}' >"$report"
      mg_detail outcome '"queued"'
      mg_user_step mac-vm missing true "Make the configured dedicated test VM available" \
        "Owner account setup needs the configured VM and retained exclusive lease."
      mg_queued_part mac-vm "Owner account setup" "configured VM unavailable before execution"
      mg_queued_settle
      ;;
    76)
      category="$(jq -r '.category | select(. == "cloudkit-test-accounts" or . == "cloudkit-dev-container")' "$report" 2>/dev/null)"
      if [ -n "$category" ] && jq -e --arg label "$label" --arg category "$category" '. == {scope:"SETUP-ONLY",outcome:"awaiting_user",category:$category,label:$label,signed_execution:false}' "$report" >/dev/null 2>&1; then
        mg_detail outcome '"awaiting_user"'
        mg_user_step "$category" missing true "Owner: confirm the dedicated VM test account" \
          "Only the owner invokes setup and enters its exact terminal approval; acct2 requires approved acct1."
        mg_status warn
      else rc=1; fi
      ;;
    78)
      mg_detail outcome '"implementation_missing"'
      mg_failure implementation_missing "Required owner setup platform capability is missing"
      mg_status fail
      ;;
  esac
  case "$rc" in 0|75|76|78) ;; *)
    category="$(jq -r 'if .outcome == "setup_completed" or .category == "publication-uncertain" then "publication-uncertain" else empty end' "$report" 2>/dev/null)"
    [ "$category" = publication-uncertain ] || category=setup-refused
    jq -nc --arg label "$label" --arg category "$category" \
      '{scope:"SETUP-ONLY",outcome:"failed_evidence",category:$category,label:$label,signed_execution:false}' >"$report"
    mg_detail outcome '"failed_evidence"'
    mg_failure "$category" "Owner setup failed; no automatic retry or rollback"
    mg_status fail
    ;;
  esac
  mg_artifact json "$report" "Owner setup outcome only; no signed execution or acceptance"
  mg_message "cloudkit-account-setup: SETUP-ONLY"
}
