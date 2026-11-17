# Meguri Sync's standalone service. Model files retain their MPL terms.
mg_job sync-server-test docker:dev mg_sync_server_test \
  "Account/service tests: locked nextest, zero retries; -- [-E|--filter-expr <expr>] [--test <binary>] (repeatable, separate values only)"
mg_job sync-server-lint docker:dev mg_sync_server_lint \
  "Standalone server clippy correctness/safety on all targets/features, locked"
mg_job sync-server-deps docker:dev mg_sync_server_deps \
  "Server graph/deny/notices evidence, locked; --resolve initial lock, --resolve-existing manifest changes, or --update <package> <exact-version>"
mg_job sync-server-format docker:dev mg_sync_server_format \
  "Optional server rustfmt check; --write explicitly applies formatting in Docker"
mg_job sync-server-candidate-build host mg_sync_server_candidate_build \
  "Clean committed candidate build in Docker: --lane desktop --phase full --author-session <recorded-id>"
mg_job sync-server-interop host mg_sync_server_interop \
  "Owned isolated loopback candidate: --lane desktop --phase <full|normal|media|final> --build-receipt <result.json> --author-session <recorded-id>; no fallback"

# One source-input definition for production freeze and execution revalidation.
mg_sync_server__archive() {
  git -C "$MG_ROOT" archive --format=tar "$1" -- server core/crates/meguri-model core/Cargo.toml core/rust-toolchain.toml LICENSE docker/dev.Dockerfile docker/dev.Dockerfile.dockerignore docker/compose.yml tools/mg tools/mg.d/meguri-sync.sh
}

mg_sync_server__clean() {
  git -C "$MG_ROOT" status --porcelain=v1 -z --untracked-files=all >"$MG_RUN_DIR/source-status" || return 1
  if [ -s "$MG_RUN_DIR/source-status" ]; then
    mg_failure source-dirty "Candidate builds require clean committed source, including the lock; checkpoint WIP before building"
    return 1
  fi
}
# The responsible producer is explicit; checkout/fixer identity is separate.
# Read committed records only, never an uncommitted append or the last row.
mg_sync_server__author_session() {
  local ledger
  if [ -z "$1" ] || ! ledger="$(git -C "$MG_ROOT" show HEAD:docs/process/provenance/sessions.jsonl)"; then
    mg_failure candidate-author-session "An explicit committed producer session is required"; return 1
  fi
  if ! printf '%s\n' "$ledger" | jq -se --arg session "$1" '[.[] | select(.session==$session)] | length==1' >/dev/null; then
    mg_failure candidate-author-session "Producer session must match exactly one committed ledger record"; return 1
  fi
  mg_detail author_session "$(jq -nc --arg session "$1" '$session')"
}

# Resolve configured tags even when Compose has no retained containers.
mg_sync_server__image() {
  local service="$1" tag
  shift
  tag="$(mg_compose "$@" config --images "$service")" || return 1
  [ -n "$tag" ] && [ "$(printf '%s\n' "$tag" | wc -l | tr -d ' ')" = 1 ] || return 1
  docker image inspect --format '{{.Id}}' "$tag"
}

# Hooks capture names before creation, so cancellation during creation is covered.
# Never delete by name alone: check the exact name, run and creation nonce first.
mg_sync_server__remove_container() {
  local name="$1" owner="$2" ids inspected remaining
  if ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")"; then
    [ -n "$ids" ] || return 0
    if [[ "$ids" =~ ^[0-9a-f]{64}$ ]] && inspected="$(docker inspect "$ids")" &&
      jq -e --arg name "/$name" --arg run "$MG_RUN_ID" --arg owner "$owner" --arg id "$ids" '
        length==1 and .[0].Id==$id and .[0].Name==$name and
        .[0].Config.Labels["meguri.mg.run-id"]==$run and
        .[0].Config.Labels["meguri.sync.owner"]==$owner
      ' <<<"$inspected" >/dev/null && docker rm -f "$ids" >/dev/null &&
      remaining="$(docker container ls -aq --no-trunc --filter "name=^/$name$")" && [ -z "$remaining" ]; then
      return 0
    fi
  fi
  mg_failure candidate-cleanup "Cannot prove removal of the owned container; retain failure and investigate"
  mg_status fail
  return 1
}

mg_sync_server__remove_secret() {
  if python3 - "$1" "$2" <<'PY'
import json, os, pathlib, stat, sys
path, receipt = map(pathlib.Path, sys.argv[1:])
if not path.exists() and not path.is_symlink():
    raise SystemExit(0)
owner = json.loads(receipt.read_text())
s = path.lstat()
if not (stat.S_ISDIR(s.st_mode) and s.st_uid == os.getuid()
        and [s.st_dev, s.st_ino, s.st_uid] == owner):
    raise SystemExit('secret directory ownership changed')
for child in path.iterdir():
    s = child.lstat()
    if not (child.name == 'credentials.json' and stat.S_ISREG(s.st_mode)
            and s.st_uid == os.getuid() and s.st_nlink == 1):
        raise SystemExit('unexpected secret directory entry')
    child.unlink()
path.rmdir()
PY
  then return 0; fi
  mg_failure candidate-secret-cleanup "Cannot prove removal of the owned secret directory"
  mg_status fail
  return 1
}

mg_sync_server__target_identity() {
  python3 - "$1" "$2" <<'PY'
import json, os, pathlib, stat, sys
path = pathlib.Path(sys.argv[1])
s = path.lstat()
if not (stat.S_ISDIR(s.st_mode) and s.st_uid == os.getuid()
        and stat.S_IMODE(s.st_mode) == 0o700):
    raise SystemExit('invalid owned build target')
if sys.argv[2] == 'empty' and any(path.iterdir()):
    raise SystemExit('candidate build target is not empty')
print(json.dumps(dict(device=s.st_dev, inode=s.st_ino, uid=s.st_uid, mode=stat.S_IMODE(s.st_mode)), sort_keys=True))
PY
}

# Compose must merge read_only explicitly; run -v inherits the dev mount's RW.
mg_sync_server__build_mount_override() {
  jq -n --arg source "$1" '
    {services:{dev:{volumes:[{type:"bind",source:$source,target:"/work",read_only:true}]}}}
  ' >"$2"
}

mg_sync_server__build_script() {
  cat <<'BUILD'
set -euo pipefail
cd /work/server
export RUSTFLAGS= CARGO_ENCODED_RUSTFLAGS=
[ "$CARGO_TARGET_DIR" = /candidate-target ]
[ -d /candidate-target ] && [ -z "$(ls -A /candidate-target)" ]
rustc -Vv > /candidate-out/rustc.txt
# Prepare from this frozen archive, using only the explicitly installed locked
# web dependencies. Generated inputs stay out of the archive and Cargo source.
web_work="$(mktemp -d /tmp/meguri-candidate-web.XXXXXX)"
MEGURI_WEB_SOURCE=/work/server/web MEGURI_WEB_WORK="$web_work" \
  MEGURI_WEB_OUTPUT=/candidate-out/web MEGURI_WEB_DEPS=/sync-web-deps \
  bash /work/server/web/scripts/run.sh build
export MEGURI_WEB_PREPARED_DIR=/candidate-out/web/prepared
cargo build --locked --no-default-features --bin meguri-sync-server --message-format json-render-diagnostics --target-dir /candidate-target --profile dev > /candidate-out/build.jsonl
cp /candidate-target/debug/meguri-sync-server /candidate-out/meguri-sync-server
cmp /candidate-target/debug/meguri-sync-server /candidate-out/meguri-sync-server
BUILD
}

