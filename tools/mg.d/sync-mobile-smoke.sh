# SPDX-License-Identifier: MIT
# One temporary synthetic phone endpoint. No compatibility or device credit.
mg_job sync-server-mobile-smoke host mg_sync_mobile_smoke \
  "Temporary owned Quick Tunnel: prepare|start|status|stop (no arguments)"

MG_MOBILE_TOOL_REF=cloudflare/cloudflared:2026.11.0@sha256:b94b96edf2c9c4360f8879c8580cfaa99077f19c849ec595ca2f711b27d4f344

# Save intent before creation. A failed create is not evidence of absence.
mg_mobile__intent() {
  local kind="$1" name="$2" image="$3" network="$4"
  jq -n --arg name "$name" --arg image "$image" --arg network "$network" \
    --arg run "$MG_RUN_ID" --arg owner "$MG_MOBILE_OWNER" \
    '{name:$name,image:$image,network:$network,run:$run,owner:$owner}' >"$MG_MOBILE_STATE/$kind-intent.json"
}

mg_mobile__observe() {
  local kind="$1" phase="${2:-live}" ids created_id=''
  ids="$(docker container ls -aq --no-trunc --filter "name=^/$(jq -r .name "$MG_MOBILE_STATE/$kind-intent.json")$")" || return 1
  [[ "$ids" =~ ^[0-9a-f]{64}$ ]] || return 1
  case "$kind" in
    service) [ ! -s "$MG_RUN_DIR/service-id" ] || created_id="$(cat "$MG_RUN_DIR/service-id")" ;;
    *) [ ! -s "$MG_RUN_DIR/mobile-$kind-id" ] || created_id="$(cat "$MG_RUN_DIR/mobile-$kind-id")" ;;
  esac
  [ -z "$created_id" ] || [ "$ids" = "$created_id" ] || return 1
  docker inspect "$ids" >"$MG_RUN_DIR/mobile-observed.json" || return 1
  jq -e --arg id "$ids" --slurpfile intent "$MG_MOBILE_STATE/$kind-intent.json" '
    $intent[0] as $i | length==1 and .[0].Id==$id and .[0].Name==("/"+$i.name) and
    .[0].Image==$i.image and .[0].Config.Image==$i.image and .[0].Config.Labels["meguri.sync.owner"]==$i.owner and
    .[0].Config.Labels["meguri.mg.run-id"]==$i.run and
    .[0].HostConfig.NetworkMode==$i.network and (.[0].Created | length>0)
  ' "$MG_RUN_DIR/mobile-observed.json" >/dev/null || return 1
  # Baseline is frozen once. Include Created and all signal-relevant config.
  if [ -f "$MG_MOBILE_STATE/$kind-container.json" ]; then
    jq -e --arg phase "$phase" --slurpfile before "$MG_MOBILE_STATE/$kind-container.json" '
      # Engine may report this optional boolean as false at create and null
      # after exit. Normalize only the comparison; retain the saved evidence.
      def binding: {Id,Name,Created,Image,Config,
        HostConfig:(.HostConfig | if .OomKillDisable==false then .OomKillDisable=null else . end),
        mounts:[.Mounts[] | select(.Type!="tmpfs")]};
      # Docker clears endpoint addressing on graceful stop. All other network
      # fields stay bound; each cleared field must be original or its empty value.
      def stopped_network($old):
        . as $now |
        {EndpointID:"",Gateway:"",IPAddress:"",IPPrefixLen:0,IPv6Gateway:"",
         GlobalIPv6Address:"",GlobalIPv6PrefixLen:0,MacAddress:""} as $empty |
        (delpaths($empty|keys|map([.])))==($old|delpaths($empty|keys|map([.]))) and
        all($empty|keys[]; . as $k |
          $now[$k]==$old[$k] or $now[$k]==$empty[$k]);
      .[0].HostConfig.OomKillDisable!=true and
      $before[0][0].HostConfig.OomKillDisable!=true and
      (.[0]|binding)==($before[0][0]|binding) and
      (if $phase=="stopped" then
        .[0].State.Running==false and .[0].State.ExitCode==0 and
        (.[0].NetworkSettings.Networks as $now |
         $before[0][0].NetworkSettings.Networks as $old |
         ($now|keys)==($old|keys) and
         all($now|keys[]; . as $k | $now[$k]|stopped_network($old[$k])))
       elif $before[0][0].State.Running then
        .[0].NetworkSettings.Networks==$before[0][0].NetworkSettings.Networks
       else true end)
    ' "$MG_RUN_DIR/mobile-observed.json" >/dev/null || return 1
    # Docker assigns the endpoint IPv4 on start, not create. Freeze it at the
    # first live observation; later observations must retain that exact peer.
    if jq -e '.[0].State.Running==false' "$MG_MOBILE_STATE/$kind-container.json" >/dev/null &&
       jq -e '.[0].State.Running' "$MG_RUN_DIR/mobile-observed.json" >/dev/null; then
      cp "$MG_RUN_DIR/mobile-observed.json" "$MG_MOBILE_STATE/$kind-container.json" || return 1
    fi
  else
    cp "$MG_RUN_DIR/mobile-observed.json" "$MG_MOBILE_STATE/$kind-container.json" || return 1
  fi
}

