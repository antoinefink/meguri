#!/usr/bin/env bats
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "CloudAgent selected generated coordinator barriers and refusals only" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_agent_selected_test.py"
}

@test "CloudAgent selected scope rejects external inputs and smoke substitution" {
  for arguments in '--selected --header /generated.json' '--selected --smoke' '--selected --results-directory /generated'; do
    run bash -c '
      mg_job() { :; }; mg_failure() { printf "%s\n" "$1"; }; mg_status() { printf "%s\n" "$1"; }
      MG_ROOT="$1" MG_RUN_DIR="$2"; shift 2
      source "$MG_ROOT/tools/mg.d/cloud-agent.sh"
      mg_cloudkit_signed "$@"
    ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR" $arguments
    [ "$status" -eq 0 ]
    [ "$output" = $'usage\nerror' ]
  done
}

@test "CloudAgent selected needs its clean parent without new registered jobs" {
  run bash -c '
    mg_job() { printf "%s\n" "$1"; }; mg_failure() { printf "%s\n" "$1"; }; mg_status() { printf "%s\n" "$1"; }
    MG_ROOT="$1" MG_RUN_DIR="$2" MG_JOB=cloudkit-signed MG_COMMIT_DIRTY=true
    source "$1/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_signed --selected
  ' _ "$REPO_ROOT" "$BATS_TEST_TMPDIR"
  [ "$status" -eq 0 ]
  [ "$output" = $'cloudkit-signed\ncloudkit-agent-install\nselected-source\nfail' ]
}

@test "CloudAgent selected failure retains fixed preflight and cleanup codes without private strings" {
  local parent=20261110T000000Z-100
  local fixture="$BATS_TEST_TMPDIR/public-root"
  mkdir -p "$fixture/build/mg/cloudkit-signed/$parent"
  run bash -c '
    MG_ROOT="$1" MG_RUN_ID="$2" MG_RUN_DIR="$1/build/mg/cloudkit-signed/$2" MG_TOOLS="$3/tools"
    MG_JOB=cloudkit-signed MG_COMMIT_DIRTY=false
    mg_job() { :; }; mg_detail() { :; }; mg_failure() { :; }; mg_status() { :; }; mg_message() { :; }; mg_artifact() { :; }
    source "$3/tools/mg.d/cloud-agent.sh"
    mg_cloudkit_signed_selected_private() {
      jq -nc '\''{scope:"SELECTED-GENERATED",selection:"sync_now",outcome:"failed_evidence",category:"selected-refused",signed_execution:false,stage:"simulator-os",refusal:"vm-simctl"}'\'' >"$2"
      jq -nc '\''{scope:"SELECTED-GENERATED",selection:"sync_now",outcome:"failed_evidence",category:"selected-refused",signed_execution:false,stage:"simulator-lease",refusal:"lease-changed"}'\'' >"$1/cleanup.json"
      return 1
    }
    mg_cloudkit_signed --selected
    jq -ce '\''{stage,refusal,cleanup,signed_execution}'\'' "$MG_RUN_DIR/selected-report.json"
  ' _ "$fixture" "$parent" "$REPO_ROOT"
  [ "$status" -eq 0 ]
  [ "$output" = '{"stage":"simulator-os","refusal":"vm-simctl","cleanup":{"drain":"joined","refusal":"lease-changed","reservation":"retained-or-unconfirmed","stage":"simulator-lease"},"signed_execution":false}' ]
}