mg_sync_server__build_container_check() {
  jq -e --arg image "$2" --arg run "$3" --arg owner "$4" --arg script "$(mg_sync_server__build_script)" \
    --arg source "$5" --arg product "$6" --arg target "$7" '
    length==1 and .[0].Image==$image and .[0].State.ExitCode==0 and
    .[0].Config.Labels["meguri.mg.run-id"]==$run and .[0].Config.Labels["meguri.sync.owner"]==$owner and
    .[0].Config.Cmd==["bash","-c",$script] and
    ([.[0].Config.Env[] | select(startswith("CARGO_TARGET_DIR="))]==["CARGO_TARGET_DIR=/candidate-target"]) and
    ([.[0].Mounts[] | select(.Destination=="/work")] | length==1 and .[0].Source==$source and .[0].RW==false) and
    ([.[0].Mounts[] | select(.Destination=="/candidate-out")] | length==1 and .[0].Source==$product and .[0].RW==true) and
    ([.[0].Mounts[] | select(.Destination=="/candidate-target")] | length==1 and .[0].Source==$target and .[0].Type=="bind" and .[0].RW==true)
  ' "$1" >/dev/null
}

mg_sync_server_candidate_build() {
  if [ "$#" -ne 6 ] || [ "$1" != --lane ] || [ "$2" != desktop ] || [ "$3" != --phase ] || [ "$5" != --author-session ] || [ -z "$6" ]; then
    mg_failure usage "sync-server-candidate-build --lane desktop --phase full --author-session <recorded-id>"; mg_status error; return 0
  fi
  mg_sync_server__author_session "$6" || { mg_status fail; return 0; }
  if [ "$4" != full ]; then
    mg_failure candidate-phase "Only Desktop full is supported; complete coverage is unavailable"; mg_status error; return 0
  fi
  mg_sync_server__clean || { mg_status fail; return 0; }
  local commit source_hash lock_hash executable_hash image version server build_name build_rc=0 target_owner owner
  commit="$(git -C "$MG_ROOT" rev-parse --verify HEAD)" || { mg_status error; return 0; }
  # Freeze only public production/build inputs. No quarantined runtime, private
  # data, hidden credentials or reference fixtures enter the build snapshot.
  if ! mg_sync_server__archive "$commit" >"$MG_RUN_DIR/source.tar"; then
    mg_failure source-freeze "Cannot freeze committed candidate inputs"; mg_status fail; return 0
  fi
  source_hash="$(shasum -a 256 "$MG_RUN_DIR/source.tar" | awk '{print $1}')"
  mkdir "$MG_RUN_DIR/source" "$MG_RUN_DIR/product" || { mg_status error; return 0; }
  tar -xf "$MG_RUN_DIR/source.tar" -C "$MG_RUN_DIR/source" || { mg_status error; return 0; }
  if [ ! -f "$MG_RUN_DIR/source/server/Cargo.lock" ]; then mg_failure lock-missing "Candidate requires committed server lock"; mg_status fail; return 0; fi
  image="$(mg_sync_server__image dev)" || { mg_status fail; return 0; }
  if ! [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]]; then mg_failure image-missing "Build the existing pinned dev image through MG first"; mg_status fail; return 0; fi
  build_name="meguri-candidate-build-$MG_RUN_ID"
  mkdir -m 700 "$MG_RUN_DIR/target" || { mg_status fail; return 0; }
  target_owner="$(mg_sync_server__target_identity "$MG_RUN_DIR/target" empty)" || { mg_status fail; return 0; }
  mg_sync_server__build_mount_override "$MG_RUN_DIR/source" "$MG_RUN_DIR/build-mounts.compose.json" || { mg_status fail; return 0; }
  mg_artifact candidate-build-mounts "$MG_RUN_DIR/build-mounts.compose.json"
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || { mg_status fail; return 0; }
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$build_name" "$owner")"
  mg_compose -f "$MG_RUN_DIR/build-mounts.compose.json" run --name "$build_name" -T --no-deps --pull never --label "meguri.mg.run-id=$MG_RUN_ID" \
      --label "meguri.sync.owner=$owner" \
      -v "$MG_RUN_DIR/product:/candidate-out" \
      -v "$MG_RUN_DIR/target:/candidate-target" \
      -e CARGO_TARGET_DIR=/candidate-target dev bash -c "$(mg_sync_server__build_script)" || build_rc=$?
  docker inspect "$build_name" >"$MG_RUN_DIR/build-container.json" || build_rc=1
  mg_sync_server__remove_container "$build_name" "$owner" || build_rc=1
  [ "$(mg_sync_server__target_identity "$MG_RUN_DIR/target" retained)" = "$target_owner" ] || build_rc=1
  mg_sync_server__build_container_check "$MG_RUN_DIR/build-container.json" "$image" "$MG_RUN_ID" "$owner" \
    "$MG_RUN_DIR/source" "$MG_RUN_DIR/product" "$MG_RUN_DIR/target" || build_rc=1
  if [ "$build_rc" -ne 0 ] || ! jq -e --arg image "$image" --arg run "$MG_RUN_ID" '
    length==1 and .[0].Image==$image and .[0].Config.Labels["meguri.mg.run-id"]==$run and .[0].State.ExitCode==0
  ' "$MG_RUN_DIR/build-container.json" >/dev/null; then
    mg_failure candidate-build "Locked candidate Docker build failed"; mg_status fail; return 0
  fi
  if ! jq -se '[.[] | select(.reason=="compiler-artifact" and .target.name=="meguri-sync-server" and .executable!=null)] | length==1 and .[0].fresh==false and .[0].features==[] and .[0].profile.test==false and .[0].profile.debug_assertions==true and .[0].profile.opt_level=="0" and .[0].executable=="/candidate-target/debug/meguri-sync-server"' "$MG_RUN_DIR/product/build.jsonl" >/dev/null; then
    mg_failure build-features "Cargo did not report exactly one production candidate without test hooks"; mg_status fail; return 0
  fi
  lock_hash="$(shasum -a 256 "$MG_RUN_DIR/source/server/Cargo.lock" | awk '{print $1}')"
  executable_hash="$(shasum -a 256 "$MG_RUN_DIR/product/meguri-sync-server" | awk '{print $1}')"
  version="$(awk -F '"' '/^version = / {print $2; exit}' "$MG_RUN_DIR/source/server/Cargo.toml")"
  server="$(jq -nc --arg version "$version" --arg commit "$commit" --arg source "$source_hash" --arg lock "$lock_hash" --arg binary "$executable_hash" \
    '{kind:"meguri_sync",version:$version,build:$commit,source_commit:$commit,source_sha256:$source,lock_sha256:$lock,executable_sha256:$binary}')" || { mg_status error; return 0; }
  mg_sync_server__clean || { mg_status fail; return 0; }
  if [ "$(git -C "$MG_ROOT" rev-parse HEAD)" != "$commit" ]; then mg_failure source-advanced "Source commit changed during candidate build"; mg_status fail; return 0; fi
  mg_detail candidate_build "$(jq -nc --argjson server "$server" '{server:$server,source_dirty:false,profile:"debug",features:[]}')"
  mg_detail build_image "$(jq -nc --arg image "$image" '{id:$image}')"
  mg_detail candidate_build_inputs "$(jq -nc --arg target "$MG_RUN_DIR/target" --argjson owner "$target_owner" --arg source "$source_hash" --arg lock "$lock_hash" --arg image "$image" --arg nonce "$owner" '
    {command:["cargo","build","--locked","--no-default-features","--bin","meguri-sync-server","--message-format","json-render-diagnostics","--target-dir","/candidate-target","--profile","dev"],
     working_directory:"/work/server",target_directory:"/candidate-target",target_host_directory:$target,
     target_initially_empty:true,target_owner:$owner,container_owner:$nonce,source_sha256:$source,lock_sha256:$lock,image_id:$image,
     profile:"debug",features:[],rustflags:"",encoded_rustflags:""}')"
  mg_artifact candidate-source "$MG_RUN_DIR/source.tar"
  mg_artifact candidate-lock "$MG_RUN_DIR/source/server/Cargo.lock"
  mg_artifact candidate-executable "$MG_RUN_DIR/product/meguri-sync-server"
  mg_artifact candidate-build-container "$MG_RUN_DIR/build-container.json"
  mg_artifact candidate-toolchain "$MG_RUN_DIR/product/rustc.txt"
  mg_artifact candidate-build-jsonl "$MG_RUN_DIR/product/build.jsonl"
  # MG writes this final receipt after the function returns; no self-hash.
  mg_artifact candidate-build-receipt "$MG_RUN_DIR/result.json"
  mg_status pass
}