mg_mobile__remove() {
  local kind="$1" name ids
  [ -f "$MG_MOBILE_STATE/$kind-intent.json" ] || return 0
  name="$(jq -er .name "$MG_MOBILE_STATE/$kind-intent.json")" || return 1
  ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")" || return 1
  [ -n "$ids" ] || return 0
  mg_mobile__observe "$kind" || return 1
  # Signal by admitted CID, never by a reusable name.
  docker rm -f "$(jq -r '.[0].Id' "$MG_RUN_DIR/mobile-observed.json")" >/dev/null || return 1
  ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")" || return 1
  [ -z "$ids" ]
}

mg_mobile__network_check() {
  local ids
  ids="$(docker network ls -q --no-trunc --filter "name=^$MG_MOBILE_NETWORK$")" || return 1
  [[ "$ids" =~ ^[0-9a-f]{64}$ ]] || return 1
  docker network inspect "$ids" >"$MG_RUN_DIR/mobile-network.json" || return 1
  jq -e --slurpfile intent "$MG_MOBILE_STATE/network-intent.json" '
    $intent[0] as $i | length==1 and .[0].Name==$i.name and .[0].Driver=="bridge" and
    .[0].Internal==false and .[0].Labels["meguri.sync.owner"]==$i.owner and
    .[0].Labels["meguri.mg.run-id"]==$i.run
  ' "$MG_RUN_DIR/mobile-network.json" >/dev/null || return 1
  if [ -f "$MG_MOBILE_STATE/network.json" ]; then
    jq -e --slurpfile before "$MG_MOBILE_STATE/network.json" '
      def binding: {Id,Name,Created,Driver,Internal,Labels,Options,IPAM};
      (.[0]|binding)==($before[0][0]|binding)
    ' "$MG_RUN_DIR/mobile-network.json" >/dev/null || return 1
  else cp "$MG_RUN_DIR/mobile-network.json" "$MG_MOBILE_STATE/network.json" || return 1; fi
}

