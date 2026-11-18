#!/usr/bin/env bats
# Exact input-byte binding for the isolated reference observation lane.
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "recovery and final neutral inputs affect the reference digest" {
  local sandbox="$BATS_TEST_TMPDIR/recovery-inputs" path before after_fixture after_selection after_dirty_fixture after_dirty_selection after_final
  # Never copy quarantine bodies; this digest input is wholly synthetic.
  mkdir -p "$sandbox/oracle/meguri-sync-server"
  printf 'synthetic reference digest placeholder\n' >"$sandbox/oracle/meguri-sync-server/synthetic.txt"
  # Copy only public digest inputs, without requiring Git metadata.
  for path in docker/meguri-sync-reference.Dockerfile \
    docker/meguri-sync-reference.Dockerfile.dockerignore docker/meguri-sync-reference.compose.yml \
    tools/mg tools/mg.d/meguri-sync.sh tools/mg.d/meguri-sync-reference.sh conformance/schema/meguri-sync-server.schema.json \
    conformance/catalog/meguri-sync-server.json conformance/fixtures/meguri-sync-server/original.json \
    conformance/recordings/scenarios/meguri-sync-server/desktop-selection.json \
    conformance/fixtures/meguri-sync-server/edges.json conformance/recordings/scenarios/meguri-sync-server/desktop-edge-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-normal-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-media-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/desktop-media-selection.json \
    conformance/fixtures/meguri-sync-server/media.json \
    conformance/fixtures/meguri-sync-server/recovery.json \
    conformance/fixtures/meguri-sync-server/recovery-dirty.json \
    conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-dirty-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-full-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-complete-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-retained-final-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json; do
    mkdir -p "$sandbox/$(dirname "$path")"
    cp "$REPO_ROOT/$path" "$sandbox/$path"
  done
  # The host helper uses macOS tools; the dev image supplies equivalent primitives.
  rg() { [ "$1" = --files ]; shift; find "$@" -type f; }
  shasum() { [ "$1" = -a ] && [ "$2" = 256 ]; shift 2; sha256sum "$@"; }
  mg_job() { :; }
  source "$sandbox/tools/mg.d/meguri-sync-reference.sh"
  MG_ROOT="$sandbox"
  before="$(mg_sync_reference_digest)"
  printf '\n' >>"$sandbox/conformance/fixtures/meguri-sync-server/recovery.json"
  after_fixture="$(mg_sync_reference_digest)"
  [ "$before" != "$after_fixture" ]
  printf '\n' >>"$sandbox/conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-selection.json"
  after_selection="$(mg_sync_reference_digest)"
  [ "$after_fixture" != "$after_selection" ]
  printf '\n' >>"$sandbox/conformance/fixtures/meguri-sync-server/recovery-dirty.json"
  after_dirty_fixture="$(mg_sync_reference_digest)"
  [ "$after_selection" != "$after_dirty_fixture" ]
  printf '\n' >>"$sandbox/conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-dirty-selection.json"
  after_dirty_selection="$(mg_sync_reference_digest)"
  [ "$after_dirty_fixture" != "$after_dirty_selection" ]
  # Mutate each final metadata input independently from the same baseline.
  for path in tools/mg tools/mg.d/meguri-sync.sh \
    conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-retained-final-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json; do
    printf '{"synthetic_digest_probe":true}\n' >"$sandbox/$path"
    after_final="$(mg_sync_reference_digest)"
    [ "$after_dirty_selection" != "$after_final" ]
    cp "$REPO_ROOT/$path" "$sandbox/$path"
    [ "$after_dirty_selection" = "$(mg_sync_reference_digest)" ]
  done
  # The same real pipeline must refuse a missing required Mobile input.
  path=conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json
  rm "$sandbox/$path"
  run mg_sync_reference_digest
  [ "$status" -ne 0 ]
  cp "$REPO_ROOT/$path" "$sandbox/$path"
  [ "$after_dirty_selection" = "$(mg_sync_reference_digest)" ]
}

# Run the real wrapper, image resolver, measurement predicates and removal
# helper. Only Docker/Compose, source identity and MG receipt sinks are faked.
# The fake inspection is derived from the actual create arguments, not from a
# pre-approved inspection fixture, so missing launch controls fail the tests.
reference_execution_fixture() {
  REF="$BATS_TEST_TMPDIR/reference-$1"
  mkdir -p "$REF/root/build/input" "$REF/run"
  cat >"$REF/docker.py" <<'PY'
import copy, datetime, json, os, pathlib, sys
root = pathlib.Path(os.environ['REF'])
args = sys.argv[1:]
with (root/'docker.jsonl').open('a') as stream:
    stream.write(json.dumps(args)+'\n')
image = 'sha256:'+'1'*64
cid = '2'*64
def emit(value):
    print(json.dumps(value))
def load():
    return json.loads((root/'container.json').read_text())
def save(value):
    (root/'container.json').write_text(json.dumps(value))
def fail(code):
    raise SystemExit(code)
defect = os.environ.get('DEFECT', '')
if args[:4] == ['image','inspect','--format','{{.Id}}']:
    assert args[4:] == ['configured-reference-tag']
    # Rebind immediately after resolution. Create must still use image 1.
    (root/'tag-now').write_text('sha256:'+'9'*64)
    print('bad-image' if defect == 'invalid-image' else image)
elif args[0] == 'create':
    assert not (root/'create-seen').exists()
    (root/'create-seen').touch()
    assert (root/'hooks').read_text().strip() == 'mg_sync_reference__cancel_cleanup'
    record = json.loads((root/'run/reference-control/owner.json').read_text())
    options, index = {}, 1
    flags = {'--init','--read-only'}
    while args[index].startswith('--'):
        key = args[index]
        value = True if key in flags else args[index+1]
        options.setdefault(key, []).append(value)
        index += 1 if key in flags else 2
    assert set(options) == {'--pull','--name','--label','--user','--env','--network','--init',
                            '--read-only','--cap-drop','--security-opt','--memory','--pids-limit','--tmpfs','--mount'}
    assert options['--pull'] == ['never']
    assert args[index:] == [image, os.environ['MODE']]
    assert options['--name'] == [record['name']]
    labels = dict(item.split('=',1) for item in options['--label'])
    assert labels == {'meguri.mg.run-id':record['run'],'meguri.sync.owner':record['nonce']}
    mount = dict(item.split('=',1) for item in options['--mount'][0].split(','))
    assert mount == {'type':'bind','src':str(root/'run/reference-output'),'dst':'/out'}
    assert not (pathlib.Path(mount['src'])/'reference-control').exists()
    created = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ')
    tmpfs = dict(item.split(':',1) for item in options['--tmpfs'])
    value = dict(Id=cid, Image=args[index], Name='/'+options['--name'][0], Created=created,
      Path='python', Args=['/opt/reference/driver.py',args[index+1]], RestartCount=0,
      Config=dict(Image=args[index], Entrypoint=['python','/opt/reference/driver.py'], Cmd=[args[index+1]],
        User=options['--user'][0], WorkingDir='/scratch', ExposedPorts=None, Volumes=None,
        Env=['NOT_PUBLIC=private-fixture-value']+options['--env'],Labels=labels),
      HostConfig=dict(NetworkMode=options['--network'][0],PortBindings={},PublishAllPorts=False,
        Init=options['--init'][0],ReadonlyRootfs=options['--read-only'][0],CapDrop=options['--cap-drop'],
        CapAdd=None,SecurityOpt=options['--security-opt'],Memory=2147483648 if options['--memory']==['2g'] else 0,
        OomKillDisable=False,PidsLimit=int(options['--pids-limit'][0]),AutoRemove=False,RestartPolicy=dict(Name='no',MaximumRetryCount=0),
        Privileged=False,Devices=[],DeviceRequests=None,VolumesFrom=None,Links=None,ExtraHosts=None,
        PidMode='',IpcMode='private',UTSMode='',UsernsMode='',Binds=None,
        Mounts=[dict(Type=mount['type'],Source=mount['src'],Target=mount['dst'])],Tmpfs=tmpfs),
      Mounts=[dict(Type=mount['type'],Source=mount['src'],Destination=mount['dst'],RW=True)],
      NetworkSettings=dict(Ports={},Networks={}),
      State=dict(Status='created',Running=False,Paused=False,Restarting=False,Dead=False,OOMKilled=False,
        ExitCode=0,Error='',StartedAt='0001-01-01T00:00:00Z',FinishedAt='0001-01-01T00:00:00Z'))
    # Synthetic raw input for direct tests of the real inspection producer.
    (root/'inspection.json').write_text(json.dumps([value]))
    if defect != 'create-absent':
        save(value)
    if defect in ('create-uncertain','create-absent'):
        fail(19)
    print(cid)
elif args[0] == 'inspect':
    assert args[1:] == [cid]
    started = (root/'started').exists()
    if defect == ('post-inspect-fail' if started else 'pre-inspect-fail'):
        fail(51)
    value = copy.deepcopy(load())
    inspection_fault = defect == 'raw-inspect' or defect == (
        'post-inspect-output-fail' if started else 'pre-inspect-output-fail')
    if inspection_fault and (root/'inspect-fault-emitted').exists():
        # Only the selected measurement is damaged. A later cleanup inspection
        # sees the real synthetic ownership, without overwriting fault bytes.
        emit([value])
        fail(0)
    mutation_phase = os.environ.get('MUTATION_PHASE','before')
    if mutation_phase == 'both' or started == (mutation_phase == 'after'):
        exec(os.environ.get('MUTATION',''), {}, {'c':value})
    if defect == 'numeric-token':
        # Inject the original token without any test-side numeric conversion.
        value['Config']['Healthcheck'] = {'Interval':'SYNTHETIC_JSON_NUMBER','Test':['NONE']}
        raw = json.dumps([value]).replace('"SYNTHETIC_JSON_NUMBER"', os.environ['NUMBER_TOKEN']).encode()+b'\n'
    else:
        raw = json.dumps([value]).encode()+b'\n'
    if defect == 'inspect-whitespace':
        raw = b' \r\n\t'+raw+b' \t\r\n\n'
    if defect == 'raw-inspect':
        raw = raw.replace(b'private-fixture-value', b'SYNTHETIC_RAW_VALUE')
        damage = os.environ['RAW_DEFECT']
        if damage == 'nul-prefix': raw = b'\x00'+raw
        elif damage == 'nul-key': raw = raw.replace(b'"Config"', b'"Con\x00fig"')
        elif damage == 'nul-string': raw = raw.replace(b'SYNTHETIC_RAW_VALUE', b'SYNTHETIC_\x00RAW_VALUE')
        elif damage == 'nul-integer': raw = raw.replace(b'2147483648', b'2147\x00483648')
        elif damage == 'nul-between': raw = raw.replace(b'[{', b'[\x00{')
        elif damage == 'nul-suffix': raw += b'\x00'
        elif damage == 'invalid-utf8': raw = raw.replace(b'SYNTHETIC_RAW_VALUE', b'\xffSYNTHETIC_RAW_VALUE')
        elif damage == 'malformed': raw = raw[:-3]
        elif damage == 'duplicate': raw = raw.replace(b'"Path": "python"', b'"Path":"SYNTHETIC_RAW_VALUE","Path":"python"')
        elif damage == 'oversized':
            # A complete JSON object one byte beyond the existing bound. The
            # producer can finish, so this isolates consumer bounds refusal.
            raw = raw.replace(b'SYNTHETIC_RAW_VALUE', b'X'*(8*1024*1024+1-len(raw)+len(b'SYNTHETIC_RAW_VALUE')))
        else: fail(98)
    (root/'inspection.json').write_bytes(raw)
    (root/('inspection-after.json' if started else 'inspection-before.json')).write_bytes(raw)
    sys.stdout.buffer.write(raw)
    sys.stdout.buffer.flush()
    if inspection_fault:
        (root/'inspect-fault-emitted').touch()
    if defect == ('post-inspect-output-fail' if started else 'pre-inspect-output-fail'):
        fail(51)
elif args[0] == 'start':
    assert args == ['start','--attach',cid] and not (root/'started').exists()
    (root/'started').touch()
    value = load()
    assert value['State']['Status'] == 'created'
    value['State'].update(Status='exited',ExitCode=37 if defect in ('process','process-cleanup') else 0,
      StartedAt=value['Created'],FinishedAt=value['Created'])
    value['NetworkSettings']['Networks'] = {'none':{}}
    save(value)
    output = pathlib.Path(value['Mounts'][0]['Source'])
    (output/'summary.json').write_text('{"total":3,"passed":3,"failed":0}')
    if defect == 'invocation-mutated':
        (output/'invocation.json').write_text('{}')
    if defect == 'invocation-symlink':
        (output/'invocation.json').unlink()
        (output/'invocation.json').symlink_to('/unowned/never-read')
    fail(value['State']['ExitCode'])
elif args[:2] == ['container','ls']:
    assert args[2:5] == ['-aq','--no-trunc','--filter']
    record = json.loads((root/'run/reference-control/owner.json').read_text())
    assert args[5:] == ['name=^/'+record['name']+'$']
    if (root/'container.json').exists():
        print(cid)
elif args[:2] == ['rm','-f']:
    assert args[2:] == [cid]
    if defect in ('cleanup','process-cleanup'):
        fail(53)
    (root/'container.json').unlink()
else:
    fail(97)
PY
  cat >"$REF/run.sh" <<'SH'
set -u
mg_job() { :; }
. "$REPO_ROOT/tools/mg.d/meguri-sync.sh"
. "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
MG_ROOT="$REF/root" MG_RUN_DIR="$REF/run" MG_RUN_ID=reference-fixture
mg_on_exit() { printf '%s\n' "$1" >>"$REF/hooks"; }
mg_artifact() { :; }
mg_failure() { printf '%s\n' "$1" >>"$REF/failures"; }
mg_status() { printf '%s\n' "$1" >"$REF/status"; }
mg_count() { printf '%s=%s\n' "$1" "$2" >>"$REF/counts"; }
mg_detail() { jq -nc --arg key "$1" --argjson value "$2" '{key:$key,value:$value}' >>"$REF/details.jsonl"; }
docker() { python3 "$REF/docker.py" "$@"; }
mg_sync_reference_digest() {
  if [ "${DEFECT:-}" = source-post ] && [ -f "$REF/started" ]; then printf '%064d\n' 9; else printf '%064d\n' 1; fi
}
shasum() {
  case "${3:-}" in "$MG_ROOT/oracle/meguri-sync-server/requirements.lock") printf '%064d  metadata-only\n' 1 ;; *) command shasum "$@" ;; esac
}
git() {
  case "$3" in
    rev-parse) printf '%s\n' 3793b1880c5e906086c2bc9401c8ad47abfeb547 ;;
    show)
      local session
      for session in s-20261115T133013Z-d8cec007 s-20261115T211543Z-556b04e9 s-20261115T224434Z-376ca308 \
        s-20261115T231059Z-ff95725a s-20261116T001757Z-8fe546bc s-20261116T020051Z-5e05fa3f \
        s-20261116T092626Z-4487d759 s-20261116T124044Z-c9976196 s-20261116T225916Z-981c8d1b s-20261117T230521Z-4667d769; do
        jq -nc --arg session "$session" '{session:$session,role:"oracle-maintainer"}'
      done ;;
    *) return 97 ;;
  esac
}
mg_compose() {
  printf '%s\n' "$@" | jq -Rs 'split("\n")[:-1]' >>"$REF/compose.jsonl"
  case "$*" in
    *'config --images sync-reference') printf 'configured-reference-tag\n' ;;
    *'build sync-reference') : ;;
    *'run --rm -T --no-deps'*) printf '{"total":3,"passed":3,"failed":0}\n' >"$MG_RUN_DIR/summary.json" ;;
    *) return 97 ;;
  esac
}
export MG_RUN_ID
mg_sync_server_reference "$@"
SH
}

