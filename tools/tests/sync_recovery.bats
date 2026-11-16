#!/usr/bin/env bats
# Exact input-byte binding for the isolated reference observation lane.
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "recovery neutral inputs affect the reference digest" {
  local sandbox="$BATS_TEST_TMPDIR/recovery-inputs" path before after_fixture after_selection after_dirty_fixture after_dirty_selection
  mkdir -p "$sandbox"
  while IFS= read -r path; do
    mkdir -p "$sandbox/$(dirname "$path")"
    cp "$REPO_ROOT/$path" "$sandbox/$path"
  done < <(git -C "$REPO_ROOT" ls-files -- oracle/meguri-sync-server docker/meguri-sync-reference.Dockerfile docker/meguri-sync-reference.Dockerfile.dockerignore docker/meguri-sync-reference.compose.yml tools/mg.d/meguri-sync-reference.sh conformance/schema/meguri-sync-server.schema.json conformance/catalog/meguri-sync-server.json conformance/fixtures/meguri-sync-server conformance/recordings/scenarios/meguri-sync-server)
  # The host helper uses macOS tools; the dev image supplies equivalent primitives.
  rg() { [ "$1" = --files ]; shift; find "$@" -type f; }
  shasum() { [ "$1" = -a ] && [ "$2" = 256 ]; shift 2; sha256sum "$@"; }
  mg_job() { :; }
  source "$sandbox/tools/mg.d/meguri-sync-reference.sh"
  MG_ROOT="$sandbox"
  before="$(mg_sync_reference_digest)"
  printf '\n' >>"$sandbox/conformance/fixtures/meguri-sync-server/recovery.json"
  after_fixture="$(mg_sync_reference_digest)"
  [ "$before" != "$after_fixture" ]
  printf '\n' >>"$sandbox/conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-selection.json"
  after_selection="$(mg_sync_reference_digest)"
  [ "$after_fixture" != "$after_selection" ]
  printf '\n' >>"$sandbox/conformance/fixtures/meguri-sync-server/recovery-dirty.json"
  after_dirty_fixture="$(mg_sync_reference_digest)"
  [ "$after_selection" != "$after_dirty_fixture" ]
  printf '\n' >>"$sandbox/conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-dirty-selection.json"
  after_dirty_selection="$(mg_sync_reference_digest)"
  [ "$after_dirty_fixture" != "$after_dirty_selection" ]
}
