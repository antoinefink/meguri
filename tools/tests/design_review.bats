#!/usr/bin/env bats
# Self-tests for the design verification jobs (U10): snapshots (recording,
# the approved-baseline guard, state coverage), critic (session flags,
# schema rejection, the status ledger, the isolation probe) and gallery
# (arguments, pending commits). Each test runs tools/mg from a throwaway copy
# of the repository's tools, design docs and role docs. Apple tools are
# faked: the test run's results come from a fixture, the image-tools
# container's scripts run here (image comparisons are faked, the image
# scripts have their own tests in image-tools-selftest), and the critic
# session is a stub script.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
KEY=Mac99,9-99A999

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/docs/process" "$t/docs/design"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp -R "$REPO_ROOT/.claude" "$t/.claude"
  rm -f "$t/.claude/settings.local.json"
  cp -R "$REPO_ROOT/docs/process/roles" "$REPO_ROOT/docs/process/provenance" "$t/docs/process/"
  cp -R "$REPO_ROOT/docs/design/." "$t/docs/design/"
  cp "$REPO_ROOT/AGENTS.md" "$REPO_ROOT/.gitignore" "$REPO_ROOT/.gitattributes" "$t/"
  : >"$t/docs/process/provenance/sessions.jsonl"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  git -C "$SB" config user.name selftest
  git -C "$SB" config user.email selftest@localhost
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  export HOME="$BATS_TEST_TMPDIR/home"
  export MEGURI_HOST_LEASE_DIR="$BATS_TEST_TMPDIR/host.lock" MEGURI_HOST_LEASE_POLL_SECONDS=1
  export MEGURI_MAC_UI_DIR="$BATS_TEST_TMPDIR/mac-ui"
  # No test VM: Mac and simulator runs here take the host path (mac_vm.bats
  # and sim_vm.bats cover the VM).
  export MEGURI_MAC_VM_CONFIG="$BATS_TEST_TMPDIR/no-mac-vm.json" MEGURI_MAC_UI_ALLOW_HOST=1 MEGURI_SIM_ALLOW_HOST=1
  export TMPDIR="$BATS_TEST_TMPDIR/tmp"
  mkdir -p "$HOME" "$TMPDIR"
  : >"$STUB_LOG"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  stub ioreg 'echo "+-o Root  <class IORegistryEntry>"; echo "  \"IOConsoleUsers\" = ({\"kCGSSessionOnConsoleKey\"=Yes})"'
  stub system_profiler 'echo "{\"SPDisplaysDataType\": [{\"spdisplays_ndrvs\": [{\"_name\": \"Display\"}]}]}"'
  stub caffeinate 'exec sleep 600'
  stub xcodebuild 'echo "xcodebuild $*" >>"$STUB_LOG"; exit 70'
  export PATH="$BIN:$PATH"
  add_fakes
}

teardown() {
  pkill -f "$BATS_TEST_TMPDIR/bin/caffeinate" 2>/dev/null || true
  # Shown only when the test failed.
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, message, failures, details}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 30
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$BIN/$1" && chmod +x "$BIN/$1"; }

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }
called() { grep -Eq -- "$1" "$STUB_LOG" || { echo "no call matching: $1"; cat "$STUB_LOG"; return 1; }; }
not_called() { ! grep -Eq -- "$1" "$STUB_LOG" || { echo "unexpected call: $(grep -E -- "$1" "$STUB_LOG")"; return 1; }; }

