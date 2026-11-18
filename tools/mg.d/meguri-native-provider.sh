# SPDX-License-Identifier: MIT
# The diagnostic stays accountless. ui-test's explicit owned option reuses its
# route lifecycle; it cannot acquire a second lease or publish native evidence.
mg_job meguri-native-route host mg_meguri_native_route "One owned leased-VM HTTPS health request; explicit canonical package receipt only"

mg_native_route__source() {
  mg_sync_server__clean && [ "$(mg_meguri_provider__commit)" = "$MG_NR_COMMIT" ] &&
    [ "$(shasum -a 256 "$MG_ROOT/server/tools/package-smoke.mjs" | awk '{print $1}')" = "$MG_NR_SCRIPT_SHA" ]
}
mg_native_route__stage() { MG_NR_STAGE="$1"; }
mg_native_route__failure() {
  if [ -z "$MG_NR_FIRST" ]; then
    MG_NR_FIRST="$MG_NR_STAGE"
    mg_detail first_stage "$(jq -nc --arg s "$MG_NR_FIRST" '$s')"
    mg_failure "native-route-$MG_NR_FIRST" "Route diagnostic failed; private partial evidence retained; no retry"
  fi
  mg_status fail
}
mg_native_route__ambient() {
  # These names never grant caller authority. MG's configured VM selection is
  # used below, but a preselected wrapper/endpoint/network is never adopted.
  local key
  for key in $(compgen -e); do
    case "$key" in
      MG_NR_*|MEGURI_NATIVE_*|MG_NATIVE_*|MEGURI_SYNC_*|MG_SYNC_*|MG_MAC_VM|MG_MAC_VMS)
        return 1 ;;
    esac
  done
}
mg_native_route__gateway() {
  python3 - "$1" "$2" <<'PY'
import ipaddress, pathlib, re, sys
route, interfaces = [pathlib.Path(x).read_bytes() for x in sys.argv[1:]]
if len(route) > 16384 or len(interfaces) > 262144: raise SystemExit(1)
try:
    route = route.decode('ascii'); interfaces = interfaces.decode('ascii')
    gateways = [line for line in route.splitlines() if re.match(r'^[ \t]*gateway:', line)]
    if len(gateways) != 1: raise ValueError()
    match = re.fullmatch(r'[ \t]*gateway:[ \t]*(\S+)[ \t]*', gateways[0])
    if not match: raise ValueError()
    value = match[1]
    if not re.fullmatch(r'(?:0|[1-9][0-9]{0,2})(?:\.(?:0|[1-9][0-9]{0,2})){3}', value): raise ValueError()
    address = ipaddress.IPv4Address(value)
    if not any(address in ipaddress.IPv4Network(n) for n in ('10.0.0.0/8','172.16.0.0/12','192.168.0.0/16')): raise ValueError()
    actual = re.findall(r'^\s+inet ([0-9.]+) netmask \S+(?: broadcast \S+)?\s*$', interfaces, re.M)
    if value not in actual: raise ValueError()
except (ValueError, UnicodeError): raise SystemExit(1)
print(value)
PY
}
mg_native_route__host_interfaces() { /sbin/ifconfig -a; }
mg_native_route__route_metadata() {
  # Only the leased wrapper reads the guest default route. No SSH destination,
  # forwarding or host route modification is constructed here.
  mg_mac_vm_ssh "/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/perl -e 'alarm 10; exec @ARGV' /sbin/route -n get default" \
    >"$MG_NR_PRIVATE/guest-route" 2>"$MG_NR_PRIVATE/guest-route.err" || return 1
  mg_native_route__host_interfaces >"$MG_NR_PRIVATE/host-interfaces" 2>"$MG_NR_PRIVATE/host-interfaces.err" || return 1
  MG_NR_GATEWAY="$(mg_native_route__gateway "$MG_NR_PRIVATE/guest-route" "$MG_NR_PRIVATE/host-interfaces")"
}
mg_native_route__port() {
  # This chooses once; it is NOT a reservation. The actual publication below
  # must succeed with this exact address/port, including on a collision.
  python3 - "$MG_NR_GATEWAY" <<'PY'
import socket, sys
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
    s.bind((sys.argv[1], 0))
    port = s.getsockname()[1]
    if not 1024 <= port <= 65535: raise SystemExit(1)
    print(port)
PY
}
mg_native_route__images() {
  local suffix="$1"
  docker image inspect "$MG_NR_RUNTIME_IMAGE" >"$MG_NR_PRIVATE/runtime-image-$suffix.json" || return 1
  docker image inspect "$MG_NR_TOOL_IMAGE" >"$MG_NR_PRIVATE/tool-image-$suffix.json" || return 1
  jq -e --arg image "$MG_NR_RUNTIME_IMAGE" --arg source "$MG_NR_COMMIT" '
    length==1 and .[0].Id==$image and .[0].Config.User=="10001:10001" and
    .[0].Config.Entrypoint==["/usr/local/bin/meguri-sync-server"] and
    .[0].Config.Labels["org.opencontainers.image.revision"]==$source
  ' "$MG_NR_PRIVATE/runtime-image-$suffix.json" >/dev/null || return 1
  jq -e --arg image "$MG_NR_TOOL_IMAGE" '
    length==1 and .[0].Id==$image and .[0].Config.Entrypoint==["node","/smoke/package-smoke.mjs"]
  ' "$MG_NR_PRIVATE/tool-image-$suffix.json" >/dev/null || return 1
  if [ "$suffix" != before ]; then
    cmp -s "$MG_NR_PRIVATE/runtime-image-before.json" "$MG_NR_PRIVATE/runtime-image-$suffix.json" &&
      cmp -s "$MG_NR_PRIVATE/tool-image-before.json" "$MG_NR_PRIVATE/tool-image-$suffix.json" || return 1
  fi
}
mg_native_route__network() {
  local kind="${1:-backend}" name id internal file
  case "$kind" in
    backend) name="$MG_NR_NETWORK"; id="$MG_NR_NETWORK_ID"; internal=true; file=network ;;
    ingress) name="$MG_NR_INGRESS"; id="$MG_NR_INGRESS_ID"; internal=false; file=ingress ;;
    *) return 1 ;;
  esac
  docker network inspect "$name" >"$MG_NR_PRIVATE/$file.json" || return 1
  jq -e --arg name "$name" --arg id "$id" --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" \
    --argjson internal "$internal" '
    length==1 and .[0].Id==$id and .[0].Name==$name and .[0].Driver=="bridge" and
    .[0].Internal==$internal and .[0].EnableIPv6==false and
    (.[0].EnableIPv4==true or .[0].EnableIPv4==null) and (.[0].Containers | type)=="object" and
    .[0].Labels=={"meguri.mg.run-id":$run,"meguri.sync.owner":$owner} and
    (.[0].IPAM.Config | length==1 and (.[0].Subnet | type)=="string")
  ' "$MG_NR_PRIVATE/$file.json" >/dev/null || return 1
  python3 - "$MG_NR_PRIVATE/$file.json" <<'PYNET'
import ipaddress, json, sys
try:
    config = json.load(open(sys.argv[1]))[0]['IPAM']['Config'][0]
    net = ipaddress.IPv4Network(config['Subnet'])
    gateway = ipaddress.IPv4Address(config['Gateway'])
    if gateway not in net or gateway in (net.network_address, net.broadcast_address): raise ValueError()
