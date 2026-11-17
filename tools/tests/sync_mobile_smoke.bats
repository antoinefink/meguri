#!/usr/bin/env bats
# SPDX-License-Identifier: MIT
# Policy doubles only. No Docker/network/device/compatibility success.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup() {
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/host.sh"
  source "$REPO_ROOT/tools/mg.d/meguri-sync.sh"
  source "$REPO_ROOT/tools/mg.d/sync-package.sh"
  source "$REPO_ROOT/tools/mg.d/sync-mobile-smoke.sh"
  export MG_ROOT="$BATS_TEST_TMPDIR/root" MG_TOOLS="$REPO_ROOT/tools"
  export MOBILE_FAKE="$BATS_TEST_TMPDIR/docker" MOBILE_MODE=valid
  mkdir -p "$MG_ROOT" "$MOBILE_FAKE" "$BATS_TEST_TMPDIR/bin"
  ln -s "$REPO_ROOT/server" "$MG_ROOT/server"
  export PATH="$BATS_TEST_TMPDIR/bin:$PATH"
  mg_failure() { printf '%s\n' "$1" >>"$MG_RUN_DIR/failures"; }
  mg_status() { printf '%s\n' "$1" >"$MG_RUN_DIR/status"; }
  mg_detail() { printf '%s=%s\n' "$1" "$2" >>"$MG_RUN_DIR/details"; }
  mg_message() { printf '%s\n' "$1"; }
  mg_artifact() {
    case "$2" in *setup.txt|*/sync-server-mobile-smoke) return 97 ;; esac
    printf '%s\n' "$2" >>"$MG_RUN_DIR/artifacts"
  }
  # Only source identity is doubled. The actual receipt validator, volume,
  # initialization, Compose start, readiness and stop entrypoints run below.
  git() {
    case "$3" in status) return 0 ;; rev-parse) printf '%040d\n' 1 ;; *) return 97 ;; esac
  }
  mg_compose() {
    env MEGURI_SYNC_IMAGE="$MEGURI_SYNC_IMAGE" MEGURI_SYNC_VOLUME="$MEGURI_SYNC_VOLUME" \
      MEGURI_SYNC_OWNER="$MEGURI_SYNC_OWNER" MEGURI_SYNC_ORIGIN="$MEGURI_SYNC_ORIGIN" \
      MEGURI_SYNC_TRUSTED_PEER="$MEGURI_SYNC_TRUSTED_PEER" MEGURI_SYNC_NETWORK="$MEGURI_SYNC_NETWORK" \
      MEGURI_SYNC_PORT="$MEGURI_SYNC_PORT" docker __compose "$@"
  }
  local package="$MG_ROOT/build/mg/sync-server-package/package-fixture"
  mkdir -p "$package/context" "$package/product"
  printf '{}\n' >"$package/context/package-inputs.json"
  printf '{}\n' >"$package/product/runtime-inventory.json"
  jq -n --arg root "$MG_ROOT" --arg commit "$(printf '%040d' 1)" \
    --arg image "sha256:$(printf '%064d' 2)" --arg client "sha256:$(printf '%064d' 3)" \
    --arg hash "$(shasum -a 256 "$package/context/package-inputs.json" | awk '{print $1}')" '
    {job:"sync-server-package",status:"pass",exit_code:0,failures:[],dry_run:false,args:[],
     run_id:"package-fixture",root:$root,commit:{sha:$commit,dirty:false},details:{package:{
       source_commit:$commit,image_id:$image,client_image_id:$client,features:[],profile:"release",
       inputs_sha256:$hash,runtime_inventory_sha256:$hash}}}' >"$package/result.json"
  ln -s package-fixture "$MG_ROOT/build/mg/sync-server-package/latest"
  export MG_RUN_ID
  cat >"$BATS_TEST_TMPDIR/bin/docker" <<'PY'
