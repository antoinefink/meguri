#!/usr/bin/env bats
# Exact input-byte binding for the isolated reference observation lane.
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "recovery and final neutral inputs affect the reference digest" {
  local sandbox="$BATS_TEST_TMPDIR/recovery-inputs" path before after_fixture after_selection after_dirty_fixture after_dirty_selection after_final
  # Never copy quarantine bodies; this digest input is wholly synthetic.
  mkdir -p "$sandbox/oracle/meguri-sync-server"
  printf 'synthetic reference digest placeholder\n' >"$sandbox/oracle/meguri-sync-server/synthetic.txt"
  # Copy only public digest inputs, without requiring Git metadata.
  for path in docker/meguri-sync-reference.Dockerfile \
    docker/meguri-sync-reference.Dockerfile.dockerignore docker/meguri-sync-reference.compose.yml \
    tools/mg.d/meguri-sync-reference.sh conformance/schema/meguri-sync-server.schema.json \
    conformance/catalog/meguri-sync-server.json conformance/fixtures/meguri-sync-server/original.json \
    conformance/recordings/scenarios/meguri-sync-server/desktop-selection.json \
    conformance/fixtures/meguri-sync-server/edges.json conformance/recordings/scenarios/meguri-sync-server/desktop-edge-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-normal-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-media-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/desktop-media-selection.json \
    conformance/fixtures/meguri-sync-server/media.json \
    conformance/fixtures/meguri-sync-server/recovery.json \
    conformance/fixtures/meguri-sync-server/recovery-dirty.json \
    conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-dirty-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/desktop-recovery-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-full-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-complete-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json \
    conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json; do
    mkdir -p "$sandbox/$(dirname "$path")"
    cp "$REPO_ROOT/$path" "$sandbox/$path"
  done
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
  # Mutate each final metadata input independently from the same baseline.
  for path in conformance/recordings/scenarios/meguri-sync-server/candidate-final-additions.json \
    conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json; do
    printf '{"synthetic_digest_probe":true}\n' >"$sandbox/$path"
    after_final="$(mg_sync_reference_digest)"
    [ "$after_dirty_selection" != "$after_final" ]
    cp "$REPO_ROOT/$path" "$sandbox/$path"
    [ "$after_dirty_selection" = "$(mg_sync_reference_digest)" ]
  done
}

@test "reference author literals retain ordered source responsibility metadata" {
  # Metadata-only policy seam; the producer still validates committed roles/uniqueness.
  local expected='["s-20261115T133013Z-d8cec007","s-20261115T211543Z-556b04e9","s-20261115T224434Z-376ca308","s-20261115T231059Z-ff95725a","s-20261116T001757Z-8fe546bc","s-20261116T020051Z-5e05fa3f","s-20261116T092626Z-4487d759","s-20261116T124044Z-c9976196","s-20261116T225916Z-981c8d1b"]'
  local author_literal authors_literal
  author_literal="$(sed -n 's/^  author_session=//p' "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh")"
  authors_literal="$(sed -n 's/^  source_author_sessions=//p' "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh")"
  [ "$author_literal" = '"s-20261116T124044Z-c9976196"' ]
  [ "$authors_literal" = "'$expected'" ]
}