# Fakes that replace the real functions (the file sorts after the jobs):
# the lint gate and builds pass, the image-tools scripts run here (their
# paths mapped from /work), and image comparisons compare the first line of
# the files ("look: <colour>"), so a later line stands for a difference under
# the tolerance. The snapshot test run writes FX_RESULTS (JSON lines with
# @OUT@ for its output directory) and copies FX_IMAGES/* into it.
add_fakes() {
  cat >"$SB/tools/mg.d/zz-fakes.sh" <<'EOF'
fx_host() { case "$1" in /work/*) printf '%s\n' "$MG_ROOT/${1#/work/}" ;; *) printf '%s\n' "$1" ;; esac; }
mg_lint_ui_first() { echo "fake lint-ui" >>"$STUB_LOG"; return 0; }
mg_app_build() { echo "fake build $*" >>"$STUB_LOG"; return 0; }
fx_look() { head -n 1 "$1" 2>/dev/null; }
fx_image_diff() {
  local cmd="$1" report="" pairs="" p
  shift
  case "$cmd" in
    guard)
      pairs="$1"; shift
      while [ "$#" -gt 0 ]; do case "$1" in --report) report="$2"; shift 2 ;; *) shift ;; esac; done
      : >"$report.tmp"
      jq -c '.[]' "$pairs" | while IFS= read -r p; do
        local c a
        c="$(fx_host "$(jq -r .current <<<"$p")")"; a="$(fx_host "$(jq -r .approved <<<"$p")")"
        if [ -f "$c" ] && [ -f "$a" ] && [ "$(fx_look "$c")" = "$(fx_look "$a")" ]; then
          jq -c '. + {match: true}' <<<"$p"
        else
          jq -c '. + {match: false, reason: "looks differ"}' <<<"$p"
        fi >>"$report.tmp"
      done
      jq -s '{verdict: (if any(.[]; .match | not) then "needs-human-review" else "pass" end), checked: ., needs_human_review: [.[] | select(.match | not)]}' "$report.tmp" >"$report"
      ;;
    differs) echo '{"unchanged": []}' ;;
  esac
}
mg_image_tools() {
  echo "fake image-tools $*" >>"$STUB_LOG"
  local args=() a
  for a in "$@"; do args[${#args[@]}]="$(fx_host "$a")"; done
  case "$2" in
    */image_diff.py) fx_image_diff "${args[@]:2}" ;;
    *) (cd "$MG_ROOT" && "${args[@]}") ;;
  esac
}
mg_snapshots__xcodebuild() {
  local out="$1" dest="$2" suites="$3" mode="" dir="" target="" group="" fx
  shift 3
  for a in "$@"; do
    case "$a" in
      TEST_RUNNER_MEGURI_SNAPSHOT_MODE=*) mode="${a#*=}" ;;
      TEST_RUNNER_MEGURI_SNAPSHOT_OUT=*) dir="${a#*=}" ;;
      TEST_RUNNER_MEGURI_SNAPSHOT_TARGET=*) target="${a#*=}" ;;
      TEST_RUNNER_MEGURI_SNAPSHOT_GROUP=*) group="${a#*=}" ;;
    esac
  done
  echo "fake snapshot run mode=$mode dest=$dest suites=$suites target=$target group=$group" >>"$STUB_LOG"
  mkdir -p "$dir" "$out.xcresult"
  echo "xcodebuild log" >"$out.log"
  if [ "$mode" = plan ]; then
    [ -z "${FX_PLAN:-}" ] || cp "$FX_PLAN" "$dir/plan.jsonl"
    return 0
  fi
  [ -z "${FX_IMAGES:-}" ] || cp -R "$FX_IMAGES/." "$dir/"
  fx="$FX_RESULTS"
  [ -z "${FX_RESULTS_DIR:-}" ] || [ ! -f "$FX_RESULTS_DIR/$target-$group.jsonl" ] || fx="$FX_RESULTS_DIR/$target-$group.jsonl"
  sed "s|@OUT@|$dir|g; s|@MODE@|$mode|g" "$fx" >"$dir/results.jsonl"
  [ -n "${FX_NO_COVERAGE:-}" ] || echo '{"screens": [{"id": "home.deckList", "states": ["typical"], "platforms": ["iphone", "mac"]}], "problems": []}' >"$dir/state-coverage.json"
  return 0
}
mg_sim_lease() { echo "fake lease $1" >>"$STUB_LOG"; MG_SIM_UDID="FAKE-$1"; return 0; }
mg_sim_check_runtime() { return 0; }
mg_snapshots__settings() { echo "fake settings $2 on $1" >>"$STUB_LOG"; return 0; }
EOF
}

# A snapshot result line.
result_line() { # name status [actual] [diff]
  jq -nc --arg n "$1" --arg s "$2" --arg a "${3:-}" --arg d "${4:-}" --arg k "$KEY" '
    ($n | split("/")) as $p | {name: $n, subject: $p[0], state: $p[1], cell: $p[2], status: $s, platform: "mac", group: "mac",
      key: $k, layout: "screen", baseline: "app/Snapshots/\($k)/\($n).png"}
    + (if $a == "" then {} else {actual: $a} end) + (if $d == "" then {} else {diff: $d} end)'
}

baseline() { mkdir -p "$(dirname "$SB/app/Snapshots/$KEY/$1.png")" && printf '%s\n' "$2" >"$SB/app/Snapshots/$KEY/$1.png"; }

# --- snapshots -------------------------------------------------------------

@test "snapshots rejects bad arguments before building" {
  mg snapshots --platform watch
  [ "$(field .status)" = error ]
  mg snapshots --platform iphone,ipad --device 0000
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].message')" = "snapshots: --device 0000 is not one of the kinds --platform iphone,ipad renders on (iphone iphone-small ipad)" ]
  mg snapshots --record
  [ "$(field .status)" = error ]
  not_called 'fake build'
}

