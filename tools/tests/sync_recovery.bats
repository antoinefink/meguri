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
    conformance/recordings/scenarios/meguri-sync-server/candidate-retained-final-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json \
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
    conformance/recordings/scenarios/meguri-sync-server/candidate-retained-final-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json \
    conformance/recordings/scenarios/meguri-sync-server/final-fixture-recipes.json; do
    printf '{"synthetic_digest_probe":true}\n' >"$sandbox/$path"
    after_final="$(mg_sync_reference_digest)"
    [ "$after_dirty_selection" != "$after_final" ]
    cp "$REPO_ROOT/$path" "$sandbox/$path"
    [ "$after_dirty_selection" = "$(mg_sync_reference_digest)" ]
  done
  # The same real pipeline must refuse a missing required Mobile input.
  path=conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json
  rm "$sandbox/$path"
  run mg_sync_reference_digest
  [ "$status" -ne 0 ]
  cp "$REPO_ROOT/$path" "$sandbox/$path"
  [ "$after_dirty_selection" = "$(mg_sync_reference_digest)" ]
}

@test "reference Docker context admits the exact neutral Mobile selection" {
  python3 - "$REPO_ROOT" <<'PY'
from pathlib import Path, PurePosixPath
import sys

root = Path(sys.argv[1])
selection = "conformance/recordings/scenarios/meguri-sync-server/candidate-mobile-final-selection.json"
destination = "/opt/reference/neutral/candidate-mobile-final-selection.json"
copies = [line.split()[1:] for line in
          (root / "docker/meguri-sync-reference.Dockerfile").read_text().splitlines()
          if line.startswith("COPY ")]
assert [copy for copy in copies if "mobile" in " ".join(copy)] == [[selection, destination]]
# All context exceptions must serve an explicit COPY or its parent directories.
# The existing quarantined source exceptions stay exact; no bodies are read.
expected = {"**", "!oracle/", "!oracle/meguri-sync-server/", "!oracle/meguri-sync-server/**"}
for source, target in copies:
    if source.startswith("oracle/"):
        assert [source, target] in [
            ["oracle/meguri-sync-server/requirements.lock", "/opt/reference/requirements.lock"],
            ["oracle/meguri-sync-server/", "/opt/reference/"],
        ]
        continue
    assert source.startswith("conformance/") and source.endswith(".json")
    assert not any(char in source for char in "*?[")
    expected.add("!" + source)
    expected.update("!" + str(parent) + "/" for parent in PurePosixPath(source).parents
                    if str(parent) != ".")
rules = (root / "docker/meguri-sync-reference.Dockerfile.dockerignore").read_text().splitlines()
assert rules[0] == "**"
assert len(rules) == len(set(rules)) and set(rules) == expected
assert "!" + selection in rules
PY
}

@test "reference invocation retains complete committed author identities and rejects role defects" {
  local observed_status observed_failure records defect positive
  # Independent fixed order, checked against emitted invocation bytes below.
  local expected='["s-20261115T133013Z-d8cec007","s-20261115T211543Z-556b04e9","s-20261115T224434Z-376ca308","s-20261115T231059Z-ff95725a","s-20261116T001757Z-8fe546bc","s-20261116T020051Z-5e05fa3f","s-20261116T092626Z-4487d759","s-20261116T124044Z-c9976196","s-20261116T225916Z-981c8d1b","s-20261117T230521Z-4667d769"]'
  MG_ROOT="$REPO_ROOT"
  MG_RUN_ID=source-author-metadata-only
  MG_RUN_DIR="$BATS_TEST_TMPDIR/authors-positive"
  mkdir -p "$MG_RUN_DIR"
  mg_job() { :; }
  source "$REPO_ROOT/tools/mg.d/meguri-sync-reference.sh"
  # Exercise the actual producer/role validator with only public committed
  # records. Stub transport and hashes: no SDK files, image or execution proof.
  mg_compose() { :; }
  mg_sync_reference_digest() { printf '%064d\n' 0; }
  shasum() { printf '%064d  metadata-only\n' 0; }
  mg_artifact() { :; }
  mg_detail() { :; }
  mg_status() { observed_status="$1"; }
  mg_failure() { observed_failure="$1"; }
  observed_status= observed_failure=
  mg_sync_server_reference selftest
  [ "$observed_status" = pass ]
  [ -z "$observed_failure" ]
  positive="$MG_RUN_DIR/source-authors.json"
  jq -e --slurpfile invocation "$MG_RUN_DIR/invocation.json" '
    $invocation[0].source_author_sessions as $authors |
    all(.[]; .role == "oracle-maintainer") and
    ((map(.session) | sort) == ($authors | sort)) and
    ((map(.session) | unique | length) == ($authors | length))' "$positive"
  jq -e --argjson expected "$expected" '
    .source_author_sessions == $expected and
    .author_session == "s-20261116T124044Z-c9976196" and
    .arguments == ["selftest"]' "$MG_RUN_DIR/invocation.json"
  # Substitute only the public role-record transport, keeping the validator real.
  git() {
    if [ "$#" -eq 4 ] && [ "$1" = -C ] && [ "$2" = "$MG_ROOT" ] && [ "$3" = show ]; then
      cat "$records"
    else
      command git "$@"
    fi
  }
  for defect in missing wrong-role ambiguous; do
    MG_RUN_DIR="$BATS_TEST_TMPDIR/authors-$defect"
    mkdir -p "$MG_RUN_DIR"
    records="$MG_RUN_DIR/records.jsonl"
    jq -c --arg defect "$defect" --arg author s-20261117T230521Z-4667d769 '
      (if $defect == "missing" then map(select(.session != $author))
       elif $defect == "wrong-role" then
         map(if .session == $author then .role = "reviewer" else . end)
       else . + [(.[] | select(.session == $author) | .duplicate_probe = true)]
       end)[]' "$positive" >"$records"
    observed_status= observed_failure=
    mg_sync_server_reference selftest
    [ "$observed_status" = error ]
    [ "$observed_failure" = session-missing ]
    [ ! -e "$MG_RUN_DIR/invocation.json" ]
  done
}
