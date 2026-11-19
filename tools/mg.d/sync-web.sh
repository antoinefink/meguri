# SPDX-License-Identifier: MIT
# Host functions orchestrate only; all compilers and fixture processes run in Docker.
mg_job sync-web-image host mg_sync_web_image "Build and probe the pinned browser/Rust image"
mg_job sync-web-deps host mg_sync_web_deps "Resolve (--resolve), locked install (--install), or audit dependencies"
mg_job sync-web-browser-install host mg_sync_web_browser_install "Explicitly install locked Chromium after dependency audit"
mg_job sync-web-typecheck host mg_sync_web_typecheck "Strict browser TypeScript check"
mg_job sync-web-unit host mg_sync_web_unit "Browser API/policy unit tests"
mg_job sync-web-build host mg_sync_web_build "Production browser build and prepared embedded assets"
mg_job sync-web-check host mg_sync_web_check "Typecheck, unit tests and production prepared assets"
mg_job sync-web-actualserver-build host mg_sync_web_actualserver_build "Compile the actual Rust server using a prepared browser build"
mg_job sync-web-preview-up host mg_sync_web_preview_up "Start synthetic actual-server HTTPS fixtures on loopback"
mg_job sync-web-preview-down host mg_sync_web_preview_down "Stop this checkout's owned HTTPS fixtures"
mg_job sync-web-browser host mg_sync_web_browser "Zero-retry browser/axe tests against the isolated actual-server fixture"
mg_job sync-web-verify host mg_sync_web_verify "Locked Docker web verification with a run-private actual-server HTTPS fixture; no host ports"

