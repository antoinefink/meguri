# Local sync server rig (U13). Sourced by tools/mg.
#
# A host job: it builds the oracle image (the pinned Anki release; tools/mg
# hashes oracle/, callers never read it) and runs Anki's sync server from it as
# the compose service `syncserver`, with `toxiproxy` in front and, for
# recordings, `mitmdump` (docker/compose.yml). Every port is published on
# 127.0.0.1 only, at a port Docker picks (fixed ones through MEGURI_SYNC_PORT,
# MEGURI_SYNC_PROXY_PORT, MEGURI_TOXIPROXY_API_PORT, MEGURI_SYNC_RECORD_PORT);
# `up` and `status` report them in details.endpoints. The rig stays up after
# `up` until `down`. Conformance runs do not use it (their server is a child of
# the oracle worker, KTD23).

mg_job sync-rig host mg_sync_rig \
  "Local Anki sync server on 127.0.0.1 with toxiproxy: up [--record <name>] | down | status"

MG_SYNC_RIG_SERVICES="syncserver toxiproxy mitmdump"
# Anki's sync server rejects a client clock more than 300 s off (R1932); the
# end-to-end server must be on the real clock.
MG_SYNC_RIG_MAX_SKEW=300

mg_sync_rig__usage() {
  mg_failure usage "usage: tools/mg sync-rig up [--record <name>] | down | status" "tools/mg sync-rig status"
  mg_message "sync-rig: $1"
  mg_status error
}

mg_sync_rig__compose() {
  mg_compose --profile sync-rig --profile spec-record "$@"
}

mg_sync_rig() {
  local sub="${1:-}"
  [ "$#" -gt 0 ] && shift
  case "$sub" in
    up) mg_sync_rig_up "$@" ;;
    down) mg_sync_rig_down "$@" ;;
    status) mg_sync_rig_status "$@" ;;
    "") mg_sync_rig__usage "missing subcommand" ;;
    *) mg_sync_rig__usage "unknown subcommand '$sub'" ;;
  esac
}

mg_sync_rig_up() {
  local record=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --record)
        record="${2:-}"
        shift 2 || { mg_sync_rig__usage "--record needs a name"; return 0; }
        ;;
      *) mg_sync_rig__usage "up: unexpected argument '$1'"; return 0 ;;
    esac
  done
  local services="syncserver toxiproxy" record_dir=""
  if [ -n "$record" ]; then
    if ! printf '%s' "$record" | grep -Eq '^[a-z0-9][a-z0-9-]*$'; then
      mg_sync_rig__usage "--record name '$record' must be lower-case letters, digits and '-'"
      return 0
    fi
    record_dir="$MG_ROOT/conformance/recordings/sync/$record"
    if [ -n "$(ls -A "$record_dir" 2>/dev/null)" ]; then
      mg_failure recording-exists "conformance/recordings/sync/$record already holds a recording; remove it or pick another name"
      mg_status error
      return 0
    fi
    services="$services mitmdump"
  fi
  mg_oracle__docker_ok || return 0
  mg_oracle__tree && mg_oracle__build "$MG_ORACLE_DIR" || return 0
  [ -z "$record_dir" ] || mkdir -p "$record_dir"

  echo "sync-rig: starting $services from meguri-oracle:$MG_ORACLE_TAG" >&2
  # shellcheck disable=SC2086 # one service per word
  if ! MEGURI_ORACLE_TAG="$MG_ORACLE_TAG" MEGURI_RECORD_DIR="${record_dir:-$MG_ROOT/build/sync-rig/recording}" \
    mg_sync_rig__compose up --detach --wait --wait-timeout 120 $services >&2; then
    mg_failure compose-up "the rig did not start or become healthy; see log.txt" "tools/mg sync-rig up"
    mg_sync_rig__logs
    mg_status error
    return 0
  fi
  mg_detail image "$(jq -nc --arg t "meguri-oracle:$MG_ORACLE_TAG" '{tag: $t}')"
  [ -z "$record_dir" ] || mg_detail recording "$(jq -nc --arg p "conformance/recordings/sync/$record/exchanges.jsonl" '$p')"
  mg_sync_rig__check up
}

# The rig must never be reachable beyond this Mac: when a check finds it
# exposed, the rig is stopped.
mg_sync_rig_down() {
  [ "$#" -eq 0 ] || { mg_sync_rig__usage "down takes no arguments"; return 0; }
  mg_oracle__docker_ok || return 0
  # shellcheck disable=SC2086 # one service per word
  if ! mg_sync_rig__compose rm --stop --force --volumes $MG_SYNC_RIG_SERVICES >&2; then
    mg_failure compose-rm "removing the rig's containers failed; see log.txt"
    mg_status error
    return 0
  fi
  docker network rm "$(mg_compose_project)_sync-rig" >/dev/null 2>&1 || true
  mg_detail running false
  mg_message "sync-rig: down"
  mg_status pass
}