mg_mobile__stop() {
  # Never leave the public route open while touching the backend.
  mg_mobile__remove tunnel || return 1
  mg_mobile__remove probe || return 1
  mg_mobile__remove tool || return 1
  [ -f "$MG_MOBILE_STATE/network-intent.json" ] || return 0
  # The unchanged initializer records its nonce before Compose; cancellation
  # may leave that one fixed child behind without returning a CID to the caller.
  if [ -f "$MG_SYNC_INSTANCE/instance.json" ]; then
    if [ ! -f "$MG_MOBILE_STATE/init-intent.json" ]; then
      jq --arg network "$MG_MOBILE_NETWORK" '
        {name:("meguri-init-"+.run_id),image,network:$network,run:.run_id,owner}
      ' "$MG_SYNC_INSTANCE/instance.json" >"$MG_MOBILE_STATE/init-intent.json" || return 1
    fi
    mg_mobile__remove init || return 1
  fi
  # The local helper writes its nonce before Compose. Cover cancellation inside
  # that helper, before start's normal return can capture the service intent.
  if [ ! -f "$MG_MOBILE_STATE/service-intent.json" ] && [ -f "$MG_SYNC_INSTANCE/container.json" ]; then
    jq --arg image "$(jq -r .image "$MG_SYNC_INSTANCE/instance.json")" --arg network "$MG_MOBILE_NETWORK" \
      '. + {image:$image,network:$network}' "$MG_SYNC_INSTANCE/container.json" >"$MG_MOBILE_STATE/service-intent.json" || return 1
  fi
  if [ -f "$MG_MOBILE_STATE/service-intent.json" ]; then
    local name ids cid
    name="$(jq -r .name "$MG_MOBILE_STATE/service-intent.json")" || return 1
    ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")" || return 1
    if [ -n "$ids" ]; then
      mg_mobile__observe service || return 1
      cid="$(jq -er '.[0].Id' "$MG_MOBILE_STATE/service-container.json")" || return 1
      # Recheck the entire saved binding at disposal, never re-enter the local
      # helper that admits a reusable name using only its labels.
      mg_mobile__observe service || return 1
      [ "$(jq -r '.[0].Id' "$MG_RUN_DIR/mobile-observed.json")" = "$cid" ] || return 1
      docker stop --time 30 "$cid" >/dev/null || return 1
      mg_mobile__observe service stopped || return 1
      [ "$(jq -r '.[0].Id' "$MG_RUN_DIR/mobile-observed.json")" = "$cid" ] || return 1
      docker rm "$cid" >/dev/null || return 1
      ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")" || return 1
      [ -z "$ids" ] || return 1
    fi
  fi
  if [ -f "$MG_MOBILE_STATE/network-intent.json" ]; then
    local networks
    networks="$(docker network ls -q --no-trunc --filter "name=^$MG_MOBILE_NETWORK$")" || return 1
    if [ -n "$networks" ]; then
      mg_mobile__network_check || return 1
      jq -e '.[0].Containers | length==0' "$MG_RUN_DIR/mobile-network.json" >/dev/null || return 1
      docker network rm "$(jq -r '.[0].Id' "$MG_RUN_DIR/mobile-network.json")" >/dev/null || return 1
      networks="$(docker network ls -q --no-trunc --filter "name=^$MG_MOBILE_NETWORK$")" || return 1
      [ -z "$networks" ] || return 1
    fi
  fi
  touch "$MG_MOBILE_STATE/stopped"
}

mg_mobile__cleanup() {
  local MG_MOBILE_STATE="$1" MG_SYNC_INSTANCE="$1" MG_MOBILE_NETWORK="$2"
  [ -f "$3" ] && return 0
  if ! mg_mobile__stop; then
    mg_failure mobile-cleanup "Owned route cleanup refused or absence unproved; preserve the instance and investigate"
    mg_status fail
    return 1
  fi
}

# Called only by MG's fixed-job cancellation branch, after the job has joined.
# Acquire the same lock: an uncertain/stale lock is a refusal, never permission
# to sweep labelled containers or delete retained ownership records.
mg_mobile__cancel_cleanup() {
  local MG_MOBILE_STATE="$MG_ROOT/build/sync-server-mobile-smoke"
  local MG_SYNC_INSTANCE="$MG_ROOT/build/sync-server-mobile-smoke" MG_MOBILE_NETWORK ids rc=0
  if [ -L "$MG_MOBILE_STATE" ] || ! mkdir -p "$MG_MOBILE_STATE" || ! chmod 0700 "$MG_MOBILE_STATE" ||
     ! mkdir "$MG_MOBILE_STATE/lock"; then
    rc=1
  else
    MG_MOBILE_NETWORK="meguri-mobile-$MG_RUN_ID"
    if [ -f "$MG_MOBILE_STATE/network-intent.json" ]; then
      MG_MOBILE_NETWORK="$(jq -er .name "$MG_MOBILE_STATE/network-intent.json")" || rc=1
    fi
    if [ "$rc" -eq 0 ]; then mg_mobile__stop || rc=1; fi
    # Unknown children are evidence, not candidates for arbitrary cleanup.
    ids="$(docker ps -aq --filter "label=meguri.mg.run-id=$MG_RUN_ID")" || rc=1
    [ -z "$ids" ] || rc=1
    rmdir "$MG_MOBILE_STATE/lock" || rc=1
  fi
  if [ "$rc" -ne 0 ]; then
    mg_failure mobile-cleanup "Strict mobile cancellation cleanup refused or absence unproved; preserve owned records and volume"
    mg_status fail
    return 1
  fi
}

