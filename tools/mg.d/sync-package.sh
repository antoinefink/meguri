# SPDX-License-Identifier: MIT
# Host orchestration only. All compilation and HTTP clients execute in Docker.
mg_job sync-server-package host mg_sync_package_build "Build the pinned production image and separate synthetic client image from clean committed inputs"
mg_job sync-server-package-test host mg_sync_package_test "Build and smoke the exact non-root image with private HTTPS, native content/media and restart persistence"
mg_job sync-server-local host mg_sync_package_local "Local image/volume: init, start, stop, rotate-setup, recover-admin <account-id> [--reenable]"

mg_sync_package__failure() {
  mg_artifact packaging-evidence "$MG_RUN_DIR"
  mg_failure sync-package "$1"
  mg_status fail
}
mg_sync_package__archive() {
  git -C "$MG_ROOT" archive --format=tar HEAD -- \
    server/Dockerfile server/Dockerfile.dockerignore server/Cargo.toml server/Cargo.lock \
    server/rust-toolchain.toml server/build.rs server/LICENSE server/NOTICE server/about.toml \
    server/src server/web server/examples/package-native-smoke.rs server/tools/package-smoke.mjs \
    core/Cargo.toml core/rust-toolchain.toml core/crates/meguri-model
}
mg_sync_package_build() {
  [ "$#" -eq 0 ] || { mg_failure usage "sync-server-package takes no arguments"; mg_status error; return; }
  mg_sync_server__clean || { mg_status fail; return; }
  local commit deps web_deps image smoke name owner rc=0
  commit="$(git -C "$MG_ROOT" rev-parse HEAD)" || return 1
  # Retain the existing graph/license/advisory audit, without another license tool.
  deps="$("$MG_TOOLS/mg" sync-server-deps)" || { mg_sync_package__failure "Locked dependency/notice audit failed"; return; }
  if ! jq -e --arg commit "$commit" --arg root "$MG_ROOT" '
    .job=="sync-server-deps" and .status=="pass" and .dry_run==false and .args==[] and
    .commit=={sha:$commit,dirty:false} and .root==$root' "$deps" >/dev/null; then
    mg_sync_package__failure "Dependency evidence is stale or dirty"; return
  fi
  mg_artifact dependency-receipt "$deps"
  web_deps="$("$MG_TOOLS/mg" sync-web-deps)" || { mg_sync_package__failure "Locked frontend audit failed"; return; }
  if ! jq -e --arg commit "$commit" --arg root "$MG_ROOT" '
    .job=="sync-web-deps" and .status=="pass" and .dry_run==false and .args==[] and
    .commit=={sha:$commit,dirty:false} and .root==$root' "$web_deps" >/dev/null; then
    mg_sync_package__failure "Frontend audit evidence is stale or dirty"; return
  fi
  mg_artifact frontend-dependency-receipt "$web_deps"
  mg_sync_package__archive >"$MG_RUN_DIR/source.tar" || { mg_sync_package__failure "Source freeze failed"; return; }
  python3 "$MG_ROOT/server/tools/package-context.py" "$MG_RUN_DIR/source.tar" \
    "$(dirname "$deps")/notices.json" "$MG_RUN_DIR/context" "$commit" || { mg_sync_package__failure "Narrow context/license assembly failed"; return; }
  image="$(mg_compose_project)-sync-runtime:local"
  smoke="$(mg_compose_project)-sync-package-client:local"
  if ! BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker build --progress plain --target runtime \
      --label "org.opencontainers.image.revision=$commit" -t "$image" \
      -f "$MG_RUN_DIR/context/server/Dockerfile" "$MG_RUN_DIR/context"; then
    mg_sync_package__failure "Production Dockerfile build failed"; return
  fi
  image="$(docker image inspect --format '{{.Id}}' "$image")" || return 1
  # A separate target carries Node, TLS fixture and protocol client. Never run
  # these inside or copy them into the actual service image.
  if ! BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker build --progress plain --target smoke -t "$smoke" \
      -f "$MG_RUN_DIR/context/server/Dockerfile" "$MG_RUN_DIR/context"; then
    mg_sync_package__failure "Synthetic client image build failed"; return
  fi
  smoke="$(docker image inspect --format '{{.Id}}' "$smoke")" || return 1
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  name="meguri-package-inventory-$MG_RUN_ID"
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  docker create --name "$name" --label "meguri.mg.run-id=$MG_RUN_ID" \
    --label "meguri.sync.owner=$owner" "$image" >"$MG_RUN_DIR/inventory-container-id" || rc=1
  [ "$rc" -ne 0 ] || docker export "$name" >"$MG_RUN_DIR/runtime.tar" || rc=1
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  if [ "$rc" -ne 0 ] || ! python3 "$MG_ROOT/server/tools/package-inventory.py" \
      "$MG_RUN_DIR/runtime.tar" "$MG_RUN_DIR/context" "$MG_RUN_DIR/product"; then
    mg_sync_package__failure "Actual runtime inventory/license/feature check failed"; return
  fi
  docker image inspect "$image" >"$MG_RUN_DIR/image.json" || return 1
  if ! jq -e --arg image "$image" --arg commit "$commit" '
    length==1 and .[0].Id==$image and .[0].Config.User=="10001:10001" and
    .[0].Config.Entrypoint==["/usr/local/bin/meguri-sync-server"] and
    .[0].Config.Labels["org.opencontainers.image.revision"]==$commit' "$MG_RUN_DIR/image.json" >/dev/null; then
    mg_sync_package__failure "Runtime image identity/config differs"; return
  fi
  mg_sync_server__clean || { mg_status fail; return; }
  [ "$(git -C "$MG_ROOT" rev-parse HEAD)" = "$commit" ] || { mg_sync_package__failure "Source advanced during build"; return; }
  mg_detail package "$(jq -nc --arg image "$image" --arg smoke "$smoke" --arg commit "$commit" \
    --arg inputs "$(shasum -a 256 "$MG_RUN_DIR/context/package-inputs.json" | awk '{print $1}')" \
    --arg inventory "$(shasum -a 256 "$MG_RUN_DIR/product/runtime-inventory.json" | awk '{print $1}')" \
    '{image_id:$image,client_image_id:$smoke,source_commit:$commit,inputs_sha256:$inputs,runtime_inventory_sha256:$inventory,features:[],profile:"release"}')"
  mg_artifact package-inputs "$MG_RUN_DIR/context/package-inputs.json"
  mg_artifact package-source "$MG_RUN_DIR/source.tar"
  mg_artifact package-runtime "$MG_RUN_DIR/runtime.tar"
  mg_artifact package-product "$MG_RUN_DIR/product"
  mg_artifact package-image "$MG_RUN_DIR/image.json"
  mg_status pass
}

