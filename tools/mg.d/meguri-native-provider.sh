# SPDX-License-Identifier: MIT
# Explicit route diagnostic only. This does not exercise a Meguri app transport.
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
  docker network inspect "$MG_NR_NETWORK" >"$MG_NR_PRIVATE/network.json" || return 1
  jq -e --arg name "$MG_NR_NETWORK" --arg id "$MG_NR_NETWORK_ID" --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" '
    length==1 and .[0].Id==$id and .[0].Name==$name and .[0].Driver=="bridge" and
    .[0].Internal==true and .[0].EnableIPv6==false and
    .[0].Labels=={"meguri.mg.run-id":$run,"meguri.sync.owner":$owner}
  ' "$MG_NR_PRIVATE/network.json" >/dev/null
}
mg_native_route__container() {
  local kind="$1" name id image
  case "$kind" in
    runtime) name="$MG_NR_RUNTIME"; id="$MG_NR_RUNTIME_ID"; image="$MG_NR_RUNTIME_IMAGE" ;;
    fixture) name="$MG_NR_FIXTURE"; id="$MG_NR_FIXTURE_ID"; image="$MG_NR_TOOL_IMAGE" ;;
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
    ((.[0].HostConfig.Devices // [])==[]) and ((.[0].HostConfig.Binds // [])==[])
  ' "$MG_NR_PRIVATE/$kind.json" >/dev/null || return 1
  if [ "$kind" = runtime ]; then
    jq -e --arg net "$MG_NR_NETWORK" --arg netid "$MG_NR_NETWORK_ID" --arg gateway "$MG_NR_GATEWAY" \
      --arg port "$MG_NR_PORT" --arg origin "$MG_NR_ORIGIN" --arg volume "$(jq -r .volume "$MG_NR_PRIVATE/instance/instance.json")" '
      .[0] | .HostConfig.NetworkMode==$net and (.NetworkSettings.Networks | keys)==[$net] and
      .NetworkSettings.Networks[$net].NetworkID==$netid and
      .HostConfig.PortBindings=={"18443/tcp":[{HostIp:$gateway,HostPort:$port}]} and
      ([.NetworkSettings.Ports | to_entries[] | select(.value!=null)]==[{key:"18443/tcp",value:[{HostIp:$gateway,HostPort:$port}]}]) and
      .Config.Cmd==["serve","/data/owner",$origin,"0.0.0.0:8080","--trusted-peer","127.0.0.1"] and
      (.Mounts | map(select(.Type!="tmpfs")) | length==1 and .[0].Type=="volume" and .[0].Name==$volume and .[0].Destination=="/data" and .[0].RW==true) and
      (.Mounts | all(.[]; .Destination=="/data" or (.Type=="tmpfs" and .Destination=="/tmp"))) and
      (.HostConfig.Tmpfs | keys)==["/tmp"]
    ' "$MG_NR_PRIVATE/runtime.json" >/dev/null || return 1
    mg_sync_package__volume_check
  else
    jq -e --arg runtime "$MG_NR_RUNTIME_ID" --arg gateway "$MG_NR_GATEWAY" --arg origin "$MG_NR_ORIGIN" \
      --arg source "$MG_NR_COMMIT" --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" --arg tool "$MG_NR_TOOL_IMAGE" \
      --arg sha "$MG_NR_SCRIPT_SHA" --arg name "$MG_NR_FIXTURE" '
      .[0] | .HostConfig.NetworkMode==("container:"+$runtime) and
      ((.HostConfig.PortBindings // {})=={}) and
      (.Mounts | all(.[]; .Type=="tmpfs" and .Destination=="/tmp")) and
      .HostConfig.Tmpfs=={"/tmp":"uid=10001,gid=10001,mode=0700,size=128m"} and
      .Config.Entrypoint==["node"] and
      .Config.Cmd==["/smoke/package-smoke.mjs","native-route-fixture",$gateway,$origin,$source,$run,$owner,$runtime,$tool,$sha,$name]
    ' "$MG_NR_PRIVATE/fixture.json" >/dev/null
  fi
}
mg_native_route__wait() {
  python3 - "$MG_NR_FIXTURE_ID" <<'PY'
import os, selectors, subprocess, sys, time
p = subprocess.Popen(['docker','logs','--follow',sys.argv[1]], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
s = selectors.DefaultSelector(); s.register(p.stdout, selectors.EVENT_READ)
buf = b''; deadline = time.monotonic() + 30; startup_failed = False
try:
    while True:
        left = deadline - time.monotonic()
        if left <= 0 or not s.select(left): raise ValueError('fixture deadline')
        chunk = os.read(p.stdout.fileno(), 4096)
        if not chunk: raise ValueError('fixture ended')
        buf = (buf + chunk)[-32768:]
        if b'MEGURI_NATIVE_ROUTE_READY' in buf.splitlines(): break
except Exception:
    startup_failed = True
    raise SystemExit('native fixture startup failed') from None
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
            raise SystemExit('native fixture log follower cleanup failed') from None
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
  mg_native_route__guest_cleanup || guest_rc=1
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
  if [ "$rc" = 0 ] && [ "$MG_NR_NETWORK_ATTEMPT" = 1 ]; then
    ids="$(docker network ls -q --no-trunc --filter "name=^$MG_NR_NETWORK$")" || rc=1
    if [ -n "$ids" ]; then
      [[ "$ids" =~ ^[0-9a-f]{64}$ ]] && { [ -z "$MG_NR_NETWORK_ID" ] || [ "$ids" = "$MG_NR_NETWORK_ID" ]; } || rc=1
      if [ "$rc" = 0 ]; then
        MG_NR_NETWORK_ID="$ids"
        mg_native_route__network && jq -e '.[0].Containers=={}' "$MG_NR_PRIVATE/network.json" >/dev/null || rc=1
      fi
      if [ "$rc" = 0 ]; then
        docker network rm "$ids" >"$MG_NR_PRIVATE/network-remove" || rc=1
        ids="$(docker network ls -q --no-trunc --filter "name=^$MG_NR_NETWORK$")" || rc=1
        [ -z "$ids" ] || rc=1
      fi
    elif [ -n "$MG_NR_NETWORK_ID" ]; then rc=1; fi
  fi
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
  local ids
  ids="$(docker network ls -q --no-trunc --filter "name=^$MG_NR_NETWORK$")" || return 1
  [ -z "$ids" ] || return 1
  MG_NR_NETWORK_ATTEMPT=1
  docker network create --driver bridge --internal --label "meguri.mg.run-id=$MG_RUN_ID" \
    --label "meguri.sync.owner=$MG_NR_OWNER" "$MG_NR_NETWORK" >"$MG_NR_PRIVATE/network-id" || return 1
  MG_NR_NETWORK_ID="$(cat "$MG_NR_PRIVATE/network-id")" || return 1
  [[ "$MG_NR_NETWORK_ID" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__network || return 1
  mg_native_route__stage volume-init
  MG_NR_INIT_ATTEMPT=1
  mg_sync_package__init "$MG_NR_RECEIPT" && mg_sync_package__volume_check && mg_native_route__source || return 1
  mg_native_route__stage runtime-start
  MG_NR_RUNTIME_ATTEMPT=1
  # Sibling of package start(): no default HTTP/service-port publication, no
  # health request or package-ready HTTP polling before the countable request.
  mg_sync_package__compose run -d --name "$MG_NR_RUNTIME" --no-deps --pull never \
    --label "meguri.mg.run-id=$MG_RUN_ID" --publish "$MG_NR_GATEWAY:$MG_NR_PORT:18443" local-sync \
    >"$MG_NR_PRIVATE/runtime-id" || return 1
  MG_NR_RUNTIME_ID="$(cat "$MG_NR_PRIVATE/runtime-id")" || return 1
  [[ "$MG_NR_RUNTIME_ID" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__container runtime || return 1
  # A listening log event is not a health request. Use the existing bounded
  # runtime waiter; the fixture has its own single fixed-marker follower.
  mg_sync_package__wait "$MG_NR_RUNTIME_ID" || return 1
  mg_native_route__stage fixture-start
  MG_NR_FIXTURE_ATTEMPT=1
  docker run -d --name "$MG_NR_FIXTURE" --init --network "container:$MG_NR_RUNTIME_ID" \
    --user 10001:10001 --read-only --cap-drop ALL --security-opt no-new-privileges \
    --tmpfs /tmp:uid=10001,gid=10001,mode=0700,size=128m \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$MG_NR_OWNER" \
    --entrypoint node "$MG_NR_TOOL_IMAGE" /smoke/package-smoke.mjs native-route-fixture \
    "$MG_NR_GATEWAY" "$MG_NR_ORIGIN" "$MG_NR_COMMIT" "$MG_RUN_ID" "$MG_NR_OWNER" "$MG_NR_RUNTIME_ID" \
    "$MG_NR_TOOL_IMAGE" "$MG_NR_SCRIPT_SHA" "$MG_NR_FIXTURE" >"$MG_NR_PRIVATE/fixture-id" || return 1
  MG_NR_FIXTURE_ID="$(cat "$MG_NR_PRIVATE/fixture-id")" || return 1
  [[ "$MG_NR_FIXTURE_ID" =~ ^[0-9a-f]{64}$ ]] && mg_native_route__container fixture || return 1
  mg_native_route__stage fixture-marker
  mg_native_route__wait || return 1
  mg_native_route__stage ca-export
  mg_native_route__export || return 1
  mg_native_route__stage guest-health
  mg_native_route__guest || return 1
  mg_native_route__stage identities-after
  mg_native_route__images after && mg_native_route__network && mg_native_route__container runtime &&
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
mg_meguri_native_route() {
  # Exactly one required option, once. Never resolve a latest link or build.
  if [ "$#" -ne 2 ] || [ "$1" != --package-receipt ] || [ -z "$2" ] || [[ "$2" = --* ]]; then
    mg_failure usage "meguri-native-route --package-receipt <exact-result.json>"; mg_status error; return
  fi
  mg_native_route__ambient || { mg_failure native-route-ambient "Unexpected task-specific ambient authority"; mg_status error; return; }
  MG_NR_RECEIPT="$2"; MG_NR_STAGE=source-before; MG_NR_FIRST=
  MG_NR_CLEANED=0; MG_NR_CLEAN_OK=0; MG_NR_GUEST_ATTEMPT=0; MG_NR_GUEST_ID=
  MG_NR_NETWORK_ATTEMPT=0; MG_NR_INIT_ATTEMPT=0; MG_NR_RUNTIME_ATTEMPT=0; MG_NR_FIXTURE_ATTEMPT=0
  MG_NR_NETWORK_ID=; MG_NR_RUNTIME_ID=; MG_NR_FIXTURE_ID=; MG_NR_CA_SHA=; MG_NR_CA_BYTES=
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
    '{source_commit:$source,runtime_id:$runtime,fixture_id:$fixture,runtime_image_id:$runtime_image,tool_image_id:$tool_image,network_id:$network,owner:$owner,run:$run,root_ca_sha256:$sha,root_ca_bytes:$bytes,route_class:"RFC1918-host-interface",owned_cleanup:true,source_unchanged:true}')"
  mg_count total 1; mg_count passed 1; mg_count failed 0; mg_count skipped 0
  mg_status pass
}
