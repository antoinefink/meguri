# SPDX-License-Identifier: MIT
# Separate retained public HTTP worker. No service/client runtime on the host.
mg_job sync-server-retained docker:dev mg_sync_retained \
  "Retained public HTTP peer workspace: prepare-lock, build, check (Docker only; no compatibility credit)"

mg_sync_retained() {
  local mode="${1:-}" peer="$MG_ROOT/conformance/peers/retained-sync" graph compiler_host
  [ "$#" -eq 1 ] || { mg_failure usage "sync-server-retained needs prepare-lock, build or check"; mg_status error; return; }
  case "$mode" in prepare-lock|build|check) ;; *) mg_failure usage "Unknown retained peer operation"; mg_status error; return ;; esac
  if ! python3 "$peer/support.py" check; then
    mg_failure retained-inputs "Committed selection, recipes or workspace boundaries differ"; mg_status fail; return
  fi
  if [ "$mode" = check ]; then
    mg_status pass
    return
  fi
  if [ "$mode" = prepare-lock ]; then
    if [ -e "$peer/Cargo.lock" ]; then
      mg_failure lock-exists "Prepare-lock only creates the absent separate lock; retain existing input"; mg_status fail; return
    fi
    cp "$MG_ROOT/core/Cargo.lock" "$peer/Cargo.lock" || return 1
    # Metadata minimally extends the existing locked resolution with this root.
    # Do not use generate-lockfile: it would resolve all packages afresh.
    if ! (cd "$MG_ROOT/core" && cargo metadata --offline --format-version 1 --manifest-path "$peer/Cargo.toml") >"$MG_RUN_DIR/graph.json" ||
        ! python3 "$peer/support.py" verify-lock; then
      mg_failure retained-lock "Offline lock preparation failed; inspect the retained lock before any build"; mg_status fail; return
    fi
    mg_artifact peer-lock "$peer/Cargo.lock" "Separate prepared lock; review and commit before locked verification"
    mg_message "Lock preparation only; no compiled or compatibility evidence"
    mg_status pass
    return
  fi
  if [ ! -f "$peer/Cargo.lock" ]; then
    mg_failure lock-missing "Run sync-server-retained prepare-lock in Docker, then review and commit its separate lock"; mg_status fail; return
  fi
  if [ -n "$(git -C "$MG_ROOT" status --porcelain=v1 --untracked-files=all)" ]; then
    mg_failure source-dirty "Peer builds require clean committed source and separate reviewed lock"; mg_status fail; return
  fi
  graph="$MG_RUN_DIR/graph.json"
  compiler_host="$(cd "$MG_ROOT/core" && rustc -vV | sed -n 's/^host: //p')" || return 1
  [ -n "$compiler_host" ] || { mg_failure toolchain "Compiler host identity missing"; mg_status fail; return; }
  if ! python3 "$peer/support.py" source "$MG_RUN_DIR/source-before.json" ||
      ! python3 "$peer/support.py" verify-lock ||
      ! (cd "$MG_ROOT/core" && cargo metadata --locked --offline --filter-platform "$compiler_host" --format-version 1 --manifest-path "$peer/Cargo.toml") >"$graph" ||
      ! (cd "$MG_ROOT/core" && cargo build --locked --offline --target "$compiler_host" --message-format=json --manifest-path "$peer/Cargo.toml") >"$MG_RUN_DIR/cargo-build.jsonl" ||
      ! python3 "$peer/support.py" freeze "$MG_RUN_DIR/client" "$graph" "$MG_RUN_DIR/source-before.json" "$MG_RUN_DIR/cargo-build.jsonl"; then
    if [ -f "$MG_RUN_DIR/cargo-build.jsonl" ]; then mg_artifact compilation "$MG_RUN_DIR/cargo-build.jsonl" "Actual Cargo artifact messages and compiler diagnostics"; fi
    mg_failure retained-build "Locked retained peer build failed; no runtime or compatibility credit"; mg_status fail; return
  fi
  mg_artifact retained-client "$MG_RUN_DIR/client" "Actual executable, exact core/peer locks, source and resolved features"
  mg_detail retained_client "$(cat "$MG_RUN_DIR/client/client-build.json")"
  mg_message "Build only. Parent owns reviewed action inputs, owned final image/namespace, Desktop/fault steps and all eight case verdicts"
  mg_status pass
}