reference_execute() {
  run env REPO_ROOT="$REPO_ROOT" REF="$REF" MODE="$1" DEFECT="${2:-}" \
    MUTATION="${3:-}" MUTATION_PHASE="${4:-before}" bash "$REF/run.sh" "$1"
  printf '%s' "$output" >"$REF/wrapper.log"
  [ "$status" -eq 0 ] # MG jobs report status through the receipt sink.
}

# The real wrapper establishes a valid synthetic baseline first. Direct calls
# below replace only Docker stdout; producer and fingerprint predicates stay real.
reference_inspection_fixture() {
  reference_execution_fixture "$1"
  reference_execute identity
  [ "$(cat "$REF/status")" = pass ]
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  docker() { [ "$1" = inspect ] && cat "$REF/inspection.json"; }
}

@test "reference actual producer and predicates admit only stable empty optional config representations" {
  local mutation index=0 measured expected_hash
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  for mutation in '' "del c['Config']['ExposedPorts']" "del c['Config']['Volumes']" \
    "del c['Config']['ExposedPorts']; del c['Config']['Volumes']" \
    "c['Config']['ExposedPorts']={}; c['Config']['Volumes']={}"; do
    index=$((index+1))
    reference_execution_fixture "optional-stable-$index"
    reference_execute identity '' "$mutation" both
    [ "$(cat "$REF/status")" = pass ]
    [ -z "$output" ] && [ ! -e "$REF/failures" ] && [ ! -e "$REF/container.json" ]
    jq -se '[.[] | select(.[0]=="start")] | length==1' "$REF/docker.jsonl"
    jq -se --argjson variant "$index" 'map(select(.key=="reference_execution"))[0].value |
      .measurements_valid and .absence_verified and .execution_exit==0 and
      .before.property_fingerprints==.after.property_fingerprints and
      .before.configuration_sha256==.after.configuration_sha256 and
      (.before.property_fingerprints.Config as $c |
        ($c|has("ExposedPorts"))==($variant!=2 and $variant!=4) and
        ($c|has("Volumes"))==($variant!=3 and $variant!=4))' "$REF/details.jsonl"
    if [ "$index" -eq 1 ] || [ "$index" -eq 5 ]; then
      measured="$(jq -cs 'map(select(.key=="reference_execution"))[0].value.before' "$REF/details.jsonl")"
      # Known canonical bytes, independently hashed for both present forms.
      if [ "$index" -eq 1 ]; then
        expected_hash="$(printf null | shasum -a 256 | cut -d ' ' -f1)"
      else
        expected_hash="$(printf '{}' | shasum -a 256 | cut -d ' ' -f1)"
      fi
      jq -e --arg digest "$expected_hash" '.property_fingerprints.Config |
        .ExposedPorts==$digest and .Volumes==$digest' <<<"$measured"
    fi
  done
}

@test "reference OOM killer enabled comparison admits only stable false null and their transition" {
  local variant mutation phase before after plan damaged
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  for variant in false null false-null null-false; do
    case "$variant" in
      false) mutation="c['HostConfig']['OomKillDisable']=False"; phase=both ;;
      null) mutation="c['HostConfig']['OomKillDisable']=None"; phase=both ;;
      false-null) mutation="c['HostConfig']['OomKillDisable']=None"; phase=after ;;
      null-false) mutation="c['HostConfig']['OomKillDisable']=None"; phase=before ;;
    esac
    reference_execution_fixture "oom-enabled-$variant"
    reference_execute identity '' "$mutation" "$phase"
    [ "$(cat "$REF/status")" = pass ] && [ -z "$output" ]
    [ ! -e "$REF/failures" ] && [ ! -e "$REF/container.json" ]
    jq -se '[.[] | select(.[0]=="start")] | length==1' "$REF/docker.jsonl"
    jq -se '[.[] | select(.[0]=="rm")] | length==1' "$REF/docker.jsonl"
    python3 - "$REF" "$variant" <<'PY'
import hashlib, json, pathlib, sys
root, variant = pathlib.Path(sys.argv[1]), sys.argv[2]
execution, = [item['value'] for item in map(json.loads, (root/'details.jsonl').read_text().splitlines())
              if item['key'] == 'reference_execution']
assert execution['measurements_valid'] and execution['absence_verified']
assert all(execution[key] == 0 for key in ('attach_exit','actual_exit','cleanup_exit','execution_exit'))
def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',',':'), ensure_ascii=True, allow_nan=False).encode('ascii')
expected = {'false':(False,False), 'null':(None,None),
            'false-null':(False,None), 'null-false':(None,False)}[variant]
