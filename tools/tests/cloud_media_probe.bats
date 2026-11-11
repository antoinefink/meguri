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