mg_sync_rig_status() {
  [ "$#" -eq 0 ] || { mg_sync_rig__usage "status takes no arguments"; return 0; }
  mg_oracle__docker_ok || return 0
  if [ -z "$(mg_sync_rig__running syncserver)" ]; then
    mg_detail running false
    mg_message "sync-rig: down (tools/mg sync-rig up starts it)"
    mg_status pass
    return 0
  fi
  mg_sync_rig__check status
}

# mg_sync_rig__running <service>: the running container's id, or nothing.
mg_sync_rig__running() {
  mg_sync_rig__compose ps --quiet --status running "$1" 2>/dev/null | head -n 1
}

# mg_sync_rig__port <service> <container port>: the published host:port.
mg_sync_rig__port() {
  mg_sync_rig__compose port "$1" "$2" 2>/dev/null | head -n 1
}

mg_sync_rig__logs() {
  local log="$MG_RUN_DIR/rig.log"
  # shellcheck disable=SC2086 # one service per word
  mg_sync_rig__compose logs --no-color --timestamps $MG_SYNC_RIG_SERVICES >"$log" 2>&1
  mg_artifact log "$log" "rig container logs"
}

# The Mac's address on its default-route interface, or nothing when offline.
mg_sync_rig__lan_address() {
  local iface
  iface="$(route -n get default 2>/dev/null | awk '/interface:/ {print $2; exit}')"
  [ -n "$iface" ] || return 0
  ipconfig getifaddr "$iface" 2>/dev/null
}

# mg_sync_rig__users: the test user names of the env file(s) as a JSON array.
mg_sync_rig__users() {
  local f
  for f in docker/syncserver.env.example docker/syncserver.env; do
    [ -f "$MG_ROOT/$f" ] && sed -n 's/^SYNC_USER[0-9][0-9]*=\([^:]*\):.*/\1/p' "$MG_ROOT/$f"
  done | awk '!seen[$0]++' | jq -R . | jq -sc .
}