mg_sync_server_interop() {
  if [ "$#" -ne 8 ] || [ "$1" != --lane ] || [ "$2" != desktop ] || [ "$3" != --phase ] || [ "$5" != --build-receipt ] || [ ! -f "$6" ] || [ "$7" != --author-session ] || [ -z "$8" ]; then
    mg_failure usage "sync-server-interop --lane desktop --phase <full|normal|media|final> --build-receipt <result.json> --author-session <recorded-id>"; mg_status error; return 0
  fi
  mg_sync_server__author_session "$8" || { mg_status fail; return 0; }
  if [ "$4" != full ] && [ "$4" != normal ] && [ "$4" != media ] && [ "$4" != final ]; then
    mg_failure candidate-phase "Only Desktop full, normal, media and the fixed final expansion are supported"; mg_status error; return 0
  fi
  case "$(mg_session_role)" in
    implementer|fixer) mg_failure role-refused "Candidate reference execution requires an integration session"; mg_status fail; return 0 ;;
  esac
  mg_sync_server__clean || { mg_status fail; return 0; }
  if [ "$4" = final ]; then
    if ! mg_sync_package__final_owned "$6" "$(mg_json_array "$@")" "$8"; then mg_status fail; fi
  elif ! mg_sync_server__interop_owned "$6" "$(mg_json_array "$@")" "$8"; then mg_status fail; fi
}

mg_sync_server__interop_owned() {
  local receipt="$1" commit build_dir binary lock source server image cid secret_dir hash item uid gid arguments="$2" author_session="$3" build_run version target_owner owner candidate_name interop_rc=0
  commit="$(git -C "$MG_ROOT" rev-parse HEAD)" || return 1
  if ! jq -e --arg commit "$commit" --arg root "$MG_ROOT" --arg session "$author_session" '
    .job=="sync-server-candidate-build" and .status=="pass" and .dry_run==false and .root==$root and
    .commit=={sha:$commit,dirty:false} and .args==["--lane","desktop","--phase","full","--author-session",$session] and
    .details.author_session==$session and
    (.details.candidate_build | keys==["features","profile","server","source_dirty"]) and
    .details.candidate_build.source_dirty==false and .details.candidate_build.features==[] and
    .details.candidate_build.profile=="debug" and .details.candidate_build.server.source_commit==$commit
  ' "$receipt" >/dev/null; then mg_failure build-receipt "Missing, dirty, stale or incompatible candidate build receipt"; return 1; fi
  mg_sync_server__archive "$commit" >"$MG_RUN_DIR/current-source.tar" || return 1
  build_dir="$(cd "$(dirname "$receipt")" && pwd -P)" || return 1
  build_run="$(jq -er '.run_id | select(type=="string" and test("^[0-9]{8}T[0-9]{6}Z-[0-9]+$"))' "$receipt")" || return 1
  if [ "$build_dir" != "$MG_ROOT/build/mg/sync-server-candidate-build/$build_run" ] || [ "$(basename "$receipt")" != result.json ]; then
    mg_failure build-receipt-location "Candidate requires the actual canonical MG build receipt"; return 1
  fi
  binary="$build_dir/product/meguri-sync-server"; lock="$build_dir/source/server/Cargo.lock"; source="$build_dir/source.tar"
  if ! cmp -s "$source" "$MG_RUN_DIR/current-source.tar" || ! cmp -s "$lock" "$MG_ROOT/server/Cargo.lock"; then
    mg_failure build-source "Retained build inputs differ from the current committed production source"; return 1
  fi
  server="$(jq -c '.details.candidate_build.server' "$receipt")" || return 1
  version="$(tar -xOf "$MG_RUN_DIR/current-source.tar" server/Cargo.toml | awk -F '"' '/^version = / {print $2; exit}')" || return 1
  if ! printf '%s\n' "$server" | jq -e --arg commit "$commit" --arg version "$version" '
    .kind=="meguri_sync" and .build==$commit and .version==$version and .source_commit==$commit
  ' >/dev/null; then mg_failure build-identity "Candidate peer differs from the current source identity"; return 1; fi
  target_owner="$(mg_sync_server__target_identity "$build_dir/target" retained)" || return 1
  if ! jq -e --arg target "$build_dir/target" --argjson owner "$target_owner" '
    .details.candidate_build.server as $server | .details.build_image.id as $image |
    .details.candidate_build_inputs=={
    command:["cargo","build","--locked","--no-default-features","--bin","meguri-sync-server","--message-format","json-render-diagnostics","--target-dir","/candidate-target","--profile","dev"],
    working_directory:"/work/server",target_directory:"/candidate-target",target_host_directory:$target,
    target_initially_empty:true,target_owner:$owner,container_owner:.details.candidate_build_inputs.container_owner,
    source_sha256:$server.source_sha256,lock_sha256:$server.lock_sha256,image_id:$image,
    profile:"debug",features:[],rustflags:"",encoded_rustflags:""} and
    (.details.candidate_build_inputs.container_owner | type=="string" and test("^[0-9a-f]{32}$"))
  ' "$receipt" >/dev/null; then mg_failure build-command "Candidate build command differs from the fixed producer"; return 1; fi
  for item in executable lock source; do
    case "$item" in executable) hash="$(shasum -a 256 "$binary" | awk '{print $1}')";; lock) hash="$(shasum -a 256 "$lock" | awk '{print $1}')";; source) hash="$(shasum -a 256 "$source" | awk '{print $1}')";; esac
    if ! printf '%s\n' "$server" | jq -e --arg field "${item}_sha256" --arg hash "$hash" '.[$field]==$hash' >/dev/null; then mg_failure build-binding "Candidate artifact hash mismatch"; return 1; fi
  done
  tar -xOf "$source" server/tools/candidate-setup.py >"$MG_RUN_DIR/candidate-setup.py" || return 1
  image="$(jq -er '.details.build_image.id' "$receipt")" || return 1
  if ! [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]]; then mg_failure build-image "Invalid measured build image"; return 1; fi
  if ! mg_sync_server__build_container_check "$build_dir/build-container.json" "$image" "$build_run" \
    "$(jq -er '.details.candidate_build_inputs.container_owner' "$receipt")" "$build_dir/source" "$build_dir/product" "$build_dir/target" ||
    ! cmp -s "$binary" "$build_dir/target/debug/meguri-sync-server" || ! jq -se '
    [.[] | select(.reason=="compiler-artifact" and .target.name=="meguri-sync-server" and .executable!=null)] |
    length==1 and .[0].fresh==false and .[0].features==[] and .[0].profile.test==false and
    .[0].profile.debug_assertions==true and .[0].profile.opt_level=="0" and
    .[0].executable=="/candidate-target/debug/meguri-sync-server"
  ' "$build_dir/product/build.jsonl" >/dev/null; then
    mg_failure build-measurement "Retained build image, command, mounts or Cargo artifact differs from the fixed producer"; return 1
  fi
  uid="$(id -u)"; gid="$(id -g)"
  # Secrets live outside retained artifacts and are mounted only by the fixed
  # client adapter. Setup receives them on stdin without command/log exposure.
  secret_dir="$(mktemp -d "${TMPDIR:-/tmp}/meguri-candidate.XXXXXX")" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json")"
  python3 - "$secret_dir" "$MG_RUN_DIR/secret-owner.json" <<'PY'