for phase, oom in zip(('before','after'), expected):
    raw = (root/('inspection-'+phase+'.json')).read_bytes()
    c = json.loads(raw)[0]
    measured = execution[phase]
    assert c['HostConfig']['OomKillDisable'] is oom
    assert measured['inspection_sha256'] == hashlib.sha256(raw).hexdigest()
    complete = {key:c[key] for key in ('Config','HostConfig','Path','Args')}
    assert measured['configuration_sha256'] == hashlib.sha256(canonical(complete)).hexdigest()
    assert measured['property_fingerprints'] == {
        **{section:{name:hashlib.sha256(canonical(value)).hexdigest() for name,value in c[section].items()}
           for section in ('Config','HostConfig')},
        **{name:hashlib.sha256(canonical(c[name])).hexdigest() for name in ('Path','Args')}}
    # Known independent property preimages; false/null raw hashes stay distinct.
    assert measured['property_fingerprints']['HostConfig']['OomKillDisable'] == hashlib.sha256(
        b'false' if oom is False else b'null').hexdigest()
    complete['HostConfig'] = dict(c['HostConfig'], OomKillDisable=False)
    assert measured['oom_killer_enabled_configuration_sha256'] == hashlib.sha256(
        b'meguri-oom-killer-enabled-configuration-v1\x00' + canonical(complete)).hexdigest()
    assert measured['oom_killer_enabled_configuration_sha256'] != measured['configuration_sha256']
b, a = execution['before'], execution['after']
transition = variant in ('false-null','null-false')
assert (b['configuration_sha256'] != a['configuration_sha256']) == transition
assert (b['property_fingerprints']['HostConfig']['OomKillDisable'] !=
        a['property_fingerprints']['HostConfig']['OomKillDisable']) == transition
assert b['oom_killer_enabled_configuration_sha256'] == a['oom_killer_enabled_configuration_sha256']
assert b['inspection_sha256'] != a['inspection_sha256']
assert 'NOT_PUBLIC' not in json.dumps(execution) and 'private-fixture-value' not in json.dumps(execution)
PY
    before="$(jq -cs 'map(select(.key=="reference_execution"))[0].value.before' "$REF/details.jsonl")"
    after="$(jq -cs 'map(select(.key=="reference_execution"))[0].value.after' "$REF/details.jsonl")"
    plan="$(jq -c '.container | {id:.Id,image:.Image,name:(.Name|ltrimstr("/")),nonce:.Config.Labels.nonce,
      run:.Config.Labels.run,mode:.Config.Cmd[0],output:.HostConfig.Mounts[0].Source,user:.Config.User,creation_floor:.Created[0:19]}' <<<"$before")"
    run mg_sync_reference__check "$before" "$plan" null before
    [ "$status" -eq 0 ]
    run mg_sync_reference__check "$after" "$plan" "$before" after
    [ "$status" -eq 0 ]
    damaged="$(jq -c '.oom_killer_enabled_configuration_sha256=("a"*64)' <<<"$after")"
    run mg_sync_reference__check "$damaged" "$plan" "$before" after
    [ "$status" -ne 0 ]
    case "$variant" in false-null|null-false)
      # Effective equivalence must never accept a claim of raw equality.
      damaged="$(jq -c --arg hash "$(jq -r .configuration_sha256 <<<"$before")" '.configuration_sha256=$hash' <<<"$after")"
      run mg_sync_reference__check "$damaged" "$plan" "$before" after
      [ "$status" -ne 0 ]
      damaged="$(jq -c --arg hash "$(jq -r .property_fingerprints.HostConfig.OomKillDisable <<<"$before")" \
        '.property_fingerprints.HostConfig.OomKillDisable=$hash' <<<"$after")"
      run mg_sync_reference__check "$damaged" "$plan" "$before" after
      [ "$status" -ne 0 ] ;;
    esac
  done
}

@test "reference OOM killer disabled absent and malformed values refuse before and after start" {
  local phase value mutation index=0
  for phase in before after; do
    for value in True 0 1 -1 "'false'" "'null'" "''" '[]' '[False]' '{}' "{'enabled':False}" absent; do
      index=$((index+1))
      mutation="c['HostConfig']['OomKillDisable']=$value"
      [ "$value" != absent ] || mutation="del c['HostConfig']['OomKillDisable']"
      reference_execution_fixture "oom-refuse-$index"
      reference_execute identity '' "$mutation" "$phase"
      [ "$(cat "$REF/status")" = fail ] && [ ! -e "$REF/container.json" ]
      if [ "$phase" = before ]; then [ ! -e "$REF/started" ]; else [ -e "$REF/started" ]; fi
      jq -se --arg phase "$phase" '[.[] | select(.[0]=="start")] |
        length==(if $phase=="before" then 0 else 1 end)' "$REF/docker.jsonl"
      jq -se --arg phase "$phase" --arg value "$value" 'map(select(.key=="reference_execution"))[0].value |
        (.measurements_valid|not) and .absence_verified and .cleanup_exit==0 and .execution_exit!=0 and
        (if $phase=="before" then .actual_exit==null and .attach_exit==null and .after==null
         else .attach_exit==0 and (if $value=="absent" then .actual_exit==null else .actual_exit==0 end) end) and
        (if $value=="absent" then .[$phase]==null else
          .[$phase].validation_failure=="check" and (.[$phase]|has("container")|not) and
          .[$phase].oom_killer_enabled_configuration_sha256==null and
          (.[$phase].configuration_sha256|test("^[0-9a-f]{64}$")) and
          (.[$phase].property_fingerprints.HostConfig.OomKillDisable|test("^[0-9a-f]{64}$")) end)' "$REF/details.jsonl"
      ! rg 'private-fixture-value|NOT_PUBLIC|"enabled"' "$REF/details.jsonl"
    done
  done
}

@test "reference OOM false null equivalence refuses simultaneous other configuration changes" {
  local mutation index=0
  # Establish existing unprojected fields in both observations. Wide integers
  # reach the real byte producer, never a jq numeric round trip.
  local baseline="c['HostConfig']['OomKillDisable']=None if c['State']['Status']=='exited' else False; c['HostConfig']['LogConfig']={'Type':'json-file','Config':{'synthetic-log-key':'original'}}; c['HostConfig']['NanoCpus']=9007199254740992; c['Config']['Healthcheck']={'Interval':9007199254740992,'Test':['NONE']}; c['Config']['AttachStdin']=False"
  reference_execution_fixture oom-other-baseline
  reference_execute identity '' "$baseline" both
  [ "$(cat "$REF/status")" = pass ] && [ -e "$REF/started" ]
  [ ! -e "$REF/container.json" ] && [ ! -e "$REF/failures" ]
  jq -se 'map(select(.key=="reference_execution"))[0].value |
    .measurements_valid and .absence_verified and .actual_exit==0 and .attach_exit==0 and .cleanup_exit==0 and
    .before.oom_killer_enabled_configuration_sha256==.after.oom_killer_enabled_configuration_sha256' "$REF/details.jsonl"
  for mutation in \
    "c['HostConfig']['LogConfig']['Config']['synthetic-log-key']='changed'" \
    "c['HostConfig']['NanoCpus']=9007199254740993" \
    "c['Config']['Healthcheck']['Interval']=9007199254740993" \
    "c['Config']['AttachStdin']=0" "c['Config']['AttachStdin']='false'" "c['Config']['AttachStdin']=None" \
    "c['Config']['Volumes']={}" "c['Config']['Env'].append('UNEXPECTED=synthetic-oom-change')" \
    "c['Path']='synthetic-oom-change'" "c['Args'].append('synthetic-oom-change')" \
    "del c['HostConfig']['LogConfig']" "del c['Config']['Healthcheck']" \
    "c['HostConfig']['CpuShares']=0" "c['Config']['Shell']=[]"; do
    index=$((index+1))
    reference_execution_fixture "oom-other-change-$index"
    reference_execute identity '' "$baseline; exec(\"$mutation\" if c['State']['Status']=='exited' else '')" both
    [ "$(cat "$REF/status")" = fail ] && [ -e "$REF/started" ]
    [ ! -e "$REF/container.json" ]
    jq -se '[.[] | select(.[0]=="start")] | length==1' "$REF/docker.jsonl"
    jq -se '[.[] | select(.[0]=="rm")] | length==1' "$REF/docker.jsonl"
    jq -se 'map(select(.key=="reference_execution"))[0].value |
      (.measurements_valid|not) and .absence_verified and .cleanup_exit==0 and .execution_exit!=0 and
      .attach_exit==0 and .actual_exit==0 and (.before|has("container")) and
      (.after|has("container")|not) and .after.validation_failure=="check" and
      .before.property_fingerprints.HostConfig.OomKillDisable=="fcbcf165908dd18a9e49f7ff27810176db8e9f63b4352213741664245224f8aa" and
      .after.property_fingerprints.HostConfig.OomKillDisable=="74234e98afe7498fb5daf1f36ac2d78acc339464f950703b8c019892f982b90b" and
      .before.configuration_sha256!=.after.configuration_sha256 and
      .before.oom_killer_enabled_configuration_sha256!=.after.oom_killer_enabled_configuration_sha256' "$REF/details.jsonl"
    ! rg 'private-fixture-value|NOT_PUBLIC|synthetic-log-key|synthetic-oom-change' "$REF/details.jsonl"
  done
}