mg_mobile__remove_at() {
  local MG_MOBILE_STATE="$1"
  mg_mobile__remove "$2" || {
    mg_failure mobile-cleanup "Owned temporary child cleanup refused or absence unproved"
    mg_status fail
    return 1
  }
}

mg_mobile__tool() {
  docker image inspect "$MG_MOBILE_TOOL_REF" >"$MG_RUN_DIR/mobile-tool-current.json" || return 1
  cmp -s "$MG_RUN_DIR/mobile-tool-current.json" "$MG_MOBILE_STATE/tool-image.json" || return 1
  MG_MOBILE_IMAGE="$(jq -er '.[0].Id' "$MG_MOBILE_STATE/tool-image.json")" || return 1
  [[ "$MG_MOBILE_IMAGE" =~ ^sha256:[0-9a-f]{64}$ ]]
}

mg_mobile__prepare() {
  [ ! -e "$MG_MOBILE_STATE/network-intent.json" ] || return 1
  if [ -f "$MG_MOBILE_STATE/tool-version.txt" ]; then
    mg_mobile__tool && mg_mobile__remove tool &&
      grep -Eq '^cloudflared version 2026\.11\.0([ (]|$)' "$MG_MOBILE_STATE/tool-version.txt"
    return $?
  fi
  docker pull "$MG_MOBILE_TOOL_REF" >/dev/null || return 1
  docker image inspect "$MG_MOBILE_TOOL_REF" >"$MG_MOBILE_STATE/tool-image.json" || return 1
  jq -e --arg digest "${MG_MOBILE_TOOL_REF#*@}" '
    length==1 and .[0].Os=="linux" and
    (.[0].Architecture=="arm64" or .[0].Architecture=="amd64") and
    any(.[0].RepoDigests[]; endswith("@"+$digest))
  ' "$MG_MOBILE_STATE/tool-image.json" >/dev/null || return 1
  mg_mobile__tool || return 1
  mg_mobile__intent tool "meguri-mobile-tool-$MG_RUN_ID" "$MG_MOBILE_IMAGE" none || return 1
  mg_on_exit "$(printf '%q ' mg_mobile__remove_at "$MG_MOBILE_STATE" tool)"
  docker create --name "meguri-mobile-tool-$MG_RUN_ID" --network none \
    --user 65532:65532 --read-only --cap-drop ALL --security-opt no-new-privileges \
    --pids-limit 64 --memory 128m --tmpfs /tmp:uid=65532,gid=65532,mode=0700,size=16m \
    --label "meguri.sync.owner=$MG_MOBILE_OWNER" --label "meguri.mg.run-id=$MG_RUN_ID" \
    "$MG_MOBILE_IMAGE" --version >"$MG_RUN_DIR/mobile-tool-id" || return 1
  mg_mobile__observe tool || return 1
  jq -e --slurpfile tool "$MG_MOBILE_STATE/tool-image.json" '
    .[0] as $c | $c.Config.Entrypoint==$tool[0][0].Config.Entrypoint and
    $c.Config.Cmd==["--version"] and $c.Config.User=="65532:65532" and
    $c.HostConfig.ReadonlyRootfs and $c.HostConfig.CapDrop==["ALL"] and
    ([$c.Mounts[] | select(.Type!="tmpfs")] | length)==0 and
    (($c.HostConfig.PortBindings // {}) | length)==0
  ' "$MG_RUN_DIR/mobile-observed.json" >/dev/null || return 1
  docker start -a "$(jq -r '.[0].Id' "$MG_MOBILE_STATE/tool-container.json")" >"$MG_MOBILE_STATE/tool-version.txt" || return 1
  grep -Eq '^cloudflared version 2026\.11\.0([ (]|$)' "$MG_MOBILE_STATE/tool-version.txt" || return 1
  mg_mobile__remove tool || return 1
  # These tool records belong to this completed prepare, not a later run.
  cp "$MG_MOBILE_STATE/tool-image.json" "$MG_RUN_DIR/mobile-tool-image.json" || return 1
  mg_artifact tunnel-image "$MG_RUN_DIR/mobile-tool-image.json"
  mg_artifact tunnel-version "$MG_MOBILE_STATE/tool-version.txt"
}

# Bounded, owned log read. Never retain arbitrary log text in MG artifacts.
mg_mobile__origin() {
  python3 - "$(jq -r '.[0].Id' "$MG_MOBILE_STATE/tunnel-container.json")" <<'PY'
import re, subprocess, sys, time
cid = sys.argv[1]
deadline = time.monotonic() + 30
while time.monotonic() < deadline:
    result = subprocess.run(['docker', 'logs', '--tail', '200', cid],
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=5)
    if result.returncode:
        raise SystemExit('Owned tunnel logs unavailable')
    # Match whole URL tokens, so ports, paths, userinfo and suffix attacks fail.
    urls = re.findall(rb'https?://[^\s<>"\x27]+', result.stdout)
    candidates = [u for u in urls if b'trycloudflare.com' in u.lower()]
    if candidates:
        if len(candidates) != 1 or not re.fullmatch(
                rb'https://[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.trycloudflare\.com', candidates[0]):
            raise SystemExit('Ambiguous or invalid generated origin')
        print(candidates[0].decode('ascii'))
        raise SystemExit(0)
    time.sleep(1)
raise SystemExit('Owned tunnel did not yield one HTTPS origin')
PY
}

mg_mobile__service_admit() {
  mg_mobile__observe service || return 1
  jq -e --arg origin "$MEGURI_SYNC_ORIGIN" --arg peer "$MEGURI_SYNC_TRUSTED_PEER" \
    --arg network "$MG_MOBILE_NETWORK" --arg volume "$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json")" '
    .[0] as $c | $c.State.Running and $c.Config.User=="10001:10001" and
    $c.Config.Cmd==["serve","/data/owner",$origin,"0.0.0.0:8080","--trusted-peer",$peer] and
    $c.HostConfig.ReadonlyRootfs and $c.HostConfig.CapDrop==["ALL"] and
    ($c.HostConfig.SecurityOpt | index("no-new-privileges:true")!=null or index("no-new-privileges")!=null) and
    $c.HostConfig.PortBindings=={"8080/tcp":[{"HostIp":"127.0.0.1","HostPort":"18449"}]} and
    ([$c.Mounts[] | select(.Type!="tmpfs")] | length==1 and
      .[0].Type=="volume" and .[0].Name==$volume and .[0].Destination=="/data") and
    ($c.NetworkSettings.Networks | keys)==[$network]
  ' "$MG_RUN_DIR/mobile-observed.json" >/dev/null
}

mg_mobile__tunnel_admit() {
  mg_mobile__observe tunnel || return 1
  jq -e --arg service "meguri-local-$(jq -r .run "$MG_MOBILE_STATE/tunnel-intent.json")" \
    --arg network "$MG_MOBILE_NETWORK" --arg peer "${MEGURI_SYNC_TRUSTED_PEER:-}" \
    --arg network_id "$(jq -r '.[0].Id' "$MG_MOBILE_STATE/network.json")" \
    --slurpfile tool "$MG_MOBILE_STATE/tool-image.json" '
    .[0] as $c | $c.State.Running and $c.Config.User=="65532:65532" and
    $c.Config.Entrypoint==$tool[0][0].Config.Entrypoint and
    $c.Config.Cmd==["tunnel","--no-autoupdate","--url",("http://"+$service+":8080")] and
    $c.HostConfig.ReadonlyRootfs and $c.HostConfig.CapDrop==["ALL"] and
    ($c.HostConfig.SecurityOpt | index("no-new-privileges:true")!=null or index("no-new-privileges")!=null) and
    $c.HostConfig.PidsLimit==64 and $c.HostConfig.Memory==134217728 and
    ([$c.Mounts[] | select(.Type!="tmpfs")] | length)==0 and
    (($c.HostConfig.PortBindings // {}) | length)==0 and
    ($c.NetworkSettings.Networks | keys)==[$network] and
    $c.NetworkSettings.Networks[$network].NetworkID==$network_id and
    (if $peer!="" then $c.NetworkSettings.Networks[$network].IPAddress==$peer else true end)
  ' "$MG_RUN_DIR/mobile-observed.json" >/dev/null
}

mg_mobile__route() {
  local image="$1" name="meguri-mobile-probe-$MG_RUN_ID" rc=0
  mg_mobile__intent probe "$name" "$image" "$MG_MOBILE_NETWORK" || return 1
  mg_on_exit "$(printf '%q ' mg_mobile__remove_at "$MG_MOBILE_STATE" probe)"
  # No auth/setup data. Exact private/public health equality binds the public
  # endpoint to the admitted live package; TLS verification and redirects stay strict.
  docker create --name "$name" --network "$MG_MOBILE_NETWORK" --user 10001:10001 \
    --read-only --cap-drop ALL --security-opt no-new-privileges --pids-limit 64 --memory 128m \
    --label "meguri.sync.owner=$MG_MOBILE_OWNER" --label "meguri.mg.run-id=$MG_RUN_ID" \
    --entrypoint node "$image" --input-type=module -e '
      import assert from "node:assert/strict";
      const [backend, origin] = process.argv.slice(1);
      async function health(url) {
        const r = await fetch(url + "/health", {redirect:"error", signal:AbortSignal.timeout(15000), headers:{Origin:origin}});
        assert.equal(r.status, 200);
        const body = await r.text(); assert.ok(body.length < 4096);
        const data = JSON.parse(body).data;
        assert.equal(data.state, "account_substrate"); assert.equal(data.collection_compatibility, false);
        return body;
      }
      assert.equal(await health(backend), await health(origin));
    ' "http://meguri-local-$MG_RUN_ID:8080" "$MEGURI_SYNC_ORIGIN" >"$MG_RUN_DIR/mobile-probe-id" || return 1
  mg_mobile__observe probe || return 1
  jq -e '.[0] as $c | $c.Config.Entrypoint==["node"] and $c.Config.User=="10001:10001" and
    $c.HostConfig.ReadonlyRootfs and $c.HostConfig.CapDrop==["ALL"] and
    ([$c.Mounts[] | select(.Type!="tmpfs")] | length)==0 and
    (($c.HostConfig.PortBindings // {}) | length)==0
  ' "$MG_RUN_DIR/mobile-observed.json" >/dev/null || return 1
  # Discard all response/error bodies, including unexpected intermediary pages.
  docker start -a "$(jq -r '.[0].Id' "$MG_MOBILE_STATE/probe-container.json")" >/dev/null 2>&1 || rc=1
  mg_mobile__remove probe || rc=1
  [ "$rc" -eq 0 ]
}

mg_mobile__start() {
  [ ! -e "$MG_MOBILE_STATE/network-intent.json" ] && [ ! -e "$MG_SYNC_INSTANCE/instance.json" ] || return 1
  mg_mobile__tool || return 1
  # No endpoint/port selector and no adoption of another process's listener.
  python3 - <<'PY'
import socket
with socket.socket() as listener:
    listener.bind(('127.0.0.1', 18449))
PY
  [ "$?" -eq 0 ] || return 1
  local existing marker="$MG_RUN_DIR/mobile-retained" client
  existing="$(docker network ls -q --no-trunc --filter "name=^$MG_MOBILE_NETWORK$")" || return 1
  [ -z "$existing" ] || return 1
  mg_mobile__intent network "$MG_MOBILE_NETWORK" '' '' || return 1
  mg_on_exit "$(printf '%q ' mg_mobile__cleanup "$MG_MOBILE_STATE" "$MG_MOBILE_NETWORK" "$marker")"
  docker network create --driver bridge --label "meguri.sync.owner=$MG_MOBILE_OWNER" \
    --label "meguri.mg.run-id=$MG_RUN_ID" "$MG_MOBILE_NETWORK" >"$MG_RUN_DIR/mobile-network-id" || return 1
  mg_mobile__network_check || return 1
  mg_mobile__intent tunnel "meguri-mobile-tunnel-$MG_RUN_ID" "$MG_MOBILE_IMAGE" "$MG_MOBILE_NETWORK" || return 1
  docker create --name "meguri-mobile-tunnel-$MG_RUN_ID" --network "$MG_MOBILE_NETWORK" \
    --user 65532:65532 --read-only --cap-drop ALL --security-opt no-new-privileges \
    --pids-limit 64 --memory 128m --tmpfs /tmp:uid=65532,gid=65532,mode=0700,size=16m \
    --label "meguri.sync.owner=$MG_MOBILE_OWNER" --label "meguri.mg.run-id=$MG_RUN_ID" \
    "$MG_MOBILE_IMAGE" tunnel --no-autoupdate --url "http://meguri-local-$MG_RUN_ID:8080" >"$MG_RUN_DIR/mobile-tunnel-id" || return 1
  mg_mobile__observe tunnel || return 1
  docker start "$(jq -r '.[0].Id' "$MG_MOBILE_STATE/tunnel-container.json")" >/dev/null || return 1
  mg_mobile__tunnel_admit || return 1
  MEGURI_SYNC_ORIGIN="$(mg_mobile__origin)" || return 1
  MEGURI_SYNC_TRUSTED_PEER="$(jq -er --arg network "$MG_MOBILE_NETWORK" '.[0].NetworkSettings.Networks[$network].IPAddress' "$MG_MOBILE_STATE/tunnel-container.json")" || return 1
  python3 - "$MEGURI_SYNC_TRUSTED_PEER" <<'PY'
import ipaddress, sys
ip = ipaddress.IPv4Address(sys.argv[1])
assert not (ip.is_unspecified or ip.is_loopback or ip.is_multicast)
PY
  [ "$?" -eq 0 ] || return 1
  jq -n --arg origin "$MEGURI_SYNC_ORIGIN" --arg peer "$MEGURI_SYNC_TRUSTED_PEER" \
    --arg receipt "$MG_MOBILE_RECEIPT" '{origin:$origin,peer:$peer,receipt:$receipt}' >"$MG_MOBILE_STATE/route.json" || return 1
  mg_mobile__network_check && mg_mobile__tunnel_admit || return 1
  mg_sync_package__init "$MG_MOBILE_RECEIPT" || return 1
  # Suppress the helper's separate backend-only exit action. This utility's
  # registered cleanup owns tunnel-first ordering even during partial startup.
  touch "$MG_RUN_DIR/start-retained" || return 1
  # Start helper records its nonce before Compose creation. Capture the intent
  # immediately after it returns, including partial failures, for strict cleanup.
  local rc=0
  mg_sync_package__start || rc=1
  if [ -f "$MG_SYNC_INSTANCE/container.json" ]; then
    jq --arg image "$(jq -r .image "$MG_SYNC_INSTANCE/instance.json")" --arg network "$MG_MOBILE_NETWORK" \
      '. + {image:$image,network:$network}' "$MG_SYNC_INSTANCE/container.json" >"$MG_MOBILE_STATE/service-intent.json" || return 1
  fi
  [ "$rc" -eq 0 ] || return 1
  mg_mobile__service_admit && mg_mobile__tunnel_admit || return 1
  client="$(jq -er .details.package.client_image_id "$MG_MOBILE_RECEIPT")" || return 1
  mg_mobile__route "$client" || return 1
  mg_mobile__service_admit && mg_mobile__tunnel_admit || return 1
  touch "$marker" || return 1
  mg_message "Temporary endpoint ready. Private owner handoff: $MG_SYNC_INSTANCE/setup.txt (never an MG artifact)."
}

mg_sync_mobile_smoke() {
  local action="${1:-}" MG_MOBILE_STATE="$MG_ROOT/build/sync-server-mobile-smoke"
  local MG_SYNC_INSTANCE="$MG_ROOT/build/sync-server-mobile-smoke" MG_MOBILE_OWNER MG_MOBILE_NETWORK
  local MG_MOBILE_IMAGE MG_MOBILE_RECEIPT MEGURI_SYNC_ORIGIN MEGURI_SYNC_TRUSTED_PEER
  local MEGURI_SYNC_NETWORK MEGURI_SYNC_PORT=18449 MG_SYNC_INSTANCE_SUFFIX=mobile-smoke
  local MG_SYNC_SETUP_FILE="$MG_ROOT/build/sync-server-mobile-smoke/setup.txt" rc=0
  [ "$#" -eq 1 ] || { mg_failure usage "Use prepare|start|status|stop without options"; mg_status error; return; }
  case "$action" in prepare|start|status|stop) ;; *) mg_failure usage "Unknown mobile smoke mode"; mg_status error; return ;; esac
  [ ! -L "$MG_MOBILE_STATE" ] || return 1
  mkdir -p "$MG_MOBILE_STATE" || return 1
  chmod 0700 "$MG_MOBILE_STATE" || return 1
  mkdir "$MG_MOBILE_STATE/lock" || { mg_failure mobile-owner "Another lifecycle command holds the lock"; mg_status fail; return; }
  mg_on_exit "$(printf '%q ' rmdir "$MG_MOBILE_STATE/lock")"
  MG_MOBILE_NETWORK="meguri-mobile-$MG_RUN_ID"
  if [ -f "$MG_MOBILE_STATE/network-intent.json" ]; then
    MG_MOBILE_NETWORK="$(jq -er .name "$MG_MOBILE_STATE/network-intent.json")" || return 1
  fi
  MEGURI_SYNC_NETWORK="$MG_MOBILE_NETWORK"
  MG_MOBILE_OWNER="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  case "$action" in
    prepare|start)
      MG_MOBILE_RECEIPT="$(cd "$MG_ROOT/build/mg/sync-server-package/latest" && pwd -P)/result.json" || return 1
      mg_sync_package__receipt "$MG_MOBILE_RECEIPT" || { mg_failure package-receipt "Latest package must be current, clean and canonical"; mg_status fail; return; }
      if [ "$action" = prepare ]; then mg_mobile__prepare || rc=1; else mg_mobile__start || rc=1; fi ;;
    stop) mg_mobile__stop || rc=1 ;;
    status)
      if [ -f "$MG_MOBILE_STATE/network-intent.json" ]; then
        if [ -f "$MG_MOBILE_STATE/stopped" ]; then
          local kind ids
          for kind in tunnel service; do
            [ -f "$MG_MOBILE_STATE/$kind-intent.json" ] || continue
            ids="$(docker container ls -aq --no-trunc --filter "name=^/$(jq -r .name "$MG_MOBILE_STATE/$kind-intent.json")$")" || return 1
            [ -z "$ids" ] || rc=1
          done
        else
          MEGURI_SYNC_ORIGIN="$(jq -er .origin "$MG_MOBILE_STATE/route.json")" || return 1
          MEGURI_SYNC_TRUSTED_PEER="$(jq -er .peer "$MG_MOBILE_STATE/route.json")" || return 1
          mg_mobile__network_check && mg_mobile__tunnel_admit && mg_mobile__service_admit || rc=1
        fi
      fi ;;
  esac
  if [ "$rc" -ne 0 ]; then
    mg_failure mobile-smoke "Temporary endpoint operation failed; preserve owned records/volume; stop closes the tunnel first"
    mg_status fail; return
  fi
  mg_detail compatibility_credit false
  mg_detail tunnel_tool "$(jq -nc --arg ref "$MG_MOBILE_TOOL_REF" '{reference:$ref}')"
  [ ! -f "$MG_MOBILE_STATE/tool-image.json" ] || mg_detail tool_image "$(jq -c '.[0] | {Id,Architecture,Os,RepoDigests}' "$MG_MOBILE_STATE/tool-image.json")"
  [ ! -f "$MG_MOBILE_STATE/route.json" ] || mg_detail route "$(cat "$MG_MOBILE_STATE/route.json")"
  [ ! -f "$MG_MOBILE_STATE/instance.json" ] || mg_detail package_image "$(jq -c '{image,build_receipt}' "$MG_MOBILE_STATE/instance.json")"
  if [ -f "$MG_MOBILE_STATE/stopped" ]; then mg_detail lifecycle '"stopped; owned children absent; volume retained"'
  elif [ -f "$MG_MOBILE_STATE/route.json" ]; then mg_detail lifecycle '"running"'
  else mg_detail lifecycle '"prepared or not started"'; fi
  mg_status pass
}
