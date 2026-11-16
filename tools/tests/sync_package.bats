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
        '[{Name:$name,Labels:{"meguri.sync.owner":$owner}}]' ;;
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
