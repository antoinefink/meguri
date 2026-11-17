#!/usr/bin/env bats
# SPDX-License-Identifier: MIT
# Focused synthetic proofs; no image build, service or network.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
setup() {
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/sync-package.sh"
  MG_SYNC_INSTANCE="$BATS_TEST_TMPDIR/instance"
  MG_RUN_DIR="$BATS_TEST_TMPDIR/run"
  MG_RUN_ID=synthetic-package
  mkdir "$MG_RUN_DIR"
  MEGURI_SYNC_ORIGIN=https://localhost:18443
  MEGURI_SYNC_TRUSTED_PEER=127.0.0.1
  receipt="$BATS_TEST_TMPDIR/receipt.json"
  printf '%s\n' '{"details":{"package":{"image_id":"synthetic-image"}}}' >"$receipt"
  mg_sync_package__receipt() { :; }
  mg_on_exit() { :; }
  mg_sync_server__remove_container() { :; }
  mg_sync_package__compose() { printf 'initialized\n' >"$MG_RUN_DIR/helper-called"; return "${INIT_RC:-0}"; }
  docker() {
    case "$1 $2" in
      'volume ls') [ "${LIST_RC:-0}" -eq 0 ] || return 1
        [ ! -f "$MG_RUN_DIR/volume" ] || cat "$MG_RUN_DIR/volume" ;;
      'volume create') [ "${CREATE_RC:-0}" -eq 0 ] || return 1
        printf '%s\n' "${@: -1}" >"$MG_RUN_DIR/volume" ;;
      'volume inspect') jq -n --arg name "$3" --arg owner "$(jq -r .owner "$MG_SYNC_INSTANCE/instance.json")" \
        --arg run "$MG_RUN_ID" '[{Name:$name,Labels:{"meguri.sync.owner":$owner,"meguri.mg.run-id":$run}}]' ;;
      *) return 97 ;;
    esac
  }
}
@test "sync-package: absent failed volume creation permits a fresh init" {
  CREATE_RC=1
  run mg_sync_package__init "$receipt"
  [ "$status" -ne 0 ]
  [ ! -e "$MG_SYNC_INSTANCE/instance.json" ]
  CREATE_RC=0
  run mg_sync_package__init "$receipt"
  [ "$status" -eq 0 ]
  [ -s "$MG_SYNC_INSTANCE/instance.json" ]
  [ -s "$MG_RUN_DIR/helper-called" ]
}
@test "sync-package: failed initialization preserves owned partial volume and record" {
  INIT_RC=1
  run mg_sync_package__init "$receipt"
  [ "$status" -ne 0 ]
  [ -s "$MG_RUN_DIR/volume" ]
  [ -s "$MG_RUN_DIR/helper-called" ]
  [ -s "$MG_SYNC_INSTANCE/instance.json" ]
  run mg_sync_package__volume_check
  [ "$status" -eq 0 ]
}
@test "sync-package: uncertain volume observation preserves candidate record" {
  LIST_RC=1
  run mg_sync_package__init "$receipt"
  [ "$status" -ne 0 ]
  [ -s "$MG_SYNC_INSTANCE/instance.json" ]
}
@test "sync-package: real context assembly rejects synthetic credential inputs" {
  run python3 "$REPO_ROOT/tools/tests/sync_package_context_test.py" "$REPO_ROOT/server/tools/package-context.py"
  [ "$status" -eq 0 ]
}

@test "sync-package: phase-suffixed volumes retain the real outer ownership run" {
  MG_SYNC_INSTANCE_SUFFIX=final-rich
  run mg_sync_package__init "$receipt"
  [ "$status" -eq 0 ]
  jq -e --arg run "$MG_RUN_ID" '.run_id==$run and .volume==("meguri-sync-data-"+$run+"-final-rich")' "$MG_SYNC_INSTANCE/instance.json"
  run mg_sync_package__volume_check
  [ "$status" -eq 0 ]
}

@test "sync-package: a failed segment cannot shorten the nine-instance loop or gain credit" {
  MG_ROOT="$REPO_ROOT" MG_JOB=sync-server-interop
  mg_failure() { printf '%s\n' "$1" >>"$MG_RUN_DIR/failures"; }
  mg_artifact() { :; }
  mg_detail() { printf '%s=%s\n' "$1" "$2" >>"$MG_RUN_DIR/details"; }
  mg_sync_server__clean() { :; }
  mg_sync_package__candidate_input() {
    printf '{"server":{"source_commit":"a","source_sha256":"b","lock_sha256":"c","executable_sha256":"d"}}\n'
  }
  mg_sync_server__reference_identity() { printf '{}\n'; }
  mg_compose() { :; }
  mg_sync_server__image() { printf 'sha256:%064d\n' 0; }
  mg_sync_package__final_instance() {
    printf '%s %s\n' "$MG_RUN_ID" "$2" >>"$MG_RUN_DIR/sequence"
    [ "$2" != final-normal ]
  }
  mg_sync_package__final_assemble() { printf 'assemble\n' >>"$MG_RUN_DIR/sequence"; }
  run mg_sync_package__final_owned "$receipt" '["--lane","desktop","--phase","final"]' synthetic-author
  [ "$status" -ne 0 ]
  [ "$(wc -l <"$MG_RUN_DIR/sequence" | tr -d ' ')" -eq 10 ]
  [ "$(cut -d ' ' -f 2 "$MG_RUN_DIR/sequence" | head -n 9 | tr '\n' ' ')" = 'final-full final-normal final-media final-migration final-replacement-upload final-replacement-download final-metadata-only final-fresh-fresh final-rich ' ]
  [ "$(tail -n 1 "$MG_RUN_DIR/sequence")" = assemble ]
  [ "$(head -n 9 "$MG_RUN_DIR/sequence" | cut -d ' ' -f 1 | sort -u)" = "$MG_RUN_ID" ]
  ! rg -F 'candidate_compatibility_credit=true' "$MG_RUN_DIR/details"
}
