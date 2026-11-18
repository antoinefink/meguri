#!/usr/bin/env bats
# SPDX-License-Identifier: MIT
# Local fake process boundaries only. Never Docker, SSH, route, interfaces or
# profile/credential reads. The real parser, receipt, inspectors and cleanup run.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
setup() {
  mg_job() { :; }
  MG_TOOLS="$REPO_ROOT/tools"
  source "$MG_TOOLS/mg.d/meguri-sync.sh"
  source "$MG_TOOLS/mg.d/sync-package.sh"
  source "$MG_TOOLS/mg.d/meguri-provider-test.sh"
  source "$MG_TOOLS/mg.d/mac-vm.sh"
  source "$MG_TOOLS/mg.d/meguri-native-provider.sh"
  MG_ROOT="$BATS_TEST_TMPDIR/source"; MG_RUN_DIR="$BATS_TEST_TMPDIR/run"; MG_RUN_ID=route-test
  MG_JOB=meguri-native-route; MG_SIM_RUNNER=; MG_STATE="$BATS_TEST_TMPDIR/state"
  fake="$BATS_TEST_TMPDIR/fake"; trace="$BATS_TEST_TMPDIR/trace"; hooks="$BATS_TEST_TMPDIR/hooks"
  mkdir -p "$fake/bin" "$MG_ROOT/server/tools" "$MG_RUN_DIR" "$MG_STATE"
  touch "$trace" "$hooks"
  cp "$REPO_ROOT/server/tools/package-smoke.mjs" "$MG_ROOT/server/tools/package-smoke.mjs"
  cp "$REPO_ROOT/server/tools/package-ready.py" "$MG_ROOT/server/tools/package-ready.py"
  commit="$(printf '%040d' 1)"; runtime_id="$(printf '%064d' 2)"; fixture_id="$(printf '%064d' 3)"
  init_id="$(printf '%064d' 4)"; net_id="$(printf '%064d' 5)"
  runtime_image="sha256:$(printf '%064d' 6)"; tool_image="sha256:$(printf '%064d' 7)"
  receipt="$MG_ROOT/build/mg/sync-server-package/package-test/result.json"
  mkdir -p "$(dirname "$receipt")/context" "$(dirname "$receipt")/product"
  printf 'public synthetic inputs\n' >"$(dirname "$receipt")/context/package-inputs.json"
  printf 'public synthetic inventory\n' >"$(dirname "$receipt")/product/runtime-inventory.json"
  jq -n --arg root "$MG_ROOT" --arg commit "$commit" --arg image "$runtime_image" --arg tool "$tool_image" \
    --arg inputs "$(shasum -a 256 "$(dirname "$receipt")/context/package-inputs.json" | awk '{print $1}')" \
    --arg inventory "$(shasum -a 256 "$(dirname "$receipt")/product/runtime-inventory.json" | awk '{print $1}')" \
    '{job:"sync-server-package",run_id:"package-test",status:"pass",exit_code:0,failures:[],dry_run:false,args:[],root:$root,
      commit:{sha:$commit,dirty:false},details:{package:{source_commit:$commit,features:[],profile:"release",image_id:$image,
      client_image_id:$tool,inputs_sha256:$inputs,runtime_inventory_sha256:$inventory}}}' >"$receipt"
  mg_status() { printf 'status:%s\n' "$1" >>"$trace"; }
  mg_failure() { printf 'failure:%s\n' "$1" >>"$trace"; }
  mg_detail() { printf 'detail:%s:%s\n' "$1" "$2" >>"$trace"; }
  mg_count() { printf 'count:%s:%s\n' "$1" "$2" >>"$trace"; }
  mg_message() { :; }; mg_artifact() { :; }; mg_user_step() { :; }
  mg_queued_part() { printf 'queued:%s\n' "$1" >>"$trace"; }
  mg_queued_settle() { mg_status warn; }
  mg_on_exit() { printf '%s\n' "$1" >>"$hooks"; }
  # Mock only Git's process boundary. Real clean/commit/receipt guards remain.
  git() {
    case "$3" in
      status) if [ "${DIRTY:-0}" = 1 ]; then printf ' M synthetic\0'; fi ;;
      rev-parse) printf '%s\n' "${COMMIT_OVERRIDE:-$commit}" ;;
      *) return 97 ;;
    esac
  }
  mg_native_route__host_interfaces() { cat "$fake/interfaces"; }
  mg_native_route__port() { printf 'port-select\n' >>"$trace"; printf '24567\n'; }
  # External lease service boundary. Real VM config selection and take run.
  mg__mac_vm_lease() {
    MG_LEASE_DIR="$fake/lease"
    MG_MAC_VM="$(jq -c '.[0]' <<<"$MG_MAC_VMS")"
    printf 'lease:%s\n' "$(jq -r .name <<<"$MG_MAC_VM")" >>"$trace"
    mg_on_exit "printf 'release\\n' >>$(printf '%q' "$trace")"
  }
  mg__host_lease_release() { printf 'release\n' >>"$trace"; }
  export NR_TEST_FAKE="$fake" NR_TEST_TRACE="$trace" NR_TEST_RUNTIME_ID="$runtime_id" NR_TEST_FIXTURE_ID="$fixture_id"
  cat >"$fake/bin/tart" <<'SH'
#!/usr/bin/env bash
[ "$1" = list ] || exit 97
if [ -e "$NR_TEST_FAKE/vm-stopped" ]; then printf '[{"Name":"selected","State":"stopped"}]\n';
else printf '[{"Name":"selected","State":"running"}]\n'; fi
SH
  cat >"$fake/bin/selected-wrapper" <<'SH'
#!/usr/bin/env bash
printf 'selected-wrapper\n' >>"$NR_TEST_TRACE"
printf '%s\n' "$1" >>"$NR_TEST_FAKE/ssh-commands"
case "$1" in
  *'/sbin/route -n get default'*)
    [ ! -e "$NR_TEST_FAKE/metadata-fail" ] || exit 9
    cat "$NR_TEST_FAKE/route" ;;
  *'stat -f'*'mkdir '*|*'mkdir '*'stat -f'*)
    printf 'guest-create\n' >>"$NR_TEST_TRACE"
    touch "$NR_TEST_FAKE/guest-owned"
    [ ! -e "$NR_TEST_FAKE/guest-create-fail" ] || exit 8
    printf '1:42\n' ;;
  *'/bin/rmdir'*)
    printf 'guest-remove\n' >>"$NR_TEST_TRACE"
    [ ! -e "$NR_TEST_FAKE/guest-remove-fail" ] || exit 8
    [ ! -e "$NR_TEST_FAKE/guest-create-fail" ] || exit 8
    rm -f "$NR_TEST_FAKE/guest-owned" ;;
  *'tar -C'*)
    printf 'guest-copy\n' >>"$NR_TEST_TRACE"
    # Consume only the known two-file transport, never execute command text.
    cat >/dev/null
    [ ! -e "$NR_TEST_FAKE/copy-fail" ] || exit 8 ;;
  *'/bin/sh '*'/health.sh'*)
    printf 'guest-curl\n' >>"$NR_TEST_TRACE"
    [ ! -e "$NR_TEST_FAKE/curl-fail" ] || { printf 'synthetic-private-curl-error\n' >&2; exit 28; }
    [ ! -e "$NR_TEST_FAKE/curl-status" ] || { cat "$NR_TEST_FAKE/curl-status"; exit 1; }
    [ ! -e "$NR_TEST_FAKE/stage-hash-fail" ] || exit 1
    printf 'MEGURI_NATIVE_ROUTE_OK\n' ;;
  *'pgrep -f'*) printf 'mg-mac-vm: none\n' ;;
  *'kern.maxprocperuid'*) printf 'uid 501\nsysctl\n2900\n5000\nps\n501\nend\n' ;;
  *'stat -f %Su /dev/console'*) printf 'fixture-user\n' ;;
  *) printf 'refused-wrapper-command\n' >>"$NR_TEST_TRACE"; exit 97 ;;
