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

# Finite retained/final HOST branch of sync-server-interop. Build jobs above
# remain docker:dev. Internal case arguments come only from this fixed list.
mg_sync_retained__support() {
  python3 "$MG_ROOT/conformance/peers/retained-sync/support.py" "$@"
}
mg_sync_retained__dispatch() {
  local case_name rc=0
  # All eight actual offline attempts precede ANY package/account construction.
  for case_name in authentication full-upload full-download normal-no-change normal-bidirectional \
      media-bidirectional media-no-change manual-uncertain-recovery; do
    mg_sync_retained__support runtime-prepare "$MG_RUN_DIR" "$case_name" || rc=1
  done
  for case_name in authentication full-upload full-download normal-no-change normal-bidirectional \
      media-bidirectional media-no-change manual-uncertain-recovery; do
    mg_sync_retained__instance "$1" "$case_name" || rc=1
  done
  # Assembly includes every selected case, including unavailable factories.
  mg_sync_retained__support runtime-assemble "$MG_RUN_DIR" || rc=1
  [ "$rc" -eq 0 ]
}
mg_sync_retained__owned() {
  local receipt="$1" peer_receipt="$2" arguments="$3" session="$4" identity after image rc=0 commit
  commit="$(git -C "$MG_ROOT" rev-parse HEAD)" || return 1
  mg_detail candidate_compatibility_credit false
  mg_sync_package__receipt "$receipt" || { mg_failure build-receipt "Fresh canonical clean package required"; return 1; }
  mg_sync_retained__support runtime-receipt "$peer_receipt" || {
    mg_failure retained-client-receipt "Fresh current canonical retained build and exact source/locks/features required"; return 1;
  }
  receipt="$(cd "$(dirname "$receipt")" && pwd -P)/result.json" || return 1
  peer_receipt="$(cd "$(dirname "$peer_receipt")" && pwd -P)/result.json" || return 1
  mkdir "$MG_RUN_DIR/package-artifacts" "$MG_RUN_DIR/preparations" "$MG_RUN_DIR/instances" "$MG_RUN_DIR/output" || return 1
  mg_sync_package__candidate_input "$receipt" "$MG_RUN_DIR/package-artifacts" >"$MG_RUN_DIR/package-identity.json" || return 1
  identity="$(mg_sync_server__reference_identity)" || return 1
  mg_compose -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml" --progress plain build sync-reference || return 1
  after="$(mg_sync_server__reference_identity)" || return 1
  [ "$identity" = "$after" ] || return 1
  image="$(mg_sync_server__image sync-reference -f "$MG_ROOT/docker/meguri-sync-reference.compose.yml")" || return 1
  [[ "$image" =~ ^sha256:[0-9a-f]{64}$ ]] || return 1
  jq -nc --argjson server "$(jq -c .server "$MG_RUN_DIR/package-identity.json")" \
    --arg job "$MG_JOB" --argjson args "$arguments" --arg run "$MG_RUN_ID" --arg session "$session" '
    {source_commit:$server.source_commit,batch_base:"480df4526514e294f5a3a16dd4ab83a1fbcc7f47",
     source_sha256:$server.source_sha256,lock_sha256:$server.lock_sha256,executable_sha256:$server.executable_sha256,
     mg_job:$job,arguments:$args,run_id:$run,author_session:$session}' >"$MG_RUN_DIR/candidate-invocation.json" || return 1
  jq -nc --arg receipt "$receipt" --arg peer "$peer_receipt" --arg image "$image" \
    --argjson reference "$identity" '{package_receipt:$receipt,peer_receipt:$peer,image:$image,reference:$reference}' \
    >"$MG_RUN_DIR/runtime.json" || return 1
  # Register before the first child container. The helper removes only names
  # with the exact outer-run and per-creation nonce in its closed owner ledger.
  mg_on_exit "$(printf '%q ' mg_sync_retained__support runtime-cleanup "$MG_RUN_DIR")"
  mg_sync_retained__dispatch "$receipt" || rc=1
  mg_sync_retained__support runtime-cleanup "$MG_RUN_DIR" || rc=1
  after="$(mg_sync_server__reference_identity)" || rc=1
  [ "$identity" = "$after" ] || rc=1
  mg_sync_server__clean || rc=1
  [ "$(git -C "$MG_ROOT" rev-parse HEAD)" = "$commit" ] || rc=1
  mg_artifact retained-evidence "$MG_RUN_DIR/output" "Fixed eight automated observations; independent artifact review required; no seal"
  mg_artifact retained-preparations "$MG_RUN_DIR/preparations" "All eight actual independent offline attempts"
  mg_artifact retained-instances "$MG_RUN_DIR/instances" "Per-case package, lifetime, worker/companion/wire checkpoints and failures"
  mg_sync_retained__support runtime-counts "$MG_RUN_DIR" >"$MG_RUN_DIR/retained-counts.json" || return 1
  local count_key
  for count_key in total passed failed skipped; do
    mg_count "$count_key" "$(jq -r --arg key "$count_key" '.[$key]' "$MG_RUN_DIR/retained-counts.json")"
  done
  [ "$rc" -eq 0 ] || { mg_failure retained-runtime "Finite retained execution has failures or incomplete claims; artifacts preserved"; return 1; }
  mg_message "Automated fixed8 artifacts only; no compatibility credit until independent actual-artifact review"
  mg_status pass
}
mg_sync_retained__instance() {
  local receipt="$1" case_name="$2" outer="$MG_RUN_DIR" secret_dir rc=0 runtime_rc=0 cleanup_rc=0 factory_ready=false
  local MG_RUN_DIR="$outer/instances/$case_name" MG_SYNC_INSTANCE="$outer/instances/$case_name/instance"
  local MG_SYNC_INSTANCE_SUFFIX="retained-$case_name" MG_SYNC_SETUP_FILE=/dev/null
  local MEGURI_SYNC_ORIGIN=https://candidate.example.test MEGURI_SYNC_TRUSTED_PEER=127.0.0.1 MEGURI_SYNC_NETWORK=none
  mkdir "$MG_RUN_DIR" || return 1
  secret_dir="$(mktemp -d "${TMPDIR:-/tmp}/meguri-retained.XXXXXX")" || return 1
  mg_on_exit "$(printf '%q ' mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json")"
  python3 - "$secret_dir" "$MG_RUN_DIR/secret-owner.json" <<'PY'
