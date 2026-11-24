# Shared iOS simulator pool. Sourced by tools/mg.
#
# tools/scripts/sim-pool.sh keeps Meguri-Pool-<kind>-<n> simulators booted and
# leases them out (see its header); the pool lives in the macOS test VM (U8c,
# mac-vm.sh), and sim-pool.sh runs itself there. This host job wraps it for
# agents: details.sim_runner says where the pool is; without a VM, or with
# one that cannot be reached (U19c), the job is queued (warn, user step
# mac-vm). acquire and gc (but gc --list) change the
# VM's pool: they hold its lease (mg_sim_pool_take), as sim-setup does, so
# nothing is created, booted or deleted there while another job runs in it.
# Scripts call sim-pool.sh directly; its own acquire and gc come here when
# their caller does not hold the VM's lease. The UDID and the lease land in
# result.json:
#   udid="$(jq -r .details.udid "$(tools/mg sim acquire iphone)")"
#   tools/mg sim release "$udid"
# details: pool: {exit, output} of sim-pool.sh (what a direct call would
# have printed and returned); acquire and renew: udid, lease, device;
# release: udid, released; status: pool_dir, runtime, max_per_kind, kinds,
# devices, orphan_leases; gc: older_than_hours, list_only, targets, deleted,
# failed.

mg_job sim host mg_sim \
  "Shared iOS simulator pool, in the macOS test VM (queued without it; acquire and gc wait for the VM's lease): acquire <iphone|ipad> [--holder <label>] [--ttl-min N] | renew <udid> [--ttl-min N] | release <udid> | status | gc [--older-than-hours N] [--list]"

mg_job sim-posture host mg_sim_posture \
  "Duo posture: open|closed|calibrate|inspect <leased-udid>; prepare|status [--vm <name>] for the persistent VNC holder"

mg_sim_posture() {
  local action="${1:-}" out rc
  if [ "$action" = inspect ] || [ "$action" = open ] || [ "$action" = closed ] || [ "$action" = calibrate ]; then
    if [ $# -ne 2 ]; then
      mg_failure usage "usage: tools/mg sim-posture open|closed|calibrate|inspect <leased-udid>"
      mg_status error; return 0
    fi
    out="$("$MG_TOOLS/vm/duo-fold.sh" "$action" "$2")" && rc=0 || rc=$?
    case "$rc" in
      3) mg__mac_vm_unreachable "posture scheduler lease stayed busy" "sim-posture $action $2"; return 0 ;;
      5) mg__mac_vm_unreachable "posture guest transport unavailable" "sim-posture $action $2"; return 0 ;;
    esac
    if [ "$rc" -ne 0 ]; then
      mg_failure "posture-$action" "Duo $action failed; see log.txt. Native calibration may be required. The existing VNC session was not restarted."
      mg_status fail; return 0
    fi
    if [ "$action" = inspect ]; then
      mg_detail inspection "$out"
      mg_message "sim-posture: private guest desktop capture complete; operator calibration required"
    else
      mg_detail "$action" "$out"
      mg_message "sim-posture: $action complete"
    fi
    mg_status pass; return 0
  fi
  case "$action" in prepare|status) shift ;; *)
    mg_failure usage "usage: tools/mg sim-posture prepare|status [--vm <name>]"
    mg_status error; return 0 ;;
  esac
  if [ $# -ne 0 ] && { [ $# -ne 2 ] || [ "$1" != --vm ]; }; then
    mg_failure usage "usage: tools/mg sim-posture prepare|status [--vm <name>]"
    mg_status error; return 0
  fi
  if [ "$action" = prepare ] && ! "$MG_TOOLS/vm/duo-fold.sh" prepare "$@"; then
    mg_failure vnc-prepare "VNC preparation failed; see log.txt and the coordinator's private build/vm-fold logs"
    mg_status fail; return 0
  fi
  if ! out="$("$MG_TOOLS/vm/duo-fold.sh" status "$@")"; then
    mg_failure vnc-status "Could not read persistent VNC session status; see log.txt"
    mg_status error; return 0
  fi
  mg_detail vnc "$out"
  if [ "$action" = prepare ] && [ "$(jq -r .connected <<<"$out")" != true ]; then
    mg_failure vnc-prepare "VNC transport disconnected after preparation; inspect private build/vm-fold logs"
    mg_status fail; return 0
  fi
  mg_message "sim-posture: $(jq -r 'if .connected then "connected" else "not connected" end' <<<"$out")"
  mg_status pass
}

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
  mg_sim_begin || return 0
  case "$sub" in
    acquire) mg_sim_pool_take || return 0 ;;
    gc) mg_word_in --list "$*" || mg_sim_pool_take || return 0 ;;
  esac
  out="$(mg_sim_pool_run "$MG_RUN_DIR/sim-pool.err" "$sub" "$@")"
  rc=$?
  [ "$sub" = status ] || mg_detail pool "$(jq -n --argjson rc "$rc" --arg out "$out" '{exit: $rc, output: $out}')"
  case "$rc" in
    0) ;;
    2) mg_sim__usage "$sub: invalid arguments; see log.txt"; return 0 ;;
    4)
      mg_detail lane "$(jq -n --arg k "${1:-}" '{kind: $k, status: "unavailable", reason: "toolchain"}')"
      mg_message "sim: $sub ${1:-}: unavailable: toolchain (no such simulator in this Xcode)"
      mg_status warn
      return 0
      ;;
    3)
      mg_failure pool-busy "every pool device of this kind stayed leased until the wait ran out; see log.txt" "tools/mg sim status"
      mg_message "sim: $sub timed out waiting for a free device"
      mg_status error
      return 0
      ;;
    5)
      mg__mac_vm_unreachable "sim-pool.sh $sub: $(mg_sim_pool_queued_reason "$MG_RUN_DIR/sim-pool.err")" "sim $sub"
      return 0
      ;;
    6)
      mg_failure sim-vm "sim-pool.sh $sub: the test VM could not run the pool (exit $rc); see log.txt" "tools/mg doctor"
      mg_message "sim: $sub: the test VM could not run the pool"
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

