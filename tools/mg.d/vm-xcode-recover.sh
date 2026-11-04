# Root-owned recovery of one abandoned pre-prepare query. No lease take/drop.
mg_job vm-xcode-recover host mg_vm_xcode_recover \
  "MAINTENANCE-ONLY: --stage-run 20261102T200056Z-94582 --inspect|--stop-and-archive; never starts a VM"

mg_vm_xcode_recover() {
  local private temp_base selection config base rc=0 outcome report="$MG_RUN_DIR/recovery-report.json"
  if [ "$#" -ne 3 ] || [ "$1" != --stage-run ] ||
    [ "$2" != 20261102T200056Z-94582 ]; then
    mg_failure usage "usage: tools/mg vm-xcode-recover --stage-run 20261102T200056Z-94582 --inspect|--stop-and-archive"
    mg_status error
    return 1
  fi
  case "$3" in --inspect|--stop-and-archive) ;; *)
    mg_failure usage "vm-xcode-recover: unknown operation"
    mg_status error
    return 1 ;;
  esac
  # Use existing configuration/lease selection READS only. Never probe/take,
  # drain, start, copy, generic force-drop or register inherited lease cleanup.
  temp_base="$(CDPATH= cd -- "${TMPDIR:-/tmp}" && pwd -P)" || return 1
  private="$(mktemp -d "${temp_base%/}/meguri-vm-xcode-recover.XXXXXX")" || return 1
  chmod 700 "$private" || return 1
  (
    umask 077
    exec >"$private/helpers.log" 2>&1
    mvl_read_config || exit 1
    selection="$(jq -c '.[0]' <<<"$MVL_ALL")" || exit 1
    config="$(mvl_config_path)" || exit 1
    base="$(mvl_lease_base "$MG_ROOT")" || exit 1
    jq -nc --arg root "$MG_ROOT" --arg config "$config" --argjson selection "$selection" \
      --arg lease_base "$base" --arg temp_root "$temp_base" --arg recovery_run "$MG_RUN_ID" \
      '{root:$root,config:$config,selection:$selection,lease_base:$lease_base,temp_root:$temp_root,recovery_run:$recovery_run}' \
      >"$private/context.json" || exit 1
    python3 -B -I "$MG_TOOLS/scripts/vm-xcode-recover.py" "$@" \
      3<"$private/context.json" 6>&- >"$report"
  ) || rc=$?
  # Only this fixed public schema can be registered. Helper diagnostics and
  # routing stay private even when report writing or initial admission fails.
  if ! jq -e '
      keys == ["checks","counts","original_run","outcome","scope","stage"] and
      .scope == "VM-XCODE-MAINTENANCE-ONLY" and
      .stage == "EXACT-PRE-PREPARE-STOP-CONFIRM-ARCHIVE" and
      .original_run == "20261102T200056Z-94582" and
      (.outcome == "refused" or .outcome == "inspected" or .outcome == "archived") and
      (.checks | type == "object" and all(.[]; type == "boolean") and
        (keys - ["claim_installed","original_stop_joined","separate_proof_durable","whole_lease_archived"] | length == 0)) and
      (.counts | type == "object" and all(.[]; type == "number" and . >= 0 and floor == .) and
        (keys - ["fold_sessions","guest_managed","host_controls","host_jobs","host_workers","recordings","simulator_leases"] | length == 0))
    ' "$report" >/dev/null 2>&1; then
    rc=1
    jq -nc '{scope:"VM-XCODE-MAINTENANCE-ONLY",stage:"EXACT-PRE-PREPARE-STOP-CONFIRM-ARCHIVE",
      outcome:"refused",original_run:"20261102T200056Z-94582",checks:{},counts:{}}' >"$report" || :
  fi
  mg_artifact json "$report" "MAINTENANCE-ONLY; exact pre-prepare fence recovery" || rc=1
  outcome="$(jq -er '.outcome' "$report" 2>/dev/null)" || rc=1
  case "$3:$outcome" in --inspect:inspected|--stop-and-archive:archived) ;; *) rc=1 ;; esac
  if [ "$rc" -eq 0 ]; then
    mg_message "MAINTENANCE-ONLY; exact recovery operation completed; no staging or app acceptance"
    mg_status pass
  else
    mg_failure vm-xcode-recover "MAINTENANCE-ONLY; recovery or report publication failed; retained evidence requires root review"
    mg_status error
    return 1
  fi
}