#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys
r = pathlib.Path(os.environ['MOBILE_FAKE'])
a = sys.argv[1:]
mode = os.environ['MOBILE_MODE']
tool = 'sha256:' + '%064d' % 1
backend = 'sha256:' + '%064d' % 2
client = 'sha256:' + '%064d' % 3
ref = 'cloudflare/cloudflared:2026.11.0@sha256:b94b96edf2c9c4360f8879c8580cfaa99077f19c849ec595ca2f711b27d4f344'
def fail(): raise SystemExit(97)
def load(p): return json.loads(p.read_text())
def save(p, v): p.write_text(json.dumps(v))
def emit(v): print(json.dumps(v))
def option(args, key, default=None):
    return args[args.index(key)+1] if key in args else default
def kind(name):
    for k, prefix in [('tunnel','meguri-mobile-tunnel-'), ('tool','meguri-mobile-tool-'),
                      ('probe','meguri-mobile-probe-'), ('service','meguri-local-'), ('init','meguri-init-')]:
        if name.startswith(prefix): return k
    fail()
def container(selector):
    for p in r.glob('c-*'):
        c = load(p)
        if selector in (c['Id'], c['Name'].lstrip('/')): return p, c
    fail()
def replace_service(p, c, when):
    trigger = r/'replace-service'
    if not trigger.exists(): return
    phase, field = trigger.read_text().split(':')
    if phase != when: return
    trigger.unlink()
    if field=='cid': c['Id']='f'*64
    elif field=='created': c['Created']='replacement-created'
    elif field=='config': c['Config']['Cmd']=['foreign']
    elif field=='network': next(iter(c['NetworkSettings']['Networks'].values()))['NetworkID']='d'*64
    else: fail()
    # Deliberately retain the original name and BOTH ownership labels.
    save(p,c)
    (r/'sequence').open('a').write('replace-service\n')
with (r/'calls').open('a') as out: out.write(a[0] + '\n')
with (r/'argv').open('a') as out: out.write(json.dumps(a) + '\n')
if a[0] == 'pull':
    assert a == ['pull', ref]
elif a[:2] == ['image', 'inspect']:
    assert a[2] == ref
    digest = ref.split(':2026')[0]+'@'+ref.split('@')[1] if mode!='tool-digest' else 'foreign@sha256:'+'f'*64
    emit([{'Id':tool,'Os':'linux','Architecture':'arm64','RepoDigests':[digest],
           'Config':{'User':'65532:65532','Entrypoint':['cloudflared'],'Cmd':['version'],
                     'WorkingDir':'/home/nonroot','Env':['NO_AUTOUPDATE=true',
                     'SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt']}}])
elif a[0]=='ps':
    assert a[1:3]==['-aq','--filter']
    run = option(a,'--filter').removeprefix('label=meguri.mg.run-id=')
    (r/'sequence').open('a').write('run-inventory\n')
    for p in r.glob('c-*'):
        c=load(p)
        if c['Config']['Labels']['meguri.mg.run-id']==run: print(c['Id'])
elif a[:2] == ['network', 'ls']:
    if (r/'network').exists(): print(load(r/'network')['Id'])
elif a[:2] == ['network', 'create']:
    assert a[2:4] == ['--driver','bridge']
    labels = dict(a[i+1].split('=',1) for i,x in enumerate(a) if x=='--label')
    name = a[-1]
    assert name == 'meguri-mobile-' + os.environ['MG_RUN_ID']
    save(r/'network', {'Id':'e'*64,'Name':name,'Created':'fixed-network-created','Driver':'bridge',
         'Internal':False,'Labels':labels,'Options':{},'IPAM':{'Driver':'default'},'Containers':{}})
    print('e'*64)
elif a[:2] == ['network','inspect']:
    n = load(r/'network')
    assert a[2] == n['Id']
    n['Containers'] = {load(p)['Id']:{} for p in r.glob('c-*') if load(p)['State']['Running']}
    if mode=='foreign-network': n['Labels']['meguri.sync.owner']='foreign'
    emit([n])