# ---------------------------------------------------------------------------
# Pinned simulators (U8, R1962, R1966)

mg_job sim-setup host mg_sim_setup \
  "Check the pinned iOS runtime build and device types (tools/sim-devices.json) in the macOS test VM and make sure its pool has one device of each kind (creating it through sim-pool.sh, under the VM's lease); writes build/sim-devices.local.json. [--kinds k1,k2]"
mg_job sim-reset host mg_sim_reset \
  "Reset a pool simulator this checkout leased (user apps, privacy, keychain, UI options, status bar) and keep the lease, under the test VM's lease: <udid>"

# The pinned iOS runtime when it is available: identifier, version, build. The
# foldable kind pins its own; a newer one never changes the other kinds'.
mg_sim__pool_runtime() {
  mg_simctl list runtimes --json | jq -c --arg p "$(jq -r "${2:-.runtime}.identifier" "$1")" '
    first((.runtimes[] | select(.identifier == $p and .isAvailable != false)), null)
    | if . == null then null else {identifier, version, build: .buildversion} end'
}

mg_sim_setup() {
  local only="" pinned="tools/sim-devices.json"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --kinds) [ "$#" -ge 2 ] && only="$(printf '%s' "$2" | tr ',' ' ')" && shift 2 || { mg_sim__usage "--kinds needs a comma-separated list"; return 0; } ;;
      *) mg_sim__usage "usage: tools/mg sim-setup [--kinds iphone,ipad]"; return 0 ;;
    esac
  done
  mg_sim_begin || return 0
  # It may create and boot pool devices: under the VM's lease.
  mg_sim_pool_take || return 0
  local expected actual
  expected="$(jq -c .runtime "$pinned")"
  actual="$(mg_sim__pool_runtime "$pinned")" || actual=null
  mg_detail runtime "$(jq -nc --argjson e "$expected" --argjson a "${actual:-null}" '{expected: $e, actual: $a}')"
  if [ "$(jq -r '"\(.identifier) \(.build)"' <<<"$expected")" != "$(jq -r 'if . == null then "none" else "\(.identifier) \(.build)" end' <<<"${actual:-null}")" ]; then
    mg_failure runtime-mismatch "the pinned iOS runtime is $(jq -r 'if . == null then "missing" else "iOS \(.version) (\(.build), \(.identifier))" end' <<<"${actual:-null}"); tools/sim-devices.json pins iOS $(jq -r '"\(.version) (\(.build), \(.identifier))"' <<<"$expected"). Install the pinned runtime, or update the pin and re-record the baselines (R1966, R1974)."
    mg_message "sim-setup: runtime build mismatch: expected $(jq -r .build <<<"$expected"), found $(jq -r '.build // "none"' <<<"${actual:-null}")"
    mg_status fail
    return 0
  fi

  local types missing kinds kind foldable
  types="$(mg_simctl list devicetypes --json)"
  missing="$(jq -r --argjson t "$types" '.devices[] | select(.device_type as $d | [$t.devicetypes[].identifier] | index($d) | not) | "\(.kind) (\(.device_type))"' "$pinned")"
  if [ -n "$missing" ]; then
    mg_failure device-type-missing "Xcode has no device type for: $(printf '%s' "$missing" | tr '\n' ' ')" 
    mg_message "sim-setup: pinned device types missing from Xcode"
    mg_status fail
    return 0
  fi
  foldable="$(jq -r --argjson t "$types" '.foldable.device_type_name_pattern as $p
    | first($t.devicetypes[] | select((.productFamily // "") == "iPhone" and (.name | test($p; "i"))) | .identifier) // ""' "$pinned")"
  # The foldable lane also needs its own runtime, with the same build.
  [ "$(mg_sim__pool_runtime "$pinned" .foldable.runtime | jq -r '.build // ""')" = "$(jq -r .foldable.runtime.build "$pinned")" ] || foldable=""
  if [ -n "$foldable" ]; then
    mg_detail foldable "$(jq -n --arg t "$foldable" '{status: "available", device_type: $t}')"
  else
    mg_detail foldable '{"status": "unavailable", "reason": "toolchain"}'
  fi
  kinds="$(jq -r '.devices[].kind' "$pinned" | tr '\n' ' ')"
  [ -z "$foldable" ] || kinds="$kinds$(jq -r .foldable.kind "$pinned")"
  if [ -n "$only" ]; then
    for kind in $only; do
      mg_word_in "$kind" "$kinds $(jq -r .foldable.kind "$pinned")" || { mg_sim__usage "unknown kind '$kind' (kinds: $kinds)"; return 0; }
    done
    kinds="$only"
  fi

  local status_json devices='{}' udids udid created="" rc
  for kind in $kinds; do
    if [ "$kind" = "$(jq -r .foldable.kind "$pinned")" ] && [ -z "$foldable" ]; then
      devices="$(jq -c --arg k "$kind" '. + {($k): {status: "unavailable", reason: "toolchain", udids: []}}' <<<"$devices")"
      continue
    fi
    status_json="$("$MG_TOOLS/scripts/sim-pool.sh" status 2>/dev/null)" || status_json='{"devices": []}'
    udids="$(jq -c --arg k "$kind" '[.devices[] | select(.kind == $k and .current_runtime and .available) | .udid]' <<<"$status_json")"
    if [ "$(jq length <<<"$udids")" -eq 0 ]; then
      echo "sim-setup: no $kind pool device on the pinned runtime; creating one through the pool" >&2
      udid="$(mg_sim_pool_run "$MG_RUN_DIR/sim-pool-$kind.err" acquire "$kind" --holder sim-setup)"
      rc=$?
      if [ "$rc" -eq 5 ]; then
        # The test VM could not be reached: the rest waits for it.
        mg__mac_vm_unreachable "sim-pool.sh acquire $kind: $(mg_sim_pool_queued_reason "$MG_RUN_DIR/sim-pool-$kind.err")" "creating the $kind pool device"
        return 0
      fi
      if [ "$rc" -ne 0 ] || [ -z "$udid" ]; then
        mg_failure "create/$kind" "sim-pool.sh acquire $kind exited with code $rc; see log.txt" "tools/mg sim-setup --kinds $kind"
        continue
      fi
      "$MG_TOOLS/scripts/sim-pool.sh" release "$udid" >&2 || mg_failure "release/$kind" "sim-pool.sh release $udid failed; see log.txt"
      udids="$(jq -nc --arg u "$udid" '[$u]')"
      created="$created $kind"
      devices="$(jq -c --arg k "$kind" --argjson u "$udids" '. + {($k): {status: "created", udids: $u}}' <<<"$devices")"
    else
      devices="$(jq -c --arg k "$kind" --argjson u "$udids" '. + {($k): {status: "present", udids: $u}}' <<<"$devices")"
    fi
  done
  mkdir -p build
  jq -n --argjson rt "$actual" --argjson d "$devices" --argjson pin "$(jq -c '.devices' "$pinned")" \
    --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg runner "$MG_SIM_RUNNER" \
    --arg vm "$([ "$MG_SIM_RUNNER" != vm ] || jq -r .name <<<"$MG_MAC_VM")" '{checked_at: $at, runner: $runner,
      vm: (if $vm == "" then null else $vm end), runtime: $rt, devices: ($d | with_entries(.value += {
      device_type: (.key as $k | first($pin[] | select(.kind == $k) | .device_type) // null)}))}' >build/sim-devices.local.json
  mg_artifact json build/sim-devices.local.json "pinned simulators $([ "$MG_SIM_RUNNER" = vm ] && echo "in the test VM" || echo "on this Mac")"
  mg_detail devices "$devices"
  mg_message "sim-setup: runtime $(jq -r '"\(.version) (\(.build))"' <<<"$actual") ok; $(jq -r 'to_entries | map("\(.key) \(.value.status)") | join(", ")' <<<"$devices")$([ -n "$foldable" ] || echo "; foldable: unavailable: toolchain")"
  if [ -s "$MG_STATE/failures.jsonl" ]; then
    mg_status fail
  else
    mg_status pass
  fi
}

# A reset uninstalls the apps, the UI-test runner included: it waits for the
# lease like any job that uses a simulator (the test VM's), so it never pulls
# the app from under a drive of this checkout, and checks the lease again
# once it holds the VM (the device may have been released meanwhile).
mg_sim_reset() {
  [ "$#" -eq 1 ] || { mg_sim__usage "usage: tools/mg sim-reset <udid>"; return 0; }
  mg_sim_begin || return 0
  mg_sim_check_owned "$1" || return 0
  mg_sim_take --no-copy || return 0
  mg_sim_check_owned "$1" || return 0
  if ! "$MG_TOOLS/scripts/sim-pool.sh" reset "$1"; then
    mg_failure sim-pool "sim-pool.sh reset $1 failed; see log.txt"
    mg_status error
    return 0
  fi
  mg_sim_prepare "$1" || return 0
  mg_message "sim-reset: $1 is reset and still leased"
  mg_status pass
}
