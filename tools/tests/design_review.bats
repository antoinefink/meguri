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
  local out="$1" dest="$2" suites="$3" mode="" dir=""
  shift 3
  for a in "$@"; do
    case "$a" in
      TEST_RUNNER_MEGURI_SNAPSHOT_MODE=*) mode="${a#*=}" ;;
      TEST_RUNNER_MEGURI_SNAPSHOT_OUT=*) dir="${a#*=}" ;;
    esac
  done
  echo "fake snapshot run mode=$mode dest=$dest suites=$suites" >>"$STUB_LOG"
  mkdir -p "$dir"
  [ -z "${FX_IMAGES:-}" ] || cp -R "$FX_IMAGES/." "$dir/"
  sed "s|@OUT@|$dir|g; s|@MODE@|$mode|g" "$FX_RESULTS" >"$dir/results.jsonl"
  [ -n "${FX_NO_COVERAGE:-}" ] || echo '{"screens": [], "problems": []}' >"$dir/state-coverage.json"
  return 0
}
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
  [ "$(field '.failures[0].message')" = "snapshots: --device goes with exactly one simulator platform (--platform iphone or ipad)" ]
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

@test "a run whose state coverage check did not run fails" {
  export FX_RESULTS="$BATS_TEST_TMPDIR/results.jsonl" FX_NO_COVERAGE=1
  : >"$FX_RESULTS"
  mg snapshots --platform mac
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = state-coverage/mac ]
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
  jq -n '{job: "snapshots", status: "pass", commit: {sha: "abc"}, artifacts: []}' >"$snap/result.json"
  jq -n '{job: "a11y", status: "pass", artifacts: []}' >"$a11y/result.json"
  echo '[{"screen": "sample.deckOverview", "status": "pass"}]' >"$a11y/screens.json"
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
assert entry["evidence"]["a11y"]["status"] == "pass" and entry["evidence"]["snapshots"]["status"] == "pass", entry
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