import json, os, sys
s = os.lstat(sys.argv[1])
with open(sys.argv[2], 'x') as f: json.dump([s.st_dev, s.st_ino, s.st_uid], f)
PY
  [ "$?" -eq 0 ] || return 1
  chmod 700 "$secret_dir" || return 1
  if ! python3 - "$secret_dir/credentials.json" <<'PY'
import json, os, sys
fd=os.open(sys.argv[1],os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
with os.fdopen(fd,'w') as f: json.dump({'username':'synthetic.member','password':os.urandom(32).hex()},f)
PY
  then return 1; fi
  mkdir "$MG_RUN_DIR/input" "$MG_RUN_DIR/input/artifacts" "$MG_RUN_DIR/output" || return 1
  candidate_name="meguri-candidate-$MG_RUN_ID"
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$candidate_name" "$owner")"
  cid="$(docker create --name "$candidate_name" --network none --read-only --cap-drop ALL --security-opt no-new-privileges \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" --user "$uid:$gid" --memory 2g --pids-limit 128 \
    --tmpfs "/data:mode=0700,uid=$uid,gid=$gid" --tmpfs /tmp:mode=1777,size=64m \
    --mount "type=bind,src=$binary,dst=/candidate,readonly" \
    --mount "type=bind,src=$MG_RUN_DIR/candidate-setup.py,dst=/owned/setup.py,readonly" \
    --entrypoint /bin/sh "$image" -c '/candidate init /data/owner https://candidate.example.test >/tmp/init.log && exec /candidate serve /data/owner https://candidate.example.test 127.0.0.1:8080 >/tmp/service.log 2>&1')" || return 1
  if ! [[ "$cid" =~ ^[0-9a-f]{64}$ ]]; then mg_failure container-identity "Invalid owned candidate container identity"; return 1; fi
  # Register every exchange hook in this owning MG job shell.
  mg_sync_server__interop_exchange "$cid" "$secret_dir" "$receipt" "$binary" "$lock" "$server" "$arguments" "$author_session" "$commit" || interop_rc=$?
  mg_sync_server__remove_container "$candidate_name" "$owner" || interop_rc=1
  mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json" || interop_rc=1
  if [ "$interop_rc" -ne 0 ]; then
    mg_detail candidate_compatibility_credit false
    mg_failure candidate-interop "Owned candidate invocation failed; no compatibility result"; return 1
  fi
  mg_artifact candidate-peer "$MG_RUN_DIR/input/peer.json"
  mg_artifact candidate-execution "$MG_RUN_DIR/execution-binding.json"
  mg_status pass
}