@test "reference absent versus null changes retain distinct raw hashes and fail terminal identity" {
  local field phase index=0 before after measured_after plan damaged
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  for field in ExposedPorts Volumes; do
    for phase in before after; do
      index=$((index+1))
      reference_execution_fixture "optional-transition-$index"
      reference_execute identity '' "del c['Config']['$field']" "$phase"
      [ "$(cat "$REF/status")" = fail ] && [ -e "$REF/started" ]
      [ ! -e "$REF/container.json" ]
      jq -se '[.[] | select(.[0]=="start")] | length==1' "$REF/docker.jsonl"
      before="$(jq -cs 'map(select(.key=="reference_execution"))[0].value.before' "$REF/details.jsonl")"
      after="$(jq -cs 'map(select(.key=="reference_execution"))[0].value.after' "$REF/details.jsonl")"
      jq -ne --argjson b "$before" --argjson a "$after" --arg field "$field" --arg phase "$phase" '
        $b!=null and $a!=null and $b.configuration_sha256!=$a.configuration_sha256 and
        $b.inspection_sha256!=$a.inspection_sha256 and
        ($b.property_fingerprints.Config|has($field))==($phase=="after") and
        ($a.property_fingerprints.Config|has($field))==($phase=="before") and
        ($b.property_fingerprints.Config|del(.[$field]))==($a.property_fingerprints.Config|del(.[$field])) and
        ($b|has("container")) and ($a|has("container")|not) and $a.validation_failure=="check"'
      plan="$(jq -c '.container | {id:.Id,image:.Image,name:(.Name|ltrimstr("/")),nonce:.Config.Labels.nonce,
        run:.Config.Labels.run,mode:.Config.Cmd[0],output:.HostConfig.Mounts[0].Source,user:.Config.User,creation_floor:.Created[0:19]}' <<<"$before")"
      run mg_sync_reference__check "$before" "$plan" null before
      [ "$status" -eq 0 ]
      # Rebuild the trusted comparison input from synthetic test-only Docker
      # stdout; rejected projections are deliberately absent from publication.
      docker() { [ "$1" = inspect ] && cat "$REF/inspection-after.json"; }
      measured_after="$(mg_sync_reference__inspect synthetic)"
      jq -ne --argjson b "$before" --argjson a "$measured_after" '
        ($b.container|del(.State,.NetworkSettings))==($a.container|del(.State,.NetworkSettings))'
      # Independently retain both equality barriers: neither copied full hash
      # nor copied inventory can conceal an actual optional-field transition.
      damaged="$(jq -c --arg hash "$(jq -r .configuration_sha256 <<<"$before")" '.configuration_sha256=$hash' <<<"$measured_after")"
      run mg_sync_reference__check "$damaged" "$plan" "$before" after
      [ "$status" -ne 0 ]
      damaged="$(jq -c --argjson inv "$(jq -c .property_fingerprints <<<"$before")" '.property_fingerprints=$inv' <<<"$measured_after")"
      run mg_sync_reference__check "$damaged" "$plan" "$before" after
      [ "$status" -ne 0 ]
      jq -se 'map(select(.key=="reference_execution"))[0].value |
        (.measurements_valid|not) and .execution_exit!=0 and .absence_verified' "$REF/details.jsonl"
    done
  done
}

@test "reference nonempty declared ports and volumes fail actual safety before child start" {
  local mutation index=0
  for mutation in "c['Config']['ExposedPorts']={'80/tcp':{}}" \
    "c['Config']['Volumes']={'/unauthorized':{}}"; do
    index=$((index+1))
    reference_execution_fixture "optional-unsafe-$index"
    reference_execute identity '' "$mutation"
    [ "$(cat "$REF/status")" = fail ] && [ ! -e "$REF/started" ]
    [ ! -e "$REF/container.json" ]
    jq -se '[.[] | select(.[0]=="start")] | length==0' "$REF/docker.jsonl"
    # The producer admits the schema; the real safety predicate refuses it.
    jq -se 'map(select(.key=="reference_execution"))[0].value |
      .before!=null and .after==null and (.measurements_valid|not) and
      (.before|has("container")|not) and .before.validation_failure=="check" and
      .actual_exit==null and .execution_exit!=0 and .absence_verified and
      (.before.property_fingerprints.Config.ExposedPorts|test("^[0-9a-f]{64}$")) and
      (.before.property_fingerprints.Config.Volumes|test("^[0-9a-f]{64}$"))' "$REF/details.jsonl"
  done
}

# Check the actual wrapper publication against the complete synthetic Docker
# input, independently hashing known bytes. Raw inspections live only in this
# fake daemon's test scratch, never in an MG report or child output directory.
reference_assert_rejected_publication() {
  python3 - "$REF" "$1" "$2" <<'PY'
import hashlib, json, pathlib, sys
root, phase, marker = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
details = (root/'details.jsonl').read_text()
assert marker not in details and marker not in (root/'wrapper.log').read_text()
execution, = [item['value'] for item in map(json.loads, details.splitlines())
              if item['key'] == 'reference_execution']
assert not execution['measurements_valid'] and execution['execution_exit'] != 0
published = execution[phase]
assert set(published) == {'inspection_sha256', 'configuration_sha256',
                          'property_fingerprints', 'oom_killer_enabled_configuration_sha256', 'validation_failure'}
assert published['validation_failure'] == 'check'
raw = (root/('inspection-'+phase+'.json')).read_bytes()
assert marker in raw.decode()
assert published['inspection_sha256'] == hashlib.sha256(raw).hexdigest()
c = json.loads(raw)[0]
complete = json.dumps({key:c[key] for key in ('Config','HostConfig','Path','Args')},
                      sort_keys=True, separators=(',',':'), ensure_ascii=True, allow_nan=False).encode('ascii')
assert published['configuration_sha256'] == hashlib.sha256(complete).hexdigest()
effective = {key:c[key] for key in ('Config','HostConfig','Path','Args')}
effective['HostConfig'] = dict(c['HostConfig'], OomKillDisable=False)
assert published['oom_killer_enabled_configuration_sha256'] == hashlib.sha256(
    b'meguri-oom-killer-enabled-configuration-v1\x00' + json.dumps(effective,
        sort_keys=True, separators=(',',':'), ensure_ascii=True, allow_nan=False).encode('ascii')).hexdigest()
def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',',':'),
                                    ensure_ascii=True, allow_nan=False).encode('ascii')).hexdigest()
assert published['property_fingerprints'] == {
    **{section:{name:fingerprint(value) for name,value in c[section].items()}
       for section in ('Config','HostConfig')},
    **{name:fingerprint(c[name]) for name in ('Path','Args')}}
if phase == 'before':
    assert execution['after'] is None and execution['actual_exit'] is None
    assert not (root/'started').exists()
else:
    assert (root/'started').exists() and 'container' in execution['before']
    assert marker not in json.dumps(execution['before'])
PY
}

@test "reference actual producer and wrapper omit arbitrary rejected pre and post projections" {
  local phase mutation index=0 marker
  for phase in before after; do
    for mutation in \
      "c['Config']['ExposedPorts']={marker:{'nested':marker}}" \
      "c['Config']['Volumes']={marker:{'nested':marker}}" \
      "c['Args'].append(marker)" "c['Path']=marker" \
      "c['Config']['Entrypoint'].append(marker)" "c['Config']['Cmd'].append(marker)" \
      "c['Config']['Env'].append('MG_RUN_ID='+marker)" \
      "c['Mounts'][0]['Source']=marker" "c['Mounts'][0]['Destination']=marker" \
      "c['HostConfig']['Mounts'][0]['Source']=marker" \
      "c['HostConfig']['Mounts'][0]['Target']=marker" \
      "c['State']['Error']=marker" "c['State']['OOMKilled']=marker" \
      "c['Config']['Env'].append('UNEXPECTED='+marker); c['State']['FinishedAt']=marker; c['Mounts'][0]['Source']=marker"; do
      index=$((index+1))
      # Arbitrary synthetic data, never a real credential or private token.
      marker="synthetic-publication-$index-$(printf '%s' "$BATS_TEST_TMPDIR-$index" | shasum -a 256 | cut -c1-24)"
      reference_execution_fixture "publication-$index"
      reference_execute identity '' "marker='$marker'; $mutation" "$phase"
      [ "$(cat "$REF/status")" = fail ]
      [ ! -e "$REF/container.json" ]
      reference_assert_rejected_publication "$phase" "$marker"
      jq -se --arg phase "$phase" '[.[] | select(.[0]=="start")] |
        length==(if $phase=="before" then 0 else 1 end)' "$REF/docker.jsonl"
      jq -se 'map(select(.key=="reference_execution"))[0].value |
        .absence_verified and .cleanup_exit==0 and
        (.actual_exit==null or .actual_exit==0)' "$REF/details.jsonl"
      grep -Fx reference-execution "$REF/failures"
    done
  done
}

@test "reference rejected post publication preserves process and cleanup failures" {
  local defect marker=synthetic-post-failure-volume
  for defect in process cleanup process-cleanup; do
    reference_execution_fixture "publication-$defect"
    reference_execute identity "$defect" "c['Config']['Volumes']={'$marker':{}}" after
    [ "$(cat "$REF/status")" = fail ]
    reference_assert_rejected_publication after "$marker"
    jq -se --arg defect "$defect" 'map(select(.key=="reference_execution"))[0].value |
      if $defect=="cleanup" then .attach_exit==0 and .actual_exit==0 and .execution_exit!=0
      else .attach_exit==37 and .actual_exit==37 and .execution_exit==37 end' "$REF/details.jsonl"
    jq -se --arg defect "$defect" 'map(select(.key=="reference_execution"))[0].value |
      if $defect=="process" then .cleanup_exit==0 and .absence_verified
      else .cleanup_exit!=0 and (.absence_verified|not) end' "$REF/details.jsonl"
    grep -Fx reference-execution "$REF/failures"
  done
}

@test "reference afterstate actualExit invalidtype and bounds never disclose raw scalars" {
  local value index=0 marker=synthetic-actualExit-invalidtype
  # Isolate actualExit refusal too: no configuration mismatch is needed to
  # protect a scalar (or its nested values) from separate sibling publication.
  for value in "'$marker'" "{'$marker':['$marker']}" "['$marker']"; do
    index=$((index+1))
    reference_execution_fixture "actualExit-only-$index"
    reference_execute identity '' "c['State']['ExitCode']=$value" after
    [ "$(cat "$REF/status")" = fail ]
    reference_assert_rejected_publication after "$marker"
    jq -se 'map(select(.key=="reference_execution"))[0].value |
      .actual_exit==null and .absence_verified and .execution_exit!=0' "$REF/details.jsonl"
  done
  for value in "'$marker'" "{'$marker':['$marker']}" "['$marker']" \
    True False None -1 256; do
    index=$((index+1))
    reference_execution_fixture "actualExit-invalidtype-$index"
    # A simultaneous raw-config mismatch must not short-circuit the scalar
    # publication barrier, even with an earlier process and cleanup failure.
    reference_execute identity process-cleanup \
      "c['Config']['Env'].append('UNEXPECTED=$marker'); c['State']['ExitCode']=$value" after
    [ "$(cat "$REF/status")" = fail ]
    reference_assert_rejected_publication after "$marker"
    jq -se 'map(select(.key=="reference_execution"))[0].value |
      .actual_exit==null and .attach_exit==37 and .execution_exit==37 and
      .cleanup_exit!=0 and (.absence_verified|not)' "$REF/details.jsonl"
  done
  # Bounded integers remain useful diagnostics even on a rejected postcheck.
  for value in 0 23 255; do
    reference_execution_fixture "actualExit-bounded-$value"
    reference_execute identity '' \
      "c['Config']['Env'].append('UNEXPECTED=$marker'); c['State']['ExitCode']=$value" after
    [ "$(cat "$REF/status")" = fail ]
    reference_assert_rejected_publication after "$marker"
    jq -se --argjson expected "$value" 'map(select(.key=="reference_execution"))[0].value |
      .actual_exit==$expected and .absence_verified' "$REF/details.jsonl"
  done
}