esac
SH
  cat >"$fake/bin/docker" <<'SH'
#!/usr/bin/env bash
# Only the Python log followers use this executable. All resource calls use
# the function below; nothing can reach a real Docker executable.
case "$1" in
  logs)
    [ "$#" = 3 ] && [ "$2" = --follow ] || exit 97
    if [ "$3" = "$NR_TEST_RUNTIME_ID" ]; then printf 'listening 0.0.0.0:8080\n';
    elif [ "$3" = "$NR_TEST_FIXTURE_ID" ]; then
      if [ ! -e "$NR_TEST_FAKE/marker-missing" ]; then printf 'MEGURI_NATIVE_ROUTE_READY\n'; fi
    else exit 97; fi ;;
  inspect)
    [ "$#" = 2 ] && [ "$2" = "$NR_TEST_RUNTIME_ID" ] || exit 97
    cat "$NR_TEST_FAKE/runtime.json" ;;
  *) exit 97 ;;
esac
SH
  chmod +x "$fake/bin/"*
  export PATH="$fake/bin:$PATH"
  export MEGURI_MAC_VM_CONFIG="$fake/vm.json"
  jq -n --arg ssh "$fake/bin/selected-wrapper" --arg tart "$fake/bin/tart" \
    '{ssh:$ssh,tart:$tart,vms:["selected"]}' >"$MEGURI_MAC_VM_CONFIG"
  printf '   gateway: 192.168.64.1\n' >"$fake/route"
  printf 'vmenet0: flags=1\n\tinet 192.168.64.1 netmask 0xffffff00 broadcast 192.168.64.255\n' >"$fake/interfaces"
  # Clock injection at the external Python/log-follower boundary lets a late
  # marker fail the real deadline predicate without a 30-second test sleep.
  python3() {
    if [ "${1:-}" = - ] && [ "${2:-}" = "$fixture_id" ] && [ -n "${NR_TEST_FOLLOWER_CASE:-}" ]; then
      { fixture_follower_boundary; cat; } | command python3 "$@"
    elif [ "${1:-}" = - ] && [ "${2:-}" = "$fixture_id" ] && [ "${LATE_MARKER:-0}" = 1 ]; then
      { printf 'import time\n_clock=iter([0,31])\ntime.monotonic=lambda:next(_clock)\n'; cat; } | command python3 "$@"
    else command python3 "$@"; fi
  }
}
fixture_follower_boundary() {
  # Replace only Popen at the fixture helper's external boundary. The actual
  # selector, marker/deadline predicates, finally and caller failure path run.
  cat <<'PY'
import os, pathlib, subprocess, sys
_case = os.environ['NR_TEST_FOLLOWER_CASE']
_root = pathlib.Path(os.environ['NR_TEST_FAKE'])
def _event(value):
    with (_root / 'follower-trace').open('a') as out: out.write(value + '\n')
class _OwnedFollower:
    def __init__(self, args, **kwargs):
        assert args == ['docker', 'logs', '--follow', sys.argv[1]]
        assert kwargs == dict(stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.killed = False; self.waits = 0
        read_fd, write_fd = os.pipe()
        self.stdout = os.fdopen(read_fd, 'rb')
        if not (_root / 'marker-missing').exists():
            os.write(write_fd, b'MEGURI_NATIVE_ROUTE_READY\n')
        os.close(write_fd)
        _event('owned')
    def terminate(self):
        _event('terminate')
        if _case == 'already-exited': raise ProcessLookupError('synthetic-private-terminate')
    def kill(self):
        _event('kill')
        self.killed = True
        if _case == 'exit-race': raise ProcessLookupError('synthetic-private-kill')
    def wait(self, timeout):
        assert timeout == 5
        _event('wait:5'); self.waits += 1
        if _case not in ('normal', 'already-exited'):
            if self.waits == 1 or _case == 'final-reap-timeout':
                raise subprocess.TimeoutExpired('synthetic-private-wait', timeout)
            if _case == 'final-reap-error': raise OSError('synthetic-private-reap')
            assert self.killed
        _event('reaped')
        return -9 if self.killed else 0
subprocess.Popen = _OwnedFollower
PY
}
finish_hooks() {
  local commands=() line i rc=0
  while IFS= read -r line; do commands+=("$line"); done <"$hooks"
  for ((i=${#commands[@]}-1;i>=0;i--)); do eval "${commands[$i]}" || rc=1; done
  return "$rc"
}
invoke() { mg_meguri_native_route "$@"; finish_hooks; }
mutate_receipt() { jq "$1" "$receipt" >"$fake/receipt-new"; mv "$fake/receipt-new" "$receipt"; }
resource_json() {
  local kind="$1" name="$2" id="$3" image="$4" owner="$5" network="$6" running="$7" cmd="$8"
  jq -n --arg name "/$name" --arg id "$id" --arg image "$image" --arg owner "$owner" --arg run "$MG_RUN_ID" \
    --arg network "$network" --argjson running "$running" --argjson cmd "$cmd" --arg netid "$net_id" \
    --arg gateway "${MG_NR_GATEWAY:-192.168.64.1}" --arg port "${MG_NR_PORT:-24567}" --arg kind "$kind" \
    --arg volume "meguri-sync-data-$MG_RUN_ID" '
    [{Id:$id,Name:$name,Image:$image,Config:{User:"10001:10001",Labels:{"meguri.mg.run-id":$run,"meguri.sync.owner":$owner},
      Entrypoint:(if $kind=="fixture" then ["node"] else ["/usr/local/bin/meguri-sync-server"] end),Cmd:$cmd},
      State:{Running:$running,ExitCode:0},HostConfig:{ReadonlyRootfs:true,Privileged:false,CapAdd:null,CapDrop:["ALL"],
      SecurityOpt:["no-new-privileges"],Devices:[],Binds:[],NetworkMode:$network,
      Tmpfs:{"/tmp":(if $kind=="fixture" then "uid=10001,gid=10001,mode=0700,size=128m" else "mode=1777,size=64m" end)},
      PortBindings:(if $kind=="runtime" then {"18443/tcp":[{HostIp:$gateway,HostPort:$port}]} else {} end)},
      Mounts:(if $kind=="fixture" then [] else [{Type:"volume",Name:$volume,Destination:"/data",RW:true}] end),
      NetworkSettings:{Networks:(if $kind=="runtime" then {($network):{NetworkID:$netid}} else {} end),
      Ports:(if $kind=="runtime" then {"18443/tcp":[{HostIp:$gateway,HostPort:$port}],"8080/tcp":null} else {} end)}}]'
}
mg_compose() {
  printf 'compose:%s\n' "$*" >>"$trace"
  [ "$1" = -f ] && [ "$2" = "$MG_ROOT/server/compose.yaml" ] || return 97
  shift 2; [ "$1" = run ] || return 97; shift
  local name= arg kind=init publish= image="$MEGURI_SYNC_IMAGE"
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --name) name="$2"; shift 2 ;;
      --publish) publish="$2"; kind=runtime; shift 2 ;;
      *) shift ;;
    esac
  done
  grep -Fxq mg_native_route__cleanup "$hooks" || return 97
  if [ "$kind" = init ]; then
    resource_json init "$name" "$init_id" "$image" "$MEGURI_SYNC_OWNER" "$MEGURI_SYNC_NETWORK" false '[]' >"$fake/init.json"
    printf 'init-created\n' >>"$trace"
  else
    [ "$publish" = "$MG_NR_GATEWAY:$MG_NR_PORT:18443" ] || return 97
    local cmd
    cmd="$(jq -nc --arg origin "$MEGURI_SYNC_ORIGIN" '["serve","/data/owner",$origin,"0.0.0.0:8080","--trusted-peer","127.0.0.1"]')"
    resource_json runtime "$name" "$runtime_id" "$image" "$MEGURI_SYNC_OWNER" "$MEGURI_SYNC_NETWORK" true "$cmd" >"$fake/runtime.json"
    printf 'runtime-created\n' >>"$trace"; printf '%s\n' "$runtime_id"
    [ "${FAIL_AT:-}" != runtime-create ] && [ "${FAIL_AT:-}" != port-collision ] || return 8
  fi
}
docker() {
  local operation="$1 $2" arg name= owner= image= cmd= kind= path= id= run= filter=
  printf 'docker:%s\n' "$*" >>"$fake/process-trace"
  case "$operation" in
    'image inspect')
      image="$3"
      if [ "$image" = "$runtime_image" ]; then
        jq -n --arg id "$image" --arg source "$commit" '[{Id:$id,Config:{User:"10001:10001",Entrypoint:["/usr/local/bin/meguri-sync-server"],Labels:{"org.opencontainers.image.revision":$source}}}]'
      elif [ "$image" = "$tool_image" ]; then
        jq -n --arg id "$image" '[{Id:$id,Config:{Entrypoint:["node","/smoke/package-smoke.mjs"]}}]'
      else return 1; fi | jq "${IMAGE_MUTATION:-.}" ;;
    'network create')
      grep -Fxq mg_native_route__cleanup "$hooks" || return 97
      [[ " $* " = *' --internal '* ]] && [[ " $* " = *' --driver bridge '* ]] || return 97
      jq -n --arg name "$MG_NR_NETWORK" --arg id "$net_id" --arg run "$MG_RUN_ID" --arg owner "$MG_NR_OWNER" \
        '[{Id:$id,Name:$name,Driver:"bridge",Internal:true,EnableIPv6:false,Labels:{"meguri.mg.run-id":$run,"meguri.sync.owner":$owner},Containers:{}}]' >"$fake/network.json"
      printf 'network-created\n' >>"$trace"; printf '%s\n' "$net_id"
      [ "${FAIL_AT:-}" != network-create ] || return 8 ;;
    'network inspect')
      [ "${INSPECT_FAIL:-}" != network ] || return 8
      jq "${NETWORK_MUTATION:-.}" "$fake/network.json" ;;
    'network ls') [ ! -f "$fake/network.json" ] || printf '%s\n' "$net_id" ;;
    'network rm')
      [ "${REMOVE_FAIL:-}" != network ] || return 8
      [ "$3" = "$net_id" ] || return 97
      [ ! -f "$fake/runtime.json" ] && [ ! -f "$fake/fixture.json" ] && [ ! -f "$fake/init.json" ] || return 97
      rm "$fake/network.json"; printf 'remove:network\n' >>"$trace" ;;
    'volume ls') [ ! -f "$fake/volume.json" ] || jq -r '.[0].Name' "$fake/volume.json" ;;
    'volume create')
      shift 2
      while [ "$#" -gt 0 ]; do
        case "$1" in --label) case "$2" in meguri.sync.owner=*) owner="${2#*=}";; meguri.mg.run-id=*) run="${2#*=}";; esac; shift 2;; *) name="$1"; shift;; esac
      done
      jq -n --arg name "$name" --arg owner "$owner" --arg run "$run" \
        '[{Name:$name,Labels:{"meguri.sync.owner":$owner,"meguri.mg.run-id":$run}}]' >"$fake/volume.json"
      printf '%s\n' "$name"; printf 'volume-created\n' >>"$trace" ;;
    'volume inspect') [ "${INSPECT_FAIL:-}" != volume ] || return 8; jq "${VOLUME_MUTATION:-.}" "$fake/volume.json" ;;
    'volume rm')
      [ "${REMOVE_FAIL:-}" != volume ] || return 8
      [ ! -f "$fake/runtime.json" ] && [ ! -f "$fake/fixture.json" ] || return 97
      rm "$fake/volume.json"; printf 'remove:volume\n' >>"$trace" ;;
    'run -d')
      grep -Fxq mg_native_route__cleanup "$hooks" || return 97
      # Capture actual argv for semantic hardening assertions, not source grep.
      printf '%s\0' "$@" >"$fake/fixture.args"
      shift 2
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --name) name="$2"; shift 2;;
          --label) case "$2" in meguri.sync.owner=*) owner="${2#*=}";; esac; shift 2;;
          "$tool_image") image="$1"; shift; break;;
          *) shift;;
        esac
      done
      cmd="$(jq -nc --arg path "$1" --arg phase "$2" --arg gateway "$3" --arg origin "$4" --arg source "$5" \
        --arg run "$6" --arg owner "$7" --arg runtime "$8" --arg tool "$9" --arg sha "${10}" --arg name "${11}" \
        '[$path,$phase,$gateway,$origin,$source,$run,$owner,$runtime,$tool,$sha,$name]')"
      resource_json fixture "$name" "$fixture_id" "$image" "$owner" "container:$runtime_id" true "$cmd" >"$fake/fixture.json"
      printf 'fixture-created\n' >>"$trace"; printf '%s\n' "$fixture_id"
      [ "${FAIL_AT:-}" != fixture-create ] || return 8 ;;
    'container ls')
      [ "${LIST_FAIL:-0}" = 0 ] || return 8
      for kind in runtime fixture init; do
        path="$fake/$kind.json"; [ -f "$path" ] || continue
        name="$(jq -r '.[0].Name | ltrimstr("/")' "$path")"
        [[ " $* " = *"name=^/$name$"* ]] && jq -r '.[0].Id' "$path"
      done; return 0 ;;
    'inspect '*)
      for kind in runtime fixture init; do
        path="$fake/$kind.json"; [ -f "$path" ] || continue
        id="$(jq -r '.[0].Id' "$path")"; name="$(jq -r '.[0].Name' "$path")"
        if [ "$2" = "$id" ] || [ "$2" = "${name#/}" ]; then
          [ "${INSPECT_FAIL:-}" != "$kind" ] || return 8
          case "$kind" in runtime) jq "${RUNTIME_MUTATION:-.}" "$path";; fixture) jq "${FIXTURE_MUTATION:-.}" "$path";; *) cat "$path";; esac
          return
        fi
      done; return 1 ;;
    'stop '*) [ "${REMOVE_FAIL:-}" != stop ] || return 8; printf 'stop:%s\n' "$4" >>"$trace" ;;
    'rm '*)
      for arg in "$@"; do id="$arg"; done
      if [ "${REMOVE_FAIL:-}" = container ] && [ "$id" = "$runtime_id" ]; then return 8; fi
      for kind in runtime fixture init; do
        path="$fake/$kind.json"; [ -f "$path" ] || continue
        if [ "$(jq -r '.[0].Id' "$path")" = "$id" ]; then rm "$path"; printf 'remove:%s\n' "$kind" >>"$trace"; return; fi
      done; return 97 ;;
    'exec --user')
      [ "$4" = "$fixture_id" ] && [ "$5" = node ] && [ "$6" = -e ] && [ "$8" = root-ca.pem ] && [ "$#" = 9 ] || return 97
      printf 'ca-export\n' >>"$trace"
      [ "${FAIL_AT:-}" != ca-export ] || { printf 'synthetic-private-export-error\n' >&2; return 8; }
      if [ "${CHANGE_SOURCE:-}" = export ]; then printf 'changed\n' >>"$MG_ROOT/server/tools/package-smoke.mjs"; fi
      printf '%s\n' '-----BEGIN CERTIFICATE-----' 'UHVibGljIGZha2UgYm91bmRhcnkgQ0E=' '-----END CERTIFICATE-----' ;;
    *) printf 'forbidden-process:%s\n' "$*" >>"$trace"; return 97 ;;
  esac
}
assert_failed() { grep -Fxq status:fail "$trace"; ! grep -Fxq status:pass "$trace"; ! grep -Fxq count:passed:1 "$trace"; }
assert_before() {
  local first second
  first="$(grep -n -F "$1" "$trace" | head -1 | cut -d: -f1)"
  second="$(grep -n -F "$2" "$trace" | head -1 | cut -d: -f1)"
  [ -n "$first" ] && [ -n "$second" ] && [ "$first" -lt "$second" ]
}
reset_run() {
  rm -rf "$MG_RUN_DIR"; mkdir "$MG_RUN_DIR"
  rm -f "$fake/network.json" "$fake/runtime.json" "$fake/fixture.json" "$fake/init.json" "$fake/volume.json" "$fake/guest-owned"
  : >"$trace"; : >"$hooks"
}
@test "native route: exact call traverses real guards and counts only one after owned cleanup" {
  run invoke --package-receipt "$receipt"
  [ "$status" = 0 ]
  grep -Fxq status:pass "$trace"
  [ "$(grep '^count:' "$trace")" = "$(printf 'count:total:1\ncount:passed:1\ncount:failed:0\ncount:skipped:0')" ]
  [ "$(grep -c '^guest-curl$' "$trace")" = 1 ]
  [ "$(grep -c '^port-select$' "$trace")" = 1 ]
  assert_before remove:fixture remove:runtime
  assert_before remove:runtime remove:network
  assert_before remove:network remove:volume
  assert_before remove:volume status:pass
  assert_before guest-remove release
  ! grep -q 'refused-wrapper-command\|forbidden-process' "$trace"
  [ ! -e "$fake/guest-owned" ] && [ ! -e "$fake/network.json" ] && [ ! -e "$fake/volume.json" ]
  [[ "$output" != *192.168.64.1* && "$output" != *CERTIFICATE* && "$output" != *synthetic-private* ]]
  # Hardening assertions consume the actual executed argv, not implementation.
  jq -Rse --arg runtime "$runtime_id" --arg tool "$tool_image" '
    split("\u0000")[:-1] |
    def values($f): [range(0;length-1) as $i | select(.[$i]==$f) | .[$i+1]];
    values("--network")==["container:"+$runtime] and values("--user")==["10001:10001"] and
    values("--cap-drop")==["ALL"] and values("--security-opt")==["no-new-privileges"] and
    values("--tmpfs")==["/tmp:uid=10001,gid=10001,mode=0700,size=128m"] and
    index("--read-only")!=null and index("--mount")==null and index($tool)!=null and
    index("native-route-fixture")!=null and index("provider-fixture")==null
  ' "$fake/fixture.args"
  ! grep -Eq -- '--service-ports|--publish [^ ]*:8080|--privileged|network prune|volume prune|ssh -|security add|trust-settings|keychain' "$fake/process-trace" "$fake/ssh-commands"
}
@test "native route: missing duplicate unknown empty and missing operand fail before any lease" {
  local args
  for args in '' '--package-receipt' '--unknown x' '--package-receipt --unknown' '--package-receipt x --package-receipt y' '--package-receipt x trailing'; do
    : >"$trace"
    mg_meguri_native_route $args
    grep -Fxq failure:usage "$trace"; grep -Fxq status:error "$trace"
    [ ! -s "$hooks" ]
  done
  mg_meguri_native_route --package-receipt ''
  [ ! -s "$hooks" ]
}
@test "native route: canonical receipt rejects foreign old failed dirty altered input and noncanonical path" {
  local mutation original="$fake/original-receipt"
  cp "$receipt" "$original"
  for mutation in '.root="foreign"' '.commit.sha="old"' '.details.package.source_commit="old"' '.status="fail"' \
    '.commit.dirty=true' '.exit_code=1' '.failures=[{}]' '.dry_run=true' '.args=["x"]' \
    '.details.package.inputs_sha256="wrong"' '.details.package.runtime_inventory_sha256="wrong"' \
    '.details.package.image_id="tag:latest"' '.details.package.client_image_id="tag:latest"'; do
    reset_run; cp "$original" "$receipt"; mutate_receipt "$mutation"
    invoke --package-receipt "$receipt"
    assert_failed; [ ! -s "$hooks" ]
    grep -Fq 'package-receipt' "$trace"
  done
  reset_run; cp "$original" "$receipt"; cp "$receipt" "$fake/result.json"
  invoke --package-receipt "$fake/result.json"
  assert_failed; [ ! -s "$hooks" ]
}
@test "native route: dirty source stops before receipt and changed source after export never passes" {
  DIRTY=1; invoke --package-receipt "$receipt"; assert_failed; [ ! -s "$hooks" ]
  DIRTY=0; reset_run; CHANGE_SOURCE=export
  invoke --package-receipt "$receipt"; assert_failed
  grep -Fq 'first_stage:"ca-export"' "$trace"
  ! grep -q '^guest-curl$' "$trace"
  grep -Fxq release "$trace"
}
@test "native route: source mutation only at finish invalidates an otherwise successful route" {
  # External Git process observes a dirty checkout after all removals.
  git() {
    case "$3" in
      rev-parse) printf '%s\n' "$commit" ;;
      status) if grep -Fxq remove:volume "$trace"; then printf ' M synthetic\0'; fi ;;
      *) return 97 ;;
    esac
  }
  invoke --package-receipt "$receipt"; assert_failed
  [ "$(grep -c '^guest-curl$' "$trace")" = 1 ]
  grep -Fq 'first_stage:"source-after"' "$trace"
}
@test "native route: missing configured VM queues while answered metadata failure fails and releases" {
  rm "$MEGURI_MAC_VM_CONFIG"
  invoke --package-receipt "$receipt"
  grep -Fxq queued:mac-vm "$trace"; grep -Fxq status:warn "$trace"; [ ! -s "$hooks" ]
  jq -n --arg ssh "$fake/bin/selected-wrapper" --arg tart "$fake/bin/tart" '{ssh:$ssh,tart:$tart,vms:["selected"]}' >"$MEGURI_MAC_VM_CONFIG"
  reset_run; touch "$fake/metadata-fail"
  invoke --package-receipt "$receipt"; assert_failed
  grep -Fxq lease:selected "$trace"; grep -Fxq release "$trace"
  ! grep -q network-created "$trace"
}
@test "native route: no caller or ambient route endpoint network or VM authority is accepted" {
  local key
  for key in MEGURI_NATIVE_GATEWAY MG_NR_GATEWAY MEGURI_SYNC_NETWORK MEGURI_SYNC_ORIGIN MEGURI_SYNC_PORT MG_MAC_VM MG_MAC_VMS; do
    : >"$trace"
    export "$key=forged"
    mg_meguri_native_route --package-receipt "$receipt"
    unset "$key"
    grep -Fxq status:error "$trace"; [ ! -s "$hooks" ]
  done
  mg_meguri_native_route --package-receipt "$receipt" --gateway 192.168.64.1
  grep -Fxq failure:usage "$trace"
}
@test "native route: actual gateway parser requires exactly one strict RFC1918 gateway on a host interface" {
  local route
  for route in 'gateway: 8.8.8.8' 'gateway: 127.0.0.1' 'gateway: 0.0.0.0' 'gateway: 169.254.1.1' \
    'gateway: 192.168.064.1' 'gateway: 192.168.64.256' 'gateway: localhost' 'gateway: ::1' \
    'gateway: 192.168.64.1 extra' 'gateway: 192.168.64.2' ''; do
    reset_run; printf '%s\n' "$route" >"$fake/route"
    invoke --package-receipt "$receipt"; assert_failed
    ! grep -q network-created "$trace"; grep -Fxq release "$trace"
  done
  reset_run; printf 'gateway: 192.168.64.1\ngateway: 192.168.64.1\n' >"$fake/route"
  invoke --package-receipt "$receipt"; assert_failed
  for route in 10.1.2.3 172.16.2.1 172.31.2.1 192.168.64.1; do
    printf 'gateway: %s\n' "$route" >"$fake/route"
    printf '\tinet %s netmask 0xffffff00\n' "$route" >"$fake/interfaces"
    [ "$(mg_native_route__gateway "$fake/route" "$fake/interfaces")" = "$route" ]
  done
}
@test "native route: immutable runtime and tool image mismatch stops before creation" {
  local mutation
  for mutation in '.[0].Id="wrong"' '.[0].Config.Entrypoint=["wrong"]' \
    'if .[0].Config.Entrypoint==["node","/smoke/package-smoke.mjs"] then .[0].Id="wrong" else . end'; do
    reset_run; IMAGE_MUTATION="$mutation"
    invoke --package-receipt "$receipt"; assert_failed
    ! grep -q network-created "$trace"
  done
}
@test "native route: real network inspector rejects noninternal foreign labels and wrong immutable ID" {
  local mutation
  for mutation in '.[0].Internal=false' '.[0].Id="wrong"' '.[0].Labels["meguri.sync.owner"]="foreign"' \
    '.[0].Labels["meguri.mg.run-id"]="foreign"' '.[0].Driver="host"' '.[0].EnableIPv6=true'; do
    reset_run; NETWORK_MUTATION="$mutation"
    # The same rejected network cannot pass the registered cleanup guard.
    run invoke --package-receipt "$receipt"
    [ "$status" = 1 ]; assert_failed
    ! grep -q runtime-created "$trace"
    ! grep -Fxq remove:network "$trace"
  done
}
@test "native route: real runtime inspector rejects extra wrong wildcard HTTP and network bindings" {
  local mutation
  for mutation in '.[0].HostConfig.PortBindings["18443/tcp"][0].HostIp="0.0.0.0"' \
    '.[0].HostConfig.PortBindings["18443/tcp"][0].HostIp="::"' \
    '.[0].HostConfig.PortBindings["18443/tcp"][0].HostPort="1"' \
    '.[0].HostConfig.PortBindings["8080/tcp"]=[{HostIp:"127.0.0.1",HostPort:"8080"}]' \
    '.[0].HostConfig.PortBindings["18443/tcp"]+=[{HostIp:"127.0.0.1",HostPort:"24567"}]' \
    '.[0].NetworkSettings.Ports["18443/tcp"][0].HostIp="0.0.0.0"' \
    '.[0].NetworkSettings.Networks["foreign"]={NetworkID:"foreign"}' \
    '.[0].HostConfig.Privileged=true' '.[0].HostConfig.ReadonlyRootfs=false' \
    '.[0].Image="wrong"' '.[0].Config.Cmd[2]="https://wrong"'; do
    reset_run; RUNTIME_MUTATION="$mutation"
    run invoke --package-receipt "$receipt"
    # Cleanup admits the owned runtime unless its immutable image changed.
    if [ "$mutation" = '.[0].Image="wrong"' ]; then [ "$status" = 1 ];
    else [ "$status" = 0 ]; fi
    assert_failed
    ! grep -q fixture-created "$trace"
  done
}
@test "native route: port collision fails once and actual registered cleanup handles reached partial creation" {
  local point
  for point in network-create runtime-create port-collision fixture-create ca-export; do
    reset_run; FAIL_AT="$point"
    invoke --package-receipt "$receipt"; assert_failed
    [ "$(grep -c '^port-select$' "$trace")" = 1 ]
    grep -Fxq release "$trace"
    [ ! -f "$fake/network.json" ] && [ ! -f "$fake/runtime.json" ] && [ ! -f "$fake/fixture.json" ] && [ ! -f "$fake/volume.json" ]
    if [ "$point" != network-create ]; then assert_before remove:runtime remove:network; fi
  done
}
@test "native route: cancellation runs actual registered cleanup before creation and after every owned start" {
  local point
  # Simulate MG's TERM exit at external create/start/export/guest boundaries.
  # Returning from a nested shell preserves resource files; the real hook is
  # evaluated there before the fake lease release, as in MG's exit handler.
  for point in before network runtime fixture export guest; do
    reset_run
    (
      mg_native_route__port() {
        if [ "$point" = before ]; then finish_hooks; exit 143; fi
        printf '24567\n'
      }
      original_docker="$(declare -f docker)"
      eval "${original_docker/docker ()/boundary_docker ()}"
      docker() {
        boundary_docker "$@"; local result=$?
        case "$point:$1:$2" in
          network:network:create|fixture:run:-d|export:exec:--user) finish_hooks; exit 143 ;;
        esac
        return "$result"
      }
      original_compose="$(declare -f mg_compose)"
      eval "${original_compose/mg_compose ()/boundary_compose ()}"
      mg_compose() {
        boundary_compose "$@"; local result=$?
        if [ "$point" = runtime ] && [ -e "$fake/runtime.json" ]; then finish_hooks; exit 143; fi
        return "$result"
      }
      if [ "$point" = guest ]; then touch "$fake/curl-fail"; fi
      invoke --package-receipt "$receipt"
    ) || :
    ! grep -Fxq status:pass "$trace"
    grep -Fxq release "$trace"
    [ ! -e "$fake/runtime.json" ] && [ ! -e "$fake/fixture.json" ] && [ ! -e "$fake/network.json" ]
    rm -f "$fake/curl-fail"
  done
}
@test "native route: missing and late fixed fixture marker stop before CA export and guest request" {
  touch "$fake/marker-missing"
  invoke --package-receipt "$receipt"; assert_failed
  ! grep -Eq '^ca-export$|^guest-curl$' "$trace"
  rm "$fake/marker-missing"; reset_run; LATE_MARKER=1
  invoke --package-receipt "$receipt"; assert_failed
  ! grep -Eq '^ca-export$|^guest-curl$' "$trace"
}
@test "native route: fixture follower normal and already exited handles are reaped without kill" {
  local scenario
  MG_NR_FIXTURE_ID="$fixture_id"
  for scenario in normal already-exited; do
    export NR_TEST_FOLLOWER_CASE="$scenario"
    : >"$fake/follower-trace"
    run mg_native_route__wait
    [ "$status" = 0 ]; [ -z "$output" ]
    [ "$(cat "$fake/follower-trace")" = "$(printf 'owned\nterminate\nwait:5\nreaped')" ]
  done
}
@test "native route: TERM timeout kills only owned follower and reaps including exit before kill" {
  local scenario
  MG_NR_FIXTURE_ID="$fixture_id"
  for scenario in ignore-term exit-race; do
    export NR_TEST_FOLLOWER_CASE="$scenario"
    : >"$fake/follower-trace"
    run mg_native_route__wait
    [ "$status" = 0 ]; [ -z "$output" ]
    [ "$(cat "$fake/follower-trace")" = "$(printf 'owned\nterminate\nwait:5\nkill\nwait:5\nreaped')" ]
  done
}
@test "native route: marker failure stays primary and uncertain final reap never publishes pass" {
  local scenario marker expected
  for scenario in ignore-term final-reap-timeout final-reap-error; do
    for marker in missing ready; do
      [ "$scenario:$marker" != ignore-term:ready ] || continue
      reset_run; : >"$fake/follower-trace"
      export NR_TEST_FOLLOWER_CASE="$scenario"
      if [ "$marker" = missing ]; then
        touch "$fake/marker-missing"; expected='native fixture startup failed'
      else
        rm -f "$fake/marker-missing"; expected='native fixture log follower cleanup failed'
      fi
      run invoke --package-receipt "$receipt"
      assert_failed
      grep -Fq 'first_stage:"fixture-marker"' "$trace"
      ! grep -Eq '^ca-export$|^guest-curl$' "$trace"
      grep -Fxq "$expected" "$MG_RUN_DIR/private/native-route/control.log"
      ! grep -Eq 'synthetic-private|Traceback' "$MG_RUN_DIR/private/native-route/control.log"
      [[ "$output" != *synthetic-private* ]]
      if [ "$scenario" = ignore-term ]; then
        [ "$(cat "$fake/follower-trace")" = "$(printf 'owned\nterminate\nwait:5\nkill\nwait:5\nreaped')" ]
      else
        [ "$(cat "$fake/follower-trace")" = "$(printf 'owned\nterminate\nwait:5\nkill\nwait:5')" ]
      fi
      assert_before remove:fixture remove:runtime
      assert_before remove:runtime remove:network
      assert_before remove:network remove:volume
      grep -Fxq release "$trace"
    done
  done
}
@test "native route: staging binding and guest request failures retain first stage and never attempt twice" {
  local failure
  for failure in guest-create-fail copy-fail stage-hash-fail curl-fail curl-status; do
    reset_run; touch "$fake/$failure"
    if [ "$failure" = curl-status ]; then printf '500\n' >"$fake/curl-status"; fi
    run invoke --package-receipt "$receipt"
    assert_failed; grep -Fq 'first_stage:"guest-health"' "$trace"
    [ "$(grep -c '^guest-curl$' "$trace" || :)" -le 1 ]
    grep -Fxq release "$trace"
    [[ "$output" != *synthetic-private* ]]
    rm "$fake/$failure"
  done
}
@test "native route: foreign owners and failed inspect or remove are never hidden by cleanup" {
  local failure
  for failure in foreign-network foreign-runtime foreign-fixture foreign-volume inspect-network inspect-runtime inspect-volume remove-network remove-container remove-volume; do
    reset_run
    NETWORK_MUTATION=.; RUNTIME_MUTATION=.; FIXTURE_MUTATION=.; VOLUME_MUTATION=.; INSPECT_FAIL=; REMOVE_FAIL=
    case "$failure" in
      foreign-network) NETWORK_MUTATION='.[0].Labels["meguri.sync.owner"]="foreign"' ;;
      foreign-runtime) RUNTIME_MUTATION='.[0].Config.Labels["meguri.sync.owner"]="foreign"' ;;
      foreign-fixture) FIXTURE_MUTATION='.[0].Config.Labels["meguri.sync.owner"]="foreign"' ;;
      foreign-volume) VOLUME_MUTATION='.[0].Labels["meguri.sync.owner"]="foreign"' ;;
      inspect-*) INSPECT_FAIL="${failure#inspect-}" ;;
      remove-*) REMOVE_FAIL="${failure#remove-}" ;;
    esac
    invoke --package-receipt "$receipt" || :
    assert_failed; grep -Fxq release "$trace"
    case "$failure" in
      foreign-network|inspect-network|remove-network) [ -e "$fake/network.json" ] ;;
      foreign-runtime|inspect-runtime) [ -e "$fake/runtime.json" ] ;;
      remove-container)
        [ -e "$fake/runtime.json" ]
        assert_before remove:init runtime-created
        assert_before runtime-created "stop:$runtime_id" ;;
      foreign-fixture) [ -e "$fake/fixture.json" ] ;;
      foreign-volume|inspect-volume|remove-volume) [ -e "$fake/volume.json" ] ;;
    esac
  done
}
@test "native route: stopped selected VM queues without starting another VM or host fallback" {
  touch "$fake/vm-stopped"
  invoke --package-receipt "$receipt"
  grep -Fxq queued:mac-vm "$trace"; grep -Fxq status:warn "$trace"
  grep -Fxq lease:selected "$trace"; grep -Fxq release "$trace"
  ! grep -q network-created "$trace"
}
prepare_guest_command() {
  MG_NR_GUEST="$fake/guest-script"; mkdir "$MG_NR_GUEST"
  MG_NR_GUEST_ID=1:42; MG_NR_OWNER="$(printf '%032d' 1)"
  MG_NR_ORIGIN=https://192.168.64.1:24567
  printf '%s\n' "$MG_NR_OWNER" >"$MG_NR_GUEST/owner"
  printf 'public synthetic CA\n' >"$MG_NR_GUEST/root-ca.pem"
  MG_NR_CA_SHA="$(shasum -a 256 "$MG_NR_GUEST/root-ca.pem" | awk '{print $1}')"
  MG_NR_CA_BYTES="$(wc -c <"$MG_NR_GUEST/root-ca.pem" | tr -d ' ')"
  cat >"$fake/bin/stat-fake" <<'SH'
#!/usr/bin/env bash
printf '1:42\n'
SH
  # This is a process boundary fake, reached by generated production command.
  # Strict argv predicates fail if any TLS/endpoint/deadline option changes.
  cat >"$fake/bin/curl-fake" <<SH
#!/usr/bin/env bash
printf 'curl\\n' >>'$trace'
printf '%s\\0' "\$@" >'$fake/curl.args'
expected=(--disable --proxy '' --noproxy '*' --cacert root-ca.pem --proto '=https' --proto-redir '=https' --connect-timeout 5 --max-time 15 --max-filesize 4096 --max-redirs 0 --output body --write-out '%{http_code}' 'https://192.168.64.1:24567/health')
[ "\$#" = "\${#expected[@]}" ] || exit 97
i=0; for arg in "\$@"; do [ "\$arg" = "\${expected[\$i]}" ] || exit 97; i=\$((i+1)); done
if [ -f '$fake/bad-body' ]; then printf 'wrong body' >body;
else printf '%s' '{"data":{"collection_compatibility":false,"state":"account_substrate"}}' >body; fi
if [ -f '$fake/non200' ]; then printf 500; else printf 200; fi
[ ! -f '$fake/deadline' ] || exit 28
[ ! -f '$fake/nonzero' ] || exit 7
SH
  chmod +x "$fake/bin/stat-fake" "$fake/bin/curl-fake"
  mg_native_route__guest_script >"$fake/production-health.sh"
  # Map just the two external macOS commands; retain the actual generated shell
  # predicates, one curl invocation, env clearing, status and body validation.
  python3 - "$fake/production-health.sh" "$fake/bin" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1]); tools = sys.argv[2]
