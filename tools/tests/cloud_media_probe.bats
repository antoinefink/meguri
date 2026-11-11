#!/usr/bin/env bats

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

@test "normal media probe generated filesystem safety and private native evidence" {
  python3 -B "$REPO_ROOT/tools/tests/cloud_media_probe_test.py"
}

@test "normal media probe dry-run is inert for both actions and missing arguments" {
  local sb="$BATS_TEST_TMPDIR/repo" result
  mkdir -p "$sb"
  cp -R "$REPO_ROOT/tools" "$sb/tools"
  # If dispatch invokes the helper, the dry-run fails. No Apple tool or host
  # data is needed, even with plausible run IDs and either operation.
  printf '%s\n' 'raise RuntimeError("dry run invoked helper")' >"$sb/tools/scripts/cloud-media-probe.py"
  for action in 'stage --preview-build 20261111T010101Z-1' 'verify-phone --stage 20261111T010102Z-2 --preparation 20261111T010103Z-3' ''; do
    run "$sb/tools/mg" cloud-media-probe --dry-run $action
    [ "$status" -eq 0 ]
    result="$output"
    jq -e '.dry_run == true and .status == "pass" and .placement == "host"' "$result"
    [ ! -e "$(dirname "$result")/probe-report.json" ]
  done
  [ ! -e "$sb/build/mg/cloud-media-probe/latest" ]
}

# Exercise the real MG dispatcher and job wrapper. Only the generated copy of
# the helper changes; it can write markers/reports here and has no native code.
media_probe_cli_sandbox() {
  sb="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$sb"
  cp -R "$REPO_ROOT/tools" "$sb/tools"
  cat >"$sb/tools/scripts/cloud-media-probe.py" <<'PY'
import argparse
import json
from pathlib import Path
import sys

sandbox = Path(__file__).resolve().parents[2]
# Record every execution, including ones whose argument parsing later fails.
with (sandbox / "helper-calls.jsonl").open("a") as marker:
    marker.write(json.dumps(sys.argv[1:]) + "\n")
parser = argparse.ArgumentParser()
parser.add_argument("--root", required=True)
parser.add_argument("--run-id", required=True)
sub = parser.add_subparsers(dest="action", required=True)
sub.add_parser("stage").add_argument("--preview-build", required=True)
verify = sub.add_parser("verify-phone")
verify.add_argument("--stage", required=True)
verify.add_argument("--preparation", required=True)
args = parser.parse_args()
# Retain effective values without opening any caller-selected root or path.
(sandbox / "helper-parsed.json").write_text(json.dumps(vars(args)))
print(json.dumps({"scope": "NORMAL-APP-MEDIA-PROBE", "signed_lane_acceptance": False,
                  "outcome": "staged-only" if args.action == "stage" else "peer-byte-match"}))
PY
}

assert_media_probe_cli_refused() {
  local result
  run "$sb/tools/mg" cloud-media-probe "$@"
  # Check this first: helper execution itself is the regression, even when
  # argparse or MG subsequently reports a failure.
  if [ -e "$sb/helper-calls.jsonl" ]; then
    printf '%s\n' 'public CLI admission invoked the helper' >&2
    cat "$sb/helper-calls.jsonl" >&2
    [ ! -e "$sb/helper-parsed.json" ] || cat "$sb/helper-parsed.json" >&2
    return 1
  fi
  [ "$status" -eq 1 ]
  result="$output"
  jq -e '.status == "fail" and .dry_run == false and .placement == "host" and
         (.failures | length) == 1 and .failures[0].id == "usage" and
         (.failures[0].message | startswith("usage: tools/mg cloud-media-probe ")) and
         ([.message, .failures] | tostring | contains("untrusted-cli-sentinel") | not) and
         (.details | has("probe") | not)' "$result"
  [ ! -e "$(dirname "$result")/probe-report.json" ]
}