# Same-shell helper: failures return to the owner for checked normal cleanup;
# cancellation hooks stay registered until the MG job itself exits.
mg_sync_server__interop_exchange() {
  local cid="$1" secret_dir="$2" receipt="$3" binary="$4" lock="$5" server="$6" arguments="$7" author_session="$8" commit="$9"
  local hash item binding_hash receipt_hash
  docker start "$cid" >/dev/null || return 1
  docker inspect "$cid" >"$MG_RUN_DIR/container.json" || return 1
  jq -e --arg run "$MG_RUN_ID" --arg cid "$cid" 'length==1 and .[0].Id==$cid and .[0].State.Running and .[0].Config.Labels["meguri.mg.run-id"]==$run and .[0].HostConfig.NetworkMode=="none" and (.[0].HostConfig.PortBindings|length)==0' "$MG_RUN_DIR/container.json" >/dev/null || return 1
  docker exec -i "$cid" python3 /owned/setup.py <"$secret_dir/credentials.json" || return 1
  # Hash the executable actually held by PID 1 after setup readiness, not
  # merely the path from which a process might have been launched.
  hash="$(docker exec "$cid" sha256sum /proc/1/exe | awk '{print $1}')" || return 1
  printf '%s\n' "$server" | jq -e --arg hash "$hash" '.executable_sha256==$hash' >/dev/null || return 1
  jq -nc --argjson server "$server" --arg run "$MG_RUN_ID" --arg cid "$cid" --arg hash "$hash" \
    '{format:"meguri-sync-candidate-execution-v1",endpoint:"http://127.0.0.1:8080/",server:$server,run_id:$run,container_id:$cid,labels:{"meguri.mg.run-id":$run},observed_executable_sha256:$hash}' >"$MG_RUN_DIR/execution-binding.json" || return 1
  binding_hash="$(shasum -a 256 "$MG_RUN_DIR/execution-binding.json" | awk '{print $1}')"
  receipt_hash="$(shasum -a 256 "$receipt" | awk '{print $1}')"
  for item in "$binary" "$lock" "$receipt" "$MG_RUN_DIR/execution-binding.json"; do
    hash="$(shasum -a 256 "$item" | awk '{print $1}')"; cp "$item" "$MG_RUN_DIR/input/artifacts/$hash" || return 1
  done
  jq -nc --argjson server "$server" --arg receipt "$receipt_hash" --arg binding "$binding_hash" \
    --argjson invocation "$(jq -c '{job,run_id,arguments:.args}' "$receipt")" \
    '{format:"meguri-sync-package-v1",kind:"intermediate_build",server:$server,source_dirty:false,profile:"debug",features:[],executable_artifact_sha256:$server.executable_sha256,lock_artifact_sha256:$server.lock_sha256,mg_build_receipt_sha256:$receipt,build_invocation:$invocation,execution_binding_sha256:$binding}' >"$MG_RUN_DIR/input/peer.json" || return 1
  mg_sync_server_candidate_client "$cid" "$MG_RUN_DIR/input" "$secret_dir/credentials.json" "$MG_RUN_DIR/output" "$server" "$arguments" "$author_session" || return 1
  docker inspect "$cid" >"$MG_RUN_DIR/container-after.json" || return 1
  jq -e --arg run "$MG_RUN_ID" --arg cid "$cid" 'length==1 and .[0].Id==$cid and .[0].State.Running and .[0].Config.Labels["meguri.mg.run-id"]==$run and .[0].HostConfig.NetworkMode=="none" and (.[0].HostConfig.PortBindings|length)==0' "$MG_RUN_DIR/container-after.json" >/dev/null || return 1
  hash="$(docker exec "$cid" sha256sum /proc/1/exe | awk '{print $1}')" || return 1
  printf '%s\n' "$server" | jq -e --arg hash "$hash" '.executable_sha256==$hash' >/dev/null || return 1
  mg_sync_server__clean || return 1
  [ "$(git -C "$MG_ROOT" rev-parse HEAD)" = "$commit" ] || return 1
  local expected
  case "$(jq -r '.[3]' <<<"$arguments")" in
    full) expected=11 ;;
    normal) expected=10 ;;
    media) expected=4 ;;
    *) mg_failure candidate-phase "Unknown owned candidate execution phase"; return 1 ;;
  esac
  jq -se --argjson expected "$expected" 'add | .total==$expected and .passed==$expected and .failed==0' "$MG_STATE/counts.jsonl" >/dev/null || return 1
  jq -se 'add | (.reference_invocation|type)=="object" and (.reference_evidence|type)=="object"' "$MG_STATE/details.jsonl" >/dev/null || return 1
}

# Exact quarantined identity is measured only by the existing MG-loaded seam.
# Wiring these calls does not grant access to the hashed bytes.
mg_sync_server__reference_identity() {
  local source lock
  if ! declare -F mg_sync_reference_digest >/dev/null; then
    mg_failure reference-digest-unavailable "Required MG reference identity helper is not loaded"; return 1
  fi
  source="$(mg_sync_reference_digest)" || return 1
  lock="$(shasum -a 256 "$MG_ROOT/oracle/meguri-sync-server/requirements.lock" | awk '{print $1}')" || return 1
  if ! [[ "$source" =~ ^[0-9a-f]{64}$ ]] || [ "$lock" != 56e1bef86def9f5f230d12317e3510f2176eb079928f3a75f86afb0dcc408ec8 ]; then
    mg_failure reference-identity "Measured reference inputs or pinned lock are invalid"; return 1
  fi
  jq -nc --arg source "$source" --arg lock "$lock" '{source_sha256:$source,lock_sha256:$lock}'
}