elif a[:2] == ['network','rm']:
    assert a[2] == load(r/'network')['Id']
    assert not list(r.glob('c-*'))
    (r/'network').unlink()
elif a[:2] == ['volume','ls']:
    if (r/'volume').exists(): print(load(r/'volume')['Name'])
elif a[:2] == ['volume','create']:
    assert a[-1] == 'meguri-sync-data-' + os.environ['MG_RUN_ID'] + '-mobile-smoke'
    labels = dict(a[i+1].split('=',1) for i,x in enumerate(a) if x=='--label')
    save(r/'volume', {'Name':a[-1], 'Labels':labels})
    print(a[-1])
elif a[:2] == ['volume','inspect']:
    assert a[2] == load(r/'volume')['Name']
    emit([load(r/'volume')])
elif a[:2] == ['container','ls']:
    expression = option(a, '--filter').removeprefix('name=^/').removesuffix('$')
    for p in r.glob('c-*'):
        c = load(p)
        if c['Name'].lstrip('/') == expression: print(c['Id'])
elif a[0] in ('create','__compose'):
    compose = a[0]=='__compose'
    name = option(a, '--name'); k = kind(name)
    labels = dict(a[i+1].split('=',1) for i,x in enumerate(a) if x=='--label')
    if compose:
        labels['meguri.sync.owner'] = os.environ['MEGURI_SYNC_OWNER']
        image = os.environ['MEGURI_SYNC_IMAGE']; assert image==backend
        network = os.environ['MEGURI_SYNC_NETWORK']; assert network==load(r/'network')['Name']
        origin = os.environ['MEGURI_SYNC_ORIGIN']; peer = os.environ['MEGURI_SYNC_TRUSTED_PEER']
        assert origin=='https://owned-phone.trycloudflare.com' and peer=='172.29.0.2'
        assert os.environ['MEGURI_SYNC_VOLUME']==load(r/'volume')['Name']
        assert os.environ['MEGURI_SYNC_PORT']=='18449'
        if k=='init':
            assert a[-4:] == ['local-sync','init','/data/owner',origin]
            (r/'sequence').open('a').write('init\n')
            print('synthetic-private-setup-sentinel')
            raise SystemExit(1 if mode=='init-failure' else 0)
        assert k=='service' and '--service-ports' in a and '--pull' in a
        cmd = ['serve','/data/owner',origin,'0.0.0.0:8080','--trusted-peer',peer]
        user='10001:10001'; entry=['/usr/local/bin/meguri-sync-server']
        mounts=[{'Type':'volume','Name':load(r/'volume')['Name'],'Destination':'/data'}]
        ports={'8080/tcp':[{'HostIp':'127.0.0.1','HostPort':'18449'}]}
    else:
        assert '--mount' not in a and '-v' not in a and '-p' not in a and '--publish' not in a
        assert '--read-only' in a and option(a,'--cap-drop')=='ALL'
        assert option(a,'--security-opt')=='no-new-privileges'
        assert option(a,'--pids-limit')=='64' and option(a,'--memory')=='128m'
        network = option(a,'--network'); user=option(a,'--user')
        image = client if k=='probe' else tool
        assert image in a
        cmd = a[a.index(image)+1:]; mounts=[]; ports={}
        entry=['node'] if k=='probe' else ['cloudflared']
        if k=='tool': assert network=='none' and cmd==['--version']
        else: assert network==load(r/'network')['Name']
        if k=='tunnel':
            assert cmd==['tunnel','--no-autoupdate','--url','http://meguri-local-'+os.environ['MG_RUN_ID']+':8080']
            assert user=='65532:65532'
            assert option(a,'--tmpfs')=='/tmp:uid=65532,gid=65532,mode=0700,size=16m'
        if k=='probe':
            assert option(a,'--entrypoint')=='node' and user=='10001:10001'
            assert cmd[:2]==['--input-type=module','-e']
            assert 'redirect:"error"' in cmd[2] and 'account_substrate' in cmd[2]
            assert cmd[-2:]==['http://meguri-local-'+os.environ['MG_RUN_ID']+':8080','https://owned-phone.trycloudflare.com']
    cid = hashlib.sha256(name.encode()).hexdigest()
    c={'Id':cid,'Name':'/'+name,'Created':'created-'+name,'Image':image,
       'State':{'Running':compose,'ExitCode':0},
       'Config':{'Image':image,'User':user,'Entrypoint':entry,'Cmd':cmd,'Labels':labels,
                 'WorkingDir':'/home/nonroot' if image==tool else '/',
                 'Env':['NO_AUTOUPDATE=true','SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt'] if image==tool else []},
       'HostConfig':{'ReadonlyRootfs':True,'CapDrop':['ALL'],'SecurityOpt':['no-new-privileges'],
                     'NetworkMode':network,'PortBindings':ports,'Memory':134217728,'PidsLimit':64},
       'Mounts':mounts,'NetworkSettings':{'Networks':{}}}
    if network!='none':
        c['NetworkSettings']['Networks'][network]={'NetworkID':'e'*64,'IPAddress':'172.29.0.2' if k=='tunnel' else '172.29.0.3',
          'EndpointID':'a'*64,'Gateway':'172.29.0.1','IPPrefixLen':16,'MacAddress':'02:42:ac:1d:00:03',
          'IPv6Gateway':'','GlobalIPv6Address':'','GlobalIPv6PrefixLen':0,'Aliases':[name]}
    save(r/('c-'+name),c)
    (r/'sequence').open('a').write('create-'+k+'\n')
    print(cid)
    if compose and mode=='start-failure': raise SystemExit(1)