@test "normal media probe refuses internal globals before the action without executing helper" {
  local sb
  media_probe_cli_sandbox
  assert_media_probe_cli_refused --root "$sb/untrusted-cli-sentinel" stage --preview-build 20261111T010101Z-1
  assert_media_probe_cli_refused --run-id 20261111T010199Z-9 stage --preview-build 20261111T010101Z-1
  assert_media_probe_cli_refused --root "$sb/untrusted-cli-sentinel" --run-id 20261111T010199Z-9 stage --preview-build 20261111T010101Z-1
  assert_media_probe_cli_refused --root="$sb/untrusted-cli-sentinel" verify-phone --stage 20261111T010102Z-2 --preparation 20261111T010103Z-3
  assert_media_probe_cli_refused --run-id=20261111T010199Z-9 verify-phone --stage 20261111T010102Z-2 --preparation 20261111T010103Z-3
}

@test "normal media probe refuses extra flags after valid actions without executing helper" {
  local sb flag
  media_probe_cli_sandbox
  for flag in --root --run-id --filename --path --unknown --preview-build; do
    assert_media_probe_cli_refused stage --preview-build 20261111T010101Z-1 "$flag" untrusted-cli-sentinel
    assert_media_probe_cli_refused verify-phone --stage 20261111T010102Z-2 --preparation 20261111T010103Z-3 "$flag" untrusted-cli-sentinel
  done
  assert_media_probe_cli_refused verify-phone --stage 20261111T010102Z-2 --preparation 20261111T010103Z-3 --stage 20261111T010102Z-2
  assert_media_probe_cli_refused stage --preview-build 20261111T010101Z-1 --help
}

@test "normal media probe refuses empty unknown incomplete and path arguments without executing helper" {
  local sb value
  media_probe_cli_sandbox
  assert_media_probe_cli_refused
  assert_media_probe_cli_refused ''
  assert_media_probe_cli_refused untrusted-cli-sentinel
  assert_media_probe_cli_refused stage
  assert_media_probe_cli_refused stage --preview-build
  assert_media_probe_cli_refused verify-phone --stage 20261111T010102Z-2
  assert_media_probe_cli_refused verify-phone --stage 20261111T010102Z-2 --stage 20261111T010103Z-3
  assert_media_probe_cli_refused stage --preview 20261111T010101Z-1
  for value in '' untrusted-cli-sentinel /untrusted-cli-sentinel ../untrusted-cli-sentinel 20261111T010101Z-1.bin $'20261111T010101Z-1\n'; do
    assert_media_probe_cli_refused stage --preview-build "$value"
    assert_media_probe_cli_refused verify-phone --stage "$value" --preparation 20261111T010103Z-3
    assert_media_probe_cli_refused verify-phone --stage 20261111T010102Z-2 --preparation "$value"
  done
}

@test "normal media probe admits valid public forms and preserves internal globals" {
  local sb action result
  media_probe_cli_sandbox
  for action in 'stage --preview-build 20261111T010101Z-1' \
                'verify-phone --stage 20261111T010102Z-2 --preparation 20261111T010103Z-3' \
                'verify-phone --preparation 20261111T010103Z-3 --stage 20261111T010102Z-2'; do
    rm -f "$sb/helper-calls.jsonl" "$sb/helper-parsed.json"
    run "$sb/tools/mg" cloud-media-probe $action
    [ "$status" -eq 0 ]
    result="$output"
    jq -e '.status == "pass" and .dry_run == false and .placement == "host" and
           .details.probe.scope == "NORMAL-APP-MEDIA-PROBE" and
           .details.probe.signed_lane_acceptance == false and
           .details.probe.outcome == (if .args[0] == "stage" then "staged-only" else "peer-byte-match" end)' "$result"
    jq -se --slurpfile receipt "$result" 'length == 1 and
      .[0] == (["--root", $receipt[0].root, "--run-id", $receipt[0].run_id] + $receipt[0].args)' "$sb/helper-calls.jsonl"
    jq -e --arg root "$sb" --slurpfile receipt "$result" '
      .root == $root and .run_id == $receipt[0].run_id and
      (if .action == "stage" then .preview_build == "20261111T010101Z-1"
       else .stage == "20261111T010102Z-2" and .preparation == "20261111T010103Z-3" end)' "$sb/helper-parsed.json"
  done
}