import json, os, pathlib, sys
secret, receipt = map(pathlib.Path, sys.argv[1:])
s = secret.lstat()
with receipt.open('x') as f: json.dump([s.st_dev, s.st_ino, s.st_uid], f)
os.chmod(secret, 0o700)
fd = os.open(secret / 'credentials.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, 'w') as f:
    json.dump(dict(username='synthetic.' + os.urandom(12).hex(), password=os.urandom(32).hex()), f)
PY
  if [ "$?" -ne 0 ]; then
    mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json"
    return 1
  fi
  mg_on_exit "$(printf '%q ' mg_sync_package__volume_remove "$MG_SYNC_INSTANCE")"
  mg_on_exit "local MG_SYNC_INSTANCE=$(printf '%q' "$MG_SYNC_INSTANCE"); mg_sync_package__stop"
  if mg_sync_package__init "$receipt" && mg_sync_package__start; then
    factory_ready=true
    mg_sync_retained__support runtime-case "$outer" "$case_name" "$secret_dir/credentials.json" || runtime_rc=$?
    [ "$runtime_rc" -eq 0 ] || rc=1
  else rc=1; fi
  mg_sync_package__stop || { rc=1; cleanup_rc=1; }
  if mg_sync_package__volume_remove "$MG_SYNC_INSTANCE"; then
    if [ -f "$MG_SYNC_INSTANCE/instance.json" ]; then
      mv "$MG_SYNC_INSTANCE/instance.json" "$MG_SYNC_INSTANCE/retired-instance.json" || { rc=1; cleanup_rc=1; }
    fi
  else rc=1; cleanup_rc=1; fi
  mg_sync_server__remove_secret "$secret_dir" "$MG_RUN_DIR/secret-owner.json" || { rc=1; cleanup_rc=1; }
  jq -nc --arg case_name "$case_name" --argjson rc "$rc" --argjson runtime_rc "$runtime_rc" \
    --argjson ready "$factory_ready" --argjson cleanup "$cleanup_rc" \
    '{case:$case_name,exit_code:$rc,runtime_exit_code:$runtime_rc,factory_ready:$ready,cleanup_complete:($cleanup==0),case_credit:false}' >"$MG_RUN_DIR/factory-result.json" || return 1
  [ "$rc" -eq 0 ]
}