elif a[0]=='inspect':
    observed=[]
    for selector in a[1:]:
        p,c=container(selector); k=kind(c['Name'].lstrip('/'))
        if k=='tunnel':
            if mode=='bad-argv': c['Config']['Cmd']=['tunnel','--url','http://foreign:8080']
            if mode=='bad-mount': c['Mounts']=[{'Type':'bind','Destination':'/personal'}]
            if mode=='bad-peer': next(iter(c['NetworkSettings']['Networks'].values()))['IPAddress']='127.0.0.1'
        if k=='service' and mode=='public-publish': c['HostConfig']['PortBindings']['8080/tcp'][0]['HostIp']='0.0.0.0'
        if k=='tunnel' and (r/'corrupt').exists():
            field=(r/'corrupt').read_text()
            if field=='owner': c['Config']['Labels']['meguri.sync.owner']='foreign'
            if field=='cid': c['Id']='f'*64
            if field=='created': c['Created']='replaced'
            if field=='config': c['Config']['Cmd']=['foreign']
            if field=='peer': next(iter(c['NetworkSettings']['Networks'].values()))['IPAddress']='172.29.0.9'
        observed.append(c)
    emit(observed)
    if len(a)==2 and k=='service': replace_service(p,load(p),'before-stop')
elif a[0]=='start':
    p,c=container(a[-1]); k=kind(c['Name'].lstrip('/'))
    c['State']['Running']=k=='tunnel'; save(p,c)
    (r/'sequence').open('a').write('start-'+k+'\n')
    if k=='tool': print('cloudflared version 2026.11.0 (synthetic policy double)')
    if k=='probe' and mode=='route-failure': raise SystemExit(1)
elif a[0]=='logs':
    p,c=container(a[-1]); k=kind(c['Name'].lstrip('/'))
    if k=='service': print('listening 0.0.0.0:8080',flush=True)
    elif k=='tunnel':
        if mode=='duplicate-origin': print('https://owned-phone.trycloudflare.com https://second.trycloudflare.com')
        elif mode=='bad-origin': print('https://owned-phone.trycloudflare.com.evil.invalid')
        else: print('https://owned-phone.trycloudflare.com')
    else: fail()