@test "reference property fingerprints canonicalize object order and retain raw hashes" {
  reference_inspection_fixture property-order
  local first second
  first="$(mg_sync_reference__inspect synthetic)"
  printf '%s' "$first" >"$REF/first.json"
  cp "$REF/inspection.json" "$REF/original.json"
  python3 - "$REF/inspection.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
def reorder(value):
    if isinstance(value, dict):
        return {key: reorder(value[key]) for key in reversed(list(value))}
    if isinstance(value, list):
        return [reorder(item) for item in value]
    return value
path.write_bytes(b' \r\n\t'+json.dumps(reorder(json.loads(path.read_text())), indent=2).encode()+b'\n \t\r\n\n')
PY
  second="$(mg_sync_reference__inspect synthetic)"
  printf '%s' "$second" >"$REF/second.json"
  python3 - "$REF" <<'PY'
import hashlib, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
first, second = [json.loads((root/name).read_text()) for name in ('first.json','second.json')]
assert first['property_fingerprints'] == second['property_fingerprints']
assert first['inspection_sha256'] != second['inspection_sha256']
assert first['configuration_sha256'] == second['configuration_sha256']
for raw_name, measured in [('original.json',first),('inspection.json',second)]:
    raw = (root/raw_name).read_bytes()
    assert measured['inspection_sha256'] == hashlib.sha256(raw).hexdigest()
    original = json.loads(raw)[0]
    complete = json.dumps({key:original[key] for key in ('Config','HostConfig','Path','Args')},
                          sort_keys=True, separators=(',',':'), ensure_ascii=True, allow_nan=False).encode('ascii')
    assert measured['configuration_sha256'] == hashlib.sha256(complete).hexdigest()
    inventory = measured['property_fingerprints']
    assert set(inventory) == {'Config','HostConfig','Path','Args'}
    def fingerprint(value):
        encoded = json.dumps(value, sort_keys=True, separators=(',',':'), ensure_ascii=True, allow_nan=False).encode('ascii')
        return hashlib.sha256(encoded).hexdigest()
    for section in ('Config','HostConfig'):
        assert set(inventory[section]) == set(original[section])
        assert inventory[section] == {key:fingerprint(value) for key,value in original[section].items()}
    for name in ('Path','Args'):
        assert inventory[name] == fingerprint(original[name])
    # Independent known canonical bytes, not just a mirrored serializer.
    assert inventory['Path'] == hashlib.sha256(b'"python"').hexdigest()
    assert inventory['HostConfig']['Memory'] == hashlib.sha256(b'2147483648').hexdigest()
    assert inventory['Config']['Volumes'] == hashlib.sha256(b'null').hexdigest()
PY
}

@test "reference property fingerprints isolate changes without publishing nested keys or marker values" {
  reference_inspection_fixture property-private
  python3 - "$REF/inspection.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
c = json.loads(path.read_text())[0]
c['Config']['Env'].append('SYNTHETIC_ENV_KEY=SYNTHETIC_ENV_VALUE')
c['Config']['Labels']['synthetic-label-key'] = 'SYNTHETIC_LABEL_VALUE'
c['HostConfig']['LogConfig'] = {'Type':'json-file','Config':{'synthetic-log-key':'SYNTHETIC_LOG_VALUE'}}
c['State']['Error'] = 'SYNTHETIC_STATE_ERROR_VALUE'
path.write_text(json.dumps([c]))
PY
  local first changed target
  first="$(mg_sync_reference__inspect synthetic)"
  printf '%s' "$first" >"$REF/first.json"
  cp "$REF/inspection.json" "$REF/original.json"
  for target in Config.Env Config.Labels HostConfig.LogConfig Path Args State.Error; do
    python3 - "$REF" "$target" <<'PY'
import json, pathlib, sys
root, target = pathlib.Path(sys.argv[1]), sys.argv[2]
c = json.loads((root/'original.json').read_text())[0]
if target == 'Config.Env': c['Config']['Env'].append('SYNTHETIC_SECOND_ENV=second')
elif target == 'Config.Labels': c['Config']['Labels']['synthetic-label-key'] = 'second'
elif target == 'HostConfig.LogConfig': c['HostConfig']['LogConfig']['Config']['synthetic-log-key'] = 'second'
elif target == 'Path': c['Path'] = 'synthetic-program'
elif target == 'Args': c['Args'].append('synthetic-argument')
else: c['State']['Error'] = 'SYNTHETIC_SECOND_STATE_ERROR'
(root/'inspection.json').write_text(json.dumps([c]))
PY
    changed="$(mg_sync_reference__inspect synthetic)"
    printf '%s' "$changed" >"$REF/changed.json"
    python3 - "$REF" "$target" <<'PY'
import json, pathlib, sys
root, target = pathlib.Path(sys.argv[1]), sys.argv[2]
first, changed = [(root/name).read_text() for name in ('first.json','changed.json')]
for published in (first, changed):
    for marker in ('SYNTHETIC_ENV_KEY','SYNTHETIC_ENV_VALUE','synthetic-label-key',
                   'SYNTHETIC_LABEL_VALUE','synthetic-log-key','SYNTHETIC_LOG_VALUE',
                   'SYNTHETIC_STATE_ERROR_VALUE','SYNTHETIC_SECOND_ENV','SYNTHETIC_SECOND_STATE_ERROR',
                   'NOT_PUBLIC','private-fixture-value'):
        assert marker not in published
a, b = json.loads(first), json.loads(changed)
def flatten(inventory):
    return {section+'.'+name:value for section in ('Config','HostConfig')
            for name,value in inventory[section].items()} | {name:inventory[name] for name in ('Path','Args')}
fa, fb = flatten(a['property_fingerprints']), flatten(b['property_fingerprints'])
assert fa.keys() == fb.keys()
assert [key for key in fa if fa[key] != fb[key]] == ([] if target == 'State.Error' else [target])
assert (a['configuration_sha256'] == b['configuration_sha256']) == (target == 'State.Error')
assert a['inspection_sha256'] != b['inspection_sha256']
PY
  done
}

@test "reference full configuration and property hashes distinguish exact integers booleans strings and null" {
  reference_inspection_fixture property-types
  local token measured index=0
  for token in 0 1 -1 9007199254740992 9007199254740993 9007199254740994 9007199254740995 9223372036854775807 \
    true false '"0"' '"1"' '"true"' '"false"' '"null"' '"1e-400"' '"NaN"' null; do
    index=$((index+1))
    python3 - "$REF" "$token" "$index" <<'PY'
import pathlib, sys
root, token, index = pathlib.Path(sys.argv[1]), sys.argv[2].encode('ascii'), sys.argv[3]
# Independently specified complete canonical bytes, no numeric parsing or
# implementation serializer in either expected full or property preimage.
full = (b'{"Args":["synthetic"],"Config":{"Cmd":[],"Entrypoint":[],"Env":[],"Healthcheck":{"Interval":'
        +token+b',"Test":["NONE"]},"Image":"","Labels":{},"User":"","WorkingDir":""},'
        b'"HostConfig":{"AutoRemove":false,"Binds":null,"CapAdd":null,"CapDrop":[],"DeviceRequests":null,'
        b'"Devices":[],"ExtraHosts":null,"Init":true,"IpcMode":"private","Links":null,"Memory":2147483648,'
        b'"Mounts":[],"NetworkMode":"none","OomKillDisable":false,"PidMode":"","PidsLimit":128,"PortBindings":{},"Privileged":false,'
        b'"PublishAllPorts":false,"ReadonlyRootfs":true,"RestartPolicy":{"MaximumRetryCount":0,"Name":"no"},'
        b'"SecurityOpt":[],"Tmpfs":{},"UTSMode":"","UsernsMode":"","VolumesFrom":null},"Path":"python"}')
(root/('full-'+index+'.json')).write_bytes(full)
(root/'inspection.json').write_bytes(b'['+full[:-1]+b',"Mounts":[],"NetworkSettings":{"Networks":{},"Ports":{}},"State":{}}]\n')
PY
    measured="$(mg_sync_reference__inspect synthetic)"
    printf '%s' "$measured" >"$REF/type-$index.json"
    printf '%s' "$token" >"$REF/token-$index.txt"
  done
  python3 - "$REF" "$index" <<'PY'
import hashlib, json, pathlib, sys
root, count = pathlib.Path(sys.argv[1]), int(sys.argv[2])
digests, full_digests = [], []
for index in range(1, count+1):
    token = (root/f'token-{index}.txt').read_bytes()
    measured = json.loads((root/f'type-{index}.json').read_text())
    digest = measured['property_fingerprints']['Config']['Healthcheck']
    # Known canonical bytes, with no numeric parsing in the expectation.
    assert digest == hashlib.sha256(b'{"Interval":'+token+b',"Test":["NONE"]}').hexdigest()
    assert measured['configuration_sha256'] == hashlib.sha256((root/f'full-{index}.json').read_bytes()).hexdigest()
    digests.append(digest)
    full_digests.append(measured['configuration_sha256'])
assert len(set(digests)) == count
assert len(set(full_digests)) == count
PY
}

@test "reference unsupported numeric tokens publish no diagnostic and never start the child" {
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  docker() { [ "$1" = inspect ] && cat "$REF/inspection.json"; }
  local token index=0
  # Finite underflow, binary64 rounding collisions, ordinary decimals and
  # exponent-form integers are unsupported even in optional nested properties.
  for token in 1e-400 -1e-400 0.0 -0.0 9007199254740992.0 9007199254740993.0 \
    1.0000000000000001 1.0000000000000002 1e0 1E+0 NaN Infinity -Infinity 1e9999; do
    index=$((index+1))
    reference_execution_fixture "numeric-no-start-$index"
    NUMBER_TOKEN="$token" reference_execute identity numeric-token
    [ "$(cat "$REF/status")" = fail ]
    [ ! -e "$REF/started" ]
    jq -se '[.[] | select(.[0]=="start")] | length==0' "$REF/docker.jsonl"
    jq -se 'map(select(.key=="reference_execution"))[0].value |
      .before==null and .after==null and (.measurements_valid|not) and .actual_exit==null' "$REF/details.jsonl"
    run --separate-stderr mg_sync_reference__inspect synthetic
    [ "$status" -ne 0 ]
    [ -z "$output" ]
    [ "$stderr" = 'reference inspection refused: json' ]
  done
}