mg_sync_web__compose() {
  MEGURI_SYNC_WEB_IMAGE="$(mg_compose_project)-sync-web:local" mg_compose -f "$MG_ROOT/docker/sync-web-compose.yml" "$@"
}
mg_sync_web__failure() {
  mg_artifact directory "$MG_RUN_DIR" "Partial sync-web evidence"
  mg_failure sync-web-execution "$1"
  mg_status fail
}
mg_sync_web__directory() {
  case "$1" in "$MG_ROOT"/build/*) ;; *) return 1 ;; esac
  local directory
  directory="$(cd "$1" && pwd -P)" || return 1
  case "$directory" in "$MG_ROOT"/build/*) printf '%s\n' "$directory" ;; *) return 1 ;; esac
}
mg_sync_web__counts() {
  local mode="$1" report="$2" key value count_status=0
  python3 "$MG_ROOT/server/web/scripts/report-counts.py" "$mode" "$report" "$MG_RUN_DIR/test-counts.json" || count_status=1
  if [ -f "$MG_RUN_DIR/test-counts.json" ]; then
    while IFS=$'\t' read -r key value; do
      mg_count "$key" "$value"
    done < <(jq -r '(.counts // {}) | to_entries[] | [.key, .value] | @tsv' "$MG_RUN_DIR/test-counts.json")
    mg_artifact json "$MG_RUN_DIR/test-counts.json" "Counts bound to retained runner results"
  fi
  if [ "$count_status" -ne 0 ]; then
    mg_failure sync-web-test-report "Missing, inconsistent or nonpassing runner evidence; inspect test-counts.json"
  fi
  return "$count_status"
}
mg_sync_web__run() {
  local service="$1" mode="$2"; shift 2
  if [ "$#" -ne 0 ]; then mg_failure usage "Unexpected arguments"; mg_status error; return; fi
  if [ "$mode" != resolve ] && [ "$mode" != probe ] && [ ! -f "$MG_ROOT/server/web/package-lock.json" ]; then
    mg_failure web-lock-missing "Run explicit resolution and review/adopt the emitted lock first"; mg_status error; return
  fi
  local execution_status=0 count_status=0
  # Node unit tests compile/invoke the actual std-only preparation consumer.
  local consumer_mount=() audit_mount=() audit_image=""
  case "$mode" in
    unit|check) consumer_mount=(-v "$MG_ROOT/server/build.rs:/server/build.rs:ro") ;;
  esac
  case "$mode" in
    resolve|install|audit|browser-install)
      # Copy only the fixed reviewed controls, never a checkout/Git/private mount.
      if ! python3 "$MG_ROOT/server/web/scripts/audit-applicability.py" stage-controls \
          "$MG_ROOT" "$MG_RUN_DIR/audit-controls" > "$MG_RUN_DIR/npm-audit-controls.json"; then
        mg_sync_web__failure "Frontend build controls differ from the fixed assessment"; return
      fi
      audit_mount=(-v "$MG_RUN_DIR/audit-controls:/audit-controls:ro")
      audit_image="$(docker image inspect --format '{{.Id}}' "$(mg_compose_project)-sync-web:local")" || {
        mg_sync_web__failure "Cannot bind frontend audit tool image"; return
      }
      if ! [[ "$audit_image" =~ ^sha256:[0-9a-f]{64}$ ]]; then
        mg_sync_web__failure "Invalid frontend audit image identity"; return
      fi
      printf '%s\n' "$audit_image" > "$MG_RUN_DIR/npm-audit-image.txt"
      mg_artifact json "$MG_RUN_DIR/npm-audit-controls.json" "Fixed frontend build control identity"
      mg_artifact text "$MG_RUN_DIR/npm-audit-image.txt" "Frontend audit tool image identity"
      ;;
  esac
  MEGURI_SYNC_WEB_IMAGE="${audit_image:-$(mg_compose_project)-sync-web:local}" \
    mg_compose -f "$MG_ROOT/docker/sync-web-compose.yml" run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
      ${audit_mount[0]+"${audit_mount[@]}"} \
      ${consumer_mount[0]+"${consumer_mount[@]}"} \
      -v "$MG_ROOT/server/web:/source:ro" -v "$MG_RUN_DIR:/out" "$service" "$mode" || execution_status=$?
  if [ -n "$audit_image" ] && [ ! -s "$MG_RUN_DIR/npm-audit-decision.json" ]; then
    mg_failure sync-web-audit-decision "Missing frontend audit applicability evidence"
    count_status=1
  fi
  if [ -f "$MG_RUN_DIR/npm-audit-decision.json" ]; then
    mg_artifact json "$MG_RUN_DIR/npm-audit-decision.json" "Frontend audit applicability; raw npm result retained separately"
  fi
  case "$mode" in
    unit|check) mg_sync_web__counts unit "$MG_RUN_DIR/unit-results.tap" || count_status=1 ;;
  esac
  if [ "$execution_status" -ne 0 ] || [ "$count_status" -ne 0 ]; then
    mg_sync_web__failure "Browser tool operation failed; inspect retained outputs"; return
  fi
  mg_artifact directory "$MG_RUN_DIR" "Browser tool evidence"
  mg_status pass
}
mg_sync_web_image() {
  if [ "$#" -ne 0 ]; then mg_failure usage "Use sync-web-image"; mg_status error; return; fi
  if ! mg_sync_web__compose --progress plain build sync-web-resolver; then
    mg_sync_web__failure "Pinned tool image build failed"; return
  fi
  mg_sync_web__run sync-web-resolver probe
}
mg_sync_web_deps() {
  case "${1:-}" in
    --resolve) shift; mg_sync_web__run sync-web-resolver resolve "$@" ;;
    --install) shift; mg_sync_web__run sync-web-resolver install "$@" ;;
    "") mg_sync_web__run sync-web-resolver audit ;;
    *) mg_failure usage "Use sync-web-deps [--resolve|--install]"; mg_status error ;;
  esac
}
mg_sync_web_browser_install() { mg_sync_web__run sync-web-resolver browser-install "$@"; }
mg_sync_web_typecheck() { mg_sync_web__run sync-web-check typecheck "$@"; }
mg_sync_web_unit() { mg_sync_web__run sync-web-check unit "$@"; }
mg_sync_web_build() { mg_sync_web__run sync-web-check build "$@"; }
mg_sync_web_check() { mg_sync_web__run sync-web-check check "$@"; }
mg_sync_web_actualserver_build() {
  local prepared
  if [ "$#" -ne 1 ] || ! prepared="$(mg_sync_web__directory "$1")" || \
      [ ! -f "$prepared/meguri_web_assets.rs" ] || [ ! -f "$prepared/assets.json" ] || [ ! -d "$prepared/embedded" ]; then
    mg_failure usage "Use sync-web-actualserver-build <checkout/build/.../prepared>"; mg_status error; return
  fi
  if ! mg_sync_web__compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
      -v "$MG_ROOT/server/src:/server/src:ro" \
      -v "$MG_ROOT/server/Cargo.toml:/server/Cargo.toml:ro" \
      -v "$MG_ROOT/server/Cargo.lock:/server/Cargo.lock:ro" \
      -v "$MG_ROOT/server/build.rs:/server/build.rs:ro" \
      -v "$MG_ROOT/server/rust-toolchain.toml:/server/rust-toolchain.toml:ro" \
      -v "$MG_ROOT/server/web:/server/web:ro" \
      -v "$MG_ROOT/core/crates/meguri-model:/core/crates/meguri-model:ro" \
      -v "$MG_ROOT/core/Cargo.toml:/core/Cargo.toml:ro" \
      -v "$MG_ROOT/core/rust-toolchain.toml:/core/rust-toolchain.toml:ro" \
      -v "$prepared:/prepared:ro" -v "$MG_RUN_DIR:/out" sync-web-actualserver actualserver; then
    mg_sync_web__failure "Actual Rust server compilation failed"; return
  fi
  mg_artifact directory "$MG_RUN_DIR" "Exact actual-server binary, source/build/asset hashes"
  mg_status pass
}
mg_sync_web_preview_up() {
  local binary_dir
  if [ "$#" -ne 1 ] || ! binary_dir="$(mg_sync_web__directory "$1")" || \
      [ ! -f "$binary_dir/meguri-sync-server" ] || [ ! -f "$binary_dir/assets.json" ]; then
    mg_failure usage "Use sync-web-preview-up <checkout/build/.../actualserver-output>"; mg_status error; return
  fi
  # Persist only the owned synthetic preview service; down never touches shared services.
  # Compose run supports explicit read-only mounts without writing shared configuration.
  local name="$(mg_compose_project)-sync-web-preview"
  if ! mg_sync_web__compose run -d --no-deps --service-ports --name "$name" \
      -v "$binary_dir:/binary:ro" -v "$MG_RUN_DIR:/out" sync-web-preview preview; then
    mg_sync_web__failure "Could not start owned HTTPS fixture"; return
  fi
  if ! docker exec "$name" node /opt/sync-web/ready.mjs; then
    docker stop --time 10 "$name" >/dev/null
    docker rm "$name" >/dev/null
    mg_sync_web__failure "Fixture initialization did not publish readiness"; return
  fi
  mg_artifact directory "$MG_RUN_DIR" "Synthetic preview identities, capability descriptors and exact build identity"
  mg_detail preview_url '"https://localhost:18443"'
  mg_detail setup_url '"https://localhost:18445/setup"'
  mg_detail test_url '"https://localhost:18447"'
  mg_status pass
}
mg_sync_web_preview_down() {
  if [ "$#" -ne 0 ]; then mg_failure usage "Use sync-web-preview-down"; mg_status error; return; fi
  local name="$(mg_compose_project)-sync-web-preview"
  if ! docker stop --time 10 "$name" || ! docker rm "$name"; then
    mg_sync_web__failure "Could not stop/remove the owned preview container"; return
  fi
  mg_status pass
}
mg_sync_web_browser() {
  local fixture_dir
  if [ "$#" -ne 1 ] || ! fixture_dir="$(mg_sync_web__directory "$1")" || [ ! -f "$fixture_dir/test/fixture.json" ]; then
    mg_failure usage "Use sync-web-browser <checkout/build/.../preview-up-output>"; mg_status error; return
  fi
  local image
  image="$(docker image inspect --format '{{.Id}}' "$(mg_compose_project)-sync-web:local")" || {
    mg_sync_web__failure "Cannot resolve browser image"; return
  }
  mg_sync_web__browser "$fixture_dir" "$(mg_compose_project)-sync-web-preview" "$image" || return
  mg_status pass
}

# Internal identity parameters only; callers use the same browser invocation.
# Register hooks in the owning job shell, never inside a command substitution.
mg_sync_web__browser() {
  local fixture_dir="$1" container="$2" image="$3" identity owner name browser_user="${4:-0:0}"
  identity="$(docker inspect --format '{{.Id}} {{.Image}} {{.State.Running}}' "$container")" || {
    mg_sync_web__failure "Cannot inspect browser fixture"; return 1
  }
  local cid="${identity%% *}"
  if ! [[ "$cid" =~ ^[0-9a-f]{64}$ ]] || [ "$identity" != "$cid $image true" ]; then
    mg_sync_web__failure "Browser fixture identity or running image differs"; return 1
  fi
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  name="meguri-web-browser-$MG_RUN_ID-$owner"
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  local execution_status=0 count_status=0
  docker run --rm --init --read-only --user "$browser_user" --cap-drop ALL --security-opt no-new-privileges \
      --name "$name" --label "meguri.sync.owner=$owner" \
      --label "meguri.mg.run-id=$MG_RUN_ID" --shm-size=512m --tmpfs /tmp --tmpfs /work:exec \
      --network "container:$cid" \
      -v "$(mg_compose_project)_sync-web-deps:/deps:ro" \
      -v "$(mg_compose_project)_sync-web-browsers:/browsers:ro" \
      -v "$MG_ROOT/server/web:/source:ro" -v "$MG_RUN_DIR:/out" \
      -v "$fixture_dir/test:/fixture:ro" -e MEGURI_WEB_BASE_URL=https://localhost:18447 \
      "$image" browser || execution_status=$?
  mg_sync_server__remove_container "$name" "$owner" || execution_status=1
  if [ "$execution_status" -eq 0 ]; then
    mg_detail browser_cleanup "$(jq -nc --arg name "$name" --arg owner "$owner" \
      '{name:$name,owner:$owner,absence_confirmed:true}')"
  fi
  mg_sync_web__counts browser "$MG_RUN_DIR/browser-results.json" || count_status=1
  if [ "$execution_status" -ne 0 ] || [ "$count_status" -ne 0 ]; then
    mg_sync_web__failure "Actual fixture browser/axe suite or checked cleanup failed"; return 1
  fi
  mg_artifact directory "$MG_RUN_DIR" "Browser results and narrow/wide screenshots"
}

# Retain even failed child receipts. These are fresh invocations, never latest.
# Set the result in this shell so callers can use the exact owned output.
mg_sync_web__child() {
  local job="$1" result rc=0 arguments
  shift
  arguments="$(mg_json_array ${1+"$@"})" || return 1
  result="$("$MG_TOOLS/mg" "$job" ${1+"$@"})" || rc=$?
  case "$result" in "$MG_ROOT"/build/mg/"$job"/*/result.json) ;; *)
    mg_sync_web__failure "Missing $job child receipt"; return 1 ;;
  esac
  mg_artifact child-receipt "$result" "$job (fresh required child)"
  mg_artifact directory "${result%/result.json}" "$job underlying reports and identities"
  MG_SYNC_WEB_CHILD="$result"
  if [ "$rc" -ne 0 ] || ! jq -e --arg root "$MG_ROOT" --arg job "$job" --argjson args "$arguments" '
      .root == $root and .job == $job and .args == $args and .dry_run == false and
      .status == "pass" and .exit_code == 0 and .failures == [] and
      (.started_at | type == "string") and (.finished_at | type == "string")
    ' "$result" >/dev/null; then
    mg_sync_web__failure "Required $job child failed or has incomplete evidence"; return 1
  fi
}

