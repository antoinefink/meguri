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
  mg_sync_server__clean || return 1
  jq -e --arg root "$MG_ROOT" --arg commit "$(git -C "$MG_ROOT" rev-parse HEAD)" '
    .job=="sync-server-package" and .status=="pass" and .exit_code==0 and .failures==[] and .dry_run==false and .args==[] and .root==$root and
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
mg_sync_package__candidate_input() {
  local receipt="$1" destination="$2" directory image
  mg_sync_package__receipt "$receipt" || return 1
  directory="$(cd "$(dirname "$receipt")" && pwd -P)" || return 1
  # The package archive has its own fixed definition; debug archives differ.
  mg_sync_package__archive >"$MG_RUN_DIR/current-package-source.tar" || return 1
  cmp -s "$MG_RUN_DIR/current-package-source.tar" "$directory/source.tar" || return 1
  image="$(jq -er .details.package.image_id "$receipt")" || return 1
  docker image inspect "$image" >"$MG_RUN_DIR/current-package-image.json" || return 1
  cmp -s "$MG_RUN_DIR/current-package-image.json" "$directory/image.json" || return 1
  python3 "$MG_ROOT/server/tools/candidate-output.py" --retain-package "$directory" "$destination"
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
    --arg run "$(jq -r '.run_id // (.volume | ltrimstr("meguri-sync-data-"))' "$MG_SYNC_INSTANCE/instance.json")" \
    --arg volume "$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json")" '
    length==1 and .[0].Name==$volume and .[0].Labels["meguri.sync.owner"]==$owner and
    .[0].Labels["meguri.mg.run-id"]==$run' >/dev/null
}
mg_sync_package__volume_remove() {
  [ -f "$1/instance.json" ] || return 0
  local MG_SYNC_INSTANCE="$1" volume
  volume="$(jq -r .volume "$MG_SYNC_INSTANCE/instance.json")" || return 1
  # No absent-volume fallback hides a failed ownership observation.
  if mg_sync_package__volume_check && docker volume rm "$volume" >/dev/null; then
    local remaining
    remaining="$(docker volume ls -q --filter "name=^$volume$")" || {
      mg_sync_package__failure "Cannot observe removal of the owned synthetic volume"; return 1;
    }
    [ -z "$remaining" ] && return 0
  fi
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
  local receipt="$1" volume="meguri-sync-data-$MG_RUN_ID${MG_SYNC_INSTANCE_SUFFIX:+-$MG_SYNC_INSTANCE_SUFFIX}" owner MG_SYNC_CONTAINER_OWNER name="meguri-init-$MG_RUN_ID" rc=0
  [ ! -e "$MG_SYNC_INSTANCE/instance.json" ] || return 1
  mg_sync_package__receipt "$receipt" || return 1
  [ -n "${MEGURI_SYNC_ORIGIN:-}" ] && [ -n "${MEGURI_SYNC_TRUSTED_PEER:-}" ] || return 1
  mkdir -p "$MG_SYNC_INSTANCE" || return 1
  chmod 0700 "$MG_SYNC_INSTANCE" || return 1
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  MG_SYNC_CONTAINER_OWNER="$owner"
  jq -n --arg image "$(jq -r .details.package.image_id "$receipt")" --arg volume "$volume" --arg owner "$owner" \
    --arg origin "$MEGURI_SYNC_ORIGIN" --arg peer "$MEGURI_SYNC_TRUSTED_PEER" --arg port "${MEGURI_SYNC_PORT:-8080}" \
    --arg receipt "$receipt" --arg run "$MG_RUN_ID" '{image:$image,volume:$volume,owner:$owner,origin:$origin,peer:$peer,port:$port,build_receipt:$receipt,run_id:$run}' >"$MG_SYNC_INSTANCE/instance.json" || return 1
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
      >"${MG_SYNC_SETUP_FILE:-$MG_SYNC_INSTANCE/setup.txt}") || rc=1
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
  local MG_SYNC_SETUP_FILE=/dev/null
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
  # The four packaging fixtures retain their own counts. The existing complete
  # browser/frontend job runs fresh against this identical immutable image.
  mg_sync_web__child sync-web-verify --package-receipt "$receipt" || {
    mg_sync_package__failure "Exact-image full browser/frontend proof failed"; return;
  }
  mg_sync_server__clean || { mg_status fail; return; }
  mg_count total 4; mg_count passed 4
  mg_detail coverage '"Synthetic exact-image packaging only; no Desktop or Mobile acceptance"'
  mg_status pass
}