@test "reference literal NUL and malformed Docker bytes refuse before capture publication and child start" {
  local defect expected
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  docker() { [ "$1" = inspect ] && cat "$REF/inspection.json"; }
  for defect in nul-prefix nul-key nul-string nul-integer nul-between nul-suffix \
    invalid-utf8 malformed duplicate oversized; do
    reference_execution_fixture "raw-$defect"
    RAW_DEFECT="$defect" reference_execute identity raw-inspect
    [ "$(cat "$REF/status")" = fail ] && [ ! -e "$REF/started" ]
    [ ! -e "$REF/container.json" ]
    expected=json
    [ "$defect" != oversized ] || expected=bounds
    [ "$output" = "reference inspection refused: $expected" ]
    jq -se '[.[] | select(.[0]=="start")] | length==0' "$REF/docker.jsonl"
    jq -se 'map(select(.key=="reference_execution"))[0].value |
      .before==null and .after==null and .actual_exit==null and
      (.measurements_valid|not) and .execution_exit!=0 and .absence_verified' "$REF/details.jsonl"
    ! rg 'SYNTHETIC_RAW_VALUE|private-fixture-value|NOT_PUBLIC|ignored null byte' "$REF/details.jsonl" "$REF/wrapper.log"
    # Exercise the actual binary reader directly on the same original bytes.
    run --separate-stderr mg_sync_reference__inspect synthetic
    [ "$status" -ne 0 ] && [ -z "$output" ]
    [ "$stderr" = "reference inspection refused: $expected" ]
  done
}

@test "reference wrapper refuses valid Docker inspection stdout followed by producer failure" {
  local phase
  for phase in pre post; do
    reference_execution_fixture "output-fail-$phase"
    reference_execute identity "$phase-inspect-output-fail"
    [ "$(cat "$REF/status")" = fail ] && [ ! -e "$REF/container.json" ]
    [ "$output" = 'reference inspection refused: read' ]
    jq -se --arg phase "$phase" '[.[] | select(.[0]=="start")] |
      length==(if $phase=="pre" then 0 else 1 end)' "$REF/docker.jsonl"
    jq -se --arg phase "$phase" 'map(select(.key=="reference_execution"))[0].value |
      .after==null and .actual_exit==null and (.measurements_valid|not) and
      .execution_exit!=0 and .absence_verified and
      (if $phase=="pre" then .before==null else (.before|has("container")) end)' "$REF/details.jsonl"
    ! rg 'private-fixture-value|NOT_PUBLIC' "$REF/details.jsonl" "$REF/wrapper.log"
    if [ "$phase" = pre ]; then [ ! -e "$REF/started" ]; else [ -e "$REF/started" ]; fi
  done
}

@test "reference wrapper retains complete raw whitespace in both accepted inspection hashes" {
  reference_execution_fixture inspect-whitespace
  reference_execute identity inspect-whitespace
  [ "$(cat "$REF/status")" = pass ] && [ -z "$output" ]
  [ ! -e "$REF/container.json" ] && [ ! -e "$REF/failures" ]
  python3 - "$REF" <<'PY'
import hashlib, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
execution, = [item['value'] for item in map(json.loads, (root/'details.jsonl').read_text().splitlines())
              if item['key'] == 'reference_execution']
assert execution['measurements_valid'] and execution['absence_verified']
for phase in ('before','after'):
    raw = (root/('inspection-'+phase+'.json')).read_bytes()
    assert raw.startswith(b' \r\n\t') and raw.endswith(b' \t\r\n\n')
    assert execution[phase]['inspection_sha256'] == hashlib.sha256(raw).hexdigest()
    assert execution[phase]['inspection_sha256'] != hashlib.sha256(raw.strip()).hexdigest()
assert execution['before']['configuration_sha256'] == execution['after']['configuration_sha256']
assert execution['before']['property_fingerprints'] == execution['after']['property_fingerprints']
PY
}

@test "reference inspection refuses overcount oversized malformed and incomplete diagnostics before publication" {
  reference_inspection_fixture property-invalid
  cp "$REF/inspection.json" "$REF/original.json"
  local defect expected
  for defect in config-count host-count oversized-value oversized-name invalid-name unknown-name \
    missing-config missing-host missing-path missing-args missing-property empty-config wrong-host \
    wrong-path wrong-args extra-record duplicate-property duplicate-nested nan infinity overflow \
    missing-user missing-memory empty-input malformed-json invalid-utf8 projection-shape \
    env-shape labels-shape state-shape network-shape host-mount-shape bind-shape; do
    python3 - "$REF" "$defect" <<'PY'
import json, pathlib, sys
root, defect = pathlib.Path(sys.argv[1]), sys.argv[2]
c = json.loads((root/'original.json').read_text())[0]
if defect in ('config-count','host-count'):
    section, count = ('Config',33) if defect == 'config-count' else ('HostConfig',97)
    c[section] = {'Schema'+str(index):None for index in range(count)}
elif defect == 'oversized-value': c['Config']['Env'].append('SYNTHETIC_OVERSIZE')
elif defect == 'oversized-name': c['Config']['A'*10000] = None
elif defect == 'invalid-name': c['HostConfig']['synthetic\nname'] = None
elif defect == 'unknown-name': c['Config']['UnknownSchemaProperty'] = None
elif defect == 'missing-user': del c['Config']['User']
elif defect == 'missing-memory': del c['HostConfig']['Memory']
elif defect.startswith('missing-') and defect != 'missing-property':
    del c[{'missing-config':'Config','missing-host':'HostConfig','missing-path':'Path','missing-args':'Args'}[defect]]
elif defect == 'missing-property': del c['Config']['Env']
elif defect == 'empty-config': c['Config'] = {}
elif defect == 'wrong-host': c['HostConfig'] = []
elif defect == 'wrong-path': c['Path'] = None
elif defect == 'wrong-args': c['Args'] = [False]
elif defect == 'projection-shape': c['Mounts'] = None
elif defect == 'env-shape': c['Config']['Env'] = [{'SYNTHETIC_EXCEPTION_VALUE':['SYNTHETIC_EXCEPTION_VALUE']}]
elif defect == 'labels-shape': c['Config']['Labels'] = 'SYNTHETIC_EXCEPTION_VALUE'
elif defect == 'state-shape': c['State'] = 'SYNTHETIC_EXCEPTION_VALUE'
elif defect == 'network-shape': c['NetworkSettings']['Networks'] = 'SYNTHETIC_EXCEPTION_VALUE'
elif defect == 'host-mount-shape': c['HostConfig']['Mounts'] = ['SYNTHETIC_EXCEPTION_VALUE']
elif defect == 'bind-shape': c['HostConfig']['Mounts'][0]['BindOptions'] = 'SYNTHETIC_EXCEPTION_VALUE'
elif defect in ('nan','infinity'): c['HostConfig']['Memory'] = float('nan' if defect == 'nan' else 'inf')
raw = json.dumps([c,c] if defect == 'extra-record' else [c])
if defect == 'oversized-value':
    raw = raw.replace('SYNTHETIC_OVERSIZE', 'X'*(8*1024*1024+1-len(raw)+len('SYNTHETIC_OVERSIZE')))
if defect == 'duplicate-property': raw = raw.replace('"Path": "python"','"Path": "other", "Path": "python"')
if defect == 'duplicate-nested': raw = raw.replace('"MaximumRetryCount": 0','"MaximumRetryCount": 1, "MaximumRetryCount": 0')
if defect == 'overflow': raw = raw.replace('2147483648','1e9999')
if defect == 'empty-input': raw = ''
if defect == 'malformed-json': raw = '{"SYNTHETIC_ARBITRARY_KEY":"/synthetic/private/path"'
if defect == 'invalid-utf8':
    (root/'inspection.json').write_bytes(b'\xffSYNTHETIC_EXCEPTION_VALUE')
    raise SystemExit(0)
(root/'inspection.json').write_text(raw)
PY
    case "$defect" in
      config-count|host-count|oversized-value|empty-config|empty-input) expected=bounds ;;
      oversized-name|invalid-name|unknown-name) expected=schema ;;
      missing-path|missing-args|missing-property|missing-user|missing-memory) expected=required ;;
      missing-config|missing-host|wrong-host|wrong-path|wrong-args|extra-record|projection-shape|\
      env-shape|labels-shape|state-shape|network-shape|host-mount-shape|bind-shape) expected=shape ;;
      *) expected=json ;;
    esac
    run --separate-stderr mg_sync_reference__inspect synthetic
    [ "$status" -ne 0 ]
    [ -z "$output" ]
    [ "$stderr" = "reference inspection refused: $expected" ]
  done
}

@test "reference producer failures including valid stdout expose only fixed read failure" {
  reference_inspection_fixture property-stage
  docker() { printf '%s\n' 'SYNTHETIC_DAEMON_EXCEPTION /synthetic/private/path' >&2; return 51; }
  run --separate-stderr mg_sync_reference__inspect synthetic
  [ "$status" -ne 0 ] && [ -z "$output" ]
  [ "$stderr" = 'reference inspection refused: read' ]
  docker() {
    cat "$REF/inspection.json"
    printf '%s\n' 'SYNTHETIC_DAEMON_EXCEPTION /synthetic/private/path' >&2
    return 51
  }
  run --separate-stderr mg_sync_reference__inspect synthetic
  [ "$status" -ne 0 ] && [ -z "$output" ]
  [ "$stderr" = 'reference inspection refused: read' ]
}