mg_sync_server_candidate_client() {
  local cid="$1" input="$2" credentials="$3" output="$4" server="$5" arguments="$6" author_session="$7"
  local identity identity_after image uid gid client_rc=0 validation_rc=0 output_rc=0 report counts phase
  phase="$(jq -er '.[3] | select(.=="full" or .=="normal" or .=="media")' <<<"$arguments")" || return 1
  local validation="$MG_RUN_DIR/validation" client_name="meguri-candidate-client-$MG_RUN_ID" validation_name="meguri-candidate-validation-$MG_RUN_ID" owner
  mg_sync_server__author_session "$author_session" || return 1
  mg_detail candidate_compatibility_credit false
  if [ -L "$output" ] || [ ! -d "$output" ] || [ -n "$(ls -A "$output")" ]; then
    mg_failure candidate-output-stale "Client output must start as an empty owned directory"; return 1
  fi
  identity="$(mg_sync_server__reference_identity)" || return 1
  mg_detail candidate_reference_inputs "$identity"
  uid="$(id -u)"; gid="$(id -g)"
  # Refuse symlinks, extra links, wrong ownership and public permissions.
  python3 - "$credentials" "$uid" "$gid" <<'PY'
import json, os, stat, sys
s = os.lstat(sys.argv[1])
if not (stat.S_ISREG(s.st_mode) and stat.S_IMODE(s.st_mode) == 0o600
        and s.st_uid == int(sys.argv[2]) and s.st_gid == int(sys.argv[3]) and s.st_nlink == 1):
    raise SystemExit("invalid protected credentials ownership")
with open(sys.argv[1]) as f:
    credentials = json.load(f)
if set(credentials) != {"username", "password"} or not all(
    isinstance(value, str) and value for value in credentials.values()
):
    raise SystemExit("invalid protected credentials shape")
PY
  [ "$?" -eq 0 ] || return 1
  mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" --progress plain build sync-reference || return 1
  identity_after="$(mg_sync_server__reference_identity)" || return 1
  mg_detail candidate_reference_inputs_after_build "$identity_after"
  [ "$identity_after" = "$identity" ] || { mg_failure reference-input-changed "Reference inputs changed during image build"; return 1; }
  image="$(mg_sync_server__image sync-reference -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml")" || return 1
  [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]] || return 1
  # Reference identities stay distinct from candidate source/lock/binary hashes.
  jq -nc --argjson server "$server" \
    --arg job "$MG_JOB" --argjson args "$arguments" --arg run "$MG_RUN_ID" --arg session "$author_session" '
    {source_commit:$server.source_commit,batch_base:"aa8565719af435051de7d5f068c0c817286b568e",
     source_sha256:$server.source_sha256,lock_sha256:$server.lock_sha256,
     executable_sha256:$server.executable_sha256,mg_job:$job,arguments:$args,
     run_id:$run,author_session:$session}' >"$output/invocation.json" || return 1
  cp "$output/invocation.json" "$MG_RUN_DIR/candidate-invocation.json" || return 1
  mg_artifact reference-invocation "$output/invocation.json"
  mg_artifact reference-output "$output"
  mg_detail candidate_client_image "$(jq -nc --arg image "$image" '{id:$image}')"
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$client_name" "$owner")"
  docker run --name "$client_name" --rm --network "container:$cid" --read-only --cap-drop ALL \
    --security-opt no-new-privileges --memory 2g --pids-limit 128 --user "$uid:$gid" \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" -e MG_RUN_ID="$MG_RUN_ID" \
    -e TMPDIR=/scratch -e TZ=UTC -e PYTHONDONTWRITEBYTECODE=1 -e PYTHONUNBUFFERED=1 \
    --tmpfs /scratch:exec,mode=1777,size=1g --tmpfs /tmp:mode=1777,size=64m \
    --mount "type=bind,src=$input,dst=/in,readonly" \
    --mount "type=bind,src=$credentials,dst=/secret/credentials.json,readonly" \
    --mount "type=bind,src=$output,dst=/out" \
    --entrypoint python "$image" /opt/reference/driver.py \
    candidate --input /in --credentials /secret/credentials.json --phase "$phase" || client_rc=$?
  mg_detail candidate_client_exit "$client_rc"
  mg_sync_server__remove_container "$client_name" "$owner" || output_rc=1
  if [ -f "$output/summary.json" ]; then mg_artifact reference-summary "$output/summary.json"; fi
  if [ -f "$output/evidence.json" ]; then mg_artifact reference-evidence "$output/evidence.json"; fi
  # The pinned test tool owns artifact JSON semantics. Its validation output is
  # separate: execution invocation/summary remain untouched and counted once.
  mkdir "$validation" || return 1
  mg_artifact candidate-semantic-validation "$validation"
  if cp "$output/invocation.json" "$validation/invocation.json"; then
    mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$validation_name" "$owner")"
    docker run --name "$validation_name" --rm --network none --read-only --cap-drop ALL \
      --security-opt no-new-privileges --memory 2g --pids-limit 128 --user "$uid:$gid" \
      --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" -e MG_RUN_ID="$MG_RUN_ID" \
      -e TMPDIR=/scratch -e TZ=UTC -e PYTHONDONTWRITEBYTECODE=1 -e PYTHONUNBUFFERED=1 \
      --tmpfs /scratch:exec,mode=1777,size=1g --tmpfs /tmp:mode=1777,size=64m \
      --mount "type=bind,src=$output,dst=/in,readonly" \
      --mount "type=bind,src=$validation,dst=/out" \
      --entrypoint python "$image" /opt/reference/driver.py validate --input /in || validation_rc=$?
    mg_sync_server__remove_container "$validation_name" "$owner" || output_rc=1
  else validation_rc=1; fi
  mg_detail candidate_validation_exit "$validation_rc"
  report="$MG_RUN_DIR/candidate-output-validation.json"
  python3 "$MG_ROOT/server/tools/candidate-output.py" "$output" "$MG_RUN_DIR/candidate-invocation.json" \
    "$input/peer.json" "$MG_ROOT/conformance/recordings/scenarios/meguri-sync-server/candidate-$phase-selection.json" >"$report" || output_rc=$?
  mg_artifact candidate-output-validation "$report"
  # Publish parsed actual output, including failed output; never the pre-run
  # invocation in place of the driver's final selection or invented coverage.
  if jq -e '.summary | type=="object"' "$report" >/dev/null; then
    mg_detail reference_evidence "$(jq -c '.summary' "$report")"
    if counts="$(jq -er '.summary | [.total,.passed,.failed] | if all(.[]; type=="number" and .>=0 and .<=2147483647 and floor==.) then @tsv else error("invalid actual counts") end' "$report")"; then
      local total passed failed
      IFS=$'\t' read -r total passed failed <<<"$counts"
      mg_count total "$total"; mg_count passed "$passed"; mg_count failed "$failed"
    else output_rc=1; fi
  fi
  if jq -e '.invocation | type=="object"' "$report" >/dev/null; then
    mg_detail reference_invocation "$(jq -c '.invocation' "$report")"
  fi
  identity_after="$(mg_sync_server__reference_identity)" || return 1
  mg_detail candidate_reference_inputs_after_execution "$identity_after"
  jq -nc --argjson before "$identity" --argjson after "$identity_after" --arg image "$image" \
    '{before:$before,after:$after,image_id:$image}' >"$MG_RUN_DIR/reference-runtime.json" || return 1
  mg_artifact candidate-reference-runtime "$MG_RUN_DIR/reference-runtime.json"
  [ "$identity_after" = "$identity" ] || { mg_failure reference-input-changed "Reference inputs changed during execution"; return 1; }
  if [ "$client_rc" -ne 0 ]; then
    mg_failure "candidate-client-exit-$client_rc" "Actual Desktop candidate invocation failed; retain exact client error and outputs"
  fi
  if [ "$output_rc" -ne 0 ]; then
    mg_failure candidate-output-contract "Missing, incomplete or mismatched actual candidate output; inspect retained validation and runtime directory"
  fi
  if [ "$validation_rc" -ne 0 ]; then
    mg_failure "candidate-validation-exit-$validation_rc" "Pinned artifact semantic validation failed; retain execution and validation outputs"
  fi
  if [ "$client_rc" -eq 0 ] && [ "$validation_rc" -eq 0 ] && [ "$output_rc" -eq 0 ]; then
    mg_detail candidate_compatibility_credit true
    return 0
  fi
  return 1

}

