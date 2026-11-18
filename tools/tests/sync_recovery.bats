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
        PidsLimit=int(options['--pids-limit'][0]),AutoRemove=False,RestartPolicy=dict(Name='no',MaximumRetryCount=0),
        Privileged=False,Devices=[],DeviceRequests=None,VolumesFrom=None,Links=None,ExtraHosts=None,
        PidMode='',IpcMode='private',UTSMode='',UsernsMode='',Binds=None,
        Mounts=[dict(Type=mount['type'],Source=mount['src'],Target=mount['dst'])],Tmpfs=tmpfs),
      Mounts=[dict(Type=mount['type'],Source=mount['src'],Destination=mount['dst'],RW=True)],
      NetworkSettings=dict(Ports={},Networks={}),
      State=dict(Status='created',Running=False,Paused=False,Restarting=False,Dead=False,OOMKilled=False,
        ExitCode=0,Error='',StartedAt='0001-01-01T00:00:00Z',FinishedAt='0001-01-01T00:00:00Z'))
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
    mutation_phase = os.environ.get('MUTATION_PHASE','before')
    if started == (mutation_phase == 'after'):
        exec(os.environ.get('MUTATION',''), {}, {'c':value})
    emit([value])
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
  [ "$status" -eq 0 ] # MG jobs report status through the receipt sink.
}

@test "reference retained selftest and identity measure the launched immutable image and terminal state" {
  local mode
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