@test "reference fingerprint predicates refuse missing malformed and inconsistent inventories" {
  reference_inspection_fixture property-predicate
  local before after plan filter damaged
  before="$(jq -cs 'map(select(.key=="reference_execution"))[0].value.before' "$REF/details.jsonl")"
  after="$(jq -cs 'map(select(.key=="reference_execution"))[0].value.after' "$REF/details.jsonl")"
  plan="$(jq -c '.container | {id:.Id,image:.Image,name:(.Name|ltrimstr("/")),nonce:.Config.Labels.nonce,
    run:.Config.Labels.run,mode:.Config.Cmd[0],output:.HostConfig.Mounts[0].Source,user:.Config.User,creation_floor:.Created[0:19]}' <<<"$before")"
  run mg_sync_reference__check "$before" "$plan" null before
  [ "$status" -eq 0 ]
  run mg_sync_reference__check "$after" "$plan" "$before" after
  [ "$status" -eq 0 ]
  for filter in 'del(.oom_killer_enabled_configuration_sha256)' \
    '.oom_killer_enabled_configuration_sha256=null' '.oom_killer_enabled_configuration_sha256=true' \
    '.oom_killer_enabled_configuration_sha256=0' '.oom_killer_enabled_configuration_sha256="false"' \
    '.oom_killer_enabled_configuration_sha256=[]' '.oom_killer_enabled_configuration_sha256={}' \
    'del(.property_fingerprints.HostConfig.OomKillDisable)' \
    '.property_fingerprints.HostConfig.OomKillDisable=("a"*64)' \
    'del(.property_fingerprints)' 'del(.property_fingerprints.Config.Env)' \
    'del(.property_fingerprints.Config.User)' 'del(.property_fingerprints.HostConfig.Memory)' \
    '.property_fingerprints.HostConfig={}' '.property_fingerprints.Config.Env="bad"' \
    '.property_fingerprints.Extra={}' '.property_fingerprints.Path=null' \
    '.property_fingerprints.Config.UnknownSchemaProperty=("a"*64)' \
    '.property_fingerprints.Config["bad-name"]=("a"*64)' \
    '.property_fingerprints.Config[("A"*41)]=("a"*64)' \
    '.property_fingerprints.Config=reduce range(33) as $i ({}; .["Schema"+($i|tostring)]=("a"*64))' \
    '.property_fingerprints.HostConfig=reduce range(97) as $i ({}; .["Schema"+($i|tostring)]=("a"*64))' \
    '.container="SYNTHETIC_PREDICATE_VALUE"' \
    '.container.HostConfig="SYNTHETIC_PREDICATE_VALUE"' \
    '.container.State="SYNTHETIC_PREDICATE_VALUE"' \
    '.property_fingerprints.Config=[{"SYNTHETIC_PREDICATE_VALUE":"SYNTHETIC_PREDICATE_VALUE"}]'; do
    damaged="$(jq -c "$filter" <<<"$before")"
    run --separate-stderr mg_sync_reference__check "$damaged" "$plan" null before
    [ "$status" -ne 0 ]
    [ -z "$output" ] && [ -z "$stderr" ]
  done
  # Even a syntactically valid property digest change remains a refusal when
  # the complete raw hash and sanitized projection are supplied unchanged.
  damaged="$(jq -c '.property_fingerprints.Config.Env=("a"*64)' <<<"$after")"
  run mg_sync_reference__check "$damaged" "$plan" "$before" after
  [ "$status" -ne 0 ]
  # Identical property diagnostics cannot excuse a different raw fingerprint.
  damaged="$(jq -c '.configuration_sha256=("a"*64)' <<<"$after")"
  run mg_sync_reference__check "$damaged" "$plan" "$before" after
  [ "$status" -ne 0 ]
}

@test "reference malformed property diagnostics fail closed without starting the child" {
  local mutation index=0
  for mutation in "c['Config']['UnknownSchemaProperty']=None" "del c['Config']['Env']" \
    "del c['Config']['User']" "del c['HostConfig']['Memory']" \
    "c['HostConfig']['synthetic-name']=None" "c['Config']['A'*10000]=None" \
    "c['Config'].update({'Schema'+str(i):None for i in range(33)})"; do
    index=$((index+1))
    reference_execution_fixture "property-no-start-$index"
    reference_execute identity '' "$mutation"
    [ "$(cat "$REF/status")" = fail ]
    [ ! -e "$REF/started" ]
    jq -se '[.[] | select(.[0]=="start")] | length==0' "$REF/docker.jsonl"
    jq -se 'map(select(.key=="reference_execution"))[0].value |
      .before==null and (.measurements_valid|not) and .actual_exit==null' "$REF/details.jsonl"
  done
}

@test "reference retained selftest and identity measure the launched immutable image and terminal state" {
  local mode measurement_phase measured published
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  for mode in selftest identity; do
    reference_execution_fixture "$mode"
    reference_execute "$mode"
    [ "$(cat "$REF/status")" = pass ]
    [ ! -e "$REF/failures" ] && [ ! -e "$REF/container.json" ]
    jq -se 'map(select(.key=="reference_execution"))[0].value |
      .measurements_valid and .absence_verified and .actual_exit==0 and .attach_exit==0 and .cleanup_exit==0 and
      .image_id==("sha256:"+("1"*64)) and .container_id==("2"*64) and
      (.invocation_sha256|test("^[0-9a-f]{64}$")) and
      (.before.inspection_sha256|test("^[0-9a-f]{64}$")) and
      (.after.inspection_sha256|test("^[0-9a-f]{64}$")) and
      .before.container.State.Status=="created" and .after.container.State.Status=="exited" and
      .after.container.RestartCount==0' "$REF/details.jsonl"
    jq -se '[.[] | select(.[0]=="create")] | length==1 and .[0][-2]==("sha256:"+("1"*64))' "$REF/docker.jsonl"
    jq -se '[.[] | select(.[0]=="start")] | length==1' "$REF/docker.jsonl"
    jq -se '[.[] | select(.[0]=="rm")] | length==1' "$REF/docker.jsonl"
    [ "$(cat "$REF/tag-now")" = "sha256:$(printf '%064d' 0 | tr 0 9)" ]
    ! rg 'private-fixture-value|NOT_PUBLIC' "$REF/details.jsonl"
    cmp "$REF/run/reference-control/invocation.json" "$REF/run/reference-output/invocation.json"
    jq -e --arg mode "$mode" 'keys==["arguments","author_session","batch_base","lock_sha256","mg_job","run_id","source_author_sessions","source_commit","source_sha256"] and
      .arguments==[$mode] and (.source_author_sessions|length)==10' "$REF/run/invocation.json"
    [ "$(cat "$REF/counts")" = "$(printf 'total=3\npassed=3\nfailed=0')" ]
    # A successful full check still publishes the entire measured object;
    # compare both phases with the real producer, including every strict field.
    docker() { [ "$1" = inspect ] && cat "$REF/inspection-$measurement_phase.json"; }
    for measurement_phase in before after; do
      measured="$(mg_sync_reference__inspect synthetic)"
      published="$(jq -cs --arg phase "$measurement_phase" '
        map(select(.key=="reference_execution"))[0].value[$phase]' "$REF/details.jsonl")"
      jq -ne --argjson measured "$measured" --argjson published "$published" '$measured==$published'
    done
  done
}

@test "reference retained pre-inspection refuses substitutions before any start" {
  local mutation index=0
  for mutation in \
    "c['Id']='3'*64" "c['Created']='1900-01-01T00:00:00.000000Z'" \
    "c['Image']='sha256:'+'3'*64" "c['Config']['Image']='mutable-tag'" \
    "c['Name']='/foreign'" "c['Config']['Labels']['meguri.mg.run-id']='foreign'" \
    "c['Config']['Labels']['meguri.sync.owner']='foreign'" "c['Config']['Labels']={}" \
    "c['Config']['Entrypoint']=['sh']" "c['Config']['Cmd']=['identity']" "c['Args']=['substituted']" \
    "c['Config']['User']='foreign-user'" "c['Config']['Env']=['MG_RUN_ID=foreign']" \
    "c['HostConfig']['NetworkMode']='bridge'" "c['HostConfig']['PortBindings']={'80/tcp':[{}]}" \
    "c['Mounts'][0]['Source']='/unowned'" "c['Mounts'][0]['RW']=False" \
    "c['Mounts'].append(dict(Type='volume',Source='owner-data',Destination='/data',RW=True))" \
    "c['HostConfig']['Tmpfs']['/tmp']='size=1g'" "c['HostConfig']['Init']=False" \
    "c['HostConfig']['ReadonlyRootfs']=False" "c['HostConfig']['CapDrop']=[]" \
    "c['HostConfig']['SecurityOpt']=[]" "c['HostConfig']['Memory']=0" "c['HostConfig']['PidsLimit']=0" \
    "c['HostConfig']['Privileged']=True" "c['HostConfig']['AutoRemove']=True"; do
    index=$((index+1))
    reference_execution_fixture "pre-$index"
    reference_execute selftest '' "$mutation"
    [ "$(cat "$REF/status")" = fail ]
    [ ! -e "$REF/started" ]
    jq -se '[.[] | select(.[0]=="start")] | length==0' "$REF/docker.jsonl"
    # Foreign/missing ownership must prevent removal by the real helper too.
    case "$mutation" in
      *"['Id']"*|*"['Name']"*|*"['Labels']"*)
        jq -se '[.[] | select(.[0]=="rm")] | length==0' "$REF/docker.jsonl" ;;
    esac
  done
}

@test "reference retained terminal failures and cleanup cannot erase the first process failure" {
  local defect expected
  for defect in process cleanup process-cleanup pre-inspect-fail post-inspect-fail create-uncertain create-absent invocation-mutated invocation-symlink source-post invalid-image; do
    reference_execution_fixture "$defect"
    reference_execute identity "$defect"
    [ "$(cat "$REF/status")" = fail ]
    grep -Fx reference-execution "$REF/failures"
    case "$defect" in
      process|process-cleanup)
        jq -se 'map(select(.key=="reference_execution"))[0].value |
          .attach_exit==37 and .actual_exit==37 and .execution_exit==37 and .after.container.State.Status=="exited"' "$REF/details.jsonl" ;;
      create-uncertain|create-absent)
        [ ! -e "$REF/started" ] && [ ! -e "$REF/container.json" ]
        jq -se 'map(select(.key=="reference_execution"))[0].value |
          .execution_exit==19 and .actual_exit==null and (.measurements_valid|not) and .absence_verified' "$REF/details.jsonl" ;;
      cleanup)
        jq -se 'map(select(.key=="reference_execution"))[0].value |
          .actual_exit==0 and .execution_exit!=0 and .cleanup_exit!=0 and (.absence_verified|not)' "$REF/details.jsonl" ;;
      invalid-image) [ ! -e "$REF/create-seen" ] ;;
    esac
  done
}