except (ValueError, KeyError, TypeError, IndexError): raise SystemExit(1)
PYNET
}
mg_native_route__members() {
  # Fixture shares the runtime namespace and has no independent attachment.
  [ "$MG_NR_NETWORK_ID" != "$MG_NR_INGRESS_ID" ] &&
    mg_native_route__network && mg_native_route__network ingress || return 1
  jq -e --arg runtime "$MG_NR_RUNTIME_ID" --arg relay "$MG_NR_RELAY_ID" '
    (.[0].Containers | keys)==([$runtime,$relay] | map(select(.!="")) | sort)
  ' "$MG_NR_PRIVATE/network.json" >/dev/null &&
    jq -e --arg relay "$MG_NR_RELAY_ID" '
      (.[0].Containers | keys)==([$relay] | map(select(.!="")))
    ' "$MG_NR_PRIVATE/ingress.json" >/dev/null
}
mg_native_route__target() {
  mg_native_route__container runtime && mg_native_route__members || return 1
  local ip
  ip="$(python3 - "$MG_NR_PRIVATE/runtime.json" "$MG_NR_PRIVATE/network.json" "$MG_NR_NETWORK" \
    "$MG_NR_NETWORK_ID" "$MG_NR_RUNTIME_ID" "$MG_NR_RUNTIME" <<'PYTARGET'
import ipaddress, json, re, sys
try:
    runtime, network = [json.load(open(p))[0] for p in sys.argv[1:3]]
    name, netid, runtimeid, runtime_name = sys.argv[3:]
    if runtime['Id'] != runtimeid or network['Id'] != netid: raise ValueError()
    attachment = runtime['NetworkSettings']['Networks'][name]
    if attachment['NetworkID'] != netid: raise ValueError()
    value = attachment['IPAddress']
    if not re.fullmatch(r'(?:0|[1-9][0-9]{0,2})(?:\.(?:0|[1-9][0-9]{0,2})){3}', value): raise ValueError()
    address = ipaddress.IPv4Address(value)
    subnet = ipaddress.IPv4Network(network['IPAM']['Config'][0]['Subnet'])
    if address not in subnet or address in (subnet.network_address, subnet.broadcast_address): raise ValueError()
    if address == ipaddress.IPv4Address(network['IPAM']['Config'][0]['Gateway']): raise ValueError()
    if attachment['IPPrefixLen'] != subnet.prefixlen: raise ValueError()
    member = network['Containers'][runtimeid]
    if member['Name'] != runtime_name or member['IPv4Address'] != f'{address}/{subnet.prefixlen}': raise ValueError()
    if not member['EndpointID'] or member['EndpointID'] != attachment['EndpointID']: raise ValueError()
except (ValueError, KeyError, TypeError, IndexError): raise SystemExit(1)
print(value)
PYTARGET
)" || return 1
  [ -z "$MG_NR_TARGET" ] || [ "$MG_NR_TARGET" = "$ip" ] || return 1
  MG_NR_TARGET="$ip"
}
mg_native_route__relay_source() {
  # Fixed TLS-transparent byte forwarding only; no TLS, HTTP, DNS or options.
  cat <<'JS'
const net = require('node:net');
const [target, runtime, network, phase] = process.argv.slice(1);
const fail = () => { process.stderr.write('MEGURI_NATIVE_RELAY_FAILED\n'); process.exit(1); };
if (process.argv.length !== 5 || !net.isIPv4(target) || !/^[0-9a-f]{64}$/.test(runtime) || !/^[0-9a-f]{64}$/.test(network) ||
    !['native-route-fixture', 'native-owned-journey-fixture'].includes(phase)) fail();
const journey = phase === 'native-owned-journey-fixture';
const limits = journey
  ? { active: 16, connections: 128, bytes: 33554432, connection: 90000, idle: 30000, life: 900000 }
  : { active: 1, connections: 1, bytes: 65536, connection: 20000, idle: 10000, life: 180000 };
let active = 0, connections = 0, received = 0, sent = 0;
const server = net.createServer({ allowHalfOpen: true }, client => {
  if (active >= limits.active || connections >= limits.connections) { client.destroy(); fail(); }
  active++; connections++;
  const upstream = net.createConnection({ host: target, port: 18443, family: 4, allowHalfOpen: true });
  const deadline = setTimeout(fail, limits.connection);
  let closed = 0;
  client.on('data', b => { received += b.length; if (received > limits.bytes) fail(); });
  upstream.on('data', b => { sent += b.length; if (sent > limits.bytes) fail(); });
  for (const socket of [client, upstream]) {
    socket.setTimeout(limits.idle, fail);
    socket.on('error', fail);
    socket.on('close', hadError => {
      // A graceful close may precede the other pipe's queued writes/finish.
      // Errors and incomplete closes still fail without exposing payloads.
      if (hadError || !socket.readableEnded || !socket.writableFinished) return fail();
      if (++closed === 2) { clearTimeout(deadline); active--; }
    });
  }
  client.pipe(upstream); upstream.pipe(client);
});
server.on('error', fail);
setTimeout(fail, limits.life);
server.listen({ host: '0.0.0.0', port: 18443 }, () => process.stdout.write('MEGURI_NATIVE_RELAY_READY\n'));
JS
}
mg_native_route__container() {
  local kind="$1" name id image
  case "${MG_NR_PHASE:-native-route-fixture}" in
    native-route-fixture|native-owned-journey-fixture) ;;
    *) return 1 ;;
  esac
  case "$kind" in
    runtime) name="$MG_NR_RUNTIME"; id="$MG_NR_RUNTIME_ID"; image="$MG_NR_RUNTIME_IMAGE" ;;
    fixture) name="$MG_NR_FIXTURE"; id="$MG_NR_FIXTURE_ID"; image="$MG_NR_TOOL_IMAGE" ;;
    relay) name="$MG_NR_RELAY"; id="$MG_NR_RELAY_ID"; image="$MG_NR_TOOL_IMAGE" ;;
    *) return 1 ;;
  esac
  docker inspect "$id" >"$MG_NR_PRIVATE/$kind.json" || return 1
  jq -e --arg name "/$name" --arg id "$id" --arg image "$image" --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" '
    length==1 and .[0].Id==$id and .[0].Name==$name and .[0].Image==$image and
    .[0].Config.Labels["meguri.mg.run-id"]==$run and .[0].Config.Labels["meguri.sync.owner"]==$owner and
    .[0].State.Running==true and .[0].Config.User=="10001:10001" and
    .[0].HostConfig.ReadonlyRootfs==true and .[0].HostConfig.Privileged==false and
    ((.[0].HostConfig.CapAdd // [])==[]) and .[0].HostConfig.CapDrop==["ALL"] and
    (.[0].HostConfig.SecurityOpt==["no-new-privileges"] or .[0].HostConfig.SecurityOpt==["no-new-privileges:true"]) and
    ((.[0].HostConfig.Devices // [])==[])
  ' "$MG_NR_PRIVATE/$kind.json" >/dev/null || return 1
  if [ "$kind" = runtime ]; then
    jq -e --arg net "$MG_NR_NETWORK" --arg netid "$MG_NR_NETWORK_ID" --arg origin "$MG_NR_ORIGIN" \
      --arg volume "$(jq -r .volume "$MG_NR_PRIVATE/instance/instance.json")" '
      .[0] | .HostConfig.NetworkMode==$net and (.NetworkSettings.Networks | keys)==[$net] and
      .NetworkSettings.Networks[$net].NetworkID==$netid and
      (.HostConfig.PortBindings==null or .HostConfig.PortBindings=={}) and
      (.NetworkSettings.Ports==null or (.NetworkSettings.Ports | type)=="object") and
      ([.NetworkSettings.Ports // {} | to_entries[] | select(.value!=null)]==[]) and
      (.HostConfig.Binds==null or .HostConfig.Binds==[] or .HostConfig.Binds==[($volume+":/data:rw")]) and
      .Config.Entrypoint==["/usr/local/bin/meguri-sync-server"] and
      .Config.Cmd==["serve","/data/owner",$origin,"0.0.0.0:8080","--trusted-peer","127.0.0.1"] and
      (.Mounts | map(select(.Type!="tmpfs")) | length==1 and .[0].Type=="volume" and .[0].Name==$volume and .[0].Destination=="/data" and .[0].RW==true) and
      (.Mounts | all(.[]; (.Type=="volume" and .Destination=="/data") or (.Type=="tmpfs" and .Destination=="/tmp"))) and
      (.Mounts | map(select(.Type=="tmpfs")) | length<=1) and
      (.HostConfig.Tmpfs | keys)==["/tmp"]
    ' "$MG_NR_PRIVATE/runtime.json" >/dev/null || return 1
    mg_sync_package__volume_check
  elif [ "$kind" = relay ]; then
    local relay
    relay="$(mg_native_route__relay_source)" || return 1
    jq -e --arg backend "$MG_NR_NETWORK" --arg netid "$MG_NR_NETWORK_ID" \
      --arg ingress "$MG_NR_INGRESS" --arg ingressid "$MG_NR_INGRESS_ID" \
      --arg gateway "$MG_NR_GATEWAY" --arg port "$MG_NR_PORT" --arg target "$MG_NR_TARGET" \
      --arg runtime "$MG_NR_RUNTIME_ID" --arg relay "$relay" --arg phase "${MG_NR_PHASE:-native-route-fixture}" '
      .[0] | .HostConfig.NetworkMode==$ingress and .HostConfig.Init==true and
      (.HostConfig.Binds==null or .HostConfig.Binds==[]) and .Mounts==[] and
      (.HostConfig.Tmpfs==null or .HostConfig.Tmpfs=={}) and
      (.NetworkSettings.Networks | keys)==([$backend,$ingress] | sort) and
      .NetworkSettings.Networks[$backend].NetworkID==$netid and
      .NetworkSettings.Networks[$ingress].NetworkID==$ingressid and
      .NetworkSettings.Networks[$ingress].GwPriority==1 and
      .NetworkSettings.Networks[$backend].GwPriority==0 and
      .HostConfig.PortBindings=={"18443/tcp":[{HostIp:$gateway,HostPort:$port}]} and
      ([.NetworkSettings.Ports | to_entries[] | select(.value!=null)]==[{key:"18443/tcp",value:[{HostIp:$gateway,HostPort:$port}]}]) and
      .Config.Entrypoint==["node"] and .Config.Cmd==["-e",$relay,$target,$runtime,$netid,$phase]
    ' "$MG_NR_PRIVATE/relay.json" >/dev/null
  else
    jq -e --arg runtime "$MG_NR_RUNTIME_ID" --arg gateway "$MG_NR_GATEWAY" --arg origin "$MG_NR_ORIGIN" \
      --arg source "$MG_NR_COMMIT" --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" --arg tool "$MG_NR_TOOL_IMAGE" \
      --arg sha "$MG_NR_SCRIPT_SHA" --arg name "$MG_NR_FIXTURE" \
      --arg phase "${MG_NR_PHASE:-native-route-fixture}" \
      --arg volume "$(jq -er .volume "$MG_NR_PRIVATE/instance/instance.json")" '
      .[0] | .HostConfig.NetworkMode==("container:"+$runtime) and
      (if $phase=="native-owned-journey-fixture" then
        .HostConfig.Binds==[($volume+":/data:ro")] and
        (.HostConfig.Mounts==null or .HostConfig.Mounts==[])
       else (.HostConfig.Binds==null or .HostConfig.Binds==[]) and
        (.HostConfig.Mounts==null or .HostConfig.Mounts==[]) end) and
      (.HostConfig.PortBindings==null or .HostConfig.PortBindings=={}) and
      (.NetworkSettings.Ports==null or (.NetworkSettings.Ports | type)=="object") and
      ([.NetworkSettings.Ports // {} | to_entries[] | select(.value!=null)]==[]) and
      (if $phase=="native-owned-journey-fixture" then
        (.Mounts | map(select(.Type!="tmpfs")) | length==1 and .[0].Type=="volume" and
          .[0].Name==$volume and .[0].Destination=="/data" and .[0].RW==false) and
        (.Mounts | all(.[]; (.Type=="volume" and .Destination=="/data") or (.Type=="tmpfs" and .Destination=="/tmp")))
       else (.Mounts | all(.[]; .Type=="tmpfs" and .Destination=="/tmp")) end) and
      .HostConfig.Tmpfs=={"/tmp":"uid=10001,gid=10001,mode=0700,size=128m"} and
      .Config.Entrypoint==["node"] and
      .Config.Cmd==["/smoke/package-smoke.mjs",$phase,$gateway,$origin,$source,$run,$owner,$runtime,$tool,$sha,$name]
    ' "$MG_NR_PRIVATE/fixture.json" >/dev/null
  fi
}
mg_native_route__wait() {
  local id="${1:-$MG_NR_FIXTURE_ID}" marker=MEGURI_NATIVE_ROUTE_READY
  [ "$id" != "${MG_NR_RELAY_ID:-}" ] || marker=MEGURI_NATIVE_RELAY_READY
  python3 - "$id" "$marker" <<'PY'
import os, selectors, subprocess, sys, time
p = subprocess.Popen(['docker','logs','--follow',sys.argv[1]], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
s = selectors.DefaultSelector(); s.register(p.stdout, selectors.EVENT_READ)
buf = b''; deadline = time.monotonic() + 30; startup_failed = False
role = 'relay' if sys.argv[2] == 'MEGURI_NATIVE_RELAY_READY' else 'fixture'
try:
    while True:
        left = deadline - time.monotonic()
        if left <= 0 or not s.select(left): raise ValueError('fixture deadline')
        chunk = os.read(p.stdout.fileno(), 4096)
        if not chunk: raise ValueError('fixture ended')
        buf = (buf + chunk)[-32768:]
        if sys.argv[2].encode('ascii') in buf.splitlines(): break
except Exception:
    startup_failed = True
    raise SystemExit(f'native {role} startup failed') from None
finally:
    s.close()
    try:
        try: p.terminate()
        except ProcessLookupError: pass
        try:
            p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            # Escalate only this owned follower, then require a bounded reap.
            try: p.kill()
            except ProcessLookupError: pass
            p.wait(timeout=5)
    except Exception:
        # A marker failure stays primary, including when cleanup is uncertain.
        if not startup_failed:
            raise SystemExit(f'native {role} log follower cleanup failed') from None
PY
}
mg_native_route__exporter() {
  # Only this literal public CA may cross tmpfs. The manifest and leaf remain
  # private; all role/SAN/source/hash and parent bindings are rechecked here.
  cat <<'JS'
const fs = require('node:fs');
const { X509Certificate, createHash } = require('node:crypto');
try {
  if (process.argv.length !== 3 || process.argv[1] !== 'root-ca.pem') throw Error();
  const expected = JSON.parse(process.argv[2]);
  const read = path => {
    const fd = fs.openSync(path, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
    try {
      const s = fs.fstatSync(fd);
      if (!s.isFile() || s.nlink !== 1 || s.uid !== 10001 || s.size > 16384) throw Error();
      return fs.readFileSync(fd);
    } finally { fs.closeSync(fd); }
  };
  const dir = fs.lstatSync('/tmp/native-route');
  if (!dir.isDirectory() || dir.isSymbolicLink() || dir.uid !== 10001 || (dir.mode & 0o777) !== 0o700) throw Error();
  const binding = JSON.parse(read('/tmp/native-route/binding.json'));
  const root = read('/tmp/native-route/root-ca.pem');
  const ca = new X509Certificate(root); const leaf = new X509Certificate(read('/tmp/native-route/leaf.pem'));
  const hash = x => createHash('sha256').update(x).digest('hex');
  if (JSON.stringify(Object.keys(binding).sort()) !== JSON.stringify([...Object.keys(expected), 'caSha', 'caBytes'].sort())) throw Error();
  for (const [key, value] of Object.entries(expected)) if (binding[key] !== value) throw Error();
  if (binding.caSha !== hash(root) || binding.caBytes !== root.length || !ca.ca || leaf.ca ||
      !ca.verify(ca.publicKey) || !leaf.verify(ca.publicKey) ||
      leaf.subjectAltName !== `DNS:localhost, IP Address:127.0.0.1, IP Address:${expected.gateway}` ||
      leaf.checkIP(expected.gateway) !== expected.gateway || leaf.checkIP('127.0.0.1') !== '127.0.0.1' ||
      leaf.checkHost('localhost', { subject: 'never' }) !== 'localhost' ||
      hash(fs.readFileSync('/smoke/package-smoke.mjs')) !== expected.scriptSha) throw Error();
  for (const c of [ca, leaf]) {
    if (Date.parse(c.validFrom) > Date.now() || Date.parse(c.validTo) <= Date.now() ||
        Date.parse(c.validTo) - Date.parse(c.validFrom) !== 86400000) throw Error();
  }
  if (!/^-----BEGIN CERTIFICATE-----\n[A-Za-z0-9+/=\n]+-----END CERTIFICATE-----\n$/.test(root.toString('ascii'))) throw Error();
  process.stdout.write(root);
} catch { process.exitCode = 1; }
JS
}
mg_native_route__export() {
  mg_native_route__source && mg_native_route__container runtime && mg_native_route__container fixture && mg_native_route__network || return 1
  local binding exporter
  binding="$(jq -nc --arg gateway "$MG_NR_GATEWAY" --arg origin "$MG_NR_ORIGIN" --arg source "$MG_NR_COMMIT" \
    --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" --arg runtime "$MG_NR_RUNTIME_ID" --arg tool "$MG_NR_TOOL_IMAGE" \
    --arg scriptSha "$MG_NR_SCRIPT_SHA" --arg fixture "$MG_NR_FIXTURE" \
    '{gateway:$gateway,origin:$origin,source:$source,run:$run,owner:$owner,runtime:$runtime,tool:$tool,scriptSha:$scriptSha,fixture:$fixture}')" || return 1
  printf '%s\n' "$binding" >"$MG_NR_PRIVATE/ca-binding.json" || return 1
  exporter="$(mg_native_route__exporter)" || return 1
  docker exec --user 10001:10001 "$MG_NR_FIXTURE_ID" node -e "$exporter" root-ca.pem "$binding" \
    >"$MG_NR_PRIVATE/root-ca.pem" 2>"$MG_NR_PRIVATE/ca-export.err" || return 1
  MG_NR_CA_SHA="$(shasum -a 256 "$MG_NR_PRIVATE/root-ca.pem" | awk '{print $1}')" || return 1
  MG_NR_CA_BYTES="$(wc -c <"$MG_NR_PRIVATE/root-ca.pem" | tr -d ' ')" || return 1
  [[ "$MG_NR_CA_SHA" =~ ^[0-9a-f]{64}$ ]] && [ "$MG_NR_CA_BYTES" -gt 0 ] && [ "$MG_NR_CA_BYTES" -le 16384 ] || return 1
  mg_detail root_ca "$(jq -nc --arg sha "$MG_NR_CA_SHA" --argjson bytes "$MG_NR_CA_BYTES" \
    --arg source "$MG_NR_COMMIT" --arg runtime "$MG_NR_RUNTIME_ID" --arg fixture "$MG_NR_FIXTURE_ID" \
    --arg runtime_image "$MG_NR_RUNTIME_IMAGE" --arg tool "$MG_NR_TOOL_IMAGE" --arg script "$MG_NR_SCRIPT_SHA" \
    --arg run "$MG_RUN_ID" --arg nonce "$MG_NR_OWNER" \
    '{sha256:$sha,bytes:$bytes,source_commit:$source,runtime_id:$runtime,fixture_id:$fixture,runtime_image_id:$runtime_image,tool_image_id:$tool,fixture_source_sha256:$script,run:$run,nonce:$nonce}')"
  mg_native_route__source && mg_native_route__container fixture && mg_native_route__container runtime && mg_native_route__network
}
mg_native_route__guest_script() {
  # All substitutions are parent-derived, strictly validated numbers/hex or
  # shell-quoted owned paths. env -i also removes TLS and curl proxy overrides.
  cat <<EOF
#!/bin/sh
set -eu
umask 077
cd $(mg_mac_vm_q "$MG_NR_GUEST")
[ "\$(/usr/bin/stat -f '%d:%i' .)" = '$MG_NR_GUEST_ID' ]
trap 'result=\$?; if [ "\$result" != 0 ]; then for f in status body curl.err; do if [ -f "\$f" ] && [ ! -L "\$f" ]; then /bin/cat "\$f" >&2; fi; done; fi' EXIT
[ ! -L owner ] || exit 1
[ "\$(cat owner)" = $(mg_mac_vm_q "$MG_NR_OWNER") ] || exit 1
[ ! -L root-ca.pem ] || exit 1
[ "\$(/usr/bin/shasum -a 256 root-ca.pem | /usr/bin/awk '{print \$1}')" = '$MG_NR_CA_SHA' ]
[ "\$(/usr/bin/wc -c <root-ca.pem | /usr/bin/tr -d ' ')" = '$MG_NR_CA_BYTES' ]
rc=0
/usr/bin/env -i PATH=/usr/bin:/bin HOME=/nonexistent /usr/bin/curl --disable \\
  --proxy '' --noproxy '*' --cacert root-ca.pem --proto '=https' --proto-redir '=https' \\
  --connect-timeout 5 --max-time 15 --max-filesize 4096 --max-redirs 0 \\
  --output body --write-out '%{http_code}' '$MG_NR_ORIGIN/health' >status 2>curl.err || rc=\$?
[ "\$rc" = 0 ] || exit 1
[ "\$(cat status)" = 200 ] || exit 1
printf '%s' '{"data":{"collection_compatibility":false,"state":"account_substrate"}}' >expected
/usr/bin/cmp -s body expected
printf '%s\\n' MEGURI_NATIVE_ROUTE_OK
EOF
}
mg_native_route__guest() {
  local command identity
  MG_NR_GUEST_ATTEMPT=1
  command="umask 077; mkdir $(mg_mac_vm_q "$MG_NR_GUEST") && printf '%s\\n' $(mg_mac_vm_q "$MG_NR_OWNER") >$(mg_mac_vm_q "$MG_NR_GUEST/owner") && /usr/bin/stat -f '%d:%i' $(mg_mac_vm_q "$MG_NR_GUEST")"
  mg_mac_vm_ssh "$command" >"$MG_NR_PRIVATE/guest-identity" 2>"$MG_NR_PRIVATE/guest-stage.err" || return 1
  identity="$(cat "$MG_NR_PRIVATE/guest-identity")" || return 1
  [[ "$identity" =~ ^[0-9]+:[0-9]+$ ]] || return 1
  MG_NR_GUEST_ID="$identity"
  cp "$MG_NR_PRIVATE/root-ca.pem" "$MG_NR_STAGING/root-ca.pem" || return 1
  mg_native_route__guest_script >"$MG_NR_STAGING/health.sh" || return 1
  chmod 0600 "$MG_NR_PRIVATE/root-ca.pem" "$MG_NR_STAGING/root-ca.pem" "$MG_NR_STAGING/health.sh" || return 1
  # Existing copy helper, only two public/command files, never a directory or
  # account fixture. The destination was just created under the selected lease.
  local wrapper vm
  wrapper="$(jq -er .ssh <<<"$MG_MAC_VM")" && vm="$(jq -er .name <<<"$MG_MAC_VM")" || return 1
  mvl_copy_to "$wrapper" "$vm" "$MG_NR_STAGING/root-ca.pem" "$MG_NR_GUEST" &&
    mvl_copy_to "$wrapper" "$vm" "$MG_NR_STAGING/health.sh" "$MG_NR_GUEST" || return 1
  command="/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/perl -e 'alarm 25; exec @ARGV' /bin/sh $(mg_mac_vm_q "$MG_NR_GUEST/health.sh")"
  # The selected guest owns its directory. Pin both staged
  # bytes before this single invocation, including the command itself.
  local command_sha
  command_sha="$(shasum -a 256 "$MG_NR_STAGING/health.sh" | awk '{print $1}')" || return 1
  command="test \"\$(/usr/bin/shasum -a 256 $(mg_mac_vm_q "$MG_NR_GUEST/health.sh") | /usr/bin/awk '{print \$1}')\" = '$command_sha' && $command"
  mg_mac_vm_ssh "$command" >"$MG_NR_PRIVATE/guest-command.stdout" 2>"$MG_NR_PRIVATE/guest-command.stderr" || return 1
  printf '%s\n' MEGURI_NATIVE_ROUTE_OK >"$MG_NR_PRIVATE/guest-expected"
  cmp -s "$MG_NR_PRIVATE/guest-command.stdout" "$MG_NR_PRIVATE/guest-expected"
}
mg_native_route__remove_container() {
  local name="$1" expected="$2" owner="$3" image="$4" ids
  ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")" || return 1
  [ -n "$ids" ] || { [ -z "$expected" ] || return 1; return 0; }
  [[ "$ids" =~ ^[0-9a-f]{64}$ ]] && { [ -z "$expected" ] || [ "$ids" = "$expected" ]; } || return 1
  # If create reached the daemon but its reply was lost, only the exact
  # preallocated nonce/run/name plus immutable image may admit that ID.
  docker inspect "$ids" >"$MG_NR_PRIVATE/cleanup-container.json" || return 1
  jq -e --arg id "$ids" --arg name "/$name" --arg run "$MG_RUN_ID" --arg owner "$owner" --arg image "$image" '
    length==1 and .[0].Id==$id and .[0].Name==$name and .[0].Image==$image and
    .[0].Config.Labels["meguri.mg.run-id"]==$run and .[0].Config.Labels["meguri.sync.owner"]==$owner
  ' "$MG_NR_PRIVATE/cleanup-container.json" >/dev/null || return 1
  docker stop --time 10 "$ids" >"$MG_NR_PRIVATE/cleanup-stop" || return 1
  docker rm "$ids" >"$MG_NR_PRIVATE/cleanup-remove" || return 1
  ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")" || return 1
  [ -z "$ids" ]
}
mg_native_route__guest_cleanup() {
  [ "$MG_NR_GUEST_ATTEMPT" = 1 ] || return 0
  # No recursive deletion. A missing/changed marker or directory identity leaves
  # evidence and fails. Cancellation between mkdir and marker is uncertain.
  local q command
  q="$(mg_mac_vm_q "$MG_NR_GUEST")" || return 1
  command="if [ ! -e $q ] && [ ! -L $q ]; then exit 0; fi; test -d $q && test ! -L $q && test -n $(mg_mac_vm_q "$MG_NR_GUEST_ID") && test \"\$(/usr/bin/stat -f '%d:%i' $q)\" = $(mg_mac_vm_q "$MG_NR_GUEST_ID") && cd $q && test ! -L owner && test \"\$(cat owner)\" = $(mg_mac_vm_q "$MG_NR_OWNER") || exit 1;
for f in * .[!.]* ..?*; do [ -e \"\$f\" ] || [ -L \"\$f\" ] || continue; case \"\$f\" in owner|root-ca.pem|health.sh|body|status|curl.err|expected) test -f \"\$f\" && test ! -L \"\$f\" || exit 1;; *) exit 1;; esac; done;
for f in root-ca.pem health.sh body status curl.err expected; do if [ -e \"\$f\" ]; then /bin/rm \"\$f\" || exit 1; fi; done;
/bin/rm owner && cd / && /bin/rmdir $q && test ! -e $q && test ! -L $q"
  mg_mac_vm_ssh "$command" >"$MG_NR_PRIVATE/guest-cleanup.stdout" 2>"$MG_NR_PRIVATE/guest-cleanup.stderr"
}
mg_native_route__cleanup() {
  if [ "$MG_NR_CLEANED" = 1 ]; then [ "$MG_NR_CLEAN_OK" = 1 ]; return; fi
  MG_NR_CLEANED=1
  local rc=0 guest_rc=0 ids owner MG_SYNC_INSTANCE="$MG_NR_PRIVATE/instance"
  if [ "${MG_NR_PHASE:-}" = native-owned-journey-fixture ]; then
    mg_native_route__journey_unstage || guest_rc=1
  fi
  mg_native_route__guest_cleanup || guest_rc=1
  if [ "$MG_NR_RELAY_ATTEMPT" = 1 ]; then
    mg_native_route__remove_container "$MG_NR_RELAY" "$MG_NR_RELAY_ID" "$MG_NR_OWNER" "$MG_NR_TOOL_IMAGE" || rc=1
  fi
  if [ "$MG_NR_FIXTURE_ATTEMPT" = 1 ]; then
    mg_native_route__remove_container "$MG_NR_FIXTURE" "$MG_NR_FIXTURE_ID" "$MG_NR_OWNER" "$MG_NR_TOOL_IMAGE" || rc=1
  fi
  if [ "$MG_NR_RUNTIME_ATTEMPT" = 1 ]; then
    mg_native_route__remove_container "$MG_NR_RUNTIME" "$MG_NR_RUNTIME_ID" "$MG_NR_OWNER" "$MG_NR_RUNTIME_IMAGE" || rc=1
  fi
  if [ "$MG_NR_INIT_ATTEMPT" = 1 ] && [ -f "$MG_SYNC_INSTANCE/instance.json" ]; then
    owner="$(jq -er .owner "$MG_SYNC_INSTANCE/instance.json")" || rc=1
    if [ -n "$owner" ]; then
      mg_native_route__remove_container "meguri-init-$MG_RUN_ID" "" "$owner" "$MG_NR_RUNTIME_IMAGE" || rc=1
    fi
  fi
  # A surviving or unobserved container prevents network/volume destruction.
  local kind attempt expected name file
  for kind in ingress backend; do
    case "$kind" in
      ingress) attempt="$MG_NR_INGRESS_ATTEMPT"; expected="$MG_NR_INGRESS_ID"; name="$MG_NR_INGRESS"; file=ingress ;;
      backend) attempt="$MG_NR_NETWORK_ATTEMPT"; expected="$MG_NR_NETWORK_ID"; name="$MG_NR_NETWORK"; file=network ;;
    esac
    if [ "$rc" = 0 ] && [ "$attempt" = 1 ]; then
      ids="$(docker network ls -q --no-trunc --filter "name=^$name$")" || rc=1
      if [ -n "$ids" ]; then
        [[ "$ids" =~ ^[0-9a-f]{64}$ ]] && { [ -z "$expected" ] || [ "$ids" = "$expected" ]; } || rc=1
        if [ "$rc" = 0 ]; then
          case "$kind" in ingress) MG_NR_INGRESS_ID="$ids" ;; backend) MG_NR_NETWORK_ID="$ids" ;; esac
          mg_native_route__network "$kind" && jq -e '.[0].Containers=={}' "$MG_NR_PRIVATE/$file.json" >/dev/null || rc=1
        fi
        if [ "$rc" = 0 ]; then
          docker network rm "$ids" >"$MG_NR_PRIVATE/$file-remove" || rc=1
          ids="$(docker network ls -q --no-trunc --filter "name=^$name$")" || rc=1
          [ -z "$ids" ] || rc=1
        fi
      elif [ -n "$expected" ]; then rc=1; fi
    fi
  done
  if [ "$rc" = 0 ] && [ "$MG_NR_INIT_ATTEMPT" = 1 ] && [ -f "$MG_SYNC_INSTANCE/instance.json" ]; then
    # The existing volume guard checks both creation nonce and run before rm,
    # and observes absence afterward. No name-only or absent-inspect fallback.
    mg_sync_package__volume_remove "$MG_SYNC_INSTANCE" || rc=1
  fi
  [ "$guest_rc" = 0 ] || rc=1
  if [ "$rc" = 0 ]; then MG_NR_CLEAN_OK=1; else
    mg_native_route__failure
    mg_detail owned_cleanup false
  fi
  [ "$rc" = 0 ]
}
mg_native_route__execute() {
  # Both callers share this initializer. Cancellation at the first operation
  # must see a complete, fresh ownership plan, never another caller's ingress.
  MG_NR_CLEANED=0; MG_NR_CLEAN_OK=0; MG_NR_GUEST_ATTEMPT=0; MG_NR_GUEST_ID=
  MG_NR_NETWORK_ATTEMPT=0; MG_NR_INIT_ATTEMPT=0; MG_NR_RUNTIME_ATTEMPT=0; MG_NR_FIXTURE_ATTEMPT=0
  MG_NR_INGRESS_ATTEMPT=0; MG_NR_RELAY_ATTEMPT=0; MG_NR_HOST_STAGE_ATTEMPT=0; MG_NR_GUEST_STAGE_ATTEMPT=0
  MG_NR_NETWORK_ID=; MG_NR_INGRESS_ID=; MG_NR_RUNTIME_ID=; MG_NR_FIXTURE_ID=; MG_NR_RELAY_ID=; MG_NR_TARGET=
  MG_NR_CA_SHA=; MG_NR_CA_BYTES=; MG_NR_TRANSPORT_SHA=; MG_NR_CREDENTIALS_SHA=; MG_NR_UI_OK=0
  MG_NR_PRODUCER_SHA=
  MG_NR_NETWORK=; MG_NR_INGRESS=; MG_NR_RELAY=; MG_NR_GUEST=; MG_NR_RUNTIME=; MG_NR_FIXTURE=
  MG_NR_GATEWAY=; MG_NR_PORT=; MG_NR_ORIGIN=
  MG_NR_PHASE="${MG_NR_PHASE:-native-route-fixture}"
  case "$MG_NR_PHASE" in native-route-fixture|native-owned-journey-fixture) ;; *) return 1 ;; esac
  MG_NR_LOCKED_PHASE="$MG_NR_PHASE"
  [[ "$MG_NR_OWNER" =~ ^[0-9a-f]{32}$ ]] && [[ "$MG_RUN_ID" =~ ^[A-Za-z0-9_-]{1,100}$ ]] || return 1
  if [ "$MG_NR_PHASE" = native-owned-journey-fixture ]; then
    [[ "$MG_RUN_ID" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,95}$ ]] || return 1
  fi
  # Docker lowercases extended network attributes, including their name values.
  local network_run_id
  network_run_id="$(printf '%s' "$MG_RUN_ID" | LC_ALL=C tr 'A-Z' 'a-z')" || return 1
  MG_NR_NETWORK="meguri-native-net-$network_run_id-$MG_NR_OWNER"
  MG_NR_INGRESS="meguri-native-ingress-$network_run_id-$MG_NR_OWNER"
  MG_NR_RELAY="meguri-native-relay-$MG_RUN_ID-$MG_NR_OWNER"
  MG_NR_GUEST="/tmp/meguri-native-route-$MG_RUN_ID-$MG_NR_OWNER"
  MG_NR_RUNTIME="meguri-native-runtime-$MG_RUN_ID"; MG_NR_FIXTURE="meguri-native-fixture-$MG_RUN_ID"
  local MG_SYNC_INSTANCE="$MG_NR_PRIVATE/instance" MG_SYNC_CONTAINER_OWNER="$MG_NR_OWNER"
  local MEGURI_SYNC_ORIGIN MEGURI_SYNC_TRUSTED_PEER=127.0.0.1 MEGURI_SYNC_NETWORK
  local MEGURI_SYNC_PORT MG_SYNC_SETUP_FILE=/dev/null MG_SYNC_INSTANCE_SUFFIX=
  mg_native_route__stage route-metadata
  mg_native_route__route_metadata || return 1
  mg_native_route__stage port-selection
  MG_NR_PORT="$(mg_native_route__port)" || return 1
  [[ "$MG_NR_PORT" =~ ^[0-9]+$ ]] && [ "$MG_NR_PORT" -ge 1024 ] && [ "$MG_NR_PORT" -le 65535 ] || return 1
  MG_NR_ORIGIN="https://$MG_NR_GATEWAY:$MG_NR_PORT"
  MEGURI_SYNC_ORIGIN="$MG_NR_ORIGIN"; MEGURI_SYNC_PORT="$MG_NR_PORT"; MEGURI_SYNC_NETWORK="$MG_NR_NETWORK"
  mg_native_route__stage images-before
  mg_native_route__images before && mg_native_route__source || return 1
  mg_native_route__stage network-create
  local ids mounts=() volume
  ids="$(docker network ls -q --no-trunc --filter "name=^$MG_NR_NETWORK$")" || return 1
  [ -z "$ids" ] || return 1
  MG_NR_NETWORK_ATTEMPT=1
  docker network create --driver bridge --internal --label "meguri.mg.run-id=$MG_RUN_ID" \
    --label "meguri.sync.owner=$MG_NR_OWNER" "$MG_NR_NETWORK" >"$MG_NR_PRIVATE/network-id" || return 1
  MG_NR_NETWORK_ID="$(cat "$MG_NR_PRIVATE/network-id")" || return 1
  [[ "$MG_NR_NETWORK_ID" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__network &&
    jq -e '.[0].Containers=={}' "$MG_NR_PRIVATE/network.json" >/dev/null || return 1
  mg_native_route__stage ingress-create
  ids="$(docker network ls -q --no-trunc --filter "name=^$MG_NR_INGRESS$")" || return 1
  [ -z "$ids" ] || return 1
  MG_NR_INGRESS_ATTEMPT=1
  docker network create --driver bridge --label "meguri.mg.run-id=$MG_RUN_ID" \
    --label "meguri.sync.owner=$MG_NR_OWNER" "$MG_NR_INGRESS" >"$MG_NR_PRIVATE/ingress-id" || return 1
  MG_NR_INGRESS_ID="$(cat "$MG_NR_PRIVATE/ingress-id")" || return 1
  [[ "$MG_NR_INGRESS_ID" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__members || return 1
  mg_native_route__stage volume-init
  MG_NR_INIT_ATTEMPT=1
  mg_sync_package__init "$MG_NR_RECEIPT" && mg_sync_package__volume_check && mg_native_route__source || return 1
  mg_native_route__stage runtime-start
  MG_NR_RUNTIME_ATTEMPT=1
  # Sibling of package start(): no default HTTP/service-port publication, no
  # health request or package-ready HTTP polling before the countable request.
  mg_sync_package__compose run -d --name "$MG_NR_RUNTIME" --no-deps --pull never \
    --label "meguri.mg.run-id=$MG_RUN_ID" local-sync \
    >"$MG_NR_PRIVATE/runtime-id" || return 1
  MG_NR_RUNTIME_ID="$(cat "$MG_NR_PRIVATE/runtime-id")" || return 1
  [[ "$MG_NR_RUNTIME_ID" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__container runtime || return 1
  # A listening log event is not a health request. Use the existing bounded
  # runtime waiter; the fixture has its own single fixed-marker follower.
  mg_sync_package__wait "$MG_NR_RUNTIME_ID" || return 1
  mg_native_route__stage fixture-start
  if [ "$MG_NR_PHASE" = native-owned-journey-fixture ]; then
    volume="$(jq -er .volume "$MG_SYNC_INSTANCE/instance.json")" || return 1
    mg_sync_package__volume_check || return 1
    mounts=(--volume "$volume:/data:ro")
  fi
  MG_NR_FIXTURE_ATTEMPT=1
  docker run -d --name "$MG_NR_FIXTURE" --init --network "container:$MG_NR_RUNTIME_ID" \
    --user 10001:10001 --read-only --cap-drop ALL --security-opt no-new-privileges \
    --tmpfs /tmp:uid=10001,gid=10001,mode=0700,size=128m \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$MG_NR_OWNER" \
    ${mounts[0]+"${mounts[@]}"} \
    --entrypoint node "$MG_NR_TOOL_IMAGE" /smoke/package-smoke.mjs "$MG_NR_PHASE" \
    "$MG_NR_GATEWAY" "$MG_NR_ORIGIN" "$MG_NR_COMMIT" "$MG_RUN_ID" "$MG_NR_OWNER" "$MG_NR_RUNTIME_ID" \
    "$MG_NR_TOOL_IMAGE" "$MG_NR_SCRIPT_SHA" "$MG_NR_FIXTURE" >"$MG_NR_PRIVATE/fixture-id" || return 1
  MG_NR_FIXTURE_ID="$(cat "$MG_NR_PRIVATE/fixture-id")" || return 1
  [[ "$MG_NR_FIXTURE_ID" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__container fixture || return 1
  mg_native_route__stage fixture-marker
  mg_native_route__wait || return 1
  mg_native_route__stage ca-export
  mg_native_route__export || return 1
  mg_native_route__stage relay-target
  mg_native_route__target || return 1
  mg_native_route__stage relay-start
  local relay
  relay="$(mg_native_route__relay_source)" || return 1
  ids="$(docker container ls -aq --no-trunc --filter "name=^/$MG_NR_RELAY$")" || return 1
  [ -z "$ids" ] || return 1
  MG_NR_RELAY_ATTEMPT=1
  docker run -d --name "$MG_NR_RELAY" --init --pull never \
    --network "name=$MG_NR_INGRESS,gw-priority=1" --network "name=$MG_NR_NETWORK,gw-priority=0" \
    --user 10001:10001 --read-only --cap-drop ALL --security-opt no-new-privileges \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$MG_NR_OWNER" \
    --publish "$MG_NR_GATEWAY:$MG_NR_PORT:18443/tcp" --entrypoint node "$MG_NR_TOOL_IMAGE" \
    -e "$relay" "$MG_NR_TARGET" "$MG_NR_RUNTIME_ID" "$MG_NR_NETWORK_ID" "$MG_NR_PHASE" >"$MG_NR_PRIVATE/relay-id" || return 1
  MG_NR_RELAY_ID="$(cat "$MG_NR_PRIVATE/relay-id")" || return 1
  [[ "$MG_NR_RELAY_ID" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__container relay && mg_native_route__target || return 1
  mg_native_route__stage relay-marker
  mg_native_route__wait "$MG_NR_RELAY_ID" || return 1
  if [ "$MG_NR_PHASE" = native-owned-journey-fixture ]; then
    mg_native_route__stage journey-input
    mg_native_route__journey_stage || return 1
    mg_native_route__stage journey-ui
    mg_native_route__locked && mg_ui_test__owned_run && mg_native_route__locked || return 1
  else
    mg_native_route__stage guest-health
    mg_native_route__target && mg_native_route__container relay && mg_native_route__container fixture &&
      mg_native_route__images request && mg_native_route__source && mg_native_route__guest || return 1
  fi
  mg_native_route__stage identities-after
  mg_native_route__images after && mg_native_route__target && mg_native_route__container relay &&
    mg_native_route__container fixture && mg_native_route__source || return 1
  [ "$(shasum -a 256 "$MG_NR_PRIVATE/root-ca.pem" | awk '{print $1}')" = "$MG_NR_CA_SHA" ] || return 1
  # Re-export to a distinct private file and compare. This rechecks internal
  # CA/leaf/source bindings after the request; it performs no network request.
  local binding exporter
  binding="$(cat "$MG_NR_PRIVATE/ca-binding.json")"; exporter="$(mg_native_route__exporter)" || return 1
  docker exec --user 10001:10001 "$MG_NR_FIXTURE_ID" node -e "$exporter" root-ca.pem "$binding" \
    >"$MG_NR_PRIVATE/root-ca-after.pem" 2>"$MG_NR_PRIVATE/ca-after.err" || return 1
  cmp -s "$MG_NR_PRIVATE/root-ca.pem" "$MG_NR_PRIVATE/root-ca-after.pem" || return 1
}
# Exact two-file handoff, not an alternate VM/test lane. The private receipts
# mark exclusive directory identities with this route's nonce. Partial or
# substituted stages are retained on uncertainty, never recursively removed.
mg_native_route__journey_file_script() {
  cat <<'PY'
import base64, hashlib, json, os, re, ssl, stat, sys
mode, root, run, owner = sys.argv[1:5]
assert re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,95}', run)
assert re.fullmatch(r'[a-f0-9]{32}', owner)
flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
fds = []
def directory(parent, name, create=False, exclusive=False):
    if create:
        try: os.mkdir(name, 0o700, dir_fd=parent)
        except FileExistsError:
            if exclusive: raise
    fd = os.open(name, flags | os.O_DIRECTORY, dir_fd=parent); fds.append(fd)
    s = os.fstat(fd)
    assert stat.S_ISDIR(s.st_mode) and s.st_uid == os.geteuid() and not s.st_mode & 0o022
    return fd
def read(parent, name, cap, private=False):
    fd = os.open(name, flags, dir_fd=parent)
    try:
        before = os.fstat(fd)
        assert stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and 0 < before.st_size <= cap
        if private: assert before.st_uid == os.geteuid() and stat.S_IMODE(before.st_mode) == 0o600
        chunks = []; left = before.st_size
        while left:
            chunk = os.read(fd, min(left, 65536)); assert chunk
            chunks.append(chunk); left -= len(chunk)
        assert not os.read(fd, 1)
        def binding(s):
            return (s.st_dev, s.st_ino, s.st_uid, s.st_gid, s.st_mode, s.st_nlink,
                s.st_size, s.st_mtime_ns, s.st_ctime_ns)
        assert binding(before) == binding(os.fstat(fd)) == binding(os.stat(name, dir_fd=parent, follow_symlinks=False))
        return b''.join(chunks)
    finally: os.close(fd)
def write(parent, name, raw):
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
    try:
        with os.fdopen(fd, 'wb', closefd=False) as f: f.write(raw); f.flush(); os.fsync(fd)
    finally: os.close(fd)
def encoded(value): return (json.dumps(value, separators=(',', ':')) + '\n').encode()
def sha(raw): return hashlib.sha256(raw).hexdigest()
try:
    checkout = directory(None, root)
    build = directory(checkout, 'build')
    if mode == 'output':
        parent = build
        for name in ('mg', 'ui-test', run, 'private'):
            parent = directory(parent, name, create=True)
        parent = directory(parent, 'native-route', create=True, exclusive=True)
        target = directory(parent, 'ui', create=True, exclusive=True)
        name_in_parent = 'ui'
    else:
        fixtures = directory(build, 'fixtures')
        parent = directory(fixtures, 'owned-meguri-provider', create=mode in ('stage', 'reserve'))
        target = directory(parent, run, create=mode in ('stage', 'reserve'), exclusive=True)
        name_in_parent = run
    s = os.fstat(target); identity = f'{s.st_dev}:{s.st_ino}'
    assert stat.S_IMODE(s.st_mode) == 0o700
    receipt = dict(run_id=run, owner=owner, identity=identity)
    if mode == 'stage':
        private, executable = sys.argv[5:7]
        data = directory(None, private)
        producer = json.loads(read(data, 'producer.json', 4096, True))
        assert set(producer) == {'run_id', 'username', 'password', 'account_id'} and producer['run_id'] == run
        assert re.fullmatch(r'owned\.[a-f0-9]{24}', producer['username'])
        assert re.fullmatch(r'[A-Za-z0-9_-]{43}', producer['password'])
        binding = json.loads(read(data, 'ca-binding.json', 16384, True))
        assert binding['run'] == run and binding['owner'] == owner
        der = ssl.PEM_cert_to_DER_cert(read(data, 'root-ca.pem', 16384, True).decode('ascii'))
        binaries = directory(None, os.path.dirname(executable)); name = os.path.basename(executable)
        manifest = encoded(dict(format=1, purpose='meguri-owned-provider-transport', run_id=run,
            origin=binding['origin'], ca_der_base64=base64.b64encode(der).decode('ascii'), ca_der_sha256=sha(der),
            app_executable_sha256=sha(read(binaries, name, 1024**3)),
            app_debug_dylib_sha256=sha(read(binaries, name + '.debug.dylib', 1024**3))))
        credentials = encoded(dict(run_id=run, transport_manifest_sha256=sha(manifest),
            username=producer['username'], password=producer['password']))
        write(target, 'transport.json', manifest); write(target, 'credentials.json', credentials)
        receipt.update(transport_sha256=sha(manifest), credentials_sha256=sha(credentials))
    elif mode in ('check', 'remove'):
        expected, manifest_sha, credentials_sha = sys.argv[5:8]
        assert identity == expected and set(os.listdir(target)) == {'transport.json', 'credentials.json'}
        manifest = read(target, 'transport.json', 131072, True)
        credentials = read(target, 'credentials.json', 4096, True)
        assert sha(manifest) == manifest_sha and sha(credentials) == credentials_sha
        m = json.loads(manifest); c = json.loads(credentials)
        assert m['run_id'] == c['run_id'] == run and c['transport_manifest_sha256'] == manifest_sha
        assert set(c) == {'run_id', 'transport_manifest_sha256', 'username', 'password'}
        assert all(isinstance(v, str) for v in c.values())
    else: assert mode in ('reserve', 'output')
    named = os.stat(name_in_parent, dir_fd=parent, follow_symlinks=False)
    assert (named.st_dev, named.st_ino, named.st_uid, named.st_mode) == (s.st_dev, s.st_ino, s.st_uid, s.st_mode)
    if mode == 'remove':
        for name in ('credentials.json', 'transport.json'): os.unlink(name, dir_fd=target)
        os.rmdir(run, dir_fd=parent)
        try: os.stat(run, dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError: pass
        else: raise ValueError()
    elif mode in ('stage', 'reserve', 'output'): print(json.dumps(receipt, separators=(',', ':')))
except Exception:
    raise SystemExit(1) from None
finally:
    for fd in reversed(fds): os.close(fd)
PY
}

mg_native_route__locked() {
  local MG_SYNC_INSTANCE="$MG_NR_PRIVATE/instance"
  [ "$MG_NR_CLEANED" = 0 ] && [ "$MG_NR_PHASE" = "$MG_NR_LOCKED_PHASE" ] &&
    mg_native_route__images locked && mg_native_route__source && mg_native_route__target &&
    mg_native_route__container relay && mg_native_route__container fixture
}

mg_native_route__journey_producer() {
  # Only a bounded nofollow image-owned record may cross this private pipe.
  # The same exporter is used at staging and on both sides of server readback.
  cat <<'JS'
const fs=require('node:fs');
try {
  if(process.argv.length!==2) throw Error();
  const dir=fs.lstatSync('/tmp/native-route');
  if(!dir.isDirectory()||dir.isSymbolicLink()||dir.uid!==10001||(dir.mode&0o7777)!==0o700) throw Error();
  const path='/tmp/native-route/journey.json';
  const fd=fs.openSync(path,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW|fs.constants.O_NONBLOCK);
  let raw;
  try {
    const before=fs.fstatSync(fd,{bigint:true});
    if(!before.isFile()||before.uid!==10001n||before.nlink!==1n||(before.mode&0o7777n)!==0o600n||before.size<1n||before.size>4096n) throw Error();
    raw=Buffer.alloc(Number(before.size)); let offset=0;
    while(offset<raw.length) { const n=fs.readSync(fd,raw,offset,raw.length-offset,null); if(!n) throw Error(); offset+=n; }
    if(fs.readSync(fd,Buffer.alloc(1),0,1,null)!==0) throw Error();
    const identity=x=>JSON.stringify([x.dev,x.ino,x.uid,x.gid,x.mode,x.nlink,x.size,x.mtimeNs,x.ctimeNs],(_,v)=>typeof v==='bigint'?v.toString():v);
    if(identity(before)!==identity(fs.fstatSync(fd,{bigint:true}))||identity(before)!==identity(fs.lstatSync(path,{bigint:true}))) throw Error();
    const v=JSON.parse(raw);
    if(v.run_id!==process.argv[1]||JSON.stringify(Object.keys(v).sort())!==JSON.stringify(['account_id','password','run_id','username'])||
      !Object.values(v).every(x=>typeof x==='string'&&x.length>0)||!/^owned\.[a-f0-9]{24}$/.test(v.username)||
      !/^[A-Za-z0-9_-]{43}$/.test(v.password)||!/^[A-Za-z0-9_-]{43}$/.test(v.account_id)) throw Error();
  } finally { fs.closeSync(fd); }
  const named=fs.lstatSync('/tmp/native-route');
  if(named.dev!==dir.dev||named.ino!==dir.ino||named.mode!==dir.mode||named.uid!==dir.uid) throw Error();
  process.stdout.write(raw);
} catch { process.exitCode=1; }
JS
}

mg_native_route__journey_stage() {
  local script producer path identity MG_VM_FIXTURE
  [ "$MG_NR_PHASE" = native-owned-journey-fixture ] && mg_native_route__locked || return 1
  producer="$(mg_native_route__journey_producer)" || return 1
  (umask 077; set -C
    docker exec --user 10001:10001 "$MG_NR_FIXTURE_ID" node -e "$producer" "$MG_RUN_ID" \
      >"$MG_NR_PRIVATE/producer.json" 2>"$MG_NR_PRIVATE/producer.err"
  ) || return 1
  MG_NR_PRODUCER_SHA="$(shasum -a 256 "$MG_NR_PRIVATE/producer.json" | awk '{print $1}')" || return 1
  script="$(mg_native_route__journey_file_script)" || return 1
  MG_NR_HOST_STAGE_ATTEMPT=1
  python3 -B -I - stage "$MG_ROOT" "$MG_RUN_ID" "$MG_NR_OWNER" "$MG_NR_PRIVATE" \
    "$(mg_app_products mac Debug)/Meguri.app/Contents/MacOS/Meguri" <<<"$script" \
    >"$MG_NR_PRIVATE/stage-host.json" || return 1
  MG_NR_TRANSPORT_SHA="$(jq -er .transport_sha256 "$MG_NR_PRIVATE/stage-host.json")" &&
    MG_NR_CREDENTIALS_SHA="$(jq -er .credentials_sha256 "$MG_NR_PRIVATE/stage-host.json")" || return 1
  MG_NR_GUEST_STAGE_ATTEMPT=1
  mg_mac_vm_ssh_in "python3 -B -I - $(mg_mac_vm_q reserve "$MG_ROOT" "$MG_RUN_ID" "$MG_NR_OWNER")" <<<"$script" \
    >"$MG_NR_PRIVATE/stage-guest.json" || return 1
  # The copy helper sees only these two absent destinations inside the exclusive
  # guest directory. The external nonce/identity receipts mark our run for removal.
  for path in transport.json credentials.json; do
    mg_vm_fixture "$MG_ROOT/build/fixtures/owned-meguri-provider/$MG_RUN_ID/$path" || return 1
  done
  identity="$(jq -er .identity "$MG_NR_PRIVATE/stage-guest.json")" || return 1
  mg_mac_vm_ssh_in "python3 -B -I - $(mg_mac_vm_q check "$MG_ROOT" "$MG_RUN_ID" "$MG_NR_OWNER" "$identity" "$MG_NR_TRANSPORT_SHA" "$MG_NR_CREDENTIALS_SHA")" \
    <<<"$script" || return 1
  identity="$(jq -er .identity "$MG_NR_PRIVATE/stage-host.json")" || return 1
  python3 -B -I - check "$MG_ROOT" "$MG_RUN_ID" "$MG_NR_OWNER" "$identity" "$MG_NR_TRANSPORT_SHA" "$MG_NR_CREDENTIALS_SHA" \
    <<<"$script" && mg_native_route__locked
}

mg_native_route__journey_unstage() {
  local script identity rc=0
  script="$(mg_native_route__journey_file_script)" || return 1
  if [ "${MG_NR_GUEST_STAGE_ATTEMPT:-}" = 1 ]; then
    identity="$(jq -er --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" \
      'select(.run_id==$run and .owner==$owner) | .identity' "$MG_NR_PRIVATE/stage-guest.json")" || return 1
    mg_mac_vm_ssh_in "python3 -B -I - $(mg_mac_vm_q remove "$MG_ROOT" "$MG_RUN_ID" "$MG_NR_OWNER" "$identity" "$MG_NR_TRANSPORT_SHA" "$MG_NR_CREDENTIALS_SHA")" \
      <<<"$script" || rc=1
  fi
  if [ "${MG_NR_HOST_STAGE_ATTEMPT:-}" = 1 ]; then
    identity="$(jq -er --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" \
      'select(.run_id==$run and .owner==$owner) | .identity' "$MG_NR_PRIVATE/stage-host.json")" || return 1
    python3 -B -I - remove "$MG_ROOT" "$MG_RUN_ID" "$MG_NR_OWNER" "$identity" "$MG_NR_TRANSPORT_SHA" "$MG_NR_CREDENTIALS_SHA" \
      <<<"$script" || rc=1
  fi
  return "$rc"
}

mg_native_route__journey_verify_review() {
  # Called only by the reviewed UI consumer, after its exact confirmed pass and
  # before its first teardown. This helper cannot turn a failed UI into success.
  [ "$#" = 1 ] && [ "${MG_NR_PHASE:-}" = native-owned-journey-fixture ] &&
    [ "${MG_NR_UI_OK:-}" = 1 ] && [ "${MG_NR_CLEANED:-}" = 0 ] &&
    [ "$1" = "$MG_NR_PRIVATE/review-expected.json" ] &&
    [[ "${MG_NR_PRODUCER_SHA:-}" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__locked || return 1
  local producer
  producer="$(mg_native_route__journey_producer)" || return 1
  (umask 077; set -C
    docker exec --user 10001:10001 "$MG_NR_FIXTURE_ID" node -e "$producer" "$MG_RUN_ID" \
      >"$MG_NR_PRIVATE/producer-review-before.json" 2>"$MG_NR_PRIVATE/producer-review-before.err"
  ) || return 1
  mg_native_route__locked || return 1
  # Open the actual caller file by dirfd, pin it across exec and feed that same
  # descriptor to stdin. Never pass IDs, accounts or credentials in argv/env.
  python3 -B -I - "$MG_NR_PRIVATE" "$1" "$MG_NR_PRODUCER_SHA" "$MG_NR_FIXTURE_ID" <<'PY' || return 1
import hashlib, json, os, stat, subprocess, sys
root, expected, producer_sha, fixture = sys.argv[1:]
flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
fds = []
def binding(s):
    return (s.st_dev, s.st_ino, s.st_uid, s.st_gid, s.st_mode, s.st_nlink, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
def opened(parent, name, cap):
    fd = os.open(name, flags, dir_fd=parent); fds.append(fd)
    s = os.fstat(fd)
    assert stat.S_ISREG(s.st_mode) and s.st_uid == os.geteuid() and stat.S_IMODE(s.st_mode) == 0o600
    assert s.st_nlink == 1 and 0 < s.st_size <= cap
    raw = os.read(fd, cap + 1)
    assert len(raw) == s.st_size and not os.read(fd, 1)
    assert binding(s) == binding(os.fstat(fd)) == binding(os.stat(name, dir_fd=parent, follow_symlinks=False))
    return fd, s, raw
try:
    assert os.path.isabs(root) and os.path.normpath(root) == root and expected == root + '/review-expected.json'
    # No symlink in the private root's ancestry, including the final directory.
    parent = os.open('/', flags | os.O_DIRECTORY); fds.append(parent)
    for name in root.split('/')[1:]:
        parent = os.open(name, flags | os.O_DIRECTORY, dir_fd=parent); fds.append(parent)
    directory = os.fstat(parent)
    assert directory.st_uid == os.geteuid() and stat.S_IMODE(directory.st_mode) == 0o700
    _, _, original = opened(parent, 'producer.json', 4096)
    _, _, exported = opened(parent, 'producer-review-before.json', 4096)
    assert hashlib.sha256(original).hexdigest() == producer_sha and exported == original
    fd, before, raw = opened(parent, 'review-expected.json', 256)
    def unique(pairs):
        value = dict(pairs); assert len(value) == len(pairs); return value
    ids = json.loads(raw, object_pairs_hook=unique)
    assert set(ids) == {'card_id', 'note_id'}
    assert all(type(v) is int and 0 < v <= 9223372036854775807 for v in ids.values())
    os.lseek(fd, 0, os.SEEK_SET)
    outputs = []
    for name in ('review-readback.stdout', 'review-readback.stderr'):
        out = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
        fds.append(out); outputs.append(out)
    result = subprocess.run(['docker', 'exec', '-i', '--user', '10001:10001', fixture,
        '/smoke/native', 'owned-review-readback'], stdin=fd, stdout=outputs[0], stderr=outputs[1], timeout=90)
    assert result.returncode == 0
    assert binding(before) == binding(os.fstat(fd)) == binding(os.stat('review-expected.json', dir_fd=parent, follow_symlinks=False))
    os.lseek(fd, 0, os.SEEK_SET); assert os.read(fd, 257) == raw
    _, _, stdout = opened(parent, 'review-readback.stdout', 4096)
    assert stdout == b'owned-review-verified\n'
    _, _, retained = opened(parent, 'producer.json', 4096)
    assert retained == original
    named = os.stat(root, follow_symlinks=False)
    assert (named.st_dev, named.st_ino, named.st_uid, named.st_mode) == (directory.st_dev, directory.st_ino, directory.st_uid, directory.st_mode)
except Exception:
    raise SystemExit(1) from None
finally:
    for fd in reversed(fds): os.close(fd)
PY
  mg_native_route__locked || return 1
  (umask 077; set -C
    docker exec --user 10001:10001 "$MG_NR_FIXTURE_ID" node -e "$producer" "$MG_RUN_ID" \
      >"$MG_NR_PRIVATE/producer-review-after.json" 2>"$MG_NR_PRIVATE/producer-review-after.err"
  ) || return 1
  # The re-export has the exact original run/account bytes on both sides.
  python3 -B -I - "$MG_NR_PRIVATE" "$MG_NR_PRODUCER_SHA" <<'PY' || return 1
import hashlib, os, stat, sys
root, expected = sys.argv[1:]
flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
fds = []
try:
    assert os.path.isabs(root) and os.path.normpath(root) == root
    parent = os.open('/', flags | os.O_DIRECTORY); fds.append(parent)
    for name in root.split('/')[1:]:
        parent = os.open(name, flags | os.O_DIRECTORY, dir_fd=parent); fds.append(parent)
    directory = os.fstat(parent)
    assert directory.st_uid == os.geteuid() and stat.S_IMODE(directory.st_mode) == 0o700
    def binding(s):
        return (s.st_dev, s.st_ino, s.st_uid, s.st_gid, s.st_mode, s.st_nlink, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    values = []
    for name in ('producer.json', 'producer-review-before.json', 'producer-review-after.json'):
        fd = os.open(name, flags, dir_fd=parent); fds.append(fd)
        before = os.fstat(fd)
        assert stat.S_ISREG(before.st_mode) and before.st_uid == os.geteuid() and stat.S_IMODE(before.st_mode) == 0o600
        assert before.st_nlink == 1 and 0 < before.st_size <= 4096
        raw = os.read(fd, 4097)
        assert len(raw) == before.st_size and not os.read(fd, 1)
        assert binding(before) == binding(os.fstat(fd)) == binding(os.stat(name, dir_fd=parent, follow_symlinks=False))
        assert hashlib.sha256(raw).hexdigest() == expected
        values.append(raw)
    assert values[0] == values[1] == values[2]
    named = os.stat(root, follow_symlinks=False)
    assert (named.st_dev, named.st_ino, named.st_uid, named.st_mode) == (directory.st_dev, directory.st_ino, directory.st_uid, directory.st_mode)
except Exception:
    raise SystemExit(1) from None
finally:
    for fd in reversed(fds): os.close(fd)
PY
  mg_native_route__locked
}

mg_meguri_native_route() {
  # Exactly one required option, once. Never resolve a latest link or build.
  if [ "$#" -ne 2 ] || [ "$1" != --package-receipt ] || [ -z "$2" ] || [[ "$2" = --* ]]; then
    mg_failure usage "meguri-native-route --package-receipt <exact-result.json>"; mg_status error; return
  fi
  mg_native_route__ambient || { mg_failure native-route-ambient "Unexpected task-specific ambient authority"; mg_status error; return; }
  MG_NR_PHASE=native-route-fixture
  MG_NR_RECEIPT="$2"; MG_NR_STAGE=source-before; MG_NR_FIRST=
  MG_NR_CLEANED=0; MG_NR_CLEAN_OK=0; MG_NR_GUEST_ATTEMPT=0; MG_NR_GUEST_ID=
  MG_NR_NETWORK_ATTEMPT=0; MG_NR_INIT_ATTEMPT=0; MG_NR_RUNTIME_ATTEMPT=0; MG_NR_FIXTURE_ATTEMPT=0
  MG_NR_NETWORK_ID=; MG_NR_RUNTIME_ID=; MG_NR_FIXTURE_ID=; MG_NR_CA_SHA=; MG_NR_CA_BYTES=
  MG_NR_INGRESS_ATTEMPT=0; MG_NR_RELAY_ATTEMPT=0; MG_NR_INGRESS_ID=; MG_NR_RELAY_ID=; MG_NR_TARGET=
  MG_NR_PRIVATE="$MG_RUN_DIR/private/native-route"; MG_NR_STAGING="$MG_NR_PRIVATE/guest"
  MG_NR_COMMIT="$(mg_meguri_provider__commit)" || { mg_native_route__failure; return; }
  MG_NR_SCRIPT_SHA="$(shasum -a 256 "$MG_ROOT/server/tools/package-smoke.mjs" | awk '{print $1}')" || { mg_native_route__failure; return; }
  [[ "$MG_NR_COMMIT" =~ ^[0-9a-f]{40}$ ]] && [[ "$MG_NR_SCRIPT_SHA" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__source || { mg_native_route__failure; return; }
  mg_native_route__stage package-receipt
  mg_sync_package__receipt "$MG_NR_RECEIPT" || { mg_native_route__failure; return; }
  MG_NR_RUNTIME_IMAGE="$(jq -er .details.package.image_id "$MG_NR_RECEIPT")" &&
    MG_NR_TOOL_IMAGE="$(jq -er .details.package.client_image_id "$MG_NR_RECEIPT")" || { mg_native_route__failure; return; }
  mg_native_route__stage vm-selection
  local rc=0
  mg_mac_vm_probe || rc=$?
  case "$rc" in
    0) ;;
    1) mg__mac_vm_queued "Route diagnostic requires an existing test VM" "the route diagnostic"; return ;;
    *) mg__mac_vm_config_error; return ;;
  esac
  mg_mac_vm_take --no-copy --running-only || rc=$?
  case "$rc" in 0) ;; 2) return ;; *) mg_native_route__failure; return ;; esac
  mg_native_route__stage ownership-plan
  MG_NR_OWNER="$(mg_meguri_provider__nonce)" || { mg_native_route__failure; return; }
  [[ "$MG_NR_OWNER" =~ ^[0-9a-f]{32}$ ]] && [[ "$MG_RUN_ID" =~ ^[A-Za-z0-9_-]{1,100}$ ]] || { mg_native_route__failure; return; }
  MG_NR_NETWORK="meguri-native-net-$MG_RUN_ID-$MG_NR_OWNER"
  MG_NR_INGRESS="meguri-native-ingress-$MG_RUN_ID-$MG_NR_OWNER"
  MG_NR_RELAY="meguri-native-relay-$MG_RUN_ID-$MG_NR_OWNER"
  MG_NR_GUEST="/tmp/meguri-native-route-$MG_RUN_ID-$MG_NR_OWNER"
  MG_NR_RUNTIME="meguri-native-runtime-$MG_RUN_ID"; MG_NR_FIXTURE="meguri-native-fixture-$MG_RUN_ID"
  (umask 077; mkdir -p "$MG_RUN_DIR/private" && mkdir "$MG_NR_PRIVATE" && mkdir "$MG_NR_STAGING") || { mg_native_route__failure; return; }
  # Registered in this shell, after the lease and before any owned creation.
  # Globals persist for MG's cancellation/exit handler; no subshell trap.
  mg_on_exit mg_native_route__cleanup
  mg_detail coverage '"One leased-guest explicit-CA route/TLS/health diagnostic only; no app/T12, UI, sign-in, account, bootstrap, review, Mobile, compatibility or gate evidence"'
  if ! mg_native_route__execute >"$MG_NR_PRIVATE/control.log" 2>&1; then mg_native_route__failure; fi
  mg_native_route__stage owned-cleanup
  mg_native_route__cleanup >"$MG_NR_PRIVATE/cleanup.log" 2>&1 || mg_native_route__failure
  mg_native_route__stage source-after
  mg_native_route__source || mg_native_route__failure
  if [ -n "$MG_NR_FIRST" ]; then
    mg_count total 1; mg_count passed 0; mg_count failed 1; mg_count skipped 0; mg_status fail; return
  fi
  # Public evidence contains only safe classifications and immutable bindings.
  # Raw gateway/interface/port, inspect/SSH/curl output and staged CA stay private.
  mg_detail route "$(jq -nc --arg source "$MG_NR_COMMIT" --arg runtime "$MG_NR_RUNTIME_ID" --arg fixture "$MG_NR_FIXTURE_ID" \
    --arg runtime_image "$MG_NR_RUNTIME_IMAGE" --arg tool_image "$MG_NR_TOOL_IMAGE" --arg network "$MG_NR_NETWORK_ID" \
    --arg owner "$MG_NR_OWNER" --arg run "$MG_RUN_ID" --arg sha "$MG_NR_CA_SHA" --argjson bytes "$MG_NR_CA_BYTES" \
    --arg relay "$MG_NR_RELAY_ID" --arg ingress "$MG_NR_INGRESS_ID" \
    '{relay_id:$relay,ingress_network_id:$ingress,source_commit:$source,runtime_id:$runtime,fixture_id:$fixture,runtime_image_id:$runtime_image,tool_image_id:$tool_image,network_id:$network,owner:$owner,run:$run,root_ca_sha256:$sha,root_ca_bytes:$bytes,route_class:"RFC1918-host-interface",owned_cleanup:true,source_unchanged:true}')"
  mg_count total 1; mg_count passed 1; mg_count failed 0; mg_count skipped 0
  mg_status pass
}