@test "a token colour change fails the snapshot with its diff image; --record takes only failing and missing images" {
  baseline home.deckList/typical/light-standard-macDefault "look: blue"
  baseline home.deckList/typical/dark-standard-macDefault "look: dark"
  git -C "$SB" add app && git -C "$SB" commit -qm baselines
  export FX_IMAGES="$BATS_TEST_TMPDIR/images" FX_RESULTS="$BATS_TEST_TMPDIR/results.jsonl"
  mkdir -p "$FX_IMAGES/actual/home.deckList/typical" "$FX_IMAGES/diff/home.deckList/typical" "$FX_IMAGES/actual/home.deckList/empty"
  printf 'look: red\n' >"$FX_IMAGES/actual/home.deckList/typical/light-standard-macDefault.png"
  printf 'diff\n' >"$FX_IMAGES/diff/home.deckList/typical/light-standard-macDefault.png"
  printf 'look: empty\n' >"$FX_IMAGES/actual/home.deckList/empty/light-standard-macDefault.png"
  {
    result_line home.deckList/typical/light-standard-macDefault fail @OUT@/actual/home.deckList/typical/light-standard-macDefault.png @OUT@/diff/home.deckList/typical/light-standard-macDefault.png
    result_line home.deckList/typical/dark-standard-macDefault pass
  } >"$FX_RESULTS"

  mg snapshots --platform mac
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = "snapshot/home.deckList/typical/light-standard-macDefault" ]
  [[ "$(field '.failures[0].message')" == *"diff: "*"/diff/home.deckList/typical/light-standard-macDefault.png" ]]
  [ "$(field '.failures[0].repro')" = "tools/mg snapshots --platform mac --record 'home.deckList/typical/light-standard-macDefault'" ]
  [ "$(field '[.artifacts[] | select(.kind == "diff")] | length')" -eq 1 ]
  called 'fake snapshot run mode=verify dest=platform=macOS'
  [ "$(head -n 1 "$SB/app/Snapshots/$KEY/home.deckList/typical/light-standard-macDefault.png")" = "look: blue" ]
  # The Mac run's images were moved out of the sandboxed app's container.
  [ ! -d "$HOME/Library/Containers/com.antoinefink.meguri.dev/Data/tmp/meguri-snapshots" ] ||
    [ -z "$(ls -A "$HOME/Library/Containers/com.antoinefink.meguri.dev/Data/tmp/meguri-snapshots")" ]

  # Recording: the failing and the missing image are taken; the passing
  # baseline stays byte for byte.
  local dark_before
  dark_before="$(shasum "$SB/app/Snapshots/$KEY/home.deckList/typical/dark-standard-macDefault.png")"
  {
    result_line home.deckList/typical/light-standard-macDefault recorded @OUT@/actual/home.deckList/typical/light-standard-macDefault.png
    result_line home.deckList/typical/dark-standard-macDefault pass
    result_line home.deckList/empty/light-standard-macDefault recorded @OUT@/actual/home.deckList/empty/light-standard-macDefault.png
  } >"$FX_RESULTS"
  mg snapshots --platform mac --record 'home.deckList/*'
  [ "$(field .status)" = pass ]
  [ "$(field .details.recorded)" -eq 2 ]
  called 'fake snapshot run mode=record'
  [ "$(head -n 1 "$SB/app/Snapshots/$KEY/home.deckList/typical/light-standard-macDefault.png")" = "look: red" ]
  [ "$(head -n 1 "$SB/app/Snapshots/$KEY/home.deckList/empty/light-standard-macDefault.png")" = "look: empty" ]
  [ "$(shasum "$SB/app/Snapshots/$KEY/home.deckList/typical/dark-standard-macDefault.png")" = "$dark_before" ]
}

# approve <path> <signature>: an approved-baselines.json entry for the
# committed file at <path>, as the gallery writes it.
approve() {
  local path="app/Snapshots/$KEY/$1.png"
  jq --arg p "$path" --arg sha "$(shasum -a 256 "$SB/$path" | cut -d' ' -f1)" --arg blob "$(git -C "$SB" hash-object "$SB/$path")" --argjson sig "$2" \
    '.baselines += [{path: $p, screen: ($p | split("/")[3]), signature: $sig, sha256: $sha, blob: $blob, approved_by: "antoine"}]' \
    "$SB/docs/design/approved-baselines.json" >"$SB/docs/design/approved-baselines.json.tmp"
  mv "$SB/docs/design/approved-baselines.json.tmp" "$SB/docs/design/approved-baselines.json"
}