elif a[0]=='stop':
    assert a[1:3] in (['--time','30'],['-t','10'])
    for selector in a[3:]:
        p,c=container(selector); k=kind(c['Name'].lstrip('/'))
        (r/'sequence').open('a').write('stop-'+k+'\n')
        if mode=='stop-failure': raise SystemExit(1)
        c['State']['Running']=False
        for n in c['NetworkSettings']['Networks'].values():
            n.update(EndpointID='',Gateway='',IPAddress='',IPPrefixLen=0,MacAddress='',
                     IPv6Gateway='',GlobalIPv6Address='',GlobalIPv6PrefixLen=0)
        save(p,c)
        if k=='service': replace_service(p,c,'before-remove')
elif a[0]=='rm':
    for selector in (a[2:] if a[1]=='-f' else a[1:]):
        p,c=container(selector); k=kind(c['Name'].lstrip('/'))
        if a[1]!='-f': assert not c['State']['Running']
        (r/'sequence').open('a').write('remove-'+k+'\n')
        if mode=='cleanup-failure' and k=='tunnel': raise SystemExit(1)
        if mode!='absence-failure' or k!='tunnel': p.unlink()
else: fail()
PY
  chmod 700 "$BATS_TEST_TMPDIR/bin/docker"
}

invoke() {
  MG_RUN_ID="policy-$1"
  MG_RUN_DIR="$BATS_TEST_TMPDIR/run-$1"
  mkdir -p "$MG_RUN_DIR"
  MG_EXIT_CMDS=()
  mg_sync_mobile_smoke "$1"
  mg__run_exit_cmds
  [ "$(cat "$MG_RUN_DIR/status")" = pass ]
}

prepare_start() {
  invoke prepare && invoke start
}

# Run the actual MG dispatcher, mg__run_job cancellation branch and fixed-job
# cleanup. Only process interruption/delivery and Docker are policy doubles;
# the original utility, receipt validator and local helpers still run.
cancel_through_mg() {
  mkdir -p "$MG_ROOT/tools/mg.d"
  cp "$REPO_ROOT/tools/mg" "$MG_ROOT/tools/mg"
  local family
  for family in host meguri-sync sync-package sync-mobile-smoke; do
    cp "$REPO_ROOT/tools/mg.d/$family.sh" "$MG_ROOT/tools/mg.d/"
  done
  local fixture="$MG_ROOT/tools/mg.d/zz-mobile-process-fixture.sh"
  declare -f git mg_compose >"$fixture"
  cat >>"$fixture" <<'SH'
wait() {
  local rc=0
  builtin wait "$@" || rc=$?
  # Deliver modeled cancellation after start has produced fully owned children,
  # including the normal retained marker and completed child exit actions.
  printf 'wait\n' >>"$MOBILE_FAKE/process"
  MG_INTERRUPTED=SIGTERM
  if [ "$MOBILE_CANCEL_MODE" = owner ]; then
    printf owner >"$MOBILE_FAKE/corrupt"
  else
    export MOBILE_MODE="$MOBILE_CANCEL_MODE"
  fi
  return "$rc"
}
mg__descendants() {
  printf 'descendants\n' >>"$MOBILE_FAKE/process"
  printf '424242\n'
}
kill() {
  printf '%s\n' "$*" >>"$MOBILE_FAKE/process"
  case "$1" in
    -TERM) [ "$#" -eq 2 ] && [ "$2" = 424242 ] ;;
    -0) return 1 ;;
    *) return 97 ;;
  esac
}
SH
  run "$MG_ROOT/tools/mg" sync-server-mobile-smoke start
}

