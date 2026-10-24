# User-owned persistent local sync service; never part of the test-rig lifecycle.
mg_job local-sync host mg_local_sync \
  "Persistent loopback Docker sync service: up --image <installed immutable ID>, status, down"

mg_local_sync() {
  local report="$MG_RUN_DIR/local-sync.json" rc=0
  python3 "$MG_TOOLS/scripts/local_sync.py" --root "$MG_ROOT" ${1+"$@"} >"$report" || rc=$?
  if [ "$rc" -ne 0 ]; then
    mg_failure local-sync "Local sync operation failed; see the sanitized log."
    mg_status error
    return 0
  fi
  mg_detail service "$(cat "$report")"
  mg_message "Local sync operation completed; connection settings remain in the private credential file."
  mg_status pass
}