@test "a changed approved signature baseline needs human review; a change under the tolerance does not" {
  baseline home.deckList/typical/light-standard-macDefault "look: blue"
  baseline settings.general/typical/light-standard-macDefault "look: grey"
  git -C "$SB" add app && git -C "$SB" commit -qm baselines
  approve home.deckList/typical/light-standard-macDefault true
  approve settings.general/typical/light-standard-macDefault false
  export FX_RESULTS="$BATS_TEST_TMPDIR/results.jsonl"
  result_line home.deckList/typical/light-standard-macDefault pass >"$FX_RESULTS"

  # Unchanged: nothing to compare.
  mg snapshots --platform mac
  [ "$(field .status)" = pass ]
  [ "$(field .details.guard.verdict)" = pass ]
  not_called 'image_diff.py guard'

  # Re-recorded with a difference under the tolerance (same look): passes.
  printf 'look: blue\nglass noise\n' >"$SB/app/Snapshots/$KEY/home.deckList/typical/light-standard-macDefault.png"
  mg snapshots --platform mac
  [ "$(field .status)" = pass ]
  [ "$(field .details.guard.verdict)" = pass ]
  called 'image_diff.py guard'

  # Changed beyond it: needs human review, not pass. A non-signature
  # screen's approved baseline never blocks.
  printf 'look: green\n' >"$SB/app/Snapshots/$KEY/home.deckList/typical/light-standard-macDefault.png"
  printf 'look: black\n' >"$SB/app/Snapshots/$KEY/settings.general/typical/light-standard-macDefault.png"
  mg snapshots --platform mac
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .details.guard.verdict)" = needs-human-review ]
  [ "$(field -c '.details.guard.needs_human_review')" = '["app/Snapshots/'"$KEY"'/home.deckList/typical/light-standard-macDefault.png"]' ]
  [ "$(field '[.failures[] | select(.id == "needs-human-review")] | length')" -eq 1 ]
  [[ "$(field .message)" == *needs-human-review* ]]
}

# probe_line <group>: the settings probe's result in a simulator run.
probe_line() {
  jq -nc --arg g "$1" '{name: "matrix.probe/glass/light-standard-iphoneLarge", subject: "matrix.probe", state: "glass",
    cell: "light-standard-iphoneLarge", status: "pass", platform: "iphone", group: $g, key: "iPhone99,1-99A999", layout: "component",
    actual: "@OUT@/actual/matrix.probe/glass/light-standard-iphoneLarge.png"}'
}

probe_fixture() {
  export FX_IMAGES="$BATS_TEST_TMPDIR/images" FX_RESULTS="$BATS_TEST_TMPDIR/results.jsonl"
  mkdir -p "$FX_IMAGES/actual/matrix.probe/glass"
  printf 'look: glass\n' >"$FX_IMAGES/actual/matrix.probe/glass/light-standard-iphoneLarge.png"
  probe_line rt0-rm0-t50 >"$FX_RESULTS"
}

@test "the iPhone platform renders on the iphone and iphone-small kinds, each telling the tests its kind" {
  probe_fixture
  mg snapshots --platform iphone
  [ "$(field .status)" = pass ]
  called '^fake lease iphone$'
  called '^fake lease iphone-small$'
  called 'mode=plan dest=id=FAKE-iphone-small .* target=iphone-small'
  called 'mode=verify dest=id=FAKE-iphone-small suites=all target=iphone-small group=rt0-rm0-t50'
  [ -f "$SB/build/mg/snapshots/$(field .run_id)/iphone-small/rt0-rm0-t50/results.jsonl" ]
  # The registry: where the matrix registers each screen.
  [ "$(field -c '.artifacts[] | select(.kind == "registry") | .path' | xargs jq -c '.screens["home.deckList"].platforms')" = '["iphone","mac"]' ]
}

# bats test_tags=gate
@test "snapshots on a locked Mac keep the simulators' results and queue the Mac part (host-session); a simulator failure still fails (U19b)" {
  probe_fixture
  # The simulators run in the test VM, which needs no host lease; the Mac's
  # in-process run takes it.
  cat >>"$SB/tools/mg.d/zz-fakes.sh" <<'EOF'
mg_sim_begin() { MG_SIM_RUNNER=vm; return 0; }
mg_snapshots__sim_lease() { MG_SNAP_SIM_LEASED=1; return 0; }
EOF
  stub ioreg 'echo "+-o Root  <class IORegistryEntry>"; echo "  \"IOConsoleUsers\" = ({\"kCGSSessionOnConsoleKey\"=Yes,\"CGSSessionScreenIsLocked\"=Yes})"'
  mg snapshots --platform mac,iphone
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '[.details.queued_parts[] | "\(.user_step) \(.part)"] | join(", ")')" = "host-session mac" ]
  [ "$(field '.failures | length')" = 0 ]
  called 'mode=verify dest=id=FAKE-iphone suites=all target=iphone '
  called 'mode=verify dest=id=FAKE-iphone-small suites=all target=iphone-small '
  not_called 'dest=platform=macOS'
  [ "$(field .details.counts.pass)" -ge 1 ]
  [[ "$(field .message)" == *"queued: mac"*"host-session"* ]]

  # A failing iPhone snapshot fails the run, Mac queued or not.
  result_line home.deckList/typical/light-standard-iphoneLarge fail >>"$FX_RESULTS"
  mg snapshots --platform mac,iphone
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .details.queued)" = false ]
  [ "$(field '[.failures[].id] | unique | join(" ")')" = "snapshot/home.deckList/typical/light-standard-iphoneLarge" ]
}

