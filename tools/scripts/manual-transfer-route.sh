# RF4 controller forwarding and teardown, using the existing VM lease.
MG_MT_ROUTE_PID=""
MG_MT_MANIFEST=""
MG_MT_ROUTE_FD=""
MG_MT_CLEANED=""
MG_MT_ROUTE_VERIFIED=""
MG_MT_WIRE_PID=""
MG_MT_WIRE_CONFIG=""
MG_MT_WIRE_FD=""

mg_mt_start_wire() {
  # Start only after owned-rig validation and under the VM lease. The ephemeral
  # bind and capability travel through private FIFOs and memory, never artifacts.
  mvl_lease_held "$MG_LEASE_DIR" || return 1
  mkfifo "$MG_RUN_DIR/wire-input.fifo" "$MG_RUN_DIR/wire-ready.fifo" || return 1
  exec 9<>"$MG_RUN_DIR/wire-input.fifo"
  exec 7<>"$MG_RUN_DIR/wire-ready.fifo"
  MG_MT_WIRE_FD=1
  python3 "$MG_TOOLS/scripts/manual-transfer-wire.py" "$MG_MT_HOST_PORT" "$MG_RUN_DIR/native-reader-command.json" \
    <"$MG_RUN_DIR/wire-input.fifo" 9>&- 7>&- \
    >"$MG_RUN_DIR/wire-ready.fifo" 2>/dev/null &
  MG_MT_WIRE_PID=$!
  if ! IFS= read -r -t 10 MG_MT_WIRE_CONFIG <&7; then
    exec 7>&-
    mg_mt_error wire-start "controlled observer did not become ready within ten seconds"
    return 1
  fi
  exec 7>&-
  MG_MT_OBSERVER_PORT="$(printf '%s' "$MG_MT_WIRE_CONFIG" | python3 "$MG_TOOLS/scripts/manual-transfer-config.py" wire-port)" || return 1
}

mg_mt_wire_proof() {
  if ! printf '%s' "$MG_MT_WIRE_CONFIG" | python3 "$MG_TOOLS/scripts/manual-transfer-config.py" wire-proof \
      >"$MG_RUN_DIR/wire-proof.json"; then
    mg_mt_error wire-proof "observed request counts, cancellation or media-only resume were not proven" fail
    return 1
  fi
  mg_artifact wire-proof "$MG_RUN_DIR/wire-proof.json" "observed protocol aggregates at native consent/cancel/restart/resume boundaries"
}

mg_mt_open_forward() {
  local wrapper lifetime waited=0
  if [ -z "${MG_LEASE_DIR:-}" ] || ! mvl_lease_held "$MG_LEASE_DIR"; then
    mg_mt_error lease "take the test VM lease before starting a forward"
    return 1
  fi
  case "${MEGURI_MANUAL_TRANSFER_SSH_OPTION_PASSTHROUGH:-}" in
    1) ;;
    *)
      mg_mt_pending "manual-transfer-check requires attested SSH option passthrough"
      return 1
      ;;
  esac
  wrapper="$(jq -r .ssh <<<"$MG_MAC_VM")"
  mkfifo "$MG_RUN_DIR/route.fifo" || return 1
  exec 8<>"$MG_RUN_DIR/route.fifo"
  MG_MT_ROUTE_FD=1
  # The remote process owns manifest lifetime. Closing input or reaching the
  # deadline removes it even if the controller cannot run remote cleanup.
  lifetime="$(mvl_q /bin/bash -c 'file=$1; input=$2; code=$3