@test "mobile-smoke: real MG cancellation never sweeps past strict ownership or tunnel absence refusal" {
  local mode result
  for mode in owner cleanup-failure absence-failure; do
    export MOBILE_CANCEL_MODE="$mode"
    # Fresh synthetic fixture per cancellation, never real lifecycle records.
    rm -rf "$MG_ROOT/build/sync-server-mobile-smoke"
    rm -f "$MOBILE_FAKE"/c-* "$MOBILE_FAKE/network" "$MOBILE_FAKE/volume" \
      "$MOBILE_FAKE/sequence" "$MOBILE_FAKE/corrupt"
    invoke prepare
    cancel_through_mg
    [ "$status" -eq 2 ]
    result="$(printf '%s\n' "$output" | tail -n 1)"
    jq -e '.status=="error" and
      any(.failures[]; .id=="cancelled") and any(.failures[]; .id=="mobile-cleanup")' "$result"
    grep -qx wait "$MOBILE_FAKE/process"
    grep -qx descendants "$MOBILE_FAKE/process"
    grep -qx -- '-TERM 424242' "$MOBILE_FAKE/process"
    # The strict inventory may observe; the outer generic sweep must never run.
    [ "$(grep -cx run-inventory "$MOBILE_FAKE/sequence")" -eq 1 ]
    ! grep -qx stop-service "$MOBILE_FAKE/sequence"
    ! grep -qx stop-tunnel "$MOBILE_FAKE/sequence"
    ! grep -qx remove-service "$MOBILE_FAKE/sequence"
    [ -e "$MOBILE_FAKE/network" ]
    [ -e "$MOBILE_FAKE/volume" ]
    [ -e "$MG_ROOT/build/sync-server-mobile-smoke/service-container.json" ]
    [ ! -e "$MG_ROOT/build/sync-server-mobile-smoke/stopped" ]
    [[ "$output" != *synthetic-private-setup-sentinel* ]]
  done
}

@test "mobile-smoke: real MG cancellation closes tunnel before CID-bound backend disposal" {
  invoke prepare
  export MOBILE_CANCEL_MODE=valid
  cancel_through_mg
  [ "$status" -eq 2 ]
  local result
  result="$(printf '%s\n' "$output" | tail -n 1)"
  jq -e '.status=="error" and any(.failures[]; .id=="cancelled") and
    all(.failures[]; .id!="mobile-cleanup")' "$result"
  [ "$(tail -n 4 "$MOBILE_FAKE/sequence" | tr '\n' ' ')" = 'remove-tunnel stop-service remove-service run-inventory ' ]
  [ "$(grep -cx run-inventory "$MOBILE_FAKE/sequence")" -eq 1 ]
  [ -z "$(find "$MOBILE_FAKE" -name 'c-*' -print)" ]
  [ ! -e "$MOBILE_FAKE/network" ]
  [ -e "$MOBILE_FAKE/volume" ]
  [ -e "$MG_ROOT/build/sync-server-mobile-smoke/stopped" ]
}

@test "mobile-smoke: backend replacement after admission retains labels but is never signalled" {
  local mutation
  for mutation in cid created config; do
    rm -rf "$MG_ROOT/build/sync-server-mobile-smoke"
    rm -f "$MOBILE_FAKE"/c-* "$MOBILE_FAKE/network" "$MOBILE_FAKE/volume" "$MOBILE_FAKE/sequence"
    prepare_start
    printf 'before-stop:%s' "$mutation" >"$MOBILE_FAKE/replace-service"
    run invoke stop
    [ "$status" -ne 0 ]
    grep -qx replace-service "$MOBILE_FAKE/sequence"
    ! grep -qx stop-service "$MOBILE_FAKE/sequence"
    ! grep -qx remove-service "$MOBILE_FAKE/sequence"
    [ -e "$MOBILE_FAKE/c-meguri-local-policy-start" ]
    [ -e "$MOBILE_FAKE/network" ]
    [ -e "$MOBILE_FAKE/volume" ]
    [ ! -e "$MG_ROOT/build/sync-server-mobile-smoke/stopped" ]
  done
}