@test "a settings group without its probe image fails the run" {
  probe_fixture
  export FX_PLAN="$BATS_TEST_TMPDIR/plan.jsonl" FX_RESULTS_DIR="$BATS_TEST_TMPDIR/by-group"
  mkdir -p "$FX_RESULTS_DIR"
  echo '{"name": "home.deckList/typical/light-standard-iphoneLarge+rt", "group": "rt1-rm0-t50"}' >"$FX_PLAN"
  # The rt1 group of the iphone kind renders no probe (as a filter used to
  # leave it out); the iphone-small kind's does.
  : >"$FX_RESULTS_DIR/iphone-rt1-rm0-t50.jsonl"
  probe_line rt1-rm0-t50 >"$FX_RESULTS_DIR/iphone-small-rt1-rm0-t50.jsonl"
  mg snapshots --platform iphone --filter 'home.*'
  [ "$(field .status)" = fail ]
  [ "$(field -c '[.failures[].id]')" = '["probe/iphone/rt1-rm0-t50"]' ]
  called 'fake settings rt1-rm0-t50 on FAKE-iphone'
}

@test "after an OS rebaseline the guard checks an approved image through its successor" {
  local old_key="$KEY" new_key=Mac99,9-99B111 cell=home.deckList/typical/light-standard-macDefault
  baseline "$cell" "look: blue"
  git -C "$SB" add app && git -C "$SB" commit -qm baselines
  approve "$cell" true
  export FX_IMAGES="$BATS_TEST_TMPDIR/images" FX_RESULTS="$BATS_TEST_TMPDIR/results.jsonl"
  mkdir -p "$FX_IMAGES/actual/$(dirname "$cell")"
  printf 'look: blue\nnew OS build\n' >"$FX_IMAGES/actual/$cell.png"
  KEY="$new_key" result_line "$cell" recorded "@OUT@/actual/$cell.png" >"$FX_RESULTS"

  # The OS update: the new build's images are recorded, the old build's dropped.
  mg snapshots --platform mac --rebaseline
  [ "$(field .status)" = pass ]
  [ ! -d "$SB/app/Snapshots/$old_key" ]
  [ -f "$SB/app/Snapshots/$new_key/$cell.png" ]
  # Its approved image is still checked, through the successor: same look.
  [ "$(field .details.guard.verdict)" = pass ]
  [ "$(field .details.guard.checked)" -eq 1 ]
  [ "$(field --arg p "app/Snapshots/$new_key/$cell.png" '.details.guard.supersedes[$p]')" = "app/Snapshots/$old_key/$cell.png" ]

  # Beyond the tolerance on the new build: human review of the successor.
  printf 'look: green\n' >"$SB/app/Snapshots/$new_key/$cell.png"
  KEY="$new_key" result_line "$cell" pass >"$FX_RESULTS"
  mg snapshots --platform mac
  [ "$(field .status)" = fail ]
  [ "$(field -c '.details.guard.needs_human_review')" = "[\"app/Snapshots/$new_key/$cell.png\"]" ]
  [[ "$(field '.failures[] | select(.id == "needs-human-review") | .message')" == *"(approved as app/Snapshots/$old_key/$cell.png)"* ]]
}

@test "a run whose state coverage check did not run fails" {
  export FX_RESULTS="$BATS_TEST_TMPDIR/results.jsonl" FX_NO_COVERAGE=1
  : >"$FX_RESULTS"
  mg snapshots --platform mac
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = state-coverage/mac ]
}

@test "a run records the app sources it rendered: app code changes them, baselines do not" {
  export FX_RESULTS="$BATS_TEST_TMPDIR/results.jsonl"
  : >"$FX_RESULTS"
  mkdir -p "$SB/app/Meguri" "$SB/core"
  echo 'struct A {}' >"$SB/app/Meguri/A.swift"
  mg snapshots --platform mac
  local first
  first="$(field .details.input)"
  [[ "$first" =~ ^[0-9a-f]{64}$ ]]
  # A recorded baseline, committed or not, is no source.
  baseline home.deckList/typical/light-standard-macDefault "look: new"
  mg snapshots --platform mac
  [ "$(field .details.input)" = "$first" ]
  git -C "$SB" add app && git -C "$SB" commit -qm sources
  mg snapshots --platform mac
  [ "$(field .details.input)" = "$first" ]
  # Changed code is other sources; the digest leaves the staging area alone.
  echo 'struct B {}' >>"$SB/app/Meguri/A.swift"
  mg snapshots --platform mac
  [ "$(field .details.input)" != "$first" ]
  [ "$(git -C "$SB" status --porcelain -- app)" = " M app/Meguri/A.swift" ]
}

# --- critic ----------------------------------------------------------------