# Fixed Desktop final path. Outer MG identity never becomes a segment run ID.
mg_sync_package__final_owned() {
  local receipt="$1" arguments="$2" author_session="$3" commit identity after image phase rc=0
  mg_detail candidate_compatibility_credit false
  commit="$(git -C "$MG_ROOT" rev-parse HEAD)" || return 1
  mg_sync_package__receipt "$receipt" || { mg_failure build-receipt "Final execution requires the canonical clean release package"; return 1; }
  mkdir "$MG_RUN_DIR/package-artifacts" "$MG_RUN_DIR/segments" "$MG_RUN_DIR/instances" "$MG_RUN_DIR/output" || return 1
  mg_sync_package__candidate_input "$receipt" "$MG_RUN_DIR/package-artifacts" >"$MG_RUN_DIR/package-identity.json" || return 1
  mg_artifact exact-image-build "$receipt"
  # Existing quarantine digest helper is a black-box seam, not source access.
  identity="$(mg_sync_server__reference_identity)" || return 1
  mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" --progress plain build sync-reference || return 1
  after="$(mg_sync_server__reference_identity)" || return 1
  [ "$identity" = "$after" ] || { mg_failure reference-input-changed "Reference inputs changed during build"; return 1; }
  image="$(mg_sync_server__image sync-reference -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml")" || return 1
  [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]] || return 1
  # Aggregate invocation retains only the original outer fields. Factory
  # comparisons and execute/record identity belong to each segment invocation.
  jq -nc --argjson server "$(jq -c .server "$MG_RUN_DIR/package-identity.json")" \
    --arg job "$MG_JOB" --argjson args "$arguments" --arg run "$MG_RUN_ID" --arg session "$author_session" '
    {source_commit:$server.source_commit,batch_base:"aa8565719af435051de7d5f068c0c817286b568e",
     source_sha256:$server.source_sha256,lock_sha256:$server.lock_sha256,
     executable_sha256:$server.executable_sha256,mg_job:$job,arguments:$args,
     run_id:$run,author_session:$session}' >"$MG_RUN_DIR/candidate-invocation.json" || return 1
  mg_detail candidate_client_image "$(jq -nc --arg image "$image" '{id:$image}')"
  # These are nine sequential fresh factories, including the unchanged selections.
  for phase in final-full final-normal final-media final-migration final-replacement-upload \
      final-replacement-download final-metadata-only final-fresh-fresh final-rich; do
    mkdir "$MG_RUN_DIR/instances/$phase" "$MG_RUN_DIR/segments/$phase" || return 1
    mg_sync_package__final_instance "$receipt" "$phase" "$image" || rc=1
  done
  mg_sync_package__final_assemble "$image" || rc=1
  after="$(mg_sync_server__reference_identity)" || rc=1
  jq -nc --argjson before "$identity" --argjson after "${after:-null}" --arg image "$image" \
    '{before:$before,after:$after,image_id:$image}' >"$MG_RUN_DIR/reference-runtime.json" || rc=1
  mg_artifact candidate-reference-runtime "$MG_RUN_DIR/reference-runtime.json"
  [ "$identity" = "$after" ] || { mg_failure reference-input-changed "Reference inputs changed during execution"; rc=1; }
  mg_sync_server__clean || rc=1
  [ "$(git -C "$MG_ROOT" rev-parse HEAD)" = "$commit" ] || rc=1
  mg_artifact reference-output "$MG_RUN_DIR/output"
  mg_artifact final-segments "$MG_RUN_DIR/segments"
  mg_artifact final-instances "$MG_RUN_DIR/instances"
  if [ "$rc" -ne 0 ]; then
    mg_failure candidate-final "Exact-image final execution, handoff, assembly or checked cleanup failed"
    return 1
  fi
  mg_detail final_automated_evidence_complete true
  mg_message "Exact-image automated evidence retained; independent actual-artifact review and sealing remain required for final credit"
  mg_status pass
}