@test "mobile-smoke: backend binding is rechecked after stop before CID removal" {
  local mutation
  for mutation in cid config network; do
    rm -rf "$MG_ROOT/build/sync-server-mobile-smoke"
    rm -f "$MOBILE_FAKE"/c-* "$MOBILE_FAKE/network" "$MOBILE_FAKE/volume" "$MOBILE_FAKE/sequence"
    prepare_start
    printf 'before-remove:%s' "$mutation" >"$MOBILE_FAKE/replace-service"
    run invoke stop
    [ "$status" -ne 0 ]
    grep -qx stop-service "$MOBILE_FAKE/sequence"
    grep -qx replace-service "$MOBILE_FAKE/sequence"
    ! grep -qx remove-service "$MOBILE_FAKE/sequence"
    [ -e "$MOBILE_FAKE/c-meguri-local-policy-start" ]
    [ -e "$MOBILE_FAKE/network" ]
    [ ! -e "$MG_ROOT/build/sync-server-mobile-smoke/stopped" ]
  done
}

@test "mobile-smoke: actual receipt validator rejects a stale latest before Docker" {
  local receipt="$MG_ROOT/build/mg/sync-server-package/package-fixture/result.json"
  jq '.commit.dirty=true' "$receipt" >"$receipt.new"
  mv "$receipt.new" "$receipt"
  run invoke prepare
  [ "$status" -ne 0 ]
  [ ! -e "$MOBILE_FAKE/calls" ]
}

@test "mobile-smoke: MG dry-run covers every fixed mode without Docker" {
  local mode result
  for mode in prepare start status stop; do
    run "$MG_TOOLS/mg" sync-server-mobile-smoke --dry-run "$mode"
    [ "$status" -eq 0 ]
    result="$output"
    jq -e --arg mode "$mode" '.dry_run==true and .status=="pass" and .args==[$mode]' "$result"
  done
  [ ! -e "$MOBILE_FAKE/calls" ]
}

@test "mobile-smoke: prepare refuses a digest outside the reviewed manifest" {
  export MOBILE_MODE=tool-digest
  run invoke prepare
  [ "$status" -ne 0 ]
  [ ! -e "$MOBILE_FAKE/sequence" ]
}

@test "mobile-smoke: fixed pinned tool and actual local helpers preserve topology and private handoff" {
  run prepare_start
  [ "$status" -eq 0 ]
  [[ "$output" != *synthetic-private-setup-sentinel* ]]
  grep -qx synthetic-private-setup-sentinel "$MG_ROOT/build/sync-server-mobile-smoke/setup.txt"
  [ "$(head -n 5 "$MOBILE_FAKE/sequence" | tr '\n' ' ')" = 'create-tool start-tool remove-tool create-tunnel start-tunnel ' ]
  grep -qx init "$MOBILE_FAKE/sequence"
  grep -qx create-service "$MOBILE_FAKE/sequence"
  grep -qx start-probe "$MOBILE_FAKE/sequence"
  [ ! -e "$MG_ROOT/build/sync-server-local" ]
  run invoke status
  [ "$status" -eq 0 ]
  run invoke start
  [ "$status" -ne 0 ]
  run invoke stop
  [ "$status" -eq 0 ]
  [ "$(tail -n 3 "$MOBILE_FAKE/sequence" | tr '\n' ' ')" = 'remove-tunnel stop-service remove-service ' ]
  [ ! -e "$MOBILE_FAKE/network" ]
  [ -e "$MOBILE_FAKE/volume" ]
  run invoke status
  [ "$status" -eq 0 ]
  ! grep -R synthetic-private-setup-sentinel "$BATS_TEST_TMPDIR"/run-*
}

@test "mobile-smoke: generated duplicate and malformed origins never initialize a service" {
  invoke prepare
  for mode in duplicate-origin bad-origin; do
    export MOBILE_MODE="$mode"
    # Each policy case has a fresh fixture, as real retained roots refuse reuse.
    rm -rf "$MG_ROOT/build/sync-server-mobile-smoke"
    invoke prepare
    run invoke start
    [ "$status" -ne 0 ]
    ! grep -qx init "$MOBILE_FAKE/sequence"
    [ ! -e "$MOBILE_FAKE/network" ]
    [ -z "$(find "$MOBILE_FAKE" -name 'c-*' -print)" ]
  done
}

