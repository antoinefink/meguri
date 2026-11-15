# Quarantined current-release client/reference work; no production imports.
mg_job sync-server-reference host mg_sync_server_reference \
  "Pinned current-release reference harness, semantic validation and synthetic probes"

mg_sync_reference_digest() {
  (
    set -o pipefail
    cd "$MG_ROOT" || exit 1
    rg --files oracle/meguri-sync-server docker/meguri-sync-reference.Dockerfile \
      docker/meguri-sync-reference.Dockerfile.dockerignore docker/meguri-sync-reference.compose.yml \
      tools/mg.d/meguri-sync-reference.sh conformance/schema/meguri-sync-server.schema.json \
      conformance/catalog/meguri-sync-server.json conformance/fixtures/meguri-sync-server/original.json \
      conformance/recordings/scenarios/meguri-sync-server/desktop-selection.json \
      conformance/fixtures/meguri-sync-server/edges.json conformance/recordings/scenarios/meguri-sync-server/desktop-edge-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-full-selection.json \
      conformance/recordings/scenarios/meguri-sync-server/candidate-complete-selection.json |
      LC_ALL=C sort | while IFS= read -r file; do shasum -a 256 "$file" || exit 1; done | shasum -a 256 | cut -d ' ' -f1
  )
}

mg_sync_server_reference() {
  local mode="${1:-selftest}" reference_dir="$MG_ROOT/oracle/meguri-sync-server"
  [ "$#" -eq 0 ] || shift
  case "$mode" in
    deps)
      if [ "$#" -ne 0 ]; then mg_failure usage "deps takes no arguments"; mg_status error; return; fi
      local audit_status=0
      mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" run --rm -T --no-deps \
        --label "meguri.mg.run-id=$MG_RUN_ID" -u "$(id -u):$(id -g)" \
        -v "$reference_dir:/lock:ro" -v "$MG_RUN_DIR:/out" \
        sync-reference-resolver python /lock/audit_deps.py || audit_status=$?
      if [ -f "$MG_RUN_DIR/summary.json" ]; then
        mg_count total "$(jq -r .total "$MG_RUN_DIR/summary.json")"
        mg_count passed "$(jq -r .passed "$MG_RUN_DIR/summary.json")"
        mg_count failed "$(jq -r .failed "$MG_RUN_DIR/summary.json")"
      fi
      mg_artifact report "$MG_RUN_DIR/dependencies.json" "Test-only exact dependency graph review"
      if [ "$audit_status" -ne 0 ]; then mg_failure dependency-audit "test dependency metadata/hash/advisory audit failed"; mg_status fail; else mg_status pass; fi
      return ;;
    lock)
      if [ "$#" -ne 0 ]; then
        mg_failure usage "lock takes no arguments"; mg_status error; return
      fi
      if ! mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" run --rm -T --no-deps \
        --label "meguri.mg.run-id=$MG_RUN_ID" -v "$reference_dir:/lock" -w /lock \
        sync-reference-resolver sh -c 'pip install --quiet --disable-pip-version-check --root-user-action=ignore uv==0.12.24 && uv pip compile --quiet --generate-hashes --python-version 3.12 --python-platform aarch64-manylinux_2_35 --custom-compile-command "tools/mg sync-server-reference lock" requirements.in -o requirements.lock'; then
        mg_failure dependency-lock "current reference dependency resolution failed"; mg_status fail; return
      fi
      mg_artifact lock "$reference_dir/requirements.lock"
      mg_status pass; return ;;
    selftest|identity|smoke|edges|catalog|validate|seal) ;;
    *) mg_failure usage "expected lock, deps, selftest, identity, smoke, edges, catalog, validate or seal"; mg_status error; return ;;
  esac
  local source_sha lock_sha source_commit author_session argument_json
  local mount_args=() runtime_args=("$mode") input_path
  argument_json="$(printf '%s\n' "$mode" ${1+"$@"} | jq -Rs 'split("\n")[:-1]')"
  case "$mode" in
    validate|seal)
      if [ "$#" -ne 1 ]; then mg_failure usage "validate/seal requires a retained build/ evidence directory"; mg_status error; return; fi
      case "$1" in build/*) ;; *) mg_failure input-path "input must be under build/"; mg_status error; return ;; esac
      input_path="$(cd "$MG_ROOT/$1" && pwd -P)" || return 1
      case "$input_path" in "$MG_ROOT/build/"*) ;; *) mg_failure input-path "input escapes build/"; mg_status error; return ;; esac
      mount_args=(-v "$input_path:/in:ro")
      runtime_args=("$mode" --input /in) ;;
    smoke)
      if [ "$#" -gt 1 ]; then mg_failure usage "smoke accepts one optional build/ request JSON"; mg_status error; return; fi
      if [ "$#" -eq 1 ]; then
        case "$1" in build/*) ;; *) mg_failure input-path "request must be under build/"; mg_status error; return ;; esac
        input_path="$(cd "$(dirname "$MG_ROOT/$1")" && pwd -P)/$(basename "$1")" || return 1
        case "$input_path" in "$MG_ROOT/build/"*) ;; *) mg_failure input-path "request escapes build/"; mg_status error; return ;; esac
        if [ -L "$input_path" ] || [ -z "$(find "$input_path" -prune -type f -links 1 -print)" ]; then
          mg_failure input-path "request must be one owned regular file"; mg_status error; return
        fi
        mount_args=(-v "$input_path:/input/request.json:ro")
        runtime_args=(smoke --request /input/request.json)
      fi ;;
    *) if [ "$#" -ne 0 ]; then mg_failure usage "this mode takes no arguments"; mg_status error; return; fi ;;
  esac
  source_commit="$(git -C "$MG_ROOT" rev-parse HEAD)" || return 1
  source_sha="$(mg_sync_reference_digest)" || return 1
  if ! mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" --progress plain build sync-reference; then
    mg_failure image-build "current reference image build failed"; mg_status fail; return
  fi
  if [ "$source_sha" != "$(mg_sync_reference_digest)" ]; then
    mg_failure source-changed "reference source changed during its build"; mg_status fail; return
  fi
  lock_sha="$(shasum -a 256 "$reference_dir/requirements.lock" | cut -d ' ' -f1)"
  author_session="$(jq -rs '[.[] | select(.role == "oracle-maintainer" and .name == "meguri-sync-reference")] | last | .session // empty' "$MG_ROOT/docs/process/provenance/sessions.jsonl")" || return 1
  if [ -z "$author_session" ]; then
    mg_failure session-missing "record the current reference oracle-maintainer session before source work"; mg_status error; return
  fi
  jq -n --arg commit "$source_commit" --arg source "$source_sha" --arg lock "$lock_sha" \
    --arg run "$MG_RUN_ID" --arg author "$author_session" --argjson args "$argument_json" \
    '{source_commit:$commit,batch_base:"38235917e6a025daa57625c582e90d253aae62e3",source_sha256:$source,lock_sha256:$lock,mg_job:"sync-server-reference",arguments:$args,run_id:$run,author_session:$author}' \
    >"$MG_RUN_DIR/invocation.json" || return 1
  local execution_status=0
  mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" run --rm -T --no-deps \
    --label "meguri.mg.run-id=$MG_RUN_ID" -u "$(id -u):$(id -g)" \
    -v "$MG_RUN_DIR:/out" -e "MG_RUN_ID=$MG_RUN_ID" \
    ${mount_args[0]+"${mount_args[@]}"} \
    sync-reference "${runtime_args[@]}" || execution_status=$?
  if [ "$source_sha" != "$(mg_sync_reference_digest)" ] || [ "$source_commit" != "$(git -C "$MG_ROOT" rev-parse HEAD)" ]; then
    mg_failure source-changed "reference source/commit changed during execution"; execution_status=1
  fi
  if [ -f "$MG_RUN_DIR/summary.json" ]; then
    mg_count total "$(jq -r '.total // 0' "$MG_RUN_DIR/summary.json")"
    mg_count passed "$(jq -r '.passed // 0' "$MG_RUN_DIR/summary.json")"
    mg_count failed "$(jq -r '.failed // 0' "$MG_RUN_DIR/summary.json")"
    mg_detail reference_evidence "$(cat "$MG_RUN_DIR/summary.json")"
  fi
  mg_detail reference_invocation "$(cat "$MG_RUN_DIR/invocation.json")"
  if [ -f "$MG_RUN_DIR/evidence.json" ]; then mg_artifact report "$MG_RUN_DIR/evidence.json" "Exact neutral observations"; fi
  mg_artifact directory "$MG_RUN_DIR" "Current-release reference evidence"
  if [ "$execution_status" -ne 0 ]; then
    mg_failure reference-execution "reference harness failed; no compatibility credit"; mg_status fail; return
  fi
  if [ "$mode" = smoke ] || [ "$mode" = edges ]; then
    mg_message "Named Desktop reference selection passed; broad catalog and actual Mobile remain unverified"
  fi
  mg_status pass
}
