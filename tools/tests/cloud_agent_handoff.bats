#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "CloudAgent managed originals require quiet groups and one verified reap" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_process_test.py"
}

@test "CloudAgent finalized four-role handoff generated boundaries supply no signed proof" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_handoff_test.py"
}

@test "CloudAgent handoff remains internal and requires a clean signed parent" {
  run bash -c '
    mg_job() { :; }
    MG_ROOT="$1" MG_RUN_DIR="$2"
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_agent_handoff 20261102T010100Z-102
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 1 ]
  [ -z "$output" ]
}

@test "CloudAgent missing preflight implementation_missing survives internal MG reporting without success" {
  run bash -c '
    mg_job() { :; }
    MG_ROOT="$1" MG_RUN_DIR="$2" MG_JOB=cloudkit-signed MG_COMMIT_DIRTY=false
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_agent_handoff_private() {
      jq -nc '\''{scope:"PRODUCT-EVIDENCE-ONLY",outcome:"implementation_missing",category:"mac-path-preflight-entrypoint",signed_execution:false}'\'' >"$3"
      return 78
    }
    mg_detail() { [ "$1" = product_evidence ] && [ "$2" = '\''"implementation_missing"'\'' ]; }
    mg_artifact() { :; }
    mg_cloudkit_agent_handoff 20261102T010100Z-102
    rc=$?
    [ "$rc" -eq 78 ] || exit 1
    jq -e '\''.outcome == "implementation_missing" and .category == "mac-path-preflight-entrypoint" and .signed_execution == false'\'' "$2/product-evidence-report.json"
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ "$output" = true ]
}