# mg_sync_rig__check <up|status>: endpoints, publish addresses, loopback and LAN
# reachability, and the server's clock. Records details and the status.
mg_sync_rig__check() {
  local verb="$1" failed=0 checks=0
  local server proxy api recorder
  server="$(mg_sync_rig__port syncserver 8080)"
  proxy="$(mg_sync_rig__port toxiproxy 8081)"
  api="$(mg_sync_rig__port toxiproxy 8474)"
  recorder=""
  [ -z "$(mg_sync_rig__running mitmdump)" ] || recorder="$(mg_sync_rig__port mitmdump 8080)"
  mg_detail running true
  mg_detail endpoints "$(jq -nc --arg s "$server" --arg p "$proxy" --arg a "$api" --arg r "$recorder" '
    def url: if . == "" then null else "http://\(.)/" end;
    {syncserver: ($s | url), toxiproxy_proxy: ($p | url), toxiproxy_api: ($a | url), recorder: ($r | url)}')"
  mg_detail users "$(mg_sync_rig__users)"

  # Every published port binds 127.0.0.1, never a wildcard, LAN or IPv6 address.
  local exposed
  exposed="$(
    # shellcheck disable=SC2086 # one service per word
    mg_sync_rig__compose ps --format json $MG_SYNC_RIG_SERVICES 2>/dev/null |
      jq -rs '[.[] | (if type == "array" then .[] else . end)
        | .Service as $s | (.Publishers // [])[]
        | select(.PublishedPort > 0 and .URL != "127.0.0.1")
        | "\($s) \(.URL):\(.PublishedPort)"] | .[]'
  )"
  checks=$((checks + 1))
  if [ -n "$exposed" ]; then
    failed=$((failed + 1))
    mg_failure published-beyond-loopback "ports published beyond 127.0.0.1: $(printf '%s' "$exposed" | tr '\n' ',')"
  fi

  # Each endpoint answers on 127.0.0.1: HTTP, except the recorder, which gets a
  # bare TCP connect so that no probe lands in the recording.
  local name ep code probes="{}"
  for name in syncserver toxiproxy_proxy toxiproxy_api recorder; do
    case "$name" in
      syncserver) ep="$server" ;;
      toxiproxy_proxy) ep="$proxy" ;;
      toxiproxy_api) ep="$api" ;;
      recorder) ep="$recorder" ;;
    esac
    [ -n "$ep" ] || { [ "$name" = recorder ] && continue; }
    checks=$((checks + 1))
    case "$ep" in
      127.0.0.1:[0-9]*) ;;
      *)
        failed=$((failed + 1))
        mg_failure "endpoint-$name" "$name has no endpoint on 127.0.0.1 (got '${ep:-none}')"
        continue
        ;;
    esac
    if [ "$name" = recorder ]; then
      if nc -z -G 3 127.0.0.1 "${ep##*:}" >/dev/null 2>&1; then code=tcp; else code=000; fi
      probes="$(jq -c --arg n "$name" --argjson ok "$([ "$code" = tcp ] && echo true || echo false)" \
        '. + {($n): {loopback_tcp: $ok}}' <<<"$probes")"
    else
      code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "http://$ep/" 2>/dev/null)"
      probes="$(jq -c --arg n "$name" --arg c "$code" '. + {($n): {loopback_http: ($c | tonumber? // 0)}}' <<<"$probes")"
    fi
    if [ -z "$code" ] || [ "$code" = 000 ]; then
      failed=$((failed + 1))
      mg_failure "loopback-$name" "$name did not answer at $ep"
    fi
  done

  # From the Mac's LAN address, every port is refused.
  local lan port rc
  lan="$(mg_sync_rig__lan_address)"
  if [ -n "$lan" ]; then
    for name in syncserver toxiproxy_proxy toxiproxy_api recorder; do
      case "$name" in
        syncserver) ep="$server" ;;
        toxiproxy_proxy) ep="$proxy" ;;
        toxiproxy_api) ep="$api" ;;
        recorder) ep="$recorder" ;;
      esac
      port="${ep##*:}"
      [ -n "$ep" ] && [ -n "$port" ] || continue
      checks=$((checks + 1))
      curl -s -o /dev/null --max-time 3 "http://$lan:$port/" 2>/dev/null
      rc=$?
      probes="$(jq -c --arg n "$name" --argjson rc "$rc" '.[$n] += {lan_curl_exit: $rc}' <<<"$probes")"
      # curl exits 7 when the connection is refused and 28 on a timeout; 0 and
      # every HTTP-level exit mean something answered.
      case "$rc" in
        7 | 28) ;;
        *)
          failed=$((failed + 1))
          mg_failure "lan-$name" "$name answered on the LAN address $lan:$port (curl exit $rc)"
          exposed="yes"
          ;;
      esac
    done
  fi
  mg_detail probes "$(jq -c --arg lan "$lan" '{lan_address: (if $lan == "" then null else $lan end), endpoints: .}' <<<"$probes")"

  # The server is on the real clock: its Date header is within the skew limit.
  local date_header server_secs now skew
  date_header="$(curl -s -D - -o /dev/null --max-time 5 "http://$server/" 2>/dev/null |
    tr -d '\r' | sed -n 's/^[Dd]ate: //p' | head -n 1)"
  checks=$((checks + 1))
  if [ -n "$date_header" ] && server_secs="$(LC_ALL=C date -j -u -f '%a, %d %b %Y %H:%M:%S GMT' "$date_header" +%s 2>/dev/null)"; then
    now="$(date -u +%s)"
    skew=$((server_secs - now))
    mg_detail clock "$(jq -nc --arg d "$date_header" --argjson s "$skew" '{server_date: $d, skew_secs: $s}')"
    if [ "${skew#-}" -gt "$MG_SYNC_RIG_MAX_SKEW" ]; then
      failed=$((failed + 1))
      mg_failure clock-skew "the server's clock is ${skew} s off the Mac's; end-to-end runs need the real clock"
    fi
  else
    failed=$((failed + 1))
    mg_failure clock-unreadable "the server sent no readable Date header ('${date_header}'), so its clock is unverified"
  fi

  mg_count total "$checks"
  mg_count passed $((checks - failed))
  mg_count failed "$failed"
  if [ -n "$exposed" ]; then
    echo "sync-rig: the rig is reachable beyond 127.0.0.1; stopping it" >&2
    # shellcheck disable=SC2086 # one service per word
    mg_sync_rig__compose rm --stop --force --volumes $MG_SYNC_RIG_SERVICES >&2
    mg_message "sync-rig: reachable beyond 127.0.0.1, stopped"
    mg_status fail
  elif [ "$failed" -gt 0 ]; then
    mg_sync_rig__logs
    mg_message "sync-rig $verb: $failed of $checks checks failed"
    mg_status fail
  else
    mg_message "sync-rig: up, syncserver http://$server/, toxiproxy http://$proxy/${recorder:+, recorder http://$recorder/}"
    mg_status pass
  fi
}