s = p.read_text().replace('/usr/bin/curl', tools+'/curl-fake').replace('/usr/bin/stat', tools+'/stat-fake')
p.write_text(s)
PY
}
@test "native route: actual one-shot command checks curl status deadline body and never attempts twice" {
  prepare_guest_command
  local failure
  for failure in success deadline nonzero non200 bad-body; do
    : >"$trace"
    if [ "$failure" != success ]; then touch "$fake/$failure"; fi
    run /bin/sh "$fake/production-health.sh"
    if [ "$failure" = success ]; then [ "$status" = 0 ]; [ "$output" = MEGURI_NATIVE_ROUTE_OK ];
    else [ "$status" -ne 0 ]; [[ "$output" != *MEGURI_NATIVE_ROUTE_OK* ]]; rm "$fake/$failure"; fi
    [ "$(grep -c '^curl$' "$trace")" = 1 ]
  done
}
@test "native route: actual guest command rejects altered staged CA and owner before curl" {
  prepare_guest_command
  printf 'changed\n' >>"$MG_NR_GUEST/root-ca.pem"
  run /bin/sh "$fake/production-health.sh"
  [ "$status" -ne 0 ]; ! grep -q '^curl$' "$trace"
  printf 'public synthetic CA\n' >"$MG_NR_GUEST/root-ca.pem"
  printf 'foreign\n' >"$MG_NR_GUEST/owner"
  run /bin/sh "$fake/production-health.sh"
  [ "$status" -ne 0 ]; ! grep -q '^curl$' "$trace"
}
prepare_ca_export() {
  tls="$fake/tls"; mkdir -m 700 "$tls"
  # Synthetic local cryptographic fixtures only; no service/network access.
  openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=Route-Test-CA \
    -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign \
    -keyout "$tls/ca-key.pem" -out "$tls/root-ca.pem" >/dev/null 2>&1
  openssl req -new -newkey rsa:2048 -nodes -subj /CN=localhost \
    -keyout "$tls/leaf-key.pem" -out "$tls/leaf.csr" >/dev/null 2>&1
  printf 'basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1,IP:192.168.64.1\n' >"$tls/leaf.ext"
  openssl x509 -req -in "$tls/leaf.csr" -CA "$tls/root-ca.pem" -CAkey "$tls/ca-key.pem" \
    -set_serial 1 -days 1 -extfile "$tls/leaf.ext" -out "$tls/leaf.pem" >/dev/null 2>&1
  cp "$MG_ROOT/server/tools/package-smoke.mjs" "$tls/script.js"
  jq -n --arg source "$commit" --arg runtime "$runtime_id" --arg tool "$tool_image" \
    --arg script "$(shasum -a 256 "$tls/script.js" | awk '{print $1}')" \
    '{gateway:"192.168.64.1",origin:"https://192.168.64.1:24567",source:$source,run:"route-test",owner:("0"*31+"1"),
      runtime:$runtime,tool:$tool,scriptSha:$script,fixture:"meguri-native-fixture-route-test"}' >"$tls/expected.json"
  jq --arg sha "$(shasum -a 256 "$tls/root-ca.pem" | awk '{print $1}')" \
    --argjson bytes "$(wc -c <"$tls/root-ca.pem" | tr -d ' ')" '.+{caSha:$sha,caBytes:$bytes}' "$tls/expected.json" >"$tls/binding.json"
  mg_native_route__exporter >"$fake/exporter.js"
  cat >"$fake/export-harness.cjs" <<'JS'