# A null running state checks ownership only, including failed initialization.
mg_sync_web__fixture_inspect() {
  local name="$1" cid="$2" owner="$3" image="$4" binary_dir="$5" running="$6"
  local inspection="$MG_RUN_DIR/fixture-container-$running.json"
  docker inspect "$cid" >"$inspection" || return 1
  mg_artifact json "$inspection" "Private fixture immutable identity, isolation and mounts"
  jq -e --arg name "/$name" --arg cid "$cid" --arg run "$MG_RUN_ID" --arg owner "$owner" \
      --arg image "$image" --arg binary "$binary_dir" --arg out "$MG_RUN_DIR/fixture" --argjson running "$running" '
    length == 1 and (.[0] |
      .Id == $cid and .Name == $name and
      .Config.Labels["meguri.mg.run-id"] == $run and .Config.Labels["meguri.sync.owner"] == $owner and
      (if $running == null then true else
      .Image == $image and .Config.Image == $image and
      .State.Running == $running and .Config.Cmd == ["preview"] and .Config.User == "0:0" and
      .Config.Entrypoint == ["bash", "/opt/sync-web/run.sh"] and
      .HostConfig.NetworkMode == "none" and (.HostConfig.PortBindings // {} | length) == 0 and
      (.NetworkSettings.Ports // {} | length) == 0 and .HostConfig.PublishAllPorts == false and
      ((.NetworkSettings.Networks | keys) == ["none"] or
        ($running == false and (.NetworkSettings.Networks | length) == 0)) and
      .HostConfig.ReadonlyRootfs == true and .HostConfig.Privileged == false and .HostConfig.Init == true and
      .HostConfig.CapDrop == ["ALL"] and (.HostConfig.CapAdd // [] | length) == 0 and
      .HostConfig.SecurityOpt == ["no-new-privileges"] and
      .HostConfig.Tmpfs == {"/tmp":"noexec", "/work":"exec", "/fixtures":"rw,noexec,nosuid,nodev,mode=0700,uid=0,gid=0"} and
      ([.Mounts[] | select(.Type == "bind") | {Source,Destination,RW}] | sort_by(.Destination)) ==
        ([{Source:($binary + "/meguri-sync-server"),Destination:"/binary/meguri-sync-server",RW:false},
          {Source:($binary + "/assets.json"),Destination:"/binary/assets.json",RW:false},
          {Source:$out,Destination:"/out",RW:true}] | sort_by(.Destination)) and
      all(.Mounts[]; .Type == "bind" or
        (.Type == "tmpfs" and (.Destination == "/tmp" or .Destination == "/work" or .Destination == "/fixtures")))
      end))
  ' "$inspection" >/dev/null
}

# Capture by validated immutable ID before the shared helper checks removal.
# Append only when present: an idempotent exit hook must retain earlier logs.
mg_sync_web__fixture_cleanup() {
  local name="$1" owner="$2" ids rc=0
  if ids="$(docker container ls -aq --no-trunc --filter "name=^/$name$")"; then
    if [ -n "$ids" ]; then
      if [[ "$ids" =~ ^[0-9a-f]{64}$ ]] &&
          mg_sync_web__fixture_inspect "$name" "$ids" "$owner" "" "" null; then
        docker logs "$ids" >>"$MG_RUN_DIR/fixture.log" 2>&1 || rc=1
        mg_artifact log "$MG_RUN_DIR/fixture.log" "Owned detached fixture stdout/stderr before removal"
      else
        rc=1
      fi
    fi
  else
    rc=1
  fi
  if [ "$rc" -ne 0 ]; then
    mg_failure sync-web-fixture-logs "Could not preserve owned fixture logs before cleanup"
    mg_status fail
  fi
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  return "$rc"
}

mg_sync_web__verify_counts() {
  local frontend="$1" counts key value
  # Revalidate the child's TAP-validated report; build/install steps earn no tests.
  counts="$(jq -n --slurpfile frontend "$frontend/test-counts.json" \
      --slurpfile browser "$MG_RUN_DIR/test-counts.json" '
    {frontend:$frontend[0], browser:$browser[0]} |
    select(all(.[]; .errors == [] and .counts.total > 0 and
      .counts.total == .counts.completed and .counts.total == .counts.passed))
  ')" || return 1
  [ -n "$counts" ] || return 1
  mg_detail test_groups "$counts"
  mg_artifact json "$frontend/test-counts.json" "Frontend child counts bound to retained TAP"
  counts="$(jq -c '[.[].counts | to_entries[]] | group_by(.key) |
      map({key:.[0].key,value:(map(.value)|add)}) | from_entries' <<<"$counts")" || return 1
  while IFS=$'\t' read -r key value; do mg_count "$key" "$value"; done \
    < <(jq -r 'to_entries[] | [.key,.value] | @tsv' <<<"$counts")
}

# Same-shell body: return to the owner for checked cleanup on every outcome.
mg_sync_web__verify_body() {
  local name="$1" owner="$2" image="$3" binary_dir="$4" frontend="$5" cid binary_hash
  docker create --init --read-only --cap-drop ALL --security-opt no-new-privileges --user 0:0 \
      --name "$name" --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" \
      --network none --tmpfs /tmp:noexec --tmpfs /work:exec \
      --tmpfs /fixtures:rw,noexec,nosuid,nodev,mode=0700,uid=0,gid=0 \
      -v "$binary_dir/meguri-sync-server:/binary/meguri-sync-server:ro" \
      -v "$binary_dir/assets.json:/binary/assets.json:ro" -v "$MG_RUN_DIR/fixture:/out" \
      "$image" preview >"$MG_RUN_DIR/fixture.cid" || return 1
  cid="$(cat "$MG_RUN_DIR/fixture.cid")" || return 1
  [[ "$cid" =~ ^[0-9a-f]{64}$ ]] || return 1
  mg_sync_web__fixture_inspect "$name" "$cid" "$owner" "$image" "$binary_dir" false || return 1
  docker start "$cid" >/dev/null || return 1
  docker exec "$cid" node /opt/sync-web/ready.mjs || return 1
  mg_sync_web__fixture_inspect "$name" "$cid" "$owner" "$image" "$binary_dir" true || return 1
  # Verify runtime storage ownership/permissions, not just requested mount flags.
  docker exec "$cid" /bin/sh -c 'test "$(stat -c "%u:%g:%a" /fixtures)" = 0:0:700' || return 1
  [ -f "$MG_RUN_DIR/fixture/ready" ] && [ -s "$MG_RUN_DIR/fixture/test/fixture.json" ] || return 1
  binary_hash="$(shasum -a 256 "$binary_dir/meguri-sync-server" | awk '{print $1}')" || return 1
  jq -e --arg hash "$binary_hash" --slurpfile assets "$binary_dir/assets.json" '
    .binary_sha256 == $hash and .assets == $assets[0] and (.assets | length) > 0 and
    .test.origin == "https://localhost:18447"
  ' "$MG_RUN_DIR/fixture/preview.json" >/dev/null || return 1
  cmp "$binary_dir/assets.json" "$frontend/prepared/assets.json" || return 1
  # Identity probe only: require bytes from the running binary, not just the
  # copied inventory. This contributes artifacts, never test credits.
  docker exec "$cid" node --input-type=module -e '
    import { readFile, writeFile } from "node:fs/promises";
    import { createHash } from "node:crypto";
    import { get } from "node:https";
    const assets = JSON.parse(await readFile("/binary/assets.json", "utf8"));
    if (!Array.isArray(assets) || !assets.length) throw new Error("Missing asset inventory");
    const observed = [];
    for (const asset of assets) {
      if (!/^\/(index\.html|assets\/[A-Za-z0-9_.-]+)$/.test(asset.path)) throw new Error("Invalid asset path");
      const bytes = await new Promise((resolve, reject) => {
        const request = get("https://localhost:18447" + (asset.path === "/index.html" ? "/" : asset.path),
          { rejectUnauthorized: false, headers: { Origin: "https://localhost:18447" } }, response => {
            if (response.statusCode !== 200) { response.resume(); reject(new Error("Asset unavailable")); return; }
            const chunks = [];
            response.on("data", chunk => chunks.push(chunk));
            response.on("error", reject);
            response.on("end", () => resolve(Buffer.concat(chunks)));
          });
        request.on("error", reject);
        request.setTimeout(15000, () => request.destroy(new Error("Asset identity timeout")));
      });
      const sha256 = createHash("sha256").update(bytes).digest("hex");
      if (bytes.length !== asset.bytes || sha256 !== asset.sha256) throw new Error("Served asset identity differs");
      observed.push({ path: asset.path, bytes: bytes.length, sha256 });
    }
    await writeFile("/out/asset-identity.json", JSON.stringify(observed, null, 2) + "\n");
  ' || return 1
  mg_artifact directory "$MG_RUN_DIR/fixture" "Fresh synthetic HTTPS readiness and actual binary/assets identity"
  mg_sync_web__browser "$MG_RUN_DIR/fixture" "$cid" "$image" || return 1
  mg_sync_web__verify_counts "$frontend" || return 1
}

mg_sync_web_verify() {
  local package_receipt=
  if [ "$#" -eq 2 ] && [ "$1" = --package-receipt ]; then package_receipt="$2"
  elif [ "$#" -ne 0 ]; then mg_failure usage "Use sync-web-verify [--package-receipt <result.json>]"; mg_status error; return; fi
  mg_status fail
  mg_artifact directory "$MG_RUN_DIR" "Composite child receipts, reports, identities and checked cleanup"
  local frontend binary_dir image owner name rc=0
  mg_sync_web__child sync-web-image || return
  image="$(docker image inspect --format '{{.Id}}' "$(mg_compose_project)-sync-web:local")" || return
  [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]] || return
  mg_detail image_id "$(jq -nc --arg image "$image" '$image')"
  mg_sync_web__child sync-web-deps --install || return
  mg_sync_web__child sync-web-deps || return
  mg_sync_web__child sync-web-browser-install || return
  mg_sync_web__child sync-web-check || return
  frontend="${MG_SYNC_WEB_CHILD%/result.json}"
  if [ -n "$package_receipt" ]; then
    mg_sync_web__package_verify "$package_receipt" "$image" "$frontend" || return
    mg_status pass
    return
  fi
  mg_sync_web__child sync-web-actualserver-build "$frontend/prepared" || return
  binary_dir="${MG_SYNC_WEB_CHILD%/result.json}"
  [ -x "$binary_dir/meguri-sync-server" ] && [ -s "$binary_dir/assets.json" ] &&
    [ -s "$binary_dir/build-sha256.txt" ] && [ -s "$binary_dir/server-source-sha256.txt" ] &&
    [ -s "$frontend/source-sha256.txt" ] || { mg_sync_web__failure "Missing real build identities"; return; }
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return
  name="meguri-web-verify-$MG_RUN_ID-$owner"
  # Register before creation in this owner shell, including cancellation cleanup.
  mg_on_exit "$(printf '%q ' mg_sync_web__fixture_cleanup "$name" "$owner")"
  mkdir -m 700 "$MG_RUN_DIR/fixture" || return
  mg_sync_web__verify_body "$name" "$owner" "$image" "$binary_dir" "$frontend" || rc=1
  mg_sync_web__fixture_cleanup "$name" "$owner" || rc=1
  if [ "$rc" -ne 0 ]; then
    mg_sync_web__failure "Private verification or ownership-checked cleanup failed"; return
  fi
  mg_detail fixture_cleanup "$(jq -nc --arg name "$name" --arg owner "$owner" \
    '{name:$name,owner:$owner,absence_confirmed:true}')"
  mg_status pass
}

mg_sync_web__package_verify() {
  local receipt="$1" image="$2" frontend="$3" MG_SYNC_INSTANCE="$MG_RUN_DIR/instance"
  local MEGURI_SYNC_ORIGIN=https://localhost:18447 MEGURI_SYNC_TRUSTED_PEER=127.0.0.1 MEGURI_SYNC_NETWORK=none
  local MG_SYNC_SETUP_FILE=/dev/null
  local service name="meguri-web-package-$MG_RUN_ID" owner rc=0 directory
  mg_sync_package__receipt "$receipt" || { mg_sync_web__failure "Invalid exact package receipt"; return 1; }
  directory="$(cd "$(dirname "$receipt")" && pwd -P)" || return 1
  mkdir "$MG_RUN_DIR/package-artifacts" || return 1
  mkdir -m 700 "$MG_RUN_DIR/fixture" || return 1
  mg_sync_package__candidate_input "$receipt" "$MG_RUN_DIR/package-artifacts" >"$MG_RUN_DIR/package-identity.json" || return 1
  cmp -s "$directory/product/assets.json" "$MG_RUN_DIR/package-artifacts/$(jq -er .package.assets_sha256 "$MG_RUN_DIR/package-identity.json")" || return 1
  cmp -s "$frontend/prepared/assets.json" "$directory/product/assets.json" || {
    mg_sync_web__failure "Frontend proof assets differ from the exact image"; return 1;
  }
  mg_artifact exact-package "$receipt"
  mg_artifact package-identity "$MG_RUN_DIR/package-identity.json"
  mg_artifact package-artifacts "$MG_RUN_DIR/package-artifacts"
  mg_on_exit "$(printf '%q ' mg_sync_package__volume_remove "$MG_SYNC_INSTANCE")"
  mg_on_exit "local MG_SYNC_INSTANCE=$(printf '%q' "$MG_SYNC_INSTANCE"); mg_sync_package__stop"
  mg_sync_package__init "$receipt" && mg_sync_package__start || {
    mg_sync_web__failure "Exact-image browser service failed to initialize/start"; return 1;
  }
  service="$(jq -er .name "$MG_SYNC_INSTANCE/container.json")" || return 1
  owner="$(python3 -c 'import secrets; print(secrets.token_hex(16))')" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_container "$name" "$owner")"
  docker run -d --init --read-only --user 10001:10001 --cap-drop ALL --security-opt no-new-privileges \
    --name "$name" --network "container:$service" --tmpfs /tmp:uid=10001,gid=10001,mode=0700 \
    --tmpfs /out:uid=10001,gid=10001,mode=0700 \
    --label "meguri.mg.run-id=$MG_RUN_ID" --label "meguri.sync.owner=$owner" \
    --mount "type=volume,src=$(jq -er .volume "$MG_SYNC_INSTANCE/instance.json"),dst=/data,readonly" \
    "$image" package-preview >"$MG_RUN_DIR/package-proxy.cid" || rc=1
  [ "$rc" -ne 0 ] || docker exec "$name" node /opt/sync-web/ready.mjs || rc=1
  # docker cp cannot read the running proxy's tmpfs. Export just these two
  # fixed files in-container; capability bytes go directly to private files.
  if [ "$rc" -eq 0 ]; then
    python3 - "$MG_RUN_DIR" "$name" "$owner" "$image" "$MG_RUN_ID" <<'PY' || rc=1
import base64
import json
import os
import re
import stat
import subprocess
import sys

try:
    root, name, owner, image, run = sys.argv[1:]
    if not re.fullmatch(r"[0-9a-f]{32}", owner) or not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
        raise ValueError()
    with open(root + "/package-proxy.cid", encoding="ascii") as handle:
        cid = handle.read().strip()
    if not re.fullmatch(r"[0-9a-f]{64}", cid):
        raise ValueError()
    # No source selector comes from a receipt, environment or descriptor.
    script = r'''
import { constants, lstatSync, openSync, fstatSync, readFileSync, closeSync, writeSync } from "node:fs";
try {
  const path = process.argv[1];
  if (path !== "/out/ready" && path !== "/out/test/fixture.json") throw 0;
  const directories = ["/out", ...(path === "/out/ready" ? [] : ["/out/test"])];
  const parents = directories.map(directory => {
    const s = lstatSync(directory);
    if (!s.isDirectory() || s.uid !== 10001 || s.gid !== 10001 || (s.mode & 0o022) ||
        (directory === "/out" && (s.mode & 0o777) !== 0o700)) throw 0;
    return s;
  });
  const before = lstatSync(path);
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const s = fstatSync(fd);
    if (!s.isFile() || s.nlink !== 1 || s.uid !== 10001 || s.gid !== 10001 ||
        ((s.mode & 0o7777) !== 0o600 && !(path === "/out/ready" && (s.mode & 0o7777) === 0o644)) ||
        s.dev !== before.dev || s.ino !== before.ino || s.size < 1 || s.size > 4096) throw 0;
    const bytes = readFileSync(fd);
    const after = fstatSync(fd), binding = lstatSync(path);
    if (bytes.length !== s.size || after.size !== s.size || after.mtimeMs !== s.mtimeMs ||
        after.ctimeMs !== s.ctimeMs || binding.dev !== s.dev || binding.ino !== s.ino ||
        binding.nlink !== 1 || after.mode !== s.mode || after.uid !== s.uid || after.gid !== s.gid) throw 0;
    directories.forEach((directory, i) => {
      const current = lstatSync(directory), previous = parents[i];
      if (current.dev !== previous.dev || current.ino !== previous.ino || current.mode !== previous.mode ||
          current.uid !== previous.uid || current.gid !== previous.gid) throw 0;
    });
    writeSync(1, bytes);
  } finally { closeSync(fd); }
} catch { process.exit(1); }
'''
    # Pin the private destination directories and use nofollow, exclusive files.
    fixture = root + "/fixture"
    if os.path.realpath(root) != root:
        raise ValueError()
    directory = os.lstat(fixture)
    if not stat.S_ISDIR(directory.st_mode) or directory.st_uid != os.getuid() or stat.S_IMODE(directory.st_mode) != 0o700:
        raise ValueError()
    fixture_fd = os.open(fixture, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        pinned = os.fstat(fixture_fd)
        if (pinned.st_dev, pinned.st_ino) != (directory.st_dev, directory.st_ino):
            raise ValueError()
        os.mkdir("test", 0o700, dir_fd=fixture_fd)
        test_fd = os.open("test", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fixture_fd)
        try:
            test = os.fstat(test_fd)
            if test.st_uid != os.getuid() or stat.S_IMODE(test.st_mode) != 0o700:
                raise ValueError()
            for source, destination, parent in (("/out/ready", "ready", fixture_fd),
                                                  ("/out/test/fixture.json", "fixture.json", test_fd),
                                                  (None, None, None)):
                inspection = subprocess.run(["docker", "inspect", cid], check=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
                containers = json.loads(inspection.stdout)
                if len(containers) != 1:
                    raise ValueError()
                container = containers[0]
                if (container["Id"] != cid or container["Name"] != "/" + name or
                    container["Image"] != image or container["Config"]["Image"] != image or
                    container["State"]["Running"] is not True or container["Config"]["User"] != "10001:10001" or
                    container["Config"]["Cmd"] != ["package-preview"] or
                    container["Config"]["Labels"]["meguri.mg.run-id"] != run or
                    container["Config"]["Labels"]["meguri.sync.owner"] != owner):
                    raise ValueError()
                if source is None:
                    for path, previous in ((fixture, pinned), (fixture + "/test", test)):
                        current = os.lstat(path)
                        if ((current.st_dev, current.st_ino, current.st_mode, current.st_uid) !=
                            (previous.st_dev, previous.st_ino, previous.st_mode, previous.st_uid)):
                            raise ValueError()
                    continue
                fd = os.open(destination, os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
                with os.fdopen(fd, "w+b") as output:
                    subprocess.run(["docker", "exec", cid, "node", "--input-type=module", "-e", script, source],
                                   check=True, stdout=output, stderr=subprocess.DEVNULL)
                    output.seek(0)
                    data = output.read(4097)
                    s = os.fstat(output.fileno())
                    if not stat.S_ISREG(s.st_mode) or s.st_nlink != 1 or s.st_uid != os.getuid() or stat.S_IMODE(s.st_mode) != 0o600:
                        raise ValueError()
                    if source == "/out/ready":
                        if data != b"ready\n":
                            raise ValueError()
                    else:
                        descriptor = json.loads(data)
                        if (len(data) > 4096 or type(descriptor) is not dict or len(json.loads(data, object_pairs_hook=list)) != 2 or
                            set(descriptor) != {"origin", "setup_token"} or
                            descriptor["origin"] != "https://localhost:18447" or type(descriptor["setup_token"]) is not str or
                            not re.fullmatch(r"[A-Za-z0-9_-]{43}", descriptor["setup_token"])):
                            raise ValueError()
                        token = base64.urlsafe_b64decode(descriptor["setup_token"] + "=")
                        if base64.urlsafe_b64encode(token).decode().rstrip("=") != descriptor["setup_token"]:
                            raise ValueError()
        finally:
            os.close(test_fd)
    finally:
        os.close(fixture_fd)
except Exception:
    # Never include parser errors, descriptor bytes or Docker output in MG logs.
    sys.exit(1)
PY
  fi
  [ "$rc" -ne 0 ] || mg_sync_web__browser "$MG_RUN_DIR/fixture" "$name" "$image" "$(id -u):$(id -g)" || rc=1
  docker inspect "$service" >"$MG_RUN_DIR/service-container-after.json" || rc=1
  jq -e --slurpfile before "$MG_RUN_DIR/service-container.json" '
    length==1 and .[0].State.Running and .[0].Id==$before[0][0].Id and
    .[0].Image==$before[0][0].Image and .[0].Config==$before[0][0].Config and
    .[0].Mounts==$before[0][0].Mounts and .[0].HostConfig==$before[0][0].HostConfig and
    .[0].State.StartedAt==$before[0][0].State.StartedAt' "$MG_RUN_DIR/service-container-after.json" >/dev/null || rc=1
  mg_artifact service-container-after "$MG_RUN_DIR/service-container-after.json"
  mg_sync_server__remove_container "$name" "$owner" || rc=1
  mg_sync_package__stop || rc=1
  if [ "$rc" -ne 0 ]; then mg_sync_web__failure "Exact-image full browser journey or checked cleanup failed"; return 1; fi
  mg_sync_web__verify_counts "$frontend" || return 1
}