mg_sync_package__receipt() {
  local receipt="$1"
  jq -e --arg root "$MG_ROOT" --arg commit "$(git -C "$MG_ROOT" rev-parse HEAD)" '
    .job=="sync-server-package" and .status=="pass" and .dry_run==false and .args==[] and .root==$root and
    .commit=={sha:$commit,dirty:false} and .details.package.source_commit==$commit and
    .details.package.features==[] and .details.package.profile=="release" and
    (.details.package.image_id | test("^sha256:[0-9a-f]{64}$")) and
    (.details.package.client_image_id | test("^sha256:[0-9a-f]{64}$"))' "$receipt" >/dev/null || return 1
  local directory
  directory="$(cd "$(dirname "$receipt")" && pwd -P)" || return 1
  [ "$directory" = "$MG_ROOT/build/mg/sync-server-package/$(jq -r .run_id "$receipt")" ] && [ "$(basename "$receipt")" = result.json ] || return 1
  [ "$(shasum -a 256 "$directory/context/package-inputs.json" | awk '{print $1}')" = "$(jq -r .details.package.inputs_sha256 "$receipt")" ] || return 1
  [ "$(shasum -a 256 "$directory/product/runtime-inventory.json" | awk '{print $1}')" = "$(jq -r .details.package.runtime_inventory_sha256 "$receipt")" ]
}
mg_sync_package__compose() {
  MEGURI_SYNC_IMAGE="$(jq -r .image "$MG_SYNC_INSTANCE/instance.json")" \
  MEGURI_SYNC_VOLUME="$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json")" \
  MEGURI_SYNC_ORIGIN="$(jq -r .origin "$MG_SYNC_INSTANCE/instance.json")" \
  MEGURI_SYNC_TRUSTED_PEER="$(jq -r .peer "$MG_SYNC_INSTANCE/instance.json")" \
  MEGURI_SYNC_PORT="$(jq -r .port "$MG_SYNC_INSTANCE/instance.json")" \
  MEGURI_SYNC_OWNER="$MG_SYNC_CONTAINER_OWNER" \
  MEGURI_SYNC_NETWORK="${MEGURI_SYNC_NETWORK:-bridge}" \
    mg_compose -f "$MG_ROOT/server/compose.yaml" "$@"
}
mg_sync_package__volume_check() {
  docker volume inspect "$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json")" | jq -e \
    --arg owner "$(jq -r .owner "$MG_SYNC_INSTANCE/instance.json")" \
    --arg volume "$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json")" '
    length==1 and .[0].Name==$volume and .[0].Labels["meguri.sync.owner"]==$owner' >/dev/null
}
mg_sync_package__volume_remove() {
  [ -f "$1/instance.json" ] || return 0
  local MG_SYNC_INSTANCE="$1" volume
  volume="$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json")" || return 1
  # No absent-volume fallback hides a failed ownership observation.
  if mg_sync_package__volume_check && docker volume rm "$volume" >/dev/null; then return 0; fi
  mg_sync_package__failure "Cannot prove cleanup of the owned synthetic volume"
  return 1
}
mg_sync_package__stop() {
  [ -f "$MG_SYNC_INSTANCE/container.json" ] || return 0
  local name owner run rc=0
  name="$(jq -r .name "$MG_SYNC_INSTANCE/container.json")" || return 1
  owner="$(jq -r .owner "$MG_SYNC_INSTANCE/container.json")" || return 1
  run="$(jq -r .run "$MG_SYNC_INSTANCE/container.json")" || return 1
  # Existing cleanup proves the exact name/run/creation nonce before removal.
  # Graceful stop is subject to the same proof; do not signal by name alone.
  local ids
  ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")" || return 1
  if [ -n "$ids" ]; then
    docker inspect "$ids" | jq -e --arg name "/$name" --arg owner "$owner" --arg run "$run" '
      length==1 and .[0].Name==$name and .[0].Config.Labels["meguri.sync.owner"]==$owner and
      .[0].Config.Labels["meguri.mg.run-id"]==$run' >/dev/null || return 1
    docker stop --time 30 "$ids" >/dev/null || return 1
    docker inspect "$ids" >"$MG_RUN_DIR/stopped-container.json" || return 1
    mg_artifact stopped-container "$MG_RUN_DIR/stopped-container.json"
    jq -e '.[0].State.Running==false and .[0].State.ExitCode==0' "$MG_RUN_DIR/stopped-container.json" >/dev/null || rc=1
  fi
  (MG_RUN_ID="$run"; mg_sync_server__remove_container "$name" "$owner") || rc=1
  [ "$rc" -eq 0 ]
}
mg_sync_package__start_cleanup() {
  [ -f "$2" ] || (local MG_SYNC_INSTANCE="$1"; mg_sync_package__stop)
}
mg_sync_package__wait() {
  python3 "$MG_ROOT/server/tools/package-ready.py" "$1"
}
mg_sync_package__start() {
  mg_sync_package__volume_check || return 1
  local name="meguri-local-$MG_RUN_ID" marker="$MG_RUN_DIR/start-retained" MG_SYNC_CONTAINER_OWNER
  MG_SYNC_CONTAINER_OWNER="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  # Refuse a second live owner before replacing the lifecycle record.
  if [ -f "$MG_SYNC_INSTANCE/container.json" ]; then
    local previous
    previous="$(jq -r .name "$MG_SYNC_INSTANCE/container.json")" || return 1
    local previous_ids
    previous_ids="$(docker container ls -aq --filter "name=^/$previous$")" || return 1
    [ -z "$previous_ids" ] || return 1
  fi
  jq -n --arg name "$name" --arg owner "$MG_SYNC_CONTAINER_OWNER" --arg run "$MG_RUN_ID" \
    '{name:$name,owner:$owner,run:$run}' >"$MG_SYNC_INSTANCE/container.json" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_package__start_cleanup "$MG_SYNC_INSTANCE" "$marker")"
  local publish=()
  [ "${MEGURI_SYNC_NETWORK:-bridge}" = none ] || publish=(--service-ports)
  mg_sync_package__compose run -d --name "$name" --no-deps --pull never \
    --label "meguri.mg.run-id=$MG_RUN_ID" ${publish[0]+"${publish[@]}"} local-sync >"$MG_RUN_DIR/service-id" || return 1
  mg_sync_package__wait "$name" || return 1
  docker inspect "$name" >"$MG_RUN_DIR/service-container.json" || return 1
  jq -e --arg image "$(jq -r .image "$MG_SYNC_INSTANCE/instance.json")" --arg volume "$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json")" '
    length==1 and .[0].Image==$image and .[0].Config.User=="10001:10001" and .[0].State.Running and
    .[0].HostConfig.ReadonlyRootfs and
    ([.[0].Mounts[] | select(.Destination=="/data")] | length==1 and .[0].Type=="volume" and .[0].Name==$volume)
  ' "$MG_RUN_DIR/service-container.json" >/dev/null || return 1
  touch "$marker" || return 1
  mg_artifact service-container "$MG_RUN_DIR/service-container.json"
}
mg_sync_package__init() {
  local receipt="$1" volume="meguri-sync-data-$MG_RUN_ID" owner MG_SYNC_CONTAINER_OWNER name="meguri-init-$MG_RUN_ID" rc=0
  [ ! -e "$MG_SYNC_INSTANCE/instance.json" ] || return 1
  mg_sync_package__receipt "$receipt" || return 1
  [ -n "${MEGURI_SYNC_ORIGIN:-}" ] && [ -n "${MEGURI_SYNC_TRUSTED_PEER:-}" ] || return 1
  mkdir -p "$MG_SYNC_INSTANCE" || return 1
  chmod 0700 "$MG_SYNC_INSTANCE" || return 1
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  MG_SYNC_CONTAINER_OWNER="$owner"
  jq -n --arg image "$(jq -r .details.package.image_id "$receipt")" --arg volume "$volume" --arg owner "$owner" \
    --arg origin "$MEGURI_SYNC_ORIGIN" --arg peer "$MEGURI_SYNC_TRUSTED_PEER" --arg port "${MEGURI_SYNC_PORT:-8080}" \
    --arg receipt "$receipt" '{image:$image,volume:$volume,owner:$owner,origin:$origin,peer:$peer,port:$port,build_receipt:$receipt}' >"$MG_SYNC_INSTANCE/instance.json" || return 1
  # A unique run name plus creation labels is required before any volume use.
  local existing_volume
  existing_volume="$(docker volume ls -q --filter "name=^$volume$")" || return 1
  [ -z "$existing_volume" ] || return 1
  if ! docker volume create --label "meguri.sync.owner=$owner" --label "meguri.mg.run-id=$MG_RUN_ID" "$volume" >"$MG_RUN_DIR/volume-name"; then
    # A failed create can have reached Docker. Discard only a proved absent
    # candidate; retain metadata/data on uncertain or partial initialization.
    existing_volume="$(docker volume ls -q --filter "name=^$volume$")" || return 1
    if [ -z "$existing_volume" ]; then rm "$MG_SYNC_INSTANCE/instance.json" || return 1; fi
    return 1
  fi
  mg_sync_package__volume_check || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  # Private console delivery goes to a protected file, never MG's ordinary log.
  (umask 077; mg_sync_package__compose run -T --name "$name" --no-deps --pull never \
    --label "meguri.mg.run-id=$MG_RUN_ID" local-sync init /data/owner "$MEGURI_SYNC_ORIGIN" \
      >"$MG_SYNC_INSTANCE/setup.txt") || rc=1
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  [ "$rc" -eq 0 ]
}
mg_sync_package__admin() {
  local command="$1"; shift
  mg_sync_package__volume_check || return 1
  # Stop/clean the recorded lock owner first, through the exact lifecycle proof.
  mg_sync_package__stop || return 1
  local name="meguri-admin-$MG_RUN_ID" MG_SYNC_CONTAINER_OWNER rc=0 origin
  MG_SYNC_CONTAINER_OWNER="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  origin="$(jq -r .origin "$MG_SYNC_INSTANCE/instance.json")" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$MG_SYNC_CONTAINER_OWNER")"
  if [ "$command" = recover-admin ]; then
    python3 "$MG_ROOT/server/tools/package-ready.py" --password-file "${MEGURI_SYNC_RECOVERY_FILE:-}" || return 1
    cat "$MEGURI_SYNC_RECOVERY_FILE" | mg_sync_package__compose run -T --name "$name" --no-deps --pull never \
      --label "meguri.mg.run-id=$MG_RUN_ID" local-sync "$command" /data/owner "$origin" "$@" || rc=1
  else
    mg_sync_package__compose run -T --name "$name" --no-deps --pull never \
      --label "meguri.mg.run-id=$MG_RUN_ID" local-sync rotate-setup /data/owner "$origin" || rc=1
    if [ "$rc" -eq 0 ]; then
      (set -o pipefail; umask 077; docker cp "$name:/data/owner/state/setup-token" - | tar -xOf - >"$MG_SYNC_INSTANCE/setup.txt") || rc=1
      chmod 0600 "$MG_SYNC_INSTANCE/setup.txt" || rc=1
    fi
  fi
  mg_sync_server__remove_container "$name" "$MG_SYNC_CONTAINER_OWNER" || rc=1
  [ "$rc" -eq 0 ]
}
mg_sync_package_local() {
  local MG_SYNC_INSTANCE="$MG_ROOT/build/sync-server-local" action="${1:-}" rc=0 receipt
  [ "$#" -gt 0 ] || { mg_failure usage "sync-server-local init|start|stop|rotate-setup|recover-admin <account-id> [--reenable]"; mg_status error; return; }
  shift
  case "$action" in
    init|start|stop|rotate-setup) [ "$#" -eq 0 ] || rc=1 ;;
    recover-admin) [ "$#" -eq 1 ] || { [ "$#" -eq 2 ] && [ "$2" = --reenable ]; } || rc=1 ;;
    *) rc=1 ;;
  esac
  [ "$rc" -eq 0 ] || { mg_failure usage "Unexpected local service arguments"; mg_status error; return; }
  mkdir -p "$MG_SYNC_INSTANCE" || return 1
  # Serialize this checkout's operator commands; never take over another run.
  mkdir "$MG_SYNC_INSTANCE/lock" || { mg_sync_package__failure "Another local lifecycle command holds the lock"; return; }
  mg_on_exit "$(printf '%q ' rmdir "$MG_SYNC_INSTANCE/lock")"
  case "$action" in
    init)
      receipt="$(cd "$MG_ROOT/build/mg/sync-server-package/latest" && pwd -P)/result.json" || return 1
      mg_sync_package__init "$receipt" || rc=1 ;;
    start) mg_sync_package__start || rc=1 ;;
    stop) mg_sync_package__stop || rc=1 ;;
    rotate-setup|recover-admin) mg_sync_package__admin "$action" "$@" || rc=1 ;;
  esac
  if [ "$rc" -ne 0 ]; then mg_sync_package__failure "Local command failed; retain the instance and owned volume for investigation"; return; fi
  mg_message "Local command completed. Setup credential, when applicable: $MG_SYNC_INSTANCE/setup.txt. The persistent volume is retained."
  mg_artifact local-instance "$MG_SYNC_INSTANCE/instance.json"
  mg_status pass
}
mg_sync_package_test() {
  [ "$#" -eq 0 ] || { mg_failure usage "sync-server-package-test takes no arguments"; mg_status error; return; }
  mg_sync_server__clean || { mg_status fail; return; }
  local receipt MG_SYNC_INSTANCE="$MG_RUN_DIR/instance" phase name owner image browser_image service rc=0
  receipt="$("$MG_TOOLS/mg" sync-server-package)" || { mg_sync_package__failure "Exact image build failed"; return; }
  mg_sync_package__receipt "$receipt" || { mg_sync_package__failure "Invalid exact-image receipt"; return; }
  mg_artifact exact-image-build "$receipt"
  local MEGURI_SYNC_ORIGIN=https://localhost:18443 MEGURI_SYNC_TRUSTED_PEER=127.0.0.1 MEGURI_SYNC_NETWORK=none
  # Exit cleanup is LIFO: stop the service before removing its synthetic volume.
  mg_on_exit "$(printf '%q ' mg_sync_package__volume_remove "$MG_SYNC_INSTANCE")"
  mg_on_exit "local MG_SYNC_INSTANCE=$(printf '%q' "$MG_SYNC_INSTANCE"); mg_sync_package__stop"
  if ! mg_sync_package__init "$receipt"; then mg_sync_package__failure "Fresh non-root volume initialization failed"; return; fi
  image="$(jq -r .details.package.client_image_id "$receipt")" || return 1
  browser_image="$(docker image inspect --format '{{.Id}}' "$(mg_compose_project)-sync-web:local")" || return 1
  mg_detail browser_tool_image "$(jq -nc --arg image "$browser_image" '$image')"
  for phase in seed check; do
    # Distinct names/nonces for each actual start, including within this run.
    if ! mg_sync_package__start; then mg_sync_package__failure "Actual runtime startup failed"; return; fi
    service="$(jq -r .name "$MG_SYNC_INSTANCE/container.json")" || return 1
    name="meguri-package-browser-$MG_RUN_ID-$phase"
    owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
    mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
    # Same runtime container and private HTTPS proxy implementation; existing
    # locked browser tooling/dependency/browser volumes are read-only clients.
    docker run --init --user 10001:10001 --read-only --cap-drop ALL --security-opt no-new-privileges --entrypoint bash \
      --name "$name" --shm-size=512m -e HOME=/tmp \
      --tmpfs /tmp:uid=10001,gid=10001,mode=0700 \
      --tmpfs /work:exec,uid=10001,gid=10001,mode=0700 \
      --tmpfs /out:uid=10001,gid=10001,mode=0700 \
      -v "$(mg_compose_project)_sync-web-deps:/deps:ro" \
      -v "$(mg_compose_project)_sync-web-browsers:/browsers:ro" \
      --network "container:$service" --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" \
      -v "$MG_ROOT/server/web:/source:ro" \
      -v "$MG_ROOT/server/tools/package-smoke.mjs:/package-smoke.mjs:ro" \
      --mount "type=volume,src=$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json"),dst=/data,readonly" \
      "$browser_image" -c '
        rc=0
        bash /source/scripts/run.sh package-browser "$1" >&2 || rc=$?
        # Export partial evidence after the runner exits, before tmpfs vanishes.
        tar -C /out -cf - . || rc=1
        exit "$rc"
      ' bash "$phase" >"$MG_RUN_DIR/browser-$phase.tar" || rc=1
    # Unpack even failed output after container exit, before checked removal.
    tar --no-same-owner -xf "$MG_RUN_DIR/browser-$phase.tar" -C "$MG_RUN_DIR" || rc=1
    mg_sync_server__remove_container "$name" "$owner" || rc=1
    if [ "$rc" -ne 0 ] || ! jq -e --arg phase "$phase" \
        '.phase==$phase and (.checks | length)==6 and (.browser | length)>0' "$MG_RUN_DIR/browser-$phase.json" >/dev/null; then
      mg_sync_package__failure "Exact-image rendered browser journey failed; no retries"; return
    fi
    mg_artifact browser-smoke "$MG_RUN_DIR/browser-$phase.json"
    mg_artifact browser-screenshot "$MG_RUN_DIR/browser-$phase.png"
    mg_artifact browser-build "$MG_RUN_DIR/package-browsers-$phase.json"
    name="meguri-package-client-$MG_RUN_ID-$phase"
    owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
    mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
    docker run --name "$name" --init --network "container:$service" --read-only --cap-drop ALL \
      --security-opt no-new-privileges --tmpfs /tmp:mode=1777,size=128m \
      --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" \
      --mount "type=volume,src=$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json"),dst=/data,readonly" \
      --mount "type=bind,src=$(dirname "$receipt")/product,dst=/package,readonly" \
      --mount "type=bind,src=$MG_RUN_DIR,dst=/out" "$image" "$phase" || rc=1
    mg_sync_server__remove_container "$name" "$owner" || rc=1
    if [ "$rc" -ne 0 ] || ! jq -e --arg phase "$phase" '.phase==$phase and (.checks | length)==7 and .assets>1' "$MG_RUN_DIR/$phase.json" >/dev/null; then
      mg_sync_package__failure "Exact-image HTTPS/native smoke failed; no retries"; return
    fi
    cp "$MG_RUN_DIR/service-container.json" "$MG_RUN_DIR/$phase-container.json" || return 1
    mg_artifact smoke-phase "$MG_RUN_DIR/$phase.json"
    mg_artifact smoke-container "$MG_RUN_DIR/$phase-container.json"
    mg_sync_package__stop || { mg_sync_package__failure "Owned service stop failed"; return; }
  done
  mg_sync_server__clean || { mg_status fail; return; }
  mg_count total 4; mg_count passed 4
  mg_detail coverage '"Synthetic exact-image packaging only; no Desktop or Mobile acceptance"'
  mg_status pass
}