# A finished snapshots run with two images of the sample screen, and an
# a11y run that audited it.
fake_runs() {
  local snap="$SB/build/mg/snapshots/snap1" a11y="$SB/build/mg/a11y/a11y1"
  mkdir -p "$snap/mac/mac/actual/sample.deckOverview/overflow" "$a11y"
  python3 - "$snap/mac/mac/actual/sample.deckOverview/overflow/light-standard-macDefault.png" <<'PY'
import struct, sys, zlib
w, h = 2360, 1560
raw = b"".join(b"\0" + b"\xff" * (w * 3) for _ in range(h))
def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
open(sys.argv[1], "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
PY
  jq -nc --arg a "$snap/mac/mac/actual/sample.deckOverview/overflow/light-standard-macDefault.png" \
    '{name: "sample.deckOverview/overflow/light-standard-macDefault", subject: "sample.deckOverview", state: "overflow", cell: "light-standard-macDefault",
      status: "recorded", platform: "mac", group: "mac", layout: "screen", axes: {appearance: "light"}, actual: $a}' >"$snap/results.jsonl"
  jq -n '{job: "snapshots", status: "pass", commit: {sha: "abc"}, artifacts: [], details: {input: "src1"}}' >"$snap/result.json"
  echo '{"screens": {"sample.deckOverview": {"platforms": ["mac"], "states": ["overflow"], "cells": {"mac": ["overflow/light-standard-macDefault"]}}}}' >"$snap/registry.json"
  jq -n '{job: "a11y", run_id: "a11y1", status: "pass", details: {platform: "mac", input: "src1"}, artifacts: []}' >"$a11y/result.json"
  echo '[{"screen": "sample.deckOverview", "platform": "mac", "status": "pass"}]' >"$a11y/screens.json"
}

# The critic stub: logs its arguments and directory, answers with FX_ANSWER
# the way the CLI does with a schema (the message list, ending in a result).
critic_stub() {
  stub critic-cli 'echo "critic-cli cwd=$PWD args=$*" >>"$STUB_LOG"; jq -n --slurpfile a "$FX_ANSWER" "[{type: \"system\", subtype: \"init\"}, {type: \"result\", is_error: false, result: \"done\", structured_output: \$a[0]}]"'
  export MEGURI_CRITIC_CMD="$BIN/critic-cli"
}

answer() { # issues JSON
  jq -n --argjson i "$1" '{rubric_version: 1, screens: ["sample.deckOverview"], issues: $i}' >"$BATS_TEST_TMPDIR/answer.json"
  export FX_ANSWER="$BATS_TEST_TMPDIR/answer.json"
}

TRUNCATION='[{"screen": "sample.deckOverview", "state": "overflow", "variant": "light-standard-macDefault",
  "image": "screens/sample.deckOverview/overflow/light-standard-macDefault.png", "rule": "text-truncation", "severity": "high",
  "region": {"x": 24, "y": 32, "width": 1400, "height": 80}, "summary": "The deck name is cut off after one line at the largest text size."}]'

@test "AE90: a high truncation issue keeps the screen out of verified until a new critic session's re-check passes" {
  fake_runs
  critic_stub
  answer "$TRUNCATION"
  mg critic snap1 --a11y a11y1 --ledger build/status.yaml
  [ "$(field .status)" = warn ]
  [ "$(field '.details.statuses["sample.deckOverview"]')" = built ]
  [ "$(field .details.issues.high)" -eq 1 ]
  [ "$(field '.failures[0].id')" = "high/sample.deckOverview/overflow/light-standard-macDefault" ]
  [[ "$(field '.failures[0].message')" == "text-truncation: The deck name is cut off"*"region 24,32 1400x80"* ]]
  python3 - "$SB/build/status.yaml" <<'PY'
import sys, yaml
entry = yaml.safe_load(open(sys.argv[1]))["screens"]["sample.deckOverview"]
assert entry["status"] == "built", entry
assert entry["issues"][0]["rule"] == "text-truncation" and entry["issues"][0]["severity"] == "high", entry
assert entry["platforms"] == ["mac"], entry
assert entry["evidence"]["mac"]["a11y"]["status"] == "pass" and entry["evidence"]["mac"]["snapshots"]["status"] == "pass", entry
PY
  local first
  first="$(field .details.session)"

  # After the fix, a new critic session finds nothing: verified.
  answer '[]'
  mg critic snap1 --a11y a11y1 --ledger build/status.yaml
  [ "$(field .status)" = pass ]
  [ "$(field '.details.statuses["sample.deckOverview"]')" = verified ]
  [ "$(field .details.session)" != "$first" ]
  [ "$(grep -c '"role":"critic"' "$SB/docs/process/provenance/sessions.jsonl")" -eq 2 ]
  [ "$(git -C "$SB" log -1 --format=%s -- docs/process/provenance/sessions.jsonl)" = "Record session $(field .details.session) (critic, critic-$(field .run_id))" ]
}

@test "a snapshots run without every platform a screen is registered on is refused before any session starts" {
  fake_runs
  critic_stub
  answer '[]'
  echo '{"screens": {"sample.deckOverview": {"platforms": ["iphone", "ipad", "mac"]}}}' >"$SB/build/mg/snapshots/snap1/registry.json"
  mg critic snap1 --a11y a11y1 --ledger build/status.yaml
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = packet ]
  grep -q 'no image of sample.deckOverview on iphone, ipad' "$SB/build/mg/critic/$(field .run_id)/log.txt"
  not_called '^critic-cli'
  [ ! -f "$SB/build/status.yaml" ]
}