mg_sync_server__env() {
  export CARGO_TARGET_DIR=/cargo-target/server
  if [ ! -f server/Cargo.toml ] || [ ! -f server/rust-toolchain.toml ]; then
    mg_failure workspace-missing "server manifest/toolchain is missing"
    mg_status error
    return 1
  fi
  # All cargo invocations run in server/ and use its toolchain pin.
  mg_detail environment "$(cd server && jq -nc --arg rustc "$(rustc -V)" --arg nextest "$(cargo nextest --version)" \
    --arg target "$CARGO_TARGET_DIR" '{rustc:$rustc,nextest:$nextest,target:$target}')"
}
mg_sync_server__locked() {
  if [ ! -f server/Cargo.lock ]; then
    mg_failure lock-missing "Generate the initial server lock explicitly with tools/mg sync-server-deps --resolve; normal checks never resolve it"
    mg_status fail
    return 1
  fi
}
mg_sync_server__web() {
  local web_work
  web_work="$(mktemp -d /tmp/meguri-server-web.XXXXXX)" || return 1
  if ! MEGURI_WEB_SOURCE="$MG_ROOT/server/web" MEGURI_WEB_WORK="$web_work" \
      MEGURI_WEB_OUTPUT="$MG_RUN_DIR/web" MEGURI_WEB_DEPS=/sync-web-deps \
      bash "$MG_ROOT/server/web/scripts/run.sh" build; then
    mg_failure web-preparation "Real locked browser asset preparation failed; inspect log.txt and web evidence"
    mg_status fail
    return 1
  fi
  export MEGURI_WEB_PREPARED_DIR="$MG_RUN_DIR/web/prepared"
  mg_artifact browser-build "$MG_RUN_DIR/web" "Source-bound compiled browser assets and distributed-code notices"
}
mg_sync_server_test() {
  [ "${1:-}" != -- ] || shift
  # Only separate filter operands are forwarded. No Cargo/config/output/policy
  # switches (or libtest arguments after another --) may change this receipt.
  local filters=() targets=(--all-targets)
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -E|--filter-expr|--test)
        if [ "$#" -lt 2 ] || [ -z "$2" ] || [[ "$2" = -* ]]; then
          mg_failure usage "filters require a nonempty separate value"; mg_status error; return 0
        fi
        [ "$1" != --test ] || targets=()
        filters+=("$1" "$2"); shift 2 ;;
      *)
        mg_failure usage "sync-server-test supports only -- [-E|--filter-expr <expr>] [--test <binary>]"; mg_status error; return 0 ;;
    esac
  done
  mg_sync_server__env && mg_sync_server__locked || return 0
  mg_sync_server__web || return 0
  local binary="$MG_RUN_DIR/binary.jsonl" out="$MG_RUN_DIR/nextest.jsonl" rc
  # Integration tests spawn this exact feature-gated executable for lock/kill
  # evidence. Default production build is checked separately by clippy.
  (cd server && cargo build --locked --bin meguri-sync-server --features test-hooks --message-format json-render-diagnostics) >"$binary"
  rc=$?
  mg_artifact server-build-jsonl "$binary"
  if [ "$rc" -ne 0 ]; then
    mg_failure "build-exit-$rc" "server binary build failed; see log.txt"; mg_status fail; return 0
  fi
  export MEGURI_SERVER_TEST_BINARY
  MEGURI_SERVER_TEST_BINARY="$(jq -r 'select(.reason=="compiler-artifact" and .target.name=="meguri-sync-server" and .executable!=null) | .executable' "$binary" | tail -n 1)"
  if [ ! -x "$MEGURI_SERVER_TEST_BINARY" ]; then
    mg_failure binary-missing "cargo did not report an executable"; mg_status fail; return 0
  fi
  (cd server && NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --locked --workspace ${targets[0]+"${targets[@]}"} --features test-hooks \
    --retries 0 --no-fail-fast --no-tests=fail --message-format libtest-json-plus --message-format-version 0.1 ${filters[0]+"${filters[@]}"}) >"$out"
  rc=$?
  mg_artifact nextest-jsonl "$out"
  local report="$MG_RUN_DIR/nextest-evidence.json" diagnostics="$MG_RUN_DIR/nextest-evidence.log"
  local passed=0 failed=0 skipped=0 evidence_ok=false counts
  # Parse one JSON object per line, retaining good completed events even when
  # another line is broken. Per-binary state permits interleaved suites/tests.
  # Ignored tests may have no started event; executed tests must have one.
  if jq -Rnc '
    def count: if type == "number" then . >= 0 and . <= 2147483647 and floor == . else false end;
    def text: if type == "string" then length > 0 else false end;
    def test_name: if type == "string" then test("^[^$]+\\$.+$") else false end;
    def bad($m): .errors |= (if length < 20 then . + [$m[0:500]] else . end);
    reduce inputs as $line (
      {line:0, suites:{}, tests:{}, passed:0, failed:0, skipped:0, errors:[], failures:[]};
      .line += 1 |
      (try ($line | fromjson) catch null) as $e |
      if ($e | type) != "object" then bad("invalid JSON object at line \(.line)")
      elif $e.type == "suite" then
        if ($e.nextest | type) != "object" then bad("missing suite identity")
        elif ([$e.nextest.crate, $e.nextest.test_binary, $e.nextest.kind] | all(.[]; text)) then
          ($e.nextest.crate + "::" + $e.nextest.test_binary + "$") as $key |
          if $e.event == "started" then
            if .suites[$key] != null then bad("duplicate suite start: " + $key)
            else
              .suites[$key] = {count:$e.test_count, kind:$e.nextest.kind, passed:0, failed:0, skipped:0, done:false} |
              if ($e.test_count | count) then . else bad("invalid suite test_count: " + $key) end
            end
          elif $e.event == "ok" or $e.event == "failed" then
            if .suites[$key] == null or .suites[$key].done then bad("missing start or duplicate suite completion: " + $key)
            else
              .suites[$key].done = true |
              if $e.nextest.kind != .suites[$key].kind then bad("inconsistent suite identity: " + $key) else . end |
              if ([$e.passed,$e.failed,$e.ignored,$e.measured,$e.filtered_out] | all(.[]; count)) then
                if $e.measured != 0 or
                   $e.passed != .suites[$key].passed or $e.failed != .suites[$key].failed or
                   $e.ignored != .suites[$key].skipped or
                   ($e.passed + $e.failed + $e.ignored) != .suites[$key].count
                then bad("inconsistent suite counts: " + $key) else . end
              else bad("invalid suite counts: " + $key) end |
              if $e.event == "failed" or $e.failed != 0 then bad("failed suite: " + $key) else . end
            end
          else bad("unsupported suite event") end
        else bad("missing suite identity") end
      elif $e.type == "test" then
        if ($e.name | test_name) then
          $e.name as $name | (($name | split("$")[0]) + "$") as $key |
          if .suites[$key] == null or .suites[$key].done then bad("test outside active suite: " + $name) else . end |
          if $e.event == "started" then
            if .tests[$name] != null then bad("duplicate or late test start: " + $name)
            else .tests[$name] = {done:false} end
          elif $e.event == "ok" or $e.event == "failed" or $e.event == "ignored" then
            if .tests[$name].done == true then bad("duplicate test completion: " + $name)
            else
              if .tests[$name] == null and $e.event != "ignored" then bad("test completion without start: " + $name) else . end |
              .tests[$name] = {done:true} |
              (if $e.event == "ok" then "passed" elif $e.event == "failed" then "failed" else "skipped" end) as $count |
              .[$count] += 1 |
              if .suites[$key] != null then .suites[$key][$count] += 1 else . end |
              if $e.event == "failed" and (.failures | length) < 50 then
                .failures += [{id:($name | .[0:500]),
                  message:(if ($e.stdout | type) == "string" then $e.stdout[0:2000] else "failed" end),
                  repro:"tools/mg sync-server-test"}]
              else . end
            end
          else bad("unsupported test event") end
        else bad("missing fully qualified test name") end
      else bad("unsupported nextest record") end
    ) |
    if (.suites | length) == 0 then bad("no suites") else . end |
    if any(.suites[]; .done != true) then bad("incomplete suites") else . end |
    if any(.tests[]; .done != true) then bad("incomplete tests") else . end |
    {passed,failed,skipped,errors,failures}
  ' "$out" >"$report" 2>"$diagnostics"; then
    # Check every extraction before passing numbers to shell arithmetic or MG.
    if counts="$(jq -er '[.passed,.failed,.skipped] | if all(.[]; type == "number" and . >= 0 and . <= 2147483647 and floor == .) then @tsv else error("invalid counts") end' "$report")" &&
       IFS=$'\t' read -r passed failed skipped <<<"$counts" &&
       jq -c '.failures[]' "$report" >>"$MG_STATE/failures.jsonl"; then
      evidence_ok=true
    fi
  fi
  mg_artifact nextest-evidence "$report"
  mg_artifact nextest-evidence-log "$diagnostics"
  mg_count total "$((passed+failed+skipped))"; mg_count passed "$passed"; mg_count failed "$failed"; mg_count skipped "$skipped"
  if [ "$evidence_ok" != true ] || ! jq -e '.errors == []' "$report" >/dev/null 2>&1; then
    mg_failure nextest-evidence "invalid or incomplete nextest evidence; inspect retained JSONL, evidence report and log"
    evidence_ok=false
  fi
  if [ "$rc" -ne 0 ] || [ "$failed" -ne 0 ] || [ "$passed" -eq 0 ]; then
    mg_failure "nextest-exit-$rc" "nextest failed or reported zero executed tests; inspect retained JSONL/log"
    evidence_ok=false
  fi
  if [ "$evidence_ok" = true ]; then
    mg_message "sync-server-test: $passed passed, $skipped skipped (zero retries)"; mg_status pass
  else mg_status fail; fi
}
mg_sync_server_lint() {
  [ "$#" -eq 0 ] || { mg_failure usage "sync-server-lint takes no arguments"; mg_status error; return 0; }
  mg_sync_server__env && mg_sync_server__locked || return 0
  mg_sync_server__web || return 0
  local rc=0
  (cd server && cargo clippy --locked --workspace --all-targets --all-features -- -D clippy::correctness -D clippy::suspicious) >"$MG_RUN_DIR/clippy.log" 2>&1 || rc=$?
  # Also prove that the default production graph builds without testing seams.
  (cd server && cargo clippy --locked --workspace --all-targets --no-default-features -- -D clippy::correctness -D clippy::suspicious) >>"$MG_RUN_DIR/clippy.log" 2>&1 || rc=$?
  mg_artifact clippy-log "$MG_RUN_DIR/clippy.log"
  if [ "$rc" -eq 0 ]; then mg_status pass; else mg_failure clippy "server clippy failed; see clippy.log"; mg_status fail; fi
}
mg_sync_server_format() {
  local fmt_args=(--all -- --check)
  if [ "${1:-}" = --write ] && [ "$#" -eq 1 ]; then fmt_args=(--all)
  elif [ "$#" -ne 0 ]; then mg_failure usage "sync-server-format [--write]"; mg_status error; return 0; fi
  mg_sync_server__env || return 0
  if (cd server && cargo fmt "${fmt_args[@]}"); then mg_status pass; else mg_failure rustfmt "server rustfmt failed; see log.txt"; mg_status fail; fi
}
mg_sync_server_deps() {
  local LC_ALL=C
  local resolve=false resolve_existing=false update_package= update_version=
  local package_pattern='^[A-Za-z0-9][A-Za-z0-9_-]*$'
  local version_pattern='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?(\+[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?$'
  if [ "${1:-}" = --resolve ] && [ "$#" -eq 1 ]; then resolve=true
  elif [ "${1:-}" = --resolve-existing ] && [ "$#" -eq 1 ]; then resolve_existing=true
  elif [ "${1:-}" = --update ] && [ "$#" -eq 3 ] && [[ "$2" =~ $package_pattern ]] && [[ "$3" =~ $version_pattern ]]; then
    update_package="$2"; update_version="$3"
  elif [ "$#" -ne 0 ]; then mg_failure usage "sync-server-deps [--resolve | --resolve-existing | --update <package> <exact-version>]"; mg_status error; return 0; fi
  mg_sync_server__env || return 0
  if [ "$resolve" = true ]; then
    if [ -f server/Cargo.lock ]; then mg_failure lock-exists "--resolve is initial generation only; inspect existing lock explicitly"; mg_status fail; return 0; fi
    if ! (cd server && cargo generate-lockfile); then mg_failure resolve "initial server lock generation failed"; mg_status fail; return 0; fi
  fi
  mg_sync_server__locked || return 0
  if [ "$resolve_existing" = true ]; then
    # Explicit manifest reconciliation, retaining already locked external
    # versions where they still satisfy requirements. Normal jobs stay locked.
    cp server/Cargo.lock "$MG_RUN_DIR/Cargo.lock.before" || { mg_failure lock-copy "cannot retain original lock"; mg_status fail; return 0; }
    mg_artifact cargo-lock-before "$MG_RUN_DIR/Cargo.lock.before"
    if ! (cd server && cargo update --workspace); then
      mg_artifact cargo-lock-after-failure "$MG_ROOT/server/Cargo.lock"
      mg_failure resolve-existing "existing-lock manifest resolution failed; retain both lock states"; mg_status fail; return 0
    fi
    mg_detail lock_resolution '"existing-lock workspace manifest reconciliation"'
  fi
  if [ -n "$update_package" ]; then
    local update_rc=0
    (cd server && cargo update --package "$update_package" --precise "$update_version") || update_rc=$?
    if [ "$update_rc" -ne 0 ]; then
      if cp server/Cargo.lock "$MG_RUN_DIR/Cargo.lock"; then
        mg_artifact cargo-lock "$MG_RUN_DIR/Cargo.lock" "lockfile after failed cargo update"
      else
        mg_failure lockfile-artifact "could not retain Cargo.lock after failed cargo update; see log.txt"
      fi
      mg_failure "cargo-update-exit-$update_rc" "cargo update failed; inspect retained lock and log.txt"; mg_status fail; return 0
    fi
    mg_message "sync-server-deps: updated $update_package to $update_version; checking resolved graph"
  fi
  mg_sync_server__web || return 0
  local rc=0 edge
  cp server/Cargo.lock "$MG_RUN_DIR/Cargo.lock" || rc=1
  cp server/deny.toml "$MG_RUN_DIR/deny.toml" || rc=1
  cp server/about.toml "$MG_RUN_DIR/about.toml" || rc=1
  (cd server && cargo metadata --locked --format-version 1 --all-features) >"$MG_RUN_DIR/metadata.json" || rc=1
  for edge in normal build dev; do
    (cd server && cargo tree --locked --all-features --edges "$edge,features") >"$MG_RUN_DIR/tree-$edge.txt" || rc=1
    mg_artifact "tree-$edge" "$MG_RUN_DIR/tree-$edge.txt"
  done
  (cd server && cargo deny --locked --format json --config deny.toml check) >"$MG_RUN_DIR/deny.stdout" 2>"$MG_RUN_DIR/deny.jsonl" || rc=1
  # cargo-about's built-in JSON renderer avoids introducing a notice framework.
  (cd server && cargo about generate --locked --all-features --config about.toml --format json) >"$MG_RUN_DIR/notices.json" 2>"$MG_RUN_DIR/about.log" || rc=1
  mg_artifact cargo-lock "$MG_RUN_DIR/Cargo.lock"
  mg_artifact deny-config "$MG_RUN_DIR/deny.toml"
  mg_artifact about-config "$MG_RUN_DIR/about.toml"
  mg_artifact cargo-metadata "$MG_RUN_DIR/metadata.json"
  mg_artifact cargo-deny "$MG_RUN_DIR/deny.jsonl"
  mg_artifact cargo-notices "$MG_RUN_DIR/notices.json"
  mg_artifact cargo-about-log "$MG_RUN_DIR/about.log"
  if [ "$rc" -eq 0 ]; then mg_status pass; else mg_failure dependencies "one or more dependency evidence/audit commands failed; inspect retained artifacts"; mg_status fail; fi
}