const fs = require('node:fs'); const vm = require('node:vm'); const crypto = require('node:crypto');
const [codePath, root, selector, expectation] = process.argv.slice(2);
const map = p => {
  if (p === '/smoke/package-smoke.mjs') return root+'/script.js';
  const allowed = ['/tmp/native-route', '/tmp/native-route/binding.json', '/tmp/native-route/root-ca.pem', '/tmp/native-route/leaf.pem'];
  if (!allowed.includes(p)) throw Error('unadmitted file');
  return p === '/tmp/native-route' ? root : root+'/'+p.split('/').at(-1);
};
const stat = s => { s.uid = 10001; return s; };
const fakeFs = { constants: fs.constants,
  lstatSync: p => stat(fs.lstatSync(map(p))),
  openSync: (p, flags) => fs.openSync(map(p), flags),
  fstatSync: fd => stat(fs.fstatSync(fd)), closeSync: fd => fs.closeSync(fd),
  readFileSync: p => fs.readFileSync(typeof p === 'number' ? p : map(p)),
};
const result = { argv: ['node', selector, fs.readFileSync(expectation, 'utf8')], exitCode: 0, stdout: process.stdout };
vm.runInNewContext(fs.readFileSync(codePath, 'utf8'), { require: name => {
  if (name === 'node:fs') return fakeFs; if (name === 'node:crypto') return crypto; throw Error();
}, process: result });
process.exitCode = result.exitCode;
JS
}
run_export() { node "$fake/export-harness.cjs" "$fake/exporter.js" "$tls" "$1" "$tls/expected.json"; }
@test "native route: actual fixed exporter emits only public root and refuses every other selector" {
  prepare_ca_export
  run_export root-ca.pem >"$fake/exported.pem"
  cmp -s "$tls/root-ca.pem" "$fake/exported.pem"
  local selector
  for selector in input.json ca.der ca-key.pem leaf-key.pem binding.json /tmp/native-route/root-ca.pem '../root-ca.pem' ''; do
    run run_export "$selector"
    [ "$status" -ne 0 ]; [ -z "$output" ]
  done
}
@test "native route: actual exporter rejects CA SAN role source hash and parent binding changes" {
  prepare_ca_export
  local mutation
  cp "$tls/binding.json" "$fake/original-binding"
  for mutation in '.caSha="wrong"' '.caBytes=1' '.source="wrong"' '.runtime="wrong"' '.tool="wrong"' \
    '.run="wrong"' '.owner="wrong"' '.fixture="wrong"' '.origin="https://wrong"' '.scriptSha="wrong"' \
    '.gateway="192.168.64.2"' '.extra=true'; do
    jq "$mutation" "$fake/original-binding" >"$tls/binding.json"
    run run_export root-ca.pem
    [ "$status" -ne 0 ]; [ -z "$output" ]
  done
  cp "$fake/original-binding" "$tls/binding.json"
  printf 'changed\n' >>"$tls/script.js"
  run run_export root-ca.pem
  [ "$status" -ne 0 ]; [ -z "$output" ]
  cp "$MG_ROOT/server/tools/package-smoke.mjs" "$tls/script.js"
  # A CA used as a leaf fails the leaf role even with otherwise matching root.
  cp "$tls/leaf.pem" "$fake/original-leaf"
  cp "$tls/root-ca.pem" "$tls/leaf.pem"
  run run_export root-ca.pem
  [ "$status" -ne 0 ]; [ -z "$output" ]
  cp "$fake/original-leaf" "$tls/leaf.pem"
  # Alter both parent and stored gateway: exact IP SAN, not just binding equality.
  jq '.gateway="192.168.64.2"' "$tls/expected.json" >"$tls/new"; mv "$tls/new" "$tls/expected.json"
  jq '.gateway="192.168.64.2"' "$tls/binding.json" >"$tls/new"; mv "$tls/new" "$tls/binding.json"
  run run_export root-ca.pem
  [ "$status" -ne 0 ]; [ -z "$output" ]
}
@test "native route: changed commit only at finish fails the stable clean source guard" {
  git() {
    case "$3" in
      status) : ;;
      rev-parse) if grep -Fxq remove:volume "$trace"; then printf '%040d\n' 9; else printf '%s\n' "$commit"; fi ;;
      *) return 97 ;;
    esac
  }
  invoke --package-receipt "$receipt"; assert_failed
  grep -Fq 'first_stage:"source-after"' "$trace"
  [ "$(grep -c '^guest-curl$' "$trace")" = 1 ]
}
@test "native route: ownership drift in fixture namespace or image stops before export" {
  local mutation
  for mutation in '.[0].HostConfig.NetworkMode="container:foreign"' '.[0].Image="foreign"' \
    '.[0].Config.Labels["meguri.mg.run-id"]="old"' '.[0].HostConfig.CapDrop=[]' \
    '.[0].HostConfig.PortBindings={"8080/tcp":[{HostIp:"0.0.0.0",HostPort:"8080"}]}' \
    '.[0].Mounts=[{Type:"volume",Name:"foreign",Destination:"/data"}]'; do
    reset_run; FIXTURE_MUTATION="$mutation"
    invoke --package-receipt "$receipt" || :; assert_failed
    ! grep -Eq '^ca-export$|^guest-curl$' "$trace"
  done
}
@test "native route: uncertain guest cleanup and container listing keep failure despite successful request" {
  touch "$fake/guest-remove-fail"
  invoke --package-receipt "$receipt" || :; assert_failed
  [ "$(grep -c '^guest-curl$' "$trace")" = 1 ]
  [ -e "$fake/guest-owned" ]; grep -Fxq release "$trace"
  rm "$fake/guest-remove-fail"
  reset_run; LIST_FAIL=1
  invoke --package-receipt "$receipt" || :; assert_failed
  ! grep -Fxq remove:volume "$trace"
  grep -Fxq release "$trace"
}