@test "a filtered snapshots run, or an a11y run of other app sources, is refused before any session starts" {
  fake_runs
  critic_stub
  answer '[]'
  local registry="$SB/build/mg/snapshots/snap1/registry.json"
  jq '.screens["sample.deckOverview"].cells.mac += ["typical/light-standard-macDefault"]' "$registry" >"$registry.tmp" && mv "$registry.tmp" "$registry"
  mg critic snap1 --a11y a11y1 --ledger build/status.yaml
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = packet ]
  grep -q 'no image of sample.deckOverview in 1 of the cells .*mac typical/light-standard-macDefault' "$SB/build/mg/critic/$(field .run_id)/log.txt"
  fake_runs
  jq '.details.input = "src0"' "$SB/build/mg/a11y/a11y1/result.json" >"$BATS_TEST_TMPDIR/a11y.json" && mv "$BATS_TEST_TMPDIR/a11y.json" "$SB/build/mg/a11y/a11y1/result.json"
  mg critic snap1 --a11y a11y1 --ledger build/status.yaml
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = packet ]
  grep -q 'audited sample.deckOverview on mac in app sources src0, the snapshots run snap1 rendered src1' "$SB/build/mg/critic/$(field .run_id)/log.txt"
  not_called '^critic-cli'
  [ ! -f "$SB/build/status.yaml" ]
}

@test "the critic session runs restricted, in a copy of the packet outside home, with the deny profile" {
  fake_runs
  critic_stub
  answer '[]'
  mg critic snap1 --ledger build/status.yaml
  [ "$(field .status)" = pass ]
  local call
  call="$(grep '^critic-cli' "$STUB_LOG")"
  for flag in "--restricted" "--safe-mode" "--strict-mcp-config" "--tools Read,Glob,Grep" "--disable-slash-commands" \
    "--settings $SB/.claude/settings.critic.json" "--permission-mode dontAsk" "--no-session-persistence" "--json-schema"; do
    [[ "$call" == *"$flag"* ]] || { echo "missing $flag in: $call"; return 1; }
  done
  [[ "$call" == "critic-cli cwd=$TMPDIR/meguri-critic."* ]]
  [[ "$call" != *"cwd=$HOME"* ]]
  # The copy is gone once the job ends; the packet stays in the run.
  [ -z "$(ls "$TMPDIR" | grep meguri-critic)" ]
  [ -f "$SB/build/mg/critic/$(field .run_id)/packet/packet.json" ]
  jq -e '.permissions.deny | index("Bash") and index("Edit") and index("Write") and index("Read(~/**)")' "$SB/.claude/settings.critic.json"
}

@test "critic output that fails schema validation is an error and records nothing" {
  fake_runs
  critic_stub
  answer '[{"screen": "sample.deckOverview", "rule": "text-truncation", "severity": "blocker"}]'
  mg critic snap1 --ledger build/status.yaml
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '[.failures[] | select(.id == "critic-output-invalid")] | length')" -ge 1 ]
  [ ! -f "$SB/build/status.yaml" ]
  # A rule the rubric does not have is rejected too.
  answer "$(jq -c '.[0].rule = "made-up-rule"' <<<"$TRUNCATION")"
  mg critic snap1 --ledger build/status.yaml
  [ "$(field .status)" = error ]
  [[ "$(field '.failures[0].message')" == *"rule made-up-rule is not in rubric v1"* ]]
  [ ! -f "$SB/build/status.yaml" ]
}

@test "the canonical ledger refuses sample screens" {
  fake_runs
  critic_stub
  answer '[]'
  mg critic snap1
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = ledger ]
  git -C "$SB" diff --quiet -- docs/design/screen-status.yaml
}

@test "the isolation probe passes when the outside read is refused and fails when it is not" {
  stub critic-cli 'cat <<JSON
{"type":"user","message":{"content":[{"type":"tool_result","content":"1\tinside the packet: PROBE-INSIDE"}]}}
{"type":"user","message":{"content":[{"type":"tool_result","is_error":true,"content":"$FX_OUTSIDE is outside the working directory"}]}}
JSON'
  export MEGURI_CRITIC_CMD="$BIN/critic-cli" FX_OUTSIDE="$SB/AGENTS.md"
  mg critic --probe-isolation
  [ "$(field .status)" = pass ]
  [ "$(field .details.probe.outside_leaked)" = false ]
  stub critic-cli 'cat <<JSON
{"type":"user","message":{"content":[{"type":"tool_result","content":"1\tinside the packet: PROBE-INSIDE"}]}}
{"type":"user","message":{"content":[{"type":"tool_result","content":"## Hard rules"}]}}
JSON'
  mg critic --probe-isolation
  [ "$(field .status)" = fail ]
}