trap '\''exit 143'\'' TERM HUP
trap '\''rm -f "$file" "$input" "$code"; kill "$timer" "$reader" 2>/dev/null || true'\'' EXIT
(sleep 1800 & sleeper=$!; trap '\''kill "$sleeper" 2>/dev/null; exit'\'' TERM HUP; wait "$sleeper"; kill -TERM $$) </dev/null >/dev/null 2>&1 &
timer=$!
rm -f "$file"
printf "mg-transfer-forward-ready\\n"
cat <&0 >/dev/null &
reader=$!
wait "$reader"' manual-transfer-forward "$MG_MT_MANIFEST" "$MG_RUN_DIR/native-reader-input.json" "$MG_RUN_DIR/media-proof-config.py")"
  "$wrapper" -o BatchMode=yes -o ConnectTimeout=10 -o ExitOnForwardFailure=yes \
    -o ServerAliveInterval=10 -o ServerAliveCountMax=2 \
    -R "127.0.0.1:$MG_MT_VM_PORT:127.0.0.1:$MG_MT_OBSERVER_PORT" "$lifetime" \
    <"$MG_RUN_DIR/route.fifo" 8>&- 9>&- >"$MG_RUN_DIR/route.log" 2>&1 &
  MG_MT_ROUTE_PID=$!
  while [ "$waited" -lt 10 ]; do
    grep -qx mg-transfer-forward-ready "$MG_RUN_DIR/route.log" && break
    kill -0 "$MG_MT_ROUTE_PID" 2>/dev/null || break
    mg_sleep 1
    waited=$((waited + 1))
  done
  if ! grep -qx mg-transfer-forward-ready "$MG_RUN_DIR/route.log"; then
    mg_mt_pending "forward startup lacked readiness within the ten-second bound"
    return 1
  fi
  # The readiness marker alone is insufficient: inspect the listener and make
  # an HTTP request from the leased VM. Keep wrapper diagnostics unshared.
  if ! mg_mac_vm_ssh "listeners=\$(/usr/sbin/lsof -nP -iTCP:$MG_MT_VM_PORT -sTCP:LISTEN -Fn | sed -n 's/^n//p' | sort -u); test \"\$listeners\" = '127.0.0.1:$MG_MT_VM_PORT' && /usr/bin/curl --noproxy '*' --connect-timeout 3 --max-time 5 -s -o /dev/null http://127.0.0.1:$MG_MT_VM_PORT/" >/dev/null 2>&1; then
    mg_mt_pending "VM listener scope and HTTP reachability were not both established"
    return 1
  fi
  MG_MT_ROUTE_VERIFIED=1
  mg_detail native_transfer_route "$(jq -nc --argjson host_port "$MG_MT_HOST_PORT" \
    '{kind: "attested-wrapper-ssh-reverse-forward-with-fixture-observer", workstation_loopback_port: $host_port,
      vm_loopback_port: 18765, http_probe: true, listener_loopback_only: true}')"
}

mg_mt_cleanup() {
  local errors=0 waited=0
  [ -z "$MG_MT_CLEANED" ] || return 0
  MG_MT_CLEANED=1
  mg__mac_vm_stop_orphans
  if [ -n "$MG_MT_WIRE_FD" ]; then
    exec 9>&-
    MG_MT_WIRE_FD=""
  fi
  if [ -n "$MG_MT_WIRE_PID" ]; then
    kill -TERM "$MG_MT_WIRE_PID" 2>/dev/null || true
    waited=0
    while kill -0 "$MG_MT_WIRE_PID" 2>/dev/null && [ "$waited" -lt 3 ]; do
      mg_sleep 1
      waited=$((waited + 1))
    done
    kill -KILL "$MG_MT_WIRE_PID" 2>/dev/null || true
    wait "$MG_MT_WIRE_PID" 2>/dev/null || true
    MG_MT_WIRE_PID=""
  fi
  if [ -n "$MG_MT_MANIFEST" ]; then
    mg_mac_vm_ssh "rm -f $(mvl_q "$MG_MT_MANIFEST" "$MG_RUN_DIR/native.anki2")" >/dev/null 2>&1 || errors=1
  fi
  if [ -n "$MG_MT_ROUTE_FD" ]; then
    exec 8>&-
    MG_MT_ROUTE_FD=""
  fi
  if [ -n "$MG_MT_ROUTE_PID" ]; then
    kill -TERM "$MG_MT_ROUTE_PID" 2>/dev/null || true
    while kill -0 "$MG_MT_ROUTE_PID" 2>/dev/null && [ "$waited" -lt 3 ]; do
      mg_sleep 1
      waited=$((waited + 1))
    done
    kill -KILL "$MG_MT_ROUTE_PID" 2>/dev/null || true
    wait "$MG_MT_ROUTE_PID" 2>/dev/null || true
    MG_MT_ROUTE_PID=""
  fi
  if [ -n "$MG_MT_ROUTE_VERIFIED" ]; then
    mg_mac_vm_ssh "test ! -e $(mvl_q "$MG_MT_MANIFEST") && ! /usr/sbin/lsof -nP -iTCP:$MG_MT_VM_PORT -sTCP:LISTEN -Fn" >/dev/null 2>&1 || errors=1
  fi
  MG_MT_WIRE_CONFIG=""
  mg_mt_cleanup_media || errors=1
  rm -f "$MG_RUN_DIR/wire-input.fifo" "$MG_RUN_DIR/wire-ready.fifo"
  rm -f "$MG_RUN_DIR/route.fifo" "$MG_RUN_DIR/route.log"
  if [ "$errors" -ne 0 ]; then
    mg_mt_error cleanup "VM teardown is unconfirmed; the manifest watchdog expires at thirty minutes and the next VM fixture copy replaces it"
    return 1
  fi
  mg_detail native_transfer_cleanup true
}
