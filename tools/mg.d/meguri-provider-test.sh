# SPDX-License-Identifier: MIT
mg_job meguri-provider-real-server host mg_meguri_provider_real_server "Actual packaged-server CoreHost upload, fresh download and reopen with all media"

mg_meguri_provider__nonce() { python3 -c 'import secrets; print(secrets.token_hex(16))'; }
mg_meguri_provider__commit() { git -C "$MG_ROOT" rev-parse HEAD; }
mg_meguri_provider__source() {
  mg_sync_server__clean && [ "$(mg_meguri_provider__commit)" = "$provider_commit" ]
}
mg_meguri_provider__receipt() {
  local explicit="$1" latest current
  if [ -n "$explicit" ]; then
    mg_sync_package__receipt "$explicit" || return 1
    provider_receipt="$explicit"; mg_detail package_resolution '"explicit-exact-receipt"'; return 0
  fi
  # Resolve precisely one latest link. Never walk back past a later failure.
  latest="$MG_ROOT/build/mg/sync-server-package/latest"
  if [ -e "$latest" ] || [ -L "$latest" ]; then
    latest="$(cd "$latest" && pwd -P)/result.json" || return 1
    current="$(jq -er '.commit.sha | select(type=="string" and length>0)' "$latest")" || return 1
    if [ "$current" = "$provider_commit" ]; then
      mg_sync_package__receipt "$latest" || return 1
      provider_receipt="$latest"; mg_detail package_resolution '"latest-exact-clean-pass"'; return 0
    fi
    mg_detail package_resolution '"build-once-stale-commit"'
  else mg_detail package_resolution '"build-once-no-current-build"'; fi
  provider_receipt="$("$MG_TOOLS/mg" sync-server-package)" || return 1
  mg_sync_package__receipt "$provider_receipt"
}
mg_meguri_provider__owned() {
  local name="$1" owner="$2" image="$3" state="$4"
  docker inspect "$name" | jq -e --arg name "/$name" --arg run "$MG_RUN_ID" --arg owner "$owner" \
    --arg image "$image" --arg state "$state" '
    length==1 and .[0].Name==$name and .[0].Image==$image and
    .[0].Config.Labels["meguri.mg.run-id"]==$run and .[0].Config.Labels["meguri.sync.owner"]==$owner and
    (if $state=="running" then .[0].State.Running==true
     else .[0].State.Running==false and .[0].State.ExitCode==0 end)' >/dev/null
}
mg_meguri_provider__remove() {
  mg_sync_server__remove_container "$1" "$2" || {
    mg_failure provider-cleanup "Owned client cleanup could not be proved"; mg_status fail; return 1;
  }
}
mg_meguri_provider__volume_cleanup() {
  [ -f "$2" ] || { mg_sync_package__volume_remove "$1" && touch "$2"; }
}
mg_meguri_provider__stop_runtime() {
  local MG_SYNC_INSTANCE="$1"
  mg_sync_package__stop || {
    mg_failure provider-runtime-cleanup "Owned runtime stop could not be proved"; mg_status fail; return 1;
  }
}
mg_meguri_provider__build() {
  local name="meguri-provider-build-$MG_RUN_ID" owner binary
  owner="$(mg_meguri_provider__nonce)" || return 1
  mg_compose --progress plain build dev || return 1
  provider_dev_image="$(mg_sync_server__image dev)" || return 1
  [[ "$provider_dev_image" =~ ^sha256:[0-9a-f]{64}$ ]] || return 1
  mg_on_exit "$(printf '%q ' mg_meguri_provider__remove "$name" "$owner")"
  mg_meguri_provider__source || return 1
  # Retain the container until its reported artifact is copied, including on a
  # failed compile. The JSON and compiler diagnostics are private evidence.
  mg_compose run -T --no-deps --pull never --name "$name" \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" \
    --entrypoint bash dev -c 'cd core && cargo build --locked -p meguri-host --example real_server_provider --features test-hooks --message-format=json-render-diagnostics' \
    >"$provider_private/build.jsonl" 2>"$provider_private/build.stderr" || return 1
  mg_meguri_provider__owned "$name" "$owner" "$provider_dev_image" exited || return 1
  binary="$(jq -sre '[.[] | select(.reason=="compiler-artifact" and
    (.package_id | test("(^|[/#])meguri-host([#@]|$)")) and
    .target.name=="real_server_provider" and .target.kind==["example"] and .executable!=null) | .executable]
    | if length==1 and (.[0] | startswith("/")) then .[0] else error("one exact executable required") end' \
    "$provider_private/build.jsonl")" || return 1
  docker cp "$name:$binary" "$MG_RUN_DIR/product/real_server_provider" || return 1
  chmod 0555 "$MG_RUN_DIR/product/real_server_provider" || return 1
  provider_binary_sha="$(shasum -a 256 "$MG_RUN_DIR/product/real_server_provider" | awk '{print $1}')" || return 1
  mg_meguri_provider__remove "$name" "$owner" || return 1
  mg_meguri_provider__source
}
mg_meguri_provider__fixture_wait() {
  # A single bounded log follower, no HTTP readiness probes or retries. Logs
  # stay private; only the constant marker is interpreted.
  python3 - "$1" <<'PY'
import os, selectors, subprocess, sys, time
p = subprocess.Popen(['docker','logs','--follow',sys.argv[1]], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
s = selectors.DefaultSelector(); s.register(p.stdout, selectors.EVENT_READ)
deadline = time.monotonic() + 30
buf = b''
try:
    while True:
        left = deadline - time.monotonic()
        if left <= 0 or not s.select(left): raise ValueError('fixture deadline')
        chunk = os.read(p.stdout.fileno(),4096)
        if not chunk: raise ValueError('fixture ended')
        buf = (buf + chunk)[-32768:]
        if b'MEGURI_PROVIDER_FIXTURE_READY' in buf.splitlines():
            break
except Exception:
    raise SystemExit('provider fixture startup failed')
finally:
    s.close(); p.terminate(); p.wait(timeout=5)
PY
}
mg_meguri_provider__fixture() {
  provider_fixture_name="meguri-provider-fixture-$MG_RUN_ID"
  provider_fixture_owner="$(mg_meguri_provider__nonce)" || return 1
  mg_on_exit "$(printf '%q ' mg_meguri_provider__remove "$provider_fixture_name" "$provider_fixture_owner")"
  docker run -d --name "$provider_fixture_name" --init --network "container:$provider_service" \
    --user 10001:10001 --read-only --cap-drop ALL --security-opt no-new-privileges \
    --tmpfs /tmp:uid=10001,gid=10001,mode=0700,size=128m \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$provider_fixture_owner" \
    --mount "type=volume,src=$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json"),dst=/data,readonly" \
    --mount "type=bind,src=$MG_ROOT/server/tools/package-smoke.mjs,dst=/package-smoke.mjs,readonly" \
    --entrypoint node "$provider_tool_image" /package-smoke.mjs provider-fixture \
    >"$provider_private/fixture-id" || return 1
  mg_meguri_provider__fixture_wait "$provider_fixture_name" || return 1
  mg_meguri_provider__owned "$provider_fixture_name" "$provider_fixture_owner" "$provider_tool_image" running || return 1
  docker cp "$provider_fixture_name:/tmp/provider-fixture/input.json" "$provider_private/fixture/input.json" || return 1
  docker cp "$provider_fixture_name:/tmp/provider-fixture/ca.der" "$provider_private/fixture/ca.der" || return 1
  chmod 0600 "$provider_private/fixture/input.json" "$provider_private/fixture/ca.der" || return 1
}
mg_meguri_provider__wait_client() {
  # Start and terminal observation share one deadline. Timeout is a failure and the
  # registered nonce/run-checked remover owns the still-running container.
  python3 - "$1" <<'PY'
import subprocess, sys, time
try:
    deadline = time.monotonic() + 180
    started = subprocess.run(['docker','start',sys.argv[1]], capture_output=True, timeout=180)
    if started.returncode: raise ValueError('client start')
    remaining = deadline - time.monotonic()
    if remaining <= 0: raise ValueError('client deadline')
    p = subprocess.run(['docker','wait',sys.argv[1]], capture_output=True, timeout=remaining)
    if p.returncode or p.stdout.strip() != b'0': raise ValueError('client exit')
except Exception:
    raise SystemExit('provider client failed or exceeded phase deadline')
PY
}
mg_meguri_provider__result() {
  local phase="$1" result="$2"
  jq -e --arg phase "$phase" '
    type=="object" and keys==["checks","counts","format","phase","status"] and
    .format==1 and .phase==$phase and .status=="pass" and
    .counts=={notes:2,cards:2,media:2,revlogs:0,reps:0} and
    (.checks | type=="array" and length>0 and all(.[]; type=="string" and test("^[a-z][a-z0-9-]+$")) and length==(unique|length)) and
    .checks==(if $phase=="upload" then ["fresh-upload-client","explicit-collection-and-media","retained-safety-artifacts","all-media-accounted","ready-automatic","exact-fixture","closed"]
      elif $phase=="download" then ["initial-no-collection","explicit-collection-and-media","initial-no-backup","all-media-accounted","ready-automatic","exact-fixture","closed"]
      elif $phase=="reopen" then ["cold-ready","persisted-binding-epoch","exact-fixture","closed"] else [] end)' "$result" >/dev/null
}
mg_meguri_provider__phase() {
  local phase="$1" root="$2" name="meguri-provider-$MG_RUN_ID-$1" owner output="$provider_private/$1-output"
  local extra=() operands=() network="container:$provider_service" uid gid rc=0
  owner="$(mg_meguri_provider__nonce)" || return 1
  uid="$(id -u)"; gid="$(id -g)"
  mkdir "$output" || return 1
  if [ "$phase" != upload ]; then
    extra=(--mount "type=bind,src=$provider_private/expected,dst=/expected,readonly")
    operands=(--expected /expected)
  fi
  mg_on_exit "$(printf '%q ' mg_meguri_provider__remove "$name" "$owner")"
  docker create --name "$name" --init --network "$network" --user "$uid:$gid" \
    --read-only --cap-drop ALL --security-opt no-new-privileges --tmpfs /tmp:exec,mode=1777,size=128m \
    -e TMPDIR=/tmp -e HOME=/tmp \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" \
    --mount "type=bind,src=$MG_RUN_DIR/product/real_server_provider,dst=/client,readonly" \
    --mount "type=bind,src=$provider_private/fixture,dst=/fixture,readonly" \
    --mount "type=bind,src=$root,dst=/state" --mount "type=bind,src=$output,dst=/out" \
    ${extra[0]+"${extra[@]}"} --entrypoint /client "$provider_dev_image" "$phase" \
    --fixture /fixture/input.json --root /state --output /out \
    ${operands[0]+"${operands[@]}"} \
    >"$provider_private/$phase-id" || return 1
  mg_meguri_provider__wait_client "$name" || rc=1
  docker logs "$name" >"$provider_private/$phase.log" 2>&1 || rc=1
  [ "$rc" -eq 0 ] || return 1
  mg_meguri_provider__owned "$name" "$owner" "$provider_dev_image" exited || return 1
  mg_meguri_provider__remove "$name" "$owner" || return 1
  mg_meguri_provider__result "$phase" "$output/result.json" || return 1
  cp "$output/result.json" "$MG_RUN_DIR/$phase.json" || return 1
  mg_artifact provider-phase "$MG_RUN_DIR/$phase.json"
  if [ "$phase" = upload ]; then
    cp "$output/expected-facts.json" "$provider_private/expected/expected-facts.json" || return 1
    chmod 0400 "$provider_private/expected/expected-facts.json" || return 1
  fi
}
mg_meguri_provider__run() {
  local MG_SYNC_INSTANCE="$MG_RUN_DIR/instance"
  local MEGURI_SYNC_ORIGIN=https://localhost:18443 MEGURI_SYNC_TRUSTED_PEER=127.0.0.1 MEGURI_SYNC_NETWORK=none
  local provider_private="$MG_RUN_DIR/private" provider_service provider_tool_image provider_dev_image provider_binary_sha
  local provider_fixture_name provider_fixture_owner provider_runtime_image
  local old_umask
  old_umask="$(umask)"; umask 077
  mkdir "$provider_private" "$MG_RUN_DIR/product" || return 1
  mkdir "$provider_private/fixture" "$provider_private/A" "$provider_private/B" "$provider_private/expected" || return 1
  umask "$old_umask"
  mg_on_exit "$(printf '%q ' mg_meguri_provider__volume_cleanup "$MG_SYNC_INSTANCE" "$MG_RUN_DIR/volume-removed")"
  mg_on_exit "$(printf '%q ' mg_meguri_provider__stop_runtime "$MG_SYNC_INSTANCE")"
  mg_sync_package__init "$provider_receipt" || return 1
  mg_sync_package__start || return 1
  # Use the actual returned immutable container identity, never an assumed name.
  provider_service="$(jq -er '.[0].Id' "$MG_RUN_DIR/service-container.json")" || return 1
  provider_runtime_image="$(jq -er .details.package.image_id "$provider_receipt")" || return 1
  provider_tool_image="$(jq -er .details.package.client_image_id "$provider_receipt")" || return 1
  mg_meguri_provider__build || return 1
  mg_meguri_provider__fixture || return 1
  mg_meguri_provider__phase upload "$provider_private/A" || return 1
  # Only the small synthetic JSON crosses clients. B has never mounted A.
  [ -z "$(ls -A "$provider_private/B")" ] || return 1
  mg_meguri_provider__phase download "$provider_private/B" || return 1
  mg_meguri_provider__phase reopen "$provider_private/B" || return 1
  mg_meguri_provider__owned "$provider_fixture_name" "$provider_fixture_owner" "$provider_tool_image" running || return 1
  docker stop --time 30 "$provider_fixture_name" >"$provider_private/fixture-stop" || return 1
  mg_meguri_provider__owned "$provider_fixture_name" "$provider_fixture_owner" "$provider_tool_image" exited || return 1
  mg_meguri_provider__remove "$provider_fixture_name" "$provider_fixture_owner" || return 1
  mg_sync_package__stop || return 1
  mg_meguri_provider__volume_cleanup "$MG_SYNC_INSTANCE" "$MG_RUN_DIR/volume-removed" || return 1
  mg_meguri_provider__source || return 1
  mg_detail identities "$(jq -nc --arg source "$provider_commit" --arg runtime "$provider_runtime_image" \
    --arg tool "$provider_tool_image" --arg dev "$provider_dev_image" --arg binary "$provider_binary_sha" \
    '{source_commit:$source,runtime_image_id:$runtime,fixture_image_id:$tool,dev_image_id:$dev,client_sha256:$binary}')"
  mg_count total 3; mg_count passed 3; mg_count failed 0; mg_count skipped 0
}
mg_meguri_provider_real_server() {
  local explicit= provider_receipt provider_commit
  if [ "$#" -eq 2 ] && [ "$1" = --package-receipt ] && [ -n "$2" ]; then explicit="$2"
  elif [ "$#" -ne 0 ]; then
    mg_failure usage "meguri-provider-real-server [--package-receipt <result.json>]"; mg_status error; return
  fi
  mg_sync_server__clean || { mg_status fail; return; }
  provider_commit="$(mg_meguri_provider__commit)" || { mg_status fail; return; }
  if ! mg_meguri_provider__receipt "$explicit"; then
    mg_failure provider-package-receipt "Exact latest or explicit clean package receipt is invalid; no retry"; mg_status fail; return
  fi
  mg_artifact exact-image-build "$provider_receipt"
  if ! mg_meguri_provider__run; then
    mg_failure provider-integration "CoreHost bootstrap integration failed; private evidence retained, no retries"; mg_status fail; return
  fi
  mg_status pass
}