@test "critic rejects unknown runs and options" {
  mg critic nosuchrun
  [ "$(field .status)" = error ]
  mg critic
  [ "$(field .status)" = error ]
  mg critic snap1 --screen Not-An-Id
  [ "$(field .status)" = error ]
}

# --- gallery ---------------------------------------------------------------

@test "gallery serve without a built gallery is an error; build rejects a bad base" {
  mg gallery serve
  [ "$(field .status)" = error ]
  mg gallery --base no-such-revision
  [ "$(field .status)" = error ]
}

@test "approvals of a pending branch are committed in its worktree with Antoine's trailer" {
  local wt="$BATS_TEST_TMPDIR/branch" site="$BATS_TEST_TMPDIR/site"
  git -C "$SB" worktree add -q -b unit/U999 "$wt"
  mkdir -p "$site" "$wt/docs/design/reviews"
  echo '{"version": 1, "baselines": [{"path": "p.png"}]}' >"$wt/docs/design/approved-baselines.json"
  echo '{"decisions": []}' >"$wt/docs/design/reviews/2026-10-14.json"
  echo "{\"checkout\": \"/pending/1\", \"branch\": \"unit/U999\", \"files\": [\"docs/design/reviews/2026-10-14.json\", \"docs/design/approved-baselines.json\"]}" >"$site/pending-commits.jsonl"
  echo "[{\"worktree\": \"$wt\", \"container\": \"/pending/1\"}]" >"$BATS_TEST_TMPDIR/mounts.json"
  cat >"$SB/tools/mg.d/zz-gallery-probe.sh" <<EOF
mg_job fx-commit host fx_commit "fixture: commit pending approvals"
fx_commit() { mg_gallery__commit_pending "$site" "$BATS_TEST_TMPDIR/mounts.json"; mg_status pass; }
EOF
  mg fx-commit
  [ "$(field .status)" = pass ]
  [ "$(git -C "$wt" log -1 --format=%s)" = "Approve changed signature baselines in the review gallery" ]
  [ "$(git -C "$wt" log -1 --format='%(trailers:key=Approved-by,valueonly)' | sed '/^$/d')" = antoine ]
  [ -z "$(git -C "$wt" status --porcelain -- docs/design)" ]
}

@test "the gallery shows the key variants, also as a screen renders them with Reduce Transparency" {
  cat >"$SB/tools/mg.d/zz-gallery-keys.sh" <<'EOF'
mg_job fx-keys host fx_keys "fixture: key variant names"
fx_keys() {
  local p kept=""
  for p in light-standard-iphoneLarge dark-ax5-ipadRegular+rt light-standard-macDefault dark-ax5-iphoneLarge+hc \
    light-xxxLarge-ipadRegular+rt light-standard-ipadRegular+rt+rm dark-standard-ipadCompact+rt; do
    printf 'app/Snapshots/K/s/typical/%s.png\n' "$p" | grep -qE "$MG_GALLERY_KEY_VARIANT" && kept="$kept $p"
  done
  mg_detail kept "$(jq -n --arg k "${kept# }" '$k')"
  mg_status pass
}
EOF
  mg fx-keys
  [ "$(field .status)" = pass ]
  [ "$(field .details.kept)" = "light-standard-iphoneLarge dark-ax5-ipadRegular+rt light-standard-macDefault" ]
}

@test "the gallery takes a baseline's blob only when HEAD holds the file as it is" {
  local committed="app/Snapshots/$KEY/home.deckList/typical/light-standard-macDefault.png"
  local untracked="app/Snapshots/$KEY/settings.general/typical/light-standard-macDefault.png"
  baseline home.deckList/typical/light-standard-macDefault "look: blue"
  git -C "$SB" add app && git -C "$SB" commit -qm baselines
  baseline settings.general/typical/light-standard-macDefault "look: grey"
  cat >"$SB/tools/mg.d/zz-gallery-blob.sh" <<EOF2
mg_job fx-committed host fx_committed "fixture: committed blobs"
fx_committed() {
  mg_detail committed "\$(jq -n --arg b "\$(mg_gallery__committed "\$MG_ROOT" "$committed")" '\$b')"
  mg_detail untracked "\$(jq -n --arg b "\$(mg_gallery__committed "\$MG_ROOT" "$untracked")" '\$b')"
  printf 'look: green\n' >"\$MG_ROOT/$committed"
  mg_detail changed "\$(jq -n --arg b "\$(mg_gallery__committed "\$MG_ROOT" "$committed")" '\$b')"
  mg_status pass
}
EOF2
  local blob
  blob="$(git -C "$SB" rev-parse "HEAD:$committed")"
  mg fx-committed
  [ "$(field .status)" = pass ]
  [ "$(field .details.committed)" = "$blob" ]
  [ "$(field .details.untracked)" = "" ]
  [ "$(field .details.changed)" = "" ]
}