@test "reference retained post-inspection requires unchanged identity config and a terminal unrestarted process" {
  local mutation index=0
  for mutation in "c['Created']='1900-01-01T00:00:00.000000Z'" "c['Id']='3'*64" \
    "c['Image']='sha256:'+'3'*64" "c['Name']='/foreign'" \
    "c['Config']['Labels']['meguri.sync.owner']='foreign'" "c['Config']['Cmd']=['selftest']" \
    "c['Config']['User']='foreign-user'" "c['Config']['Env'].append('UNEXPECTED=value')" "c['HostConfig']['NetworkMode']='bridge'" \
    "c['Mounts'][0]['Source']='/unowned'" "c['HostConfig']['ReadonlyRootfs']=False" \
    "c['State']['Status']='running'; c['State']['Running']=True" \
    "c['State']['StartedAt']='0001-01-01T00:00:00Z'" "c['State']['FinishedAt']='0001-01-01T00:00:00Z'" \
    "c['RestartCount']=1" "c['State']['ExitCode']=23" "c['State']['OOMKilled']=True" \
    "c['State']['Error']='private-fixture-value'"; do
    index=$((index+1))
    reference_execution_fixture "post-$index"
    reference_execute identity '' "$mutation" after
    [ "$(cat "$REF/status")" = fail ] && [ -e "$REF/started" ]
    jq -se '[.[] | select(.[0]=="start")] | length==1' "$REF/docker.jsonl"
    ! rg 'private-fixture-value|NOT_PUBLIC' "$REF/details.jsonl"
  done
}

@test "reference other modes and argument guards keep the compose invocation path" {
  local mode index=0
  for mode in recovery-discover recovery recovery-dirty smoke edges normal normal-sdk normal-matrix media catalog; do
    reference_execution_fixture "$mode"
    reference_execute "$mode"
    [ "$(cat "$REF/status")" = pass ] && [ ! -e "$REF/docker.jsonl" ]
    jq -se --arg mode "$mode" '[.[] | select(index("run"))] |
      length==1 and (.[0] | index("--rm"))!=null and .[0][-2:]==["sync-reference",$mode]' "$REF/compose.jsonl"
  done
  for mode in deps lock; do
    reference_execution_fixture "$mode"
    reference_execute "$mode"
    [ "$(cat "$REF/status")" = pass ] && [ ! -e "$REF/docker.jsonl" ]
    [ ! -e "$REF/run/invocation.json" ]
    jq -se '[.[] | select(index("run"))] | length==1 and (.[0] | index("--rm"))!=null and
      (.[0] | index("sync-reference-resolver"))!=null' "$REF/compose.jsonl"
  done
  reference_execution_fixture default
  run env REPO_ROOT="$REPO_ROOT" REF="$REF" MODE=selftest bash "$REF/run.sh"
  [ "$status" -eq 0 ] && [ "$(cat "$REF/status")" = pass ]
  jq -e '.arguments==["selftest"]' "$REF/run/invocation.json"
  reference_execution_fixture explicit-empty
  run env REPO_ROOT="$REPO_ROOT" REF="$REF" MODE=selftest bash "$REF/run.sh" ''
  [ "$status" -eq 0 ] && [ "$(cat "$REF/status")" = pass ]
  jq -e '.arguments==["selftest"]' "$REF/run/invocation.json"
  jq -se '[.[] | select(.[0]=="create")] | length==1 and .[0][-1]=="selftest"' "$REF/docker.jsonl"
  [ ! -e "$REF/container.json" ] && [ ! -e "$REF/failures" ]
  for mode in '' selftest identity recovery; do
    reference_execution_fixture "args-$mode"
    run env REPO_ROOT="$REPO_ROOT" REF="$REF" MODE="$mode" bash "$REF/run.sh" "$mode" unexpected
    [ "$status" -eq 0 ] && [ "$(cat "$REF/status")" = error ]
    [ ! -e "$REF/docker.jsonl" ] && [ ! -e "$REF/compose.jsonl" ]
  done
  for mode in validate seal smoke; do
    reference_execution_fixture "input-$mode"
    local input=build/input
    if [ "$mode" = smoke ]; then
      input=build/input/request.json
      printf '{}\n' >"$REF/root/$input"
    fi
    run env REPO_ROOT="$REPO_ROOT" REF="$REF" MODE="$mode" bash "$REF/run.sh" "$mode" "$input"
    [ "$status" -eq 0 ] && [ ! -e "$REF/docker.jsonl" ]
    [ "$(cat "$REF/status")" = pass ]
    jq -e --arg mode "$mode" --arg input "$input" '.arguments==[$mode,$input]' "$REF/run/invocation.json"
    jq -se '[.[] | select(index("run"))] | length==1 and (.[0] | index("--rm"))!=null' "$REF/compose.jsonl"
  done
}

@test "reference selftest and identity dry runs keep the real MG dispatcher idle" {
  local sandbox="$BATS_TEST_TMPDIR/reference-dry" mode result
  mkdir -p "$sandbox/bin"
  cp -R "$REPO_ROOT/tools" "$sandbox/"
  git -C "$sandbox" init -q
  cat >"$sandbox/bin/docker" <<'SH'
#!/bin/sh
printf 'unexpected Docker call\n' >>"$REFERENCE_DRY_DOCKER_LOG"
exit 97
SH
  chmod +x "$sandbox/bin/docker"
  for mode in selftest identity; do
    run --separate-stderr env PATH="$sandbox/bin:$PATH" REFERENCE_DRY_DOCKER_LOG="$sandbox/docker-called" \
      "$sandbox/tools/mg" sync-server-reference --dry-run "$mode"
    result="${lines[${#lines[@]} - 1]:-}"
    [ "$status" -eq 0 ]
    jq -e --arg mode "$mode" '.status=="pass" and .dry_run and .args==[$mode] and .failures==[]' "$result"
    [ ! -e "$(dirname "$(dirname "$result")")/latest" ]
    [ ! -e "$(dirname "$result")/reference-control" ]
    [ ! -e "$sandbox/docker-called" ]
    python3 "$sandbox/tools/tests/validate_result.py" "$result"
  done
}

@test "reference Docker context admits the exact neutral Mobile selection" {
  python3 - "$REPO_ROOT" <<'PY'
from pathlib import Path, PurePosixPath
import sys

root = Path(sys.argv[1])
selection = "conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json"
destination = "/opt/reference/neutral/candidate-mobile-final-selection.json"
copies = [line.split()[1:] for line in
          (root / "docker/meguri-sync-reference.Dockerfile").read_text().splitlines()
          if line.startswith("COPY ")]
assert [copy for copy in copies if "mobile" in " ".join(copy)] == [[selection, destination]]
# All context exceptions must serve an explicit COPY or its parent directories.
# The existing quarantined source exceptions stay exact; no bodies are read.
expected = {"**", "!oracle/", "!oracle/meguri-sync-server/", "!oracle/meguri-sync-server/**"}
for source, target in copies:
    if source.startswith("oracle/"):
        assert [source, target] in [
            ["oracle/meguri-sync-server/requirements.lock", "/opt/reference/requirements.lock"],
            ["oracle/meguri-sync-server/", "/opt/reference/"],
        ]
        continue
    assert source.startswith("conformance/") and source.endswith(".json")
    assert not any(char in source for char in "*?[")
    expected.add("!" + source)
    expected.update("!" + str(parent) + "/" for parent in PurePosixPath(source).parents
                    if str(parent) != ".")
rules = (root / "docker/meguri-sync-reference.Dockerfile.dockerignore").read_text().splitlines()
assert rules[0] == "**"
assert len(rules) == len(set(rules)) and set(rules) == expected
assert "!" + selection in rules
PY
}

@test "reference invocation retains complete committed author identities and rejects role defects" {
  local observed_status observed_failure records defect positive
  # Independent fixed order, checked against emitted invocation bytes below.
  local expected='["s-20261115T133013Z-d8cec007","s-20261115T211543Z-556b04e9","s-20261115T224434Z-376ca308","s-20261115T231059Z-ff95725a","s-20261116T001757Z-8fe546bc","s-20261116T020051Z-5e05fa3f","s-20261116T092626Z-4487d759","s-20261116T124044Z-c9976196","s-20261116T225916Z-981c8d1b","s-20261117T230521Z-4667d769"]'
  MG_ROOT="$REPO_ROOT"
  MG_RUN_ID=source-author-metadata-only
  MG_RUN_DIR="$BATS_TEST_TMPDIR/authors-positive"
  mkdir -p "$MG_RUN_DIR"
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  # Exercise the actual producer/role validator with only public committed
  # records. Stub transport and hashes: no SDK files, image or execution proof.
  mg_compose() { :; }
  # This test isolates invocation/author validation; retained launch is covered
  # below through the real wrapper and owned cleanup with stubbed Docker.
  mg_sync_reference__retained() { :; }
  mg_sync_reference_digest() { printf '%064d\n' 0; }
  shasum() { printf '%064d  metadata-only\n' 0; }
  mg_artifact() { :; }
  mg_detail() { :; }
  mg_status() { observed_status="$1"; }
  mg_failure() { observed_failure="$1"; }
  observed_status= observed_failure=
  mg_sync_server_reference selftest
  [ "$observed_status" = pass ]
  [ -z "$observed_failure" ]
  positive="$MG_RUN_DIR/source-authors.json"
  jq -e --slurpfile invocation "$MG_RUN_DIR/invocation.json" '
    $invocation[0].source_author_sessions as $authors |
    all(.[]; .role == "oracle-maintainer") and
    ((map(.session) | sort) == ($authors | sort)) and
    ((map(.session) | unique | length) == ($authors | length))' "$positive"
  jq -e --argjson expected "$expected" '
    .source_author_sessions == $expected and
    .author_session == "s-20261116T124044Z-c9976196" and
    .arguments == ["selftest"]' "$MG_RUN_DIR/invocation.json"
  # Substitute only the public role-record transport, keeping the validator real.
  git() {
    if [ "$#" -eq 4 ] && [ "$1" = -C ] && [ "$2" = "$MG_ROOT" ] && [ "$3" = show ]; then
      cat "$records"
    else
      command git "$@"
    fi
  }
  for defect in missing wrong-role ambiguous; do
    MG_RUN_DIR="$BATS_TEST_TMPDIR/authors-$defect"
    mkdir -p "$MG_RUN_DIR"
    records="$MG_RUN_DIR/records.jsonl"
    jq -c --arg defect "$defect" --arg author s-20261117T230521Z-4667d769 '
      (if $defect == "missing" then map(select(.session != $author))
       elif $defect == "wrong-role" then
         map(if .session == $author then .role = "reviewer" else . end)
       else . + [(.[] | select(.session == $author) | .duplicate_probe = true)]
       end)[]' "$positive" >"$records"
    observed_status= observed_failure=
    mg_sync_server_reference selftest
    [ "$observed_status" = error ]
    [ "$observed_failure" = session-missing ]
    [ ! -e "$MG_RUN_DIR/invocation.json" ]
  done
}