@test "mobile-smoke: init, partial startup and route failures close tunnel first and preserve volume" {
  for mode in init-failure start-failure route-failure; do
    export MOBILE_MODE="$mode"
    rm -rf "$MG_ROOT/build/sync-server-mobile-smoke"
    rm -f "$MOBILE_FAKE/sequence"
    invoke prepare
    run invoke start
    [ "$status" -ne 0 ]
    [[ "$output" != *synthetic-private-setup-sentinel* ]]
    [ -e "$MOBILE_FAKE/volume" ]
    [ ! -e "$MOBILE_FAKE/network" ]
    [ -z "$(find "$MOBILE_FAKE" -name 'c-*' -print)" ]
    if [ "$mode" != init-failure ]; then
      [ "$(tail -n 3 "$MOBILE_FAKE/sequence" | tr '\n' ' ')" = 'remove-tunnel stop-service remove-service ' ]
    fi
    rm -f "$MOBILE_FAKE/volume"
  done
}

@test "mobile-smoke: actual argv, mounts, peer and loopback-only publish are admitted before success" {
  for mode in bad-argv bad-mount bad-peer public-publish; do
    export MOBILE_MODE="$mode"
    rm -rf "$MG_ROOT/build/sync-server-mobile-smoke"
    rm -f "$MOBILE_FAKE/sequence"
    invoke prepare
    run invoke start
    [ "$status" -ne 0 ]
    [ ! -e "$MOBILE_FAKE/network" ]
    [ -z "$(find "$MOBILE_FAKE" -name 'c-*' -print)" ]
    if [ "$mode" != public-publish ]; then ! grep -qx init "$MOBILE_FAKE/sequence"; fi
    rm -f "$MOBILE_FAKE/volume"
  done
}

@test "mobile-smoke: changed owner, CID, Created, config or actual peer refuses status and cleanup" {
  prepare_start
  for mutation in owner cid created config peer; do
    printf '%s' "$mutation" >"$MOBILE_FAKE/corrupt"
    run invoke status
    [ "$status" -ne 0 ]
    run invoke stop
    [ "$status" -ne 0 ]
    ! grep -qx remove-tunnel "$MOBILE_FAKE/sequence"
    ! grep -qx stop-service "$MOBILE_FAKE/sequence"
  done
  rm "$MOBILE_FAKE/corrupt"
  invoke stop
}

@test "mobile-smoke: cleanup failure or failed absence proof never signals backend" {
  prepare_start
  for mode in cleanup-failure absence-failure; do
    export MOBILE_MODE="$mode"
    run invoke stop
    [ "$status" -ne 0 ]
    ! grep -qx stop-service "$MOBILE_FAKE/sequence"
    [ -e "$MOBILE_FAKE/network" ]
  done
  export MOBILE_MODE=stop-failure
  run invoke stop
  [ "$status" -ne 0 ]
  [ ! -e "$MOBILE_FAKE/c-meguri-mobile-tunnel-policy-start" ]
  [ -e "$MOBILE_FAKE/c-meguri-local-policy-start" ]
  export MOBILE_MODE=valid
  invoke stop
}

@test "mobile-smoke: occupied fixed loopback port refuses without creating the bridge" {
  invoke prepare
  # Model the host socket admission only; all other Python helpers remain real.
  python3() {
    if [ "$#" -eq 1 ] && [ "$1" = - ]; then return 1; fi
    command python3 "$@"
  }
  run invoke start
  [ "$status" -ne 0 ]
  ! grep -qx create-tunnel "$MOBILE_FAKE/sequence"
  [ ! -e "$MOBILE_FAKE/network" ]
}