mg_sync_package__final_instance() {
  local receipt="$1" phase="$2" image="$3" outer="$MG_RUN_DIR" secret_dir rc=0
  local MG_RUN_DIR="$outer/instances/$phase" MG_SYNC_INSTANCE="$outer/instances/$phase/instance"
  local MG_SYNC_INSTANCE_SUFFIX="$phase" MG_SYNC_SETUP_FILE=/dev/null
  local MEGURI_SYNC_ORIGIN=https://candidate.example.test MEGURI_SYNC_TRUSTED_PEER=127.0.0.1 MEGURI_SYNC_NETWORK=none
  secret_dir="$(mktemp -d "${TMPDIR:-/tmp}/meguri-final.XXXXXX")" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json")"
  python3 - "$secret_dir" "$MG_RUN_DIR/secret-owner.json" <<'PY'
import json, os, pathlib, sys
secret, receipt = map(pathlib.Path, sys.argv[1:])
s = secret.lstat()
with receipt.open('x') as output: json.dump([s.st_dev, s.st_ino, s.st_uid], output)
fd = os.open(secret / 'credentials.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, 'w') as output:
    json.dump(dict(username='synthetic.' + os.urandom(12).hex(), password=os.urandom(32).hex()), output)
PY
  [ "$?" -eq 0 ] || return 1
  # Hooks are registered in the same owning shell before any factory creation.
  mg_on_exit "$(printf '%q ' mg_sync_package__volume_remove "$MG_SYNC_INSTANCE")"
  mg_on_exit "local MG_SYNC_INSTANCE=$(printf '%q' "$MG_SYNC_INSTANCE"); mg_sync_package__stop"
  mg_sync_package__init "$receipt" && mg_sync_package__start && \
    mg_sync_package__final_exchange "$receipt" "$phase" "$image" "$outer" "$secret_dir/credentials.json" || rc=1
  # Cleanup is checked even after client/preparation failure, before the next factory.
  mg_sync_package__stop || rc=1
  if mg_sync_package__volume_remove "$MG_SYNC_INSTANCE"; then
    # Retire the factory record only after checked absence; its exit hook then
    # has no live volume to remove. Preserve nonsecret ownership evidence.
    if [ -f "$MG_SYNC_INSTANCE/instance.json" ]; then
      mv "$MG_SYNC_INSTANCE/instance.json" "$MG_SYNC_INSTANCE/retired-instance.json" || rc=1
    fi
  else rc=1; fi
  mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json" || rc=1
  [ "$rc" -eq 0 ]
}

mg_sync_package__final_measure() {
  local cid="$1" image="$2" stage="$3" owner rc=0
  local name="meguri-final-measure-$MG_RUN_ID-$stage"
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  # No shell/runtime is installed in the service; tool reads only init's direct
  # children and the exact matching process in this owned PID namespace.
  docker run --name "$name" --network none --pid "container:$cid" --read-only --cap-drop ALL \
    --security-opt no-new-privileges --user 10001:10001 --memory 2g --pids-limit 128 \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" \
    --mount "type=bind,src=$MG_ROOT/server/tools/candidate-output.py,dst=/owned/candidate-output.py,readonly" \
    --mount "type=bind,src=$MG_RUN_DIR/execute-invocation.json,dst=/owned/invocation.json,readonly" \
    --entrypoint python "$image" /owned/candidate-output.py --measure-process /owned/invocation.json \
    >"$MG_RUN_DIR/process-$stage.json" || rc=1
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  [ "$rc" -eq 0 ]
}

mg_sync_package__final_client() {
  local cid="$1" input="$2" credentials="$3" output="$4" phase="$5" stage="$6" image="$7"
  local name="meguri-final-client-$MG_RUN_ID-$stage" owner rc=0 network=none mounts=()
  [ "$stage" = execute ] || [ "$stage" = record ] || return 1
  if [ "$stage" = execute ]; then
    network="container:$cid"
    mounts=(--mount "type=bind,src=$credentials,dst=/secret/credentials.json,readonly")
  fi
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  docker run --name "$name" --network "$network" --read-only --cap-drop ALL \
    --security-opt no-new-privileges --user "$(id -u):$(id -g)" --memory 2g --pids-limit 128 \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" -e MG_RUN_ID="$MG_RUN_ID" \
    -e TMPDIR=/scratch -e TZ=UTC -e PYTHONDONTWRITEBYTECODE=1 -e PYTHONUNBUFFERED=1 \
    --tmpfs /scratch:exec,mode=1777,size=1g --tmpfs /tmp:mode=1777,size=64m \
    --mount "type=bind,src=$input,dst=/in,readonly" \
    --mount "type=bind,src=$output,dst=/out" ${mounts[0]+"${mounts[@]}"} \
    --entrypoint python "$image" /opt/reference/driver.py candidate --phase "$phase" --stage "$stage" \
    --input /in --credentials /secret/credentials.json || rc=$?
  # Retain the actual child CLI/image/mount identity, independently of public input.
  docker inspect "$name" >"$MG_RUN_DIR/client-$stage-container.json" || rc=1
  jq -e --arg image "$image" --arg phase "$phase" --arg stage "$stage" --arg network "$network" '
    length==1 and .[0].Image==$image and .[0].HostConfig.NetworkMode==$network and
    .[0].Config.Cmd==["/opt/reference/driver.py","candidate","--phase",$phase,"--stage",$stage,
                     "--input","/in","--credentials","/secret/credentials.json"] and
    (if $stage=="record" then all(.[0].Mounts[]; .Destination!="/secret/credentials.json") else true end)
    ' "$MG_RUN_DIR/client-$stage-container.json" >/dev/null || rc=1
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  jq -nc --arg phase "$phase" --arg stage "$stage" --argjson rc "$rc" \
    '{child_phase:$phase,child_stage:$stage,exit_code:$rc}' >"$MG_RUN_DIR/client-$stage-result.json" || return 1
  return "$rc"
}

mg_sync_package__final_exchange() {
  local receipt="$1" phase="$2" image="$3" outer="$4" credentials="$5" cid name owner admission rc=0
  local output="$outer/segments/$phase" helper="$MG_ROOT/server/tools/candidate-output.py"
  cid="$(jq -er '.[0].Id' "$MG_RUN_DIR/service-container.json")" || return 1
  owner="$(jq -er .owner "$MG_SYNC_INSTANCE/container.json")" || return 1
  jq --arg root "$MG_ROOT" --arg volume "$(jq -er .volume "$MG_SYNC_INSTANCE/instance.json")" \
    --arg owner "$owner" --arg phase "$phase" --slurpfile inspection "$MG_RUN_DIR/service-container.json" '
    . + {root:$root,server_arguments:$inspection[0][0].Config.Cmd,data_volume:$volume,
         owner_nonce:$owner,child_phase:$phase,child_stage:"execute"}
    ' "$outer/candidate-invocation.json" >"$MG_RUN_DIR/execute-invocation.json" || return 1
  # Producer-configured arguments, never an endpoint/command chosen by a fixture.
  jq -e '.server_arguments==["serve","/data/owner","https://candidate.example.test","0.0.0.0:8080","--trusted-peer","127.0.0.1"]' \
    "$MG_RUN_DIR/execute-invocation.json" >/dev/null || return 1
  name="meguri-final-setup-$MG_RUN_ID"
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  docker run -i --name "$name" --network "container:$cid" --read-only --cap-drop ALL \
    --security-opt no-new-privileges --user 10001:10001 --memory 2g --pids-limit 128 \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" \
    --mount "type=volume,src=$(jq -er .volume "$MG_SYNC_INSTANCE/instance.json"),dst=/data,readonly" \
    --mount "type=bind,src=$helper,dst=/owned/candidate-output.py,readonly" \
    --entrypoint python "$image" /owned/candidate-output.py --setup-account <"$credentials" || rc=1
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  [ "$rc" -eq 0 ] || return 1
  mg_sync_package__final_measure "$cid" "$image" before || return 1
  docker inspect "$cid" >"$MG_RUN_DIR/container-before.json" || return 1
  mkdir "$MG_RUN_DIR/execute-input" "$MG_RUN_DIR/record-input" || return 1
  python3 "$helper" --prepare-final "$MG_RUN_DIR/execute-input" "$outer/package-artifacts" \
    "$outer/package-identity.json" "$receipt" "$MG_RUN_DIR/container-before.json" \
    "$MG_RUN_DIR/process-before.json" "$MG_RUN_DIR/execute-invocation.json" || return 1
  [ -z "$(ls -A "$output")" ] && [ ! -L "$output" ] || return 1
  cp "$MG_RUN_DIR/execute-invocation.json" "$output/invocation.json" || return 1
  mg_sync_package__final_client "$cid" "$MG_RUN_DIR/execute-input" "$credentials" "$output" "$phase" execute "$image" || rc=1
  # Capture the exact admission BEFORE any actual after-state measurement.
  admission="$(python3 "$helper" --admit-execute "$output" "$MG_RUN_DIR/execute-invocation.json" \
    "$MG_RUN_DIR/execute-input/preflight.json" "$MG_RUN_DIR/execute-snapshot.json")" || return 1
  [[ "$admission" =~ ^[0-9a-f]{64}$ ]] || return 1
  mg_sync_package__final_measure "$cid" "$image" after || return 1
  docker inspect "$cid" >"$MG_RUN_DIR/container-after.json" || return 1
  jq --arg admission "$admission" '. + {child_stage:"record",execute_admission_sha256:$admission}' \
    "$MG_RUN_DIR/execute-invocation.json" >"$MG_RUN_DIR/record-invocation.json" || return 1
  python3 "$helper" --complete-final "$MG_RUN_DIR/record-input" "$MG_RUN_DIR/execute-input" "$output/artifacts" \
    "$MG_RUN_DIR/container-after.json" "$MG_RUN_DIR/process-after.json" "$MG_RUN_DIR/record-invocation.json" || return 1
  # SAME output/CAS/raw inode. Only invocation stage and captured admission change.
  jq --arg admission "$admission" '. + {child_stage:"record",execute_admission_sha256:$admission}' \
    "$output/invocation.json" >"$MG_RUN_DIR/record-selected-invocation.json" || return 1
  cp "$MG_RUN_DIR/record-selected-invocation.json" "$output/invocation.json" || return 1
  mg_sync_package__final_client "$cid" "$MG_RUN_DIR/record-input" "$credentials" "$output" "$phase" record "$image" || rc=1
  python3 "$helper" --check-record "$output" "$MG_RUN_DIR/record-invocation.json" \
    "$MG_RUN_DIR/record-input/peer.json" "$MG_RUN_DIR/execute-snapshot.json" || rc=1
  python3 "$helper" "$output" "$MG_RUN_DIR/record-invocation.json" "$MG_RUN_DIR/record-input/peer.json" \
    "$MG_ROOT/conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json" \
    >"$MG_RUN_DIR/candidate-output-validation.json" || rc=1
  [ "$rc" -eq 0 ]
}

mg_sync_package__final_assemble() {
  local image="$1" output="$MG_RUN_DIR/output" name="meguri-final-assemble-$MG_RUN_ID" owner rc=0
  local validation="$MG_RUN_DIR/validation" helper="$MG_ROOT/server/tools/candidate-output.py" report
  cp "$MG_RUN_DIR/candidate-invocation.json" "$output/invocation.json" || return 1
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  # No credentials or service namespace are available during fixed assembly.
  docker run --name "$name" --network none --read-only --cap-drop ALL \
    --security-opt no-new-privileges --user "$(id -u):$(id -g)" --memory 2g --pids-limit 128 \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" -e MG_RUN_ID="$MG_RUN_ID" \
    -e TMPDIR=/scratch -e TZ=UTC -e PYTHONDONTWRITEBYTECODE=1 -e PYTHONUNBUFFERED=1 \
    --tmpfs /scratch:exec,mode=1777,size=1g --tmpfs /tmp:mode=1777,size=64m \
    --mount "type=bind,src=$MG_RUN_DIR/segments,dst=/in/segments,readonly" \
    --mount "type=bind,src=$output,dst=/out" \
    --entrypoint python "$image" /opt/reference/driver.py final-assemble --input /in || rc=1
  docker inspect "$name" >"$MG_RUN_DIR/assembly-container.json" || rc=1
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  python3 "$helper" --check-assembly "$MG_RUN_DIR/segments" "$output" \
    "$MG_RUN_DIR/candidate-invocation.json" "$MG_RUN_DIR/instances" || rc=1
  mkdir "$validation" || return 1
  cp "$output/invocation.json" "$validation/invocation.json" || rc=1
  name="meguri-final-validation-$MG_RUN_ID"
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  docker run --name "$name" --network none --read-only --cap-drop ALL \
    --security-opt no-new-privileges --user "$(id -u):$(id -g)" --memory 2g --pids-limit 128 \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" -e MG_RUN_ID="$MG_RUN_ID" \
    -e TMPDIR=/scratch -e TZ=UTC -e PYTHONDONTWRITEBYTECODE=1 -e PYTHONUNBUFFERED=1 \
    --tmpfs /scratch:exec,mode=1777,size=1g --tmpfs /tmp:mode=1777,size=64m \
    --mount "type=bind,src=$output,dst=/in,readonly" \
    --mount "type=bind,src=$validation,dst=/out" \
    --entrypoint python "$image" /opt/reference/driver.py validate --input /in || rc=1
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  report="$MG_RUN_DIR/candidate-output-validation.json"
  python3 "$helper" "$output" "$MG_RUN_DIR/candidate-invocation.json" \
    "$MG_RUN_DIR/instances/final-full/record-input/peer.json" \
    "$MG_ROOT/conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json" >"$report" || rc=1
  mg_artifact candidate-output-validation "$report"
  mg_artifact candidate-semantic-validation "$validation"
  # Only actual aggregate output is published/countable, including failed output.
  if jq -e '.summary | type=="object"' "$report" >/dev/null; then
    mg_detail reference_evidence "$(jq -c .summary "$report")"
    local counts total passed failed
    if counts="$(jq -er '.summary | [.total,.passed,.failed] | if all(.[]; type=="number" and .>=0 and .<=2147483647 and floor==.) then @tsv else error("invalid counts") end' "$report")"; then
      IFS=$'\t' read -r total passed failed <<<"$counts"
      mg_count total "$total"; mg_count passed "$passed"; mg_count failed "$failed"
    else rc=1; fi
  fi
  if jq -e '.invocation | type=="object"' "$report" >/dev/null; then
    mg_detail reference_invocation "$(jq -c .invocation "$report")"
  fi
  [ "$rc" -eq 0 ]
}
