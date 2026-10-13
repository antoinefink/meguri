# Shared iOS simulator pool. Sourced by tools/mg.
#
# tools/scripts/sim-pool.sh keeps Meguri-Pool-<kind>-<n> simulators booted and
# leases them out (see its header); this host job wraps it for agents. Scripts
# call sim-pool.sh directly. The UDID and the lease land in result.json:
#   udid="$(jq -r .details.udid "$(tools/mg sim acquire iphone)")"
#   tools/mg sim release "$udid"
# details: acquire and renew: udid, lease, device; release: udid, released;
# status: pool_dir, runtime, max_per_kind, kinds, devices, orphan_leases;
# gc: older_than_hours, list_only, targets, deleted, failed.

mg_job sim host mg_sim \
  "Shared iOS simulator pool: acquire <iphone|ipad> [--holder <label>] [--ttl-min N] | renew <udid> [--ttl-min N] | release <udid> | status | gc [--older-than-hours N] [--list]"

mg_sim__usage() {
  mg_failure usage "usage: tools/mg sim acquire <kind> [--holder <label>] [--ttl-min N] | renew <udid> [--ttl-min N] | release <udid> | status | gc [--older-than-hours N] [--list]" \
    "tools/mg sim status"
  mg_message "sim: $1"
  mg_status error
}

# mg_sim__record_lease <udid>: the device and its lease from the pool status;
# sets MG_SIM_NAME to the device name.
mg_sim__record_lease() {
  local rec
  rec="$("$MG_TOOLS/scripts/sim-pool.sh" status 2>/dev/null |
    jq -c --arg u "$1" 'first(.devices[] | select(.udid == $u)) // null')" || rec=null
  [ -n "$rec" ] || rec=null
  mg_detail lease "$(jq -c '.lease // null' <<<"$rec")"
  mg_detail device "$(jq -c 'if . == null then null else del(.lease) end' <<<"$rec")"
  MG_SIM_NAME="$(jq -r '.name // "?"' <<<"$rec")"
}

mg_sim() {
  local sub="${1:-}" out rc udid
  [ "$#" -gt 0 ] && shift
  case "$sub" in
    acquire | renew | release | status | gc) ;;
    "") mg_sim__usage "missing subcommand"; return 0 ;;
    *) mg_sim__usage "unknown subcommand '$sub'"; return 0 ;;
  esac
  out="$("$MG_TOOLS/scripts/sim-pool.sh" "$sub" "$@")"
  rc=$?
  case "$rc" in
    0) ;;
    2) mg_sim__usage "$sub: invalid arguments; see log.txt"; return 0 ;;
    3)
      mg_failure pool-busy "every pool device of this kind stayed leased until the wait ran out; see log.txt" "tools/mg sim status"
      mg_message "sim: $sub timed out waiting for a free device"
      mg_status error
      return 0
      ;;
    *)
      mg_failure sim-pool "sim-pool.sh $sub exited with code $rc; see log.txt"
      mg_message "sim: $sub failed"
      mg_status error
      [ "$sub" != gc ] || mg_sim__details "$out"
      return 0
      ;;
  esac
  case "$sub" in
    acquire)
      udid="$out"
      mg_detail udid "$(jq -n --arg u "$udid" '$u')"
      mg_sim__record_lease "$udid"
      mg_message "sim: leased $MG_SIM_NAME ($udid); release it with: tools/mg sim release $udid"
      ;;
    renew)
      mg_detail udid "$(jq -n --arg u "${1:-}" '$u')"
      mg_sim__record_lease "${1:-}"
      mg_message "sim: renewed the lease on ${1:-}"
      ;;
    release)
      mg_detail udid "$(jq -n --arg u "${1:-}" '$u')"
      mg_detail released true
      mg_message "sim: released ${1:-}"
      ;;
    status)
      mg_sim__details "$out"
      mg_message "sim: $(jq -r '"\(.devices | length) pool devices, \([.devices[] | select(.leased)] | length) leased"' <<<"$out")"
      ;;
    gc)
      mg_sim__details "$out"
      mg_message "sim: gc $(jq -r 'if .list_only then "would delete \(.targets | length)" else "deleted \(.deleted | length)" end' <<<"$out") leftover simulators"
      ;;
  esac
  mg_status pass
}

# mg_sim__details <JSON object>: one detail per top-level key.
mg_sim__details() {
  local k
  jq -e 'type == "object"' >/dev/null 2>&1 <<<"$1" || return 0
  for k in $(jq -r 'keys_unsorted[]' <<<"$1"); do
    mg_detail "$k" "$(jq -c --arg k "$k" '.[$k]' <<<"$1")"
  done
}
