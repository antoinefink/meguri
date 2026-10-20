#!/usr/bin/env bats
# Self-tests for Mac UI suites in the macOS test VM (U8b): the runner choice
# (VM, queued, or the host with MEGURI_MAC_UI_ALLOW_HOST=1), the VM lease, the
# headless start, the copy of products to the same absolute path in the VM,
# the remote test run with its result copied back, and the dropped-input
# check against the VM's log. tart and the VM's SSH wrapper are fakes
# (mac_vm_fake_tart.sh, mac_vm_fake_ssh.sh): the "VM" runs remote commands
# here, in its own copy of the file system under $FAKE_VM_DIR/<vm>/fs. Each
# test runs tools/mg from a throwaway copy of tools/ in its own git repo; the
# build is a fake that writes a minimal Products directory.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/app"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp -R "$REPO_ROOT/app/Fixtures" "$t/app/Fixtures"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$(new_checkout repo)"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  export MEGURI_HOST_LEASE_DIR="$BATS_TEST_TMPDIR/host.lock" MEGURI_HOST_LEASE_POLL_SECONDS=1
  export MEGURI_MAC_UI_DIR="$BATS_TEST_TMPDIR/mac-ui"
  export MEGURI_MAC_VM_POLL_SECONDS=1 MEGURI_MAC_VM_BOOT_SECONDS=6 MEGURI_MAC_VM_STOP_SECONDS=2
  unset MEGURI_MAC_UI_ALLOW_HOST MEGURI_MAC_UI_INTERVAL_MINUTES
  export FAKE_VM_DIR="$BATS_TEST_TMPDIR/vms" FAKE_VM_MIRROR="$BATS_TEST_TMPDIR" FAKE_VM_BIN="$BATS_TEST_TMPDIR/vm-bin"
  : >"$STUB_LOG"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN" "$FAKE_VM_BIN" "$FAKE_VM_DIR"
  host_stubs
  vm_stubs
  # The VM drive: its tart and SSH wrappers, as on /Volumes/T9/meguri-vm/bin.
  DRIVE="$BATS_TEST_TMPDIR/T9/meguri-vm/bin"
  mkdir -p "$DRIVE"
  ln -s "$REPO_ROOT/tools/tests/mac_vm_fake_tart.sh" "$DRIVE/tart"
  add_vm meguri-mac running
  export MEGURI_MAC_VM_CONFIG="$BATS_TEST_TMPDIR/config/mac-vm.json"
  write_config meguri-mac
  export PATH="$BIN:$PATH"
}

teardown() {
  [ ! -f "$FAKE_VM_DIR/run.pids" ] || xargs kill <"$FAKE_VM_DIR/run.pids" 2>/dev/null || true
  [ ! -f "$BATS_TEST_TMPDIR/bg.pids" ] || xargs kill <"$BATS_TEST_TMPDIR/bg.pids" 2>/dev/null || true
  cat "$FAKE_VM_DIR"/*/tests.pids 2>/dev/null | xargs kill -9 2>/dev/null || true
  pkill -f "$BATS_TEST_TMPDIR/bin/caffeinate" 2>/dev/null || true
  # Shown only when the test failed.
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, message, failures, details}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 30
}

# new_checkout <name>: a fresh copy of the template repository; prints its path.
new_checkout() {
  cp -R "$BATS_FILE_TMPDIR/template" "$BATS_TEST_TMPDIR/$1"
  add_fakes "$BATS_TEST_TMPDIR/$1"
  (cd "$BATS_TEST_TMPDIR/$1" && pwd -P)
}

stub() { printf '#!/bin/bash\n%s\n' "$2" >"$1" && chmod +x "$1"; }

# add_vm <name> <running|stopped>: a VM with its own SSH wrapper on the drive.
add_vm() {
  mkdir -p "$FAKE_VM_DIR/$1/fs"
  echo "$2" >"$FAKE_VM_DIR/$1/state"
  stub "$DRIVE/vm-ssh-$1" "FAKE_VM_NAME=$1 exec '$REPO_ROOT/tools/tests/mac_vm_fake_ssh.sh' \"\$@\""
}

# write_config <vm>...: the host's config naming the drive's wrappers.
write_config() {
  mkdir -p "$(dirname "$MEGURI_MAC_VM_CONFIG")"
  local v vms='[]'
  for v in "$@"; do
    vms="$(jq -c --arg n "$v" --arg s "$DRIVE/vm-ssh-$v" '. + [{name: $n, ssh: $s}]' <<<"$vms")"
  done
  jq -n --arg t "$DRIVE/tart" --argjson vms "$vms" '{tart: $t, vms: $vms}' >"$MEGURI_MAC_VM_CONFIG"
}

# The host: session check, caffeinate, and an xcodebuild and xcrun that would
# show any test run on the host screen. The host xcodebuild "passes" with a
# result bundle, so a host-path run can pass too.
host_stubs() {
  stub "$BIN/ioreg" 'echo "+-o Root  <class IORegistryEntry>"; echo "  \"IOConsoleUsers\" = ({\"kCGSSessionOnConsoleKey\"=Yes})"'
  stub "$BIN/system_profiler" 'echo "{\"SPDisplaysDataType\": [{\"spdisplays_ndrvs\": [{\"_name\": \"Display\"}]}]}"'
  stub "$BIN/caffeinate" 'exec sleep 600'
  stub "$BIN/osascript" 'case "$*" in *CGPreflightScreenCaptureAccess*) echo true ;; *NSScreen*) echo "{\"width\": 1512, \"height\": 982}" ;; *) echo "" ;; esac'
  stub "$BIN/screencapture" 'echo "screencapture $*" >>"$STUB_LOG"; exit 1'
  stub "$BIN/log" 'echo "host-log $*" >>"$STUB_LOG"'
  stub "$BIN/xcodebuild" '
echo "host-xcodebuild $*" >>"$STUB_LOG"
while [ $# -gt 0 ]; do [ "$1" = -resultBundlePath ] && b="$2"; shift; done
mkdir -p "$b" && echo "{\"result\": \"Passed\", \"passedTests\": 1, \"failedTests\": 0, \"skippedTests\": 0}" >"$b/summary.json"'
  stub "$BIN/xcrun" '
echo "xcrun $*" >>"$STUB_LOG"
a=("$@")
for ((i = 0; i < ${#a[@]}; i++)); do
  case "${a[$i]}" in --path) path="${a[$((i + 1))]}" ;; --output-path) out="${a[$((i + 1))]}" ;; --test-id) tid="${a[$((i + 1))]}" ;; esac
done
case "$*" in
  "xcresulttool get test-results summary "*) cat "$path/summary.json" ;;
  "xcresulttool get test-results tests "*) if [ -f "$path/tests.json" ]; then cat "$path/tests.json"; else echo "{\"testNodes\": []}"; fi ;;
  "xcresulttool get test-results activities "*) jq -e -c --arg id "$tid" "first(.[] | select(.testIdentifier == \$id))" "$path/activities.json" ;;
  "xcresulttool export attachments "*)
    mkdir -p "$out"
    if [ -d "$path/attachments" ]; then cp -R "$path/attachments/." "$out/"; else echo "[]" >"$out/manifest.json"; fi ;;
  *) exit 97 ;;
esac'
}

# The VM's commands. xcodebuild checks the test products at the -xctestrun
# path (they must have been copied there), logs its arguments, its
# TEST_RUNNER_ environment and whether the host lease is held, and writes a
# result bundle: a summary, and for a drive script the driver's attachments
# (with FAKE_VM_FRAMES sampled frames when TEST_RUNNER_MEGURI_DRIVE_FRAMES=1).
# FAKE_VM_XCODEBUILD_UNTIL=<file> makes it touch <file>.started and wait for
# <file>; FAKE_VM_XCODEBUILD_RC sets its exit code; FAKE_VM_NO_BUNDLE=1 makes
# it pass without writing a result bundle. Each run is a process of the VM
# ($FAKE_VM/tests.pids) that the fake pgrep and pkill see, and one that starts
# while another is alive logs vm-overlap. FAKE_VM_SSH_DROP=1: the VM's first
# run loses its SSH session and goes on running there until it is stopped;
# =down: SSH stops answering as it does. FAKE_VM_UNKILLABLE=1: the VM's test
# runs ignore every signal; FAKE_VM_IGNORE_TERM=1: they ignore SIGTERM, as a
# real VM's run never gets the SIGTERM that tools/mg sends the job's process
# tree when it cancels the job (the fake VM's processes are in that tree).
# FAKE_VM_RESULTS=<dir>: a failed run's bundle also has that directory's
# tests.json and activities.json, which the host's xcrun answers xcresulttool's
# test list and per-test activities from. Paths are logged as the VM sees
# them (without the fake file system's prefix).
vm_stubs() {
  stub "$FAKE_VM_BIN/sudo" 'exec "$@"'
  # The process check jobs make before they start work there (U19c).
  ln -s "$REPO_ROOT/tools/tests/mac_vm_fake_procs.sh" "$FAKE_VM_BIN/sysctl"
  ln -s "$REPO_ROOT/tools/tests/mac_vm_fake_procs.sh" "$FAKE_VM_BIN/ps"
  # pgrep -f and pkill [-9] -f, among this VM's test runs only: never the
  # host's processes.
  stub "$FAKE_VM_BIN/pgrep" '
rc=1
for pid in $(cat "$FAKE_VM/tests.pids" 2>/dev/null); do
  cmd="$(ps -o command= -p "$pid" 2>/dev/null)" || continue
  if printf "%s\n" "$cmd" | grep -Eq -- "${!#}"; then echo "$pid"; rc=0; fi
done
exit "$rc"'
  stub "$FAKE_VM_BIN/pkill" '
echo "vm-pkill $*" >>"$STUB_LOG"
sig=TERM
[ "$1" != -9 ] || sig=KILL
[ "${FAKE_VM_UNKILLABLE:-}" != 1 ] || exit 0
pids="$(pgrep -f "${!#}")" || exit 1
kill -s "$sig" $pids 2>/dev/null
exit 0'
  # FAKE_VM_INPUT_DROPPED=1: macOS dropped the input of every run;
  # FAKE_VM_INPUT_DROPPED_RE: of the runs whose arguments (with the drive
  # script) match the ERE, as xcodebuild noted them in $FAKE_VM/last-run.
  stub "$FAKE_VM_BIN/log" '
s="$*"
echo "vm-log ${s//"$FAKE_VM_FS"/}" >>"$STUB_LOG"
if [ "${FAKE_VM_INPUT_DROPPED:-}" = 1 ] ||
  { [ -n "${FAKE_VM_INPUT_DROPPED_RE:-}" ] && grep -Eq -- "$FAKE_VM_INPUT_DROPPED_RE" "$FAKE_VM/last-run" 2>/dev/null; }; then
  echo "2026-10-15 12:21:19.336 E  testmanagerd[2194:bfc28d] [com.apple.dt.xctest:GesturePerformer] Timed out after waiting 5.0s for Recap playback, implicitly confirming."
fi'
  stub "$FAKE_VM_BIN/sw_vers" 'case "$1" in -productVersion) echo "${FAKE_VM_MACOS:-27.0.1}" ;; -buildVersion) echo "${FAKE_VM_MACOS_BUILD:-26A434}" ;; *) exit 1 ;; esac'
  stub "$FAKE_VM_BIN/stat" '
if [ "$*" = "-f %Su /dev/console" ]; then
  n=$(($(cat "$FAKE_VM/console.calls" 2>/dev/null || echo 0) + 1))
  echo "$n" >"$FAKE_VM/console.calls"
  if [ "$n" -le "${FAKE_VM_LOGIN_AFTER:-0}" ]; then echo root; else echo meguri; fi
  exit 0
fi
exec '"$(command -v stat)"' "$@"'
  stub "$FAKE_VM_BIN/xcodebuild" '
unmap() { local s="$*"; printf "%s" "${s//"$FAKE_VM_FS"/}"; }
if [ "${1:-}" = -version ]; then printf "Xcode 27.0\nBuild version 27A266a\n"; exit 0; fi
echo "vm-xcodebuild $(unmap "$@")" >>"$STUB_LOG"
what="$(unmap "$@") ${TEST_RUNNER_MEGURI_DRIVE_SCRIPT:-}"
printf "%s\n" "$what" >"$FAKE_VM/last-run"
other="$(pgrep -f "xcodebuild[ ]test-without-building")" && echo "vm-overlap with $other" >>"$STUB_LOG"
echo "$$" >>"$FAKE_VM/tests.pids"
[ "${FAKE_VM_IGNORE_TERM:-}" != 1 ] || trap "" TERM
env | grep "^TEST_RUNNER_" | sort | while IFS= read -r e; do echo "vm-env $(unmap "$e")" >>"$STUB_LOG"; done
echo "vm-host-lease $([ -e "$MEGURI_HOST_LEASE_DIR" ] && echo held || echo free)" >>"$STUB_LOG"
while [ $# -gt 0 ]; do
  case "$1" in -xctestrun) run="$2" && shift 2 ;; -resultBundlePath) bundle="$2" && shift 2 ;; *) shift ;; esac
done
products="$(dirname "$run")"
app="$products/$(sed -n "s|.*__TESTROOT__/\([^/]*\)/.*|\1|p" "$run" 2>/dev/null | head -n 1)/Meguri.app/Contents/Info.plist"
[ -f "$run" ] && [ -f "$app" ] || { echo "xcodebuild: error: no test products at $(unmap "$products")"; exit 70; }
echo "vm-products $(unmap "$products") $(cat "$app")" >>"$STUB_LOG"
if [ -n "${FAKE_VM_XCODEBUILD_UNTIL:-}" ]; then
  : >"$FAKE_VM_XCODEBUILD_UNTIL.started"
  while [ ! -e "$FAKE_VM_XCODEBUILD_UNTIL" ]; do sleep 0.1; done
  [ -f "$app" ] || { echo "xcodebuild: error: the test products went away during the run"; exit 70; }
fi
if [ -n "${FAKE_VM_SSH_DROP:-}" ] && [ ! -e "$FAKE_VM/ssh-dropped" ]; then
  : >"$FAKE_VM/ssh-dropped"
  echo "Testing started"
  [ "$FAKE_VM_SSH_DROP" != down ] || : >"$FAKE_VM_DIR/ssh-down"
  kill -9 "$PPID"
  exec </dev/null >/dev/null 2>&1 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
  for _ in $(seq 1 6000); do sleep 0.1; done
  exit 0
fi
rc="${FAKE_VM_XCODEBUILD_RC:-0}"
# FAKE_VM_FAIL_RE: the runs whose arguments (with the drive script) match fail.
[ -z "${FAKE_VM_FAIL_RE:-}" ] || ! printf "%s\n" "$what" | grep -Eq -- "$FAKE_VM_FAIL_RE" || rc=65
[ "${FAKE_VM_NO_BUNDLE:-}" != 1 ] || { echo "** TEST SUCCEEDED **"; exit 0; }
mkdir -p "$bundle"
jq -n --argjson f "$([ "$rc" = 0 ] && echo 0 || echo 1)" \
  "{result: (if \$f == 0 then \"Passed\" else \"Failed\" end), passedTests: 11, failedTests: \$f, skippedTests: 1}" >"$bundle/summary.json"
[ "$rc" = 0 ] || [ -z "${FAKE_VM_RESULTS:-}" ] || cp "$FAKE_VM_RESULTS/tests.json" "$FAKE_VM_RESULTS/activities.json" "$bundle/"
if [ -n "${TEST_RUNNER_MEGURI_RECIPE_FIXTURES:-}" ]; then
  lease="$(dirname "$MEGURI_HOST_LEASE_DIR")/mac-vm.${FAKE_VM##*/}.lock/holder.json"
  echo "vm-recipe-lease $(jq -r .job "$lease")" >>"$STUB_LOG"
  cases="[]"
  old_ifs="$IFS"
  IFS=:
  for d in $TEST_RUNNER_MEGURI_RECIPE_FIXTURES; do
    [ -f "$d/collection.anki2" ] && [ -f "$d/manifest.json" ] && [ -f "$d/collection.media/asset.txt" ] || exit 65
    echo "vm-recipe $(unmap "$d") $(cat "$d/collection.media/asset.txt")" >>"$STUB_LOG"
    cases="$(jq -c --arg r "$([ "$rc" = 0 ] && echo Passed || echo Failed)" ". + [{nodeType: \"Arguments\", result: \$r}]" <<<"$cases")"
  done
  IFS="$old_ifs"
  jq -n --argjson c "$cases" "{testNodes: [{nodeType: \"Test Case\", result: \"Passed\", children: \$c}]}" >"$bundle/tests.json"
  jq -n --argjson c "$cases" "{result: (if all(\$c[]; .result == \"Passed\") then \"Passed\" else \"Failed\" end), passedTests: 1, failedTests: 0, skippedTests: 0}" >"$bundle/summary.json"
  case "${FAKE_VM_RECIPE_RESULT:-}" in
    invalid) echo invalid >"$bundle/summary.json" ;;
    empty) echo "{\"testNodes\": []}" >"$bundle/tests.json" ;;
  esac
fi
if [ -n "${TEST_RUNNER_MEGURI_DRIVE_SCRIPT:-}" ]; then
  [ -f "$TEST_RUNNER_MEGURI_DRIVE_SCRIPT" ] || { echo "AgentDriverTests: no drive script at $(unmap "$TEST_RUNNER_MEGURI_DRIVE_SCRIPT")"; exit 65; }
  att="$bundle/attachments"
  mkdir -p "$att"
  n=0
  [ "${TEST_RUNNER_MEGURI_DRIVE_FRAMES:-0}" != 1 ] || n="${FAKE_VM_FRAMES:-5}"
  jq -n --argjson t0 "$(date +%s)" --argjson n "$n" "
    def cap(\$b; \$ph): {phase: \$ph, hierarchy: \"\(\$b)-\(\$ph).json\", screenshot: \"\(\$b)-\(\$ph).png\"};
    {script: \"fixture\", path: \"/fixture.json\", platform: \"mac\", error: null, steps: [
      {index: 1, kind: \"launch\", transition: null, started: \$t0, ended: (\$t0 + 0.05), status: \"ok\", captures: [cap(\"drive-01-launch\"; \"after\")]},
      ({index: 2, kind: \"tap\", transition: \"dismiss-sheet\", started: (\$t0 + 0.1), ended: (\$t0 + 0.5), status: \"ok\",
        captures: [cap(\"drive-02-tap\"; \"before\"), cap(\"drive-02-tap\"; \"after\")]}
       + (if \$n > 0 then {frames: [range(\$n) | {file: \"drive-02-tap-frame-00\(. + 1).png\", time: (\$t0 + 0.1 + . * 0.1)}]} else {} end))
    ]}" >"$att/drive-log.json"
  for f in $(jq -r ".steps[] | (.captures[] | .hierarchy, .screenshot), (.frames[]?.file)" "$att/drive-log.json"); do
    case "$f" in
      *.json) echo "{\"schema\": 1, \"platform\": \"mac\", \"window\": {\"x\": 0, \"y\": 0, \"width\": 800, \"height\": 600}, \"root\": {}}" >"$att/$f" ;;
      *) printf png >"$att/$f" ;;
    esac
  done
  (cd "$att" && ls) | jq -R "{suggestedHumanReadableName: ., exportedFileName: .}" | jq -s "[{attachments: .}]" >"$bundle/manifest.tmp"
  mv "$bundle/manifest.tmp" "$att/manifest.json"
fi
echo "** TEST $([ "$rc" = 0 ] && echo SUCCEEDED || echo FAILED) **"
exit "$rc"'
}

# Fakes that replace the real functions (the file sorts after the jobs): the
# build writes a minimal Products directory with the UI plan's xctestrun, and
# the image-tools scripts write their reports here.
add_fakes() {
  cat >"$1/tools/mg.d/zz-vm-fakes.sh" <<'EOF'
mg_app_build() {
  echo "fake build $*" >>"$STUB_LOG"
  [ "${3:-}" != Snapshot ] || mg_on_exit recipe_env_after
  local p="$MG_ROOT/build/app/DerivedData/Build/Products"
  mkdir -p "$p/$2/Meguri.app/Contents" "$p/$2/MeguriUITests-Runner.app"
  printf 'built from %s\n' "$MG_ROOT" >"$p/$2/Meguri.app/Contents/Info.plist"
  printf '<plist><string>__TESTROOT__/%s/MeguriUITests-Runner.app</string></plist>\n' "$2" >"$p/${4:-Meguri}_${3:-UI}_macosx27.0-arm64.xctestrun"
}
fx_host() { case "$1" in /work/*) printf '%s\n' "$MG_ROOT/${1#/work/}" ;; *) printf '%s\n' "$1" ;; esac; }
mg_image_tools() {
  echo "fake image-tools $*" >>"$STUB_LOG"
  local script="$2" report="" out="" input="" files=() name
  shift 2
  case "$script" in
    */hierarchy_check.py)
      while [ "$#" -gt 0 ]; do
        case "$1" in --report) report="$2" && shift 2 ;; *) files[${#files[@]}]="$1" && shift ;; esac
      done
      printf '%s\n' "${files[@]}" | jq -R . | jq -s '{files: map({path: ., issues: []}), counts: {files: length, issues: 0}}' >"$(fx_host "$report")"
      ;;
    */contact_sheet.py)
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --out) out="$(fx_host "$2")" && shift 2 ;;
          --marks | --frame-list) input="$(fx_host "$2")" && shift 2 ;;
          *) shift ;;
        esac
      done
      for name in $(jq -r '.windows[].name' "$input"); do printf 'sheet' >"$out/$name.png"; done
      jq --arg out "$out" '{video: null, jump_cuts: [], windows: [.windows[] | {name, sheet: "\($out)/\(.name).png", jump_cuts: [], stalls: []}]}' \
        "$input" >"$out/report.json"
      ;;
  esac
}
EOF
}

# mg <args>: run the sandbox's tools/mg. Sets status, output, stderr and
# result (the result path, the last stdout line).
mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }
validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }
called() { grep -Eq -- "$1" "$STUB_LOG" || { echo "no call matching: $1"; cat "$STUB_LOG"; return 1; }; }
not_called() { ! grep -Eq -- "$1" "$STUB_LOG" || { echo "unexpected call: $(grep -E -- "$1" "$STUB_LOG")"; return 1; }; }

# bg_track <pid>: teardown terminates it. Start such a process with 3>&-:
# bats waits for whatever holds its fd 3.
bg_track() { echo "$1" >>"$BATS_TEST_TMPDIR/bg.pids"; }

# wait_for <file>: up to 10 s.
wait_for() {
  for _ in $(seq 1 100); do [ -e "$1" ] && return 0; sleep 0.1; done
  echo "$1 did not appear"
  return 1
}

# A closed hourly gate: a run started a minute ago, and another holding it.
close_the_hourly_gate() {
  mkdir -p "$MEGURI_MAC_UI_DIR/lock"
  jq -n --argjson e "$(($(date +%s) - 60))" '{epoch: $e, started_at: "a minute ago", job: "mac-ui", worktree: "/main"}' \
    >"$MEGURI_MAC_UI_DIR/last-start.json"
  sleep 600 >/dev/null 2>&1 3>&- &
  bg_track $!
  jq -n --argjson p "$!" '{job: "mac-ui", pid: $p, worktree: "/main"}' >"$MEGURI_MAC_UI_DIR/lock/holder.json"
}

vm_fs() { printf '%s/%s/fs%s\n' "$FAKE_VM_DIR" "$1" "$2"; }

# vm_tests_running <vm>: whether a test run is still alive in that fake VM.
vm_tests_running() { FAKE_VM="$FAKE_VM_DIR/$1" "$FAKE_VM_BIN/pgrep" -f 'xcodebuild[ ]test-without-building' >/dev/null; }

# Stub only generation and collection validation; run the real recipe launch
# path against the same fake VM as the Mac suites. Include spaces and both
# relative and absolute summary paths, outside app/Fixtures.
recipe_launch_fakes() {
  export MG_CONTAINER=dev
  mkdir -p "$SB/core/crates/meguri-testkit/profiles"
  echo '{"totals": {"span_days": 120}}' >"$SB/core/crates/meguri-testkit/profiles/user-shape.json"
  cat >>"$SB/tools/mg.d/zz-vm-fakes.sh" <<'EOF'
recipe_env_after() {
  echo "recipe-env-after ${TEST_RUNNER_MEGURI_RECIPE_FIXTURES-unset}" >>"$STUB_LOG"
}
mg_fixtures() {
  local name dir summaries="$MG_RUN_DIR/recipes.jsonl"
  : >"$summaries"
  for name in tiny 'with space'; do
    dir="build/fixtures/recipes/$name-1"
    mkdir -p "$MG_ROOT/$dir/collection.media"
    echo "$name collection" >"$MG_ROOT/$dir/collection.anki2"
    echo "$name media" >"$MG_ROOT/$dir/collection.media/asset.txt"
    echo '{"counts": {"notes": 1, "cards": 1, "revlog": 0, "notetypes": 1, "decks": 1, "presets": 1, "tags": 0}}' >"$MG_ROOT/$dir/manifest.json"
    [ "$name" != 'with space' ] || dir="$MG_ROOT/$dir"
    jq -nc --arg t "$name" --arg d "$dir" --argjson m "$(cat "$MG_ROOT/build/fixtures/recipes/$name-1/manifest.json")" \
      '{target: $t, kind: "recipe", dir: $d, manifest: $m}' >>"$summaries"
  done
  mg_detail fixtures "$(jq -sc . "$summaries")"
  mg_status pass
}
mg_oracle() {
  mg_detail check "$(jq -nc --argjson m "$(cat "$2/manifest.json")" --argjson bad "${FAKE_COLLECTION_PROBLEM:-0}" \
    '$m + {counts: ($m.counts + {graves: 0}), changed_tables: ["col", "config"], check_db: {card_properties: $bad}, timing: {days_elapsed: 120}}')"
  mg_status pass
}
EOF
  stub "$FAKE_VM_BIN/tar" '
case "$*" in
  *build/fixtures/recipes*"-xf -"*) [ "${FAKE_VM_RECIPE_COPY_FAIL:-}" != 1 ] || exit 42 ;;
esac
exec '"$(command -v tar)"' "$@"'
}

# bats test_tags=gate
@test "recipe launch on a locked host uses the VM lease, copies generated directories and returns Argument counts" {
  recipe_launch_fakes
  stub "$BIN/ioreg" 'echo "+-o Root  <class IORegistryEntry>"; echo "  \"IOConsoleUsers\" = ({\"kCGSSessionOnConsoleKey\"=Yes,\"CGSSessionScreenIsLocked\"=Yes})"'
  TEST_RUNNER_MEGURI_RECIPE_FIXTURES=caller-value mg fixtures-check recipes
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field '.summary | "\(.total) \(.passed) \(.failed)"')" = '3 3 0' ]
  [ "$(field .details.mac_runner)" = vm ]
  [ "$(field .details.mac_vm.lease.job)" = fixtures-check ]
  [ -z "$(field '.user_steps[] | select(.id == "host-session") | .id')" ]
  called '^fake build mac Debug Snapshot$'
  called '^vm-xcodebuild test-without-building .*Meguri_Snapshot_macosx.* -only-testing:MeguriSnapshotTests/RecipeFixtureTests$'
  called "^vm-env TEST_RUNNER_MEGURI_RECIPE_FIXTURES=$SB/build/fixtures/recipes/tiny-1:$SB/build/fixtures/recipes/with space-1\$"
  called '^vm-host-lease free$'
  called '^vm-recipe-lease fixtures-check$'
  called '^recipe-env-after caller-value$'
  local name f dir
  dir="$(dirname "$result")"
  [ "$(jq -c . "$dir/launch.json")" = '{"result":"Passed","passed":2,"failed":0,"skipped":0,"messages":[]}' ]
  for name in tiny 'with space'; do
    for f in collection.anki2 manifest.json collection.media/asset.txt; do
      cmp "$SB/build/fixtures/recipes/$name-1/$f" "$(vm_fs meguri-mac "$SB/build/fixtures/recipes/$name-1/$f")"
    done
    called "^vm-recipe $SB/build/fixtures/recipes/$name-1 $name media\$"
  done
  [ -f "$dir/launch.xcresult/tests.json" ]
  [ ! -e "$(vm_fs meguri-mac "$dir/launch.xcresult")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  not_called '^host-xcodebuild'
  validate "$result"
}

# bats test_tags=gate
@test "recipe launch without a VM queues only launch, retaining collection validation even with host opt-in" {
  recipe_launch_fakes
  rm "$MEGURI_MAC_VM_CONFIG"
  MEGURI_MAC_UI_ALLOW_HOST=1 mg fixtures-check recipes
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.summary | "\(.total) \(.passed) \(.failed)"')" = '2 2 0' ]
  [ "$(field '.details.queued_parts | length')" = 1 ]
  [ "$(field '.details.queued_parts[0] | "\(.user_step) \(.part)"')" = "mac-vm the recipes' launch check" ]
  [ "$(field '.user_steps[] | select(.id == "mac-vm") | .state')" = missing ]
  [ "$(field '.failures | length')" = 0 ]
  not_called '^host-xcodebuild|^vm-xcodebuild'
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  validate "$result"
}

# bats test_tags=gate
@test "recipe launch distinguishes unreachable VM stages from actual copy, result and test failures" {
  recipe_launch_fakes
  local mode
  for mode in boot copy test result; do
    case "$mode" in
      boot) touch "$FAKE_VM_DIR/ssh-down"; MEGURI_MAC_VM_BOOT_SECONDS=1 mg fixtures-check recipes; rm "$FAKE_VM_DIR/ssh-down" ;;
      copy) FAKE_VM_SSH_REFUSE='^rm -rf .*build/fixtures/recipes/' mg fixtures-check recipes ;;
      test) FAKE_VM_SSH_REFUSE='xcodebuild test-without-building' mg fixtures-check recipes ;;
      result) FAKE_VM_SSH_FAIL='^test -d ' mg fixtures-check recipes ;;
    esac
    [ "$status" -eq 0 ]
    [ "$(field .status)" = warn ]
    [ "$(field '.summary | "\(.passed) \(.failed)"')" = '2 0' ]
    [ "$(field '.failures | length')" = 0 ]
    [ "$(field '.details.queued_parts | length')" = 1 ]
    [ "$(field '.details.queued_parts[0] | "\(.user_step) \(.part)"')" = "mac-vm the recipes' launch check" ]
    [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  done
  FAKE_VM_RECIPE_COPY_FAIL=1 mg fixtures-check recipes
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = mac-vm-copy ]
  FAKE_VM_NO_BUNDLE=1 mg fixtures-check recipes
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  FAKE_VM_RECIPE_RESULT=invalid mg fixtures-check recipes
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  FAKE_VM_RECIPE_RESULT=empty mg fixtures-check recipes
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  FAKE_VM_XCODEBUILD_RC=65 mg fixtures-check recipes
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(jq .failed "$(dirname "$result")/launch.json")" = 2 ]
  echo invalid >"$MEGURI_MAC_VM_CONFIG"
  mg fixtures-check recipes
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = mac-vm-config ]
  not_called '^host-xcodebuild'
}

@test "recipe launch queuing preserves independent collection failures and restores an unset runner environment" {
  recipe_launch_fakes
  rm "$MEGURI_MAC_VM_CONFIG"
  FAKE_COLLECTION_PROBLEM=1 mg fixtures-check recipes
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .details.queued)" = false ]
  [ "$(field '.summary | "\(.passed) \(.failed)"')" = '0 2' ]
  write_config meguri-mac
  mg fixtures-check recipes
  [ "$(field .status)" = pass ]
  called '^recipe-env-after unset$'
  not_called '^host-xcodebuild'
}

# --- runs in the VM ---------------------------------------------------------------

@test "with the VM configured, mac-ui --suite a11y runs in the VM: no host lease, no hourly gate, the result bundle in the run directory" {
  close_the_hourly_gate
  cp "$MEGURI_MAC_UI_DIR/last-start.json" "$BATS_TEST_TMPDIR/last-start.before"
  mg mac-ui --suite a11y
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_runner)" = vm ]
  [ "$(field .details.mac_vm.name)" = meguri-mac ]
  [ "$(field .details.host_lease)" = null ]
  [ "$(field .details.queued)" = null ]
  called "^fake build mac Debug\$"
  called "^vm-xcodebuild test-without-building -xctestrun $SB/build/app/DerivedData/Build/Products/Meguri_UI_macosx27.0-arm64.xctestrun -destination platform=macOS,arch=arm64 -resultBundlePath $SB/build/mg/mac-ui/[^ ]+/a11y.xcresult -collect-test-diagnostics never -only-testing:MeguriUITests/AccessibilityHelperTests\$"
  called "^vm-host-lease free\$"
  not_called "^host-xcodebuild"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  # The gate was neither consulted nor stamped.
  cmp "$MEGURI_MAC_UI_DIR/last-start.json" "$BATS_TEST_TMPDIR/last-start.before"
  # The result bundle and the log came back into the run directory; the VM
  # keeps no copy of the bundle.
  local dir
  dir="$(dirname "$result")"
  [ "$(jq -r .passedTests "$dir/a11y.xcresult/summary.json")" = 11 ]
  grep -q 'TEST SUCCEEDED' "$dir/a11y.log"
  [ ! -e "$(vm_fs meguri-mac "$dir/a11y.xcresult")" ]
  [ "$(field '.summary | "\(.total) \(.passed)"')" = "1 1" ]
  # The VM lease is released.
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
}

@test "a11y --platform mac runs in the VM and reports the suite's counts" {
  mg a11y --platform mac
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_runner)" = vm ]
  [ "$(field '.summary | "\(.passed) \(.skipped)"')" = "11 1" ]
  [ "$(field .details.platform)" = mac ]
  called "^vm-xcodebuild test-without-building .* -only-testing:MeguriUITests/AccessibilityHelperTests\$"
  not_called "^host-xcodebuild"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

@test "ui-test --platform mac runs the named suites in the VM" {
  mg ui-test --platform mac --suite NavigationLayoutTests
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_runner)" = vm ]
  [ "$(field .details.platform)" = mac ]
  [ "$(field .details.config)" = Debug ]
  called '^fake build mac Debug$'
  called "^vm-xcodebuild test-without-building -xctestrun $SB/build/app/DerivedData/Build/Products/Meguri_UI_macosx27.0-arm64.xctestrun -destination platform=macOS,arch=arm64 -resultBundlePath $SB/build/mg/ui-test/[^ ]+/mac.xcresult -collect-test-diagnostics never -only-testing:MeguriUITests/NavigationLayoutTests\$"
  not_called "^host-xcodebuild"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  [ ! -d "$(dirname "$result")/allocations" ]
}

@test "ui-test Profile on the Mac runs only the requested method with Meguri-Profile's Performance products in the VM" {
  mg ui-test --config Profile --platform mac --suite CardSwapTimingSpikeTests/testDiagnostic
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_runner)" = vm ]
  [ "$(field .details.config)" = Profile ]
  [ "$(field '.details.suites | join(",")')" = CardSwapTimingSpikeTests/testDiagnostic ]
  [ "$(field '.summary | "\(.passed) \(.failed)"')" = "11 0" ]
  called '^fake build mac Profile Performance Meguri-Profile$'
  called "^vm-xcodebuild test-without-building -xctestrun $SB/build/app/DerivedData/Build/Products/Meguri-Profile_Performance_macosx27.0-arm64.xctestrun -destination platform=macOS,arch=arm64 -resultBundlePath $SB/build/mg/ui-test/[^ ]+/mac.xcresult -collect-test-diagnostics never -only-testing:MeguriUITests/CardSwapTimingSpikeTests/testDiagnostic\$"
  [ "$(grep -c '^vm-xcodebuild ' "$STUB_LOG")" = 1 ]
  [ -f "$(vm_fs meguri-mac "$SB/build/app/DerivedData/Build/Products/Profile/Meguri.app/Contents/Info.plist")" ]
  [ -f "$(dirname "$result")/mac.xcresult/summary.json" ]
  [ ! -e "$(vm_fs meguri-mac "$(dirname "$result")/mac.xcresult")" ]
  [ "$(field '[.artifacts[] | select(.kind == "perf-result" or .kind == "metrics")] | length')" = 0 ]
  not_called '^host-xcodebuild'
  [ ! -d "$(dirname "$result")/allocations" ]
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  validate "$result"
}

@test "ui-test refuses unsupported, invalid and missing configurations before any build or VM work" {
  local platform config
  for platform in mac iphone; do
    for config in Release AppStore invalid 'Debug Profile' ''; do
      mg ui-test --platform "$platform" --suite NavigationLayoutTests --config "$config"
      [ "$status" -eq 2 ]
      [ "$(field .status)" = error ]
      [ "$(field '.failures[0].id')" = usage ]
      [ "$(field .message)" = 'ui-test: --config takes Debug or Profile' ]
    done
    mg ui-test --platform "$platform" --suite NavigationLayoutTests --config
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
  done
  not_called '^(fake build|ssh |tart |vm-|host-xcodebuild)'
}

# This fake exercises the orchestration only. mac_allocations.bats runs the
# real helper with deterministic process/clock fakes and no real attachment.
allocation_control_fake() {
  local python fake
  printf -v python '%q' "$(command -v python3)"
  printf -v fake '%q' "$REPO_ROOT/tools/tests/mac_allocations_fake_vm.py"
  stub "$FAKE_VM_BIN/python3" '
case "${1:-}" in
  */allocations/mac-allocations.py)
    exec '"$python"' '"$fake"' "$@" ;;
  *) exec '"$python"' "$@" ;;
esac'
  cat >>"$SB/tools/mg.d/zz-vm-fakes.sh" <<'EOF'
# Assert artifact collection happens before giving the VM lease back.
eval "$(declare -f mg_mac_vm_copy_out | sed '1s/mg_mac_vm_copy_out/mg_allocation_fake_copy_out/')"
mg_mac_vm_copy_out() {
  case "$1" in */allocations)
    test -f "$(mvl_lease_base "$MG_ROOT")/mac-vm.$(jq -r .name <<<"$MG_MAC_VM").lock/holder.json" || return 1
    echo 'allocations-fetch lease-held' >>"$STUB_LOG"
    ;;
  esac
  mg_allocation_fake_copy_out "$@"
}
EOF
}

allocation_run() {
  mg ui-test --platform mac --config Profile --allocations --suite CardSwapTimingSpikeTests/testAThousandSwapsKeepTheSamePagesAndFlatMemory
}

@test "allocations rejects incompatible flags/selectors before building or probing a VM" {
  local platform config selector
  for platform in mac iphone ipad; do
    for config in Debug Profile; do
      [ "$platform/$config" != mac/Profile ] || continue
      mg ui-test --platform "$platform" --config "$config" --allocations --suite CardSwapTimingSpikeTests/testAThousandSwapsKeepTheSamePagesAndFlatMemory
      [ "$status" -eq 2 ]
      [ "$(field '.failures[0].id')" = usage ]
    done
  done
  for selector in CardSwapTimingSpikeTests CardSwapTimingSpikeTests/testDiagnostic NavigationLayoutTests; do
    mg ui-test --platform mac --config Profile --allocations --suite "$selector"
    [ "$status" -eq 2 ]
  done
  mg ui-test --platform mac --config Profile --allocations
  [ "$status" -eq 2 ]
  mg ui-test --platform mac --config Profile --allocations --suite CardSwapTimingSpikeTests/testAThousandSwapsKeepTheSamePagesAndFlatMemory --suite CardSwapTimingSpikeTests/testAThousandSwapsKeepTheSamePagesAndFlatMemory
  [ "$status" -eq 2 ]
  mg ui-test --platform mac --config Profile --allocations --device some-device --suite CardSwapTimingSpikeTests/testAThousandSwapsKeepTheSamePagesAndFlatMemory
  [ "$status" -eq 2 ]
  not_called '^(fake build|ssh |tart |vm-|host-xcodebuild)'
}

@test "allocations never falls back to the host even when ordinary ui-test explicitly may" {
  rm -f "$MEGURI_MAC_VM_CONFIG"
  MEGURI_MAC_UI_ALLOW_HOST=1 allocation_run
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  not_called '^(fake build|host-xcodebuild|vm-xcodebuild|vm-allocations)'
}

@test "allocations starts before XCTest, stops and fetches under the lease and emits diagnostic-only artifacts" {
  SB="$(new_checkout "repo's spaces")"
  allocation_control_fake
  allocation_run
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.diagnostic_mode)" = allocations ]
  [ "$(field .details.ordinary_performance_receipt)" = false ]
  [ "$(field .details.allocations.coverage)" = uncertified/partial ]
  [ "$(field '.summary | "\(.passed) \(.failed)"')" = '11 0' ]
  [ "$(grep -c '^vm-allocations --start lease-held$' "$STUB_LOG")" -eq 1 ]
  [ "$(grep -c '^vm-allocations --stop lease-held$' "$STUB_LOG")" -eq 1 ]
  [ "$(grep -c '^vm-xcodebuild ' "$STUB_LOG")" -eq 1 ]
  called '^allocations-fetch lease-held$'
  awk '/^vm-allocations --start/ {start=NR} /^vm-xcodebuild / {test=NR} /^vm-allocations --stop/ {stop=NR} END {exit !(start < test && test < stop)}' "$STUB_LOG"
  [ -f "$(dirname "$result")/allocations/allocations.trace/data" ]
  [ ! -e "$(vm_fs meguri-mac "$(dirname "$result")/allocations")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  not_called '^host-xcodebuild'
  [ "$(field '[.artifacts[] | select(.kind == "perf-result" or .kind == "metrics")] | length')" = 0 ]
  validate "$result"
}

@test "allocations missing notification/trace and unknown finalization cannot pass" {
  allocation_control_fake
  local mode
  for mode in missing-notify missing-trace cleanup-failure stop-ssh-loss; do
    FAKE_ALLOCATIONS="$mode" allocation_run
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ "$(field .details.allocations_diagnostic_failed)" = true ]
    [ "$(field .summary.passed)" = 11 ]
  done
}

@test "allocations preserves XCTest failures/counts when profiling or trace copy fails" {
  allocation_control_fake
  FAKE_ALLOCATIONS=missing-notify FAKE_VM_XCODEBUILD_RC=65 allocation_run
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .summary.failed)" = 1 ]
  [ "$(field .details.allocations_diagnostic_failed)" = true ]
  # Match copy-out only, not helper-copy-in or the unrelated xcresult fetch.
  FAKE_VM_SSH_FAIL='tar -C .* -cf - .allocations.' FAKE_VM_XCODEBUILD_RC=65 allocation_run
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .summary.failed)" = 1 ]
  [ "$(field .details.allocations_diagnostic_failed)" = true ]
}

@test "allocations losing the start SSH still stops remotely and fetches the error evidence" {
  allocation_control_fake
  FAKE_ALLOCATIONS=start-ssh-loss allocation_run
  [ "$status" -eq 2 ]
  [ "$(field .details.allocations_diagnostic_failed)" = true ]
  called '^vm-allocations --stop lease-held$'
  not_called '^vm-xcodebuild'
  [ -f "$(dirname "$result")/allocations/metadata.json" ]
}

@test "allocations cancellation stops/fetches before VM lease release" {
  allocation_control_fake
  local job
  FAKE_VM_XCODEBUILD_UNTIL="$BATS_TEST_TMPDIR/never" FAKE_VM_IGNORE_TERM=1 "$SB/tools/mg" ui-test \
    --platform mac --config Profile --allocations --suite CardSwapTimingSpikeTests/testAThousandSwapsKeepTheSamePagesAndFlatMemory \
    >"$BATS_TEST_TMPDIR/job.out" 2>"$BATS_TEST_TMPDIR/job.err" 3>&- &
  job=$!
  bg_track "$job"
  wait_for "$BATS_TEST_TMPDIR/never.started"
  kill -TERM "$job"
  wait "$job" || true
  result="$(tail -n 1 "$BATS_TEST_TMPDIR/job.out")"
  [ "$(field '.failures[0].id')" = cancelled ]
  called '^vm-allocations --stop lease-held$'
  called '^allocations-fetch lease-held$'
  [ -f "$(dirname "$result")/allocations/metadata.json" ]
  [ ! -e "$(vm_fs meguri-mac "$(dirname "$result")/allocations")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
}

@test "a locked Mac with its display asleep does not hold up a VM run" {
  stub "$BIN/ioreg" 'echo "+-o Root  <class IORegistryEntry>"; echo "  \"IOConsoleUsers\" = ({\"kCGSSessionOnConsoleKey\"=Yes,\"CGSSessionScreenIsLocked\"=Yes})"'
  stub "$BIN/system_profiler" 'echo "{\"SPDisplaysDataType\": [{\"spdisplays_ndrvs\": [{\"_name\": \"Display\", \"spdisplays_asleep\": \"spdisplays_yes\"}]}]}"'
  mg mac-ui --suite a11y
  [ "$(field .status)" = pass ]
  [ "$(field .details.blocked)" = null ]
  [ -z "$(field '.user_steps[] | select(.id == "host-session") | .id')" ]
}

@test "repeated Mac UI runs within minutes each run at once in the VM" {
  local i
  for i in 1 2 3; do
    mg mac-ui --suite a11y
    [ "$(field .status)" = pass ] || { echo "run $i: $(field .message)"; return 1; }
  done
  [ "$(grep -c '^vm-xcodebuild ' "$STUB_LOG")" = 3 ]
  [ ! -e "$MEGURI_MAC_UI_DIR/last-start.json" ]
}

@test "the products and the test fixtures are copied to the same absolute path in the VM, replacing that checkout's previous copy" {
  local products="$SB/build/app/DerivedData/Build/Products"
  mkdir -p "$(vm_fs meguri-mac "$products")"
  echo stale >"$(vm_fs meguri-mac "$products/stale-from-last-run")"
  mg mac-ui --suite test-hooks
  [ "$(field .status)" = pass ]
  called "^vm-products $products built from $SB\$"
  [ -f "$(vm_fs meguri-mac "$products/Debug/Meguri.app/Contents/Info.plist")" ]
  [ ! -e "$(vm_fs meguri-mac "$products/stale-from-last-run")" ]
  cmp "$SB/app/Fixtures/TestHooks/single-file.fixture" "$(vm_fs meguri-mac "$SB/app/Fixtures/TestHooks/single-file.fixture")"
  called "^vm-xcodebuild .* -only-testing:MeguriUITests/TestHooksTests\$"
}

@test "a stopped VM is started headless and detached, and the run waits for a logged-in session" {
  echo stopped >"$FAKE_VM_DIR/meguri-mac/state"
  export FAKE_VM_LOGIN_AFTER=2
  mg mac-ui --suite a11y
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  called "^tart run --no-graphics meguri-mac\$"
  [ "$(field .details.mac_vm.started)" = true ]
  # The console showed the login window twice before the session was there.
  [ "$(cat "$FAKE_VM_DIR/meguri-mac/console.calls")" -ge 3 ]
  # The VM outlives the job.
  local pid
  pid="$(head -n 1 "$FAKE_VM_DIR/run.pids")"
  kill -0 "$pid"
  [ "$(cat "$FAKE_VM_DIR/meguri-mac/state")" = running ]

  # A running VM is not started again.
  : >"$STUB_LOG"
  mg mac-ui --suite a11y
  [ "$(field .status)" = pass ]
  not_called "^tart run"
  [ "$(field .details.mac_vm.started)" = false ]
}

@test "two checkouts start Mac suites at once: the second waits for the VM lease, then runs, and the first checkout's copy stays" {
  local sb2 first second
  sb2="$(new_checkout repo2)"
  FAKE_VM_XCODEBUILD_UNTIL="$BATS_TEST_TMPDIR/first-done" "$SB/tools/mg" mac-ui --suite a11y \
    >"$BATS_TEST_TMPDIR/first.out" 2>"$BATS_TEST_TMPDIR/first.err" 3>&- &
  first=$!
  bg_track "$first"
  wait_for "$BATS_TEST_TMPDIR/first-done.started"
  "$sb2/tools/mg" mac-ui --suite a11y >"$BATS_TEST_TMPDIR/second.out" 2>"$BATS_TEST_TMPDIR/second.err" 3>&- &
  second=$!
  bg_track "$second"
  for _ in $(seq 1 100); do grep -q 'waiting for the VM' "$BATS_TEST_TMPDIR/second.err" && break; sleep 0.1; done
  grep -q "VM lease: waiting for the VM (meguri-mac: mac-ui in $SB" "$BATS_TEST_TMPDIR/second.err"
  [ "$(grep -c '^vm-xcodebuild ' "$STUB_LOG")" = 1 ]
  touch "$BATS_TEST_TMPDIR/first-done"
  wait "$first"
  wait "$second"
  [ "$(jq -r .status "$(tail -n 1 "$BATS_TEST_TMPDIR/first.out")")" = pass ]
  [ "$(jq -r .status "$(tail -n 1 "$BATS_TEST_TMPDIR/second.out")")" = pass ]
  [ "$(jq -r .details.mac_vm.lease.waited_s "$(tail -n 1 "$BATS_TEST_TMPDIR/second.out")")" -ge 1 ]
  # Each run used its own checkout's products, and both copies are in the VM.
  [ "$(grep '^vm-products ' "$STUB_LOG" | cut -d' ' -f2 | tr '\n' ' ')" = "$SB/build/app/DerivedData/Build/Products $sb2/build/app/DerivedData/Build/Products " ]
  [ -f "$(vm_fs meguri-mac "$SB/build/app/DerivedData/Build/Products/Debug/Meguri.app/Contents/Info.plist")" ]
  [ -f "$(vm_fs meguri-mac "$sb2/build/app/DerivedData/Build/Products/Debug/Meguri.app/Contents/Info.plist")" ]
}

@test "a second VM in the config adds capacity: a run takes the first free VM" {
  add_vm meguri-mac-2 running
  write_config meguri-mac meguri-mac-2
  local first
  FAKE_VM_XCODEBUILD_UNTIL="$BATS_TEST_TMPDIR/first-done" "$SB/tools/mg" mac-ui --suite a11y \
    >"$BATS_TEST_TMPDIR/first.out" 2>/dev/null 3>&- &
  first=$!
  bg_track "$first"
  wait_for "$BATS_TEST_TMPDIR/first-done.started"
  mg a11y --platform mac
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_vm.name)" = meguri-mac-2 ]
  [ "$(field .details.mac_vm.lease.waited_s)" = 0 ]
  touch "$BATS_TEST_TMPDIR/first-done"
  wait "$first"
  [ "$(jq -r .details.mac_vm.name "$(tail -n 1 "$BATS_TEST_TMPDIR/first.out")")" = meguri-mac ]
}

@test "a VM lease left by a run that died is taken over" {
  local dead
  sleep 0 &
  dead=$!
  wait "$dead"
  mkdir -p "$BATS_TEST_TMPDIR/mac-vm.meguri-mac.lock"
  jq -n --argjson p "$dead" '{job: "mac-ui", pid: $p, worktree: "/gone"}' >"$BATS_TEST_TMPDIR/mac-vm.meguri-mac.lock/holder.json"
  mg mac-ui --suite a11y
  [ "$(field .status)" = pass ]
  [[ "$stderr" == *"VM lease: removing a stale lease (mac-ui in /gone"* ]]
}

@test "a Mac drive in the VM copies its script and fixture there, passes the script to the driver, and samples frames instead of recording the host screen" {
  local script="$SB/tools/drive-scripts/examples/open-sample-sheet.json"
  mg drive "$script" --platform mac --record
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_runner)" = vm ]
  cmp "$script" "$(vm_fs meguri-mac "$script")"
  cmp "$SB/app/Fixtures/TestHooks/single-file.fixture" "$(vm_fs meguri-mac "$SB/app/Fixtures/TestHooks/single-file.fixture")"
  called "^vm-env TEST_RUNNER_MEGURI_DRIVE_SCRIPT=$script\$"
  called "^vm-env TEST_RUNNER_MEGURI_DRIVE_FRAMES=1\$"
  called "^vm-xcodebuild .* -only-testing:MeguriUITests/AgentDriverTests/testDriveScript\$"
  not_called "^screencapture"
  [ -z "$(field '.user_steps[] | select(.id == "screen-recording") | .id')" ]
  [ "$(field '.details.drive | "\(.capture) \(.steps[1].frames)"')" = "frames 5" ]
  [ -f "$(dirname "$result")/motion/02-dismiss-sheet.png" ]
  [ "$(field '[.artifacts[] | select(.kind == "png")] | length')" -ge 3 ]
}

@test "mac-ui in the VM runs the example drive scripts that list the Mac" {
  mg mac-ui --suite "drive:tools/drive-scripts/examples/sample-routes.json"
  [ "$(field .status)" = pass ]
  called "^vm-env TEST_RUNNER_MEGURI_DRIVE_SCRIPT=$SB/tools/drive-scripts/examples/sample-routes.json\$"
  [ -f "$(vm_fs meguri-mac "$SB/tools/drive-scripts/examples/sample-routes.json")" ]
}

# bats test_tags=gate
@test "the VM's log showing the Recap playback timeout queues the run (mac-input) and records the VM's macOS and Xcode" {
  FAKE_VM_INPUT_DROPPED=1 mg mac-ui --suite test-hooks
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-input test-hooks" ]
  [ "$(field .details.blocked)" = mac-input ]
  [ "$(field '.user_steps[] | select(.id == "mac-input") | .state')" = missing ]
  called "^vm-log show --style compact --start [0-9]{4}-[0-9]{2}-[0-9]{2} [0-9:]{8} --predicate process == \"testmanagerd\" AND eventMessage CONTAINS \"Recap playback\"\$"
  not_called "^host-log"
  # Where it was seen, for doctor: the runner and its macOS and Xcode builds,
  # beside the leases that every checkout shares.
  local seen
  seen="$(dirname "$MEGURI_HOST_LEASE_DIR")/mac-input.json"
  [ "$(jq -r '"\(.runner) \(.vm) \(.macos) \(.macos_build) \(.xcode) \(.job)"' "$seen")" = "vm meguri-mac 27.0.1 26A434 27.0 (27A266a) mac-ui" ]
  [ "$(jq -r .run_id "$seen")" = "$(field .run_id)" ]

  # The same run with the input delivered passes.
  mg mac-ui --suite test-hooks
  [ "$(field .status)" = pass ]
}

# bats test_tags=gate
@test "mac-ui with 4 suites passed and 3 blocked by mac-input is queued naming the 3; with 1 failed and 2 blocked it fails (U19b)" {
  FAKE_VM_INPUT_DROPPED_RE='TestHooksTests|open-sample-sheet|sample-transition' mg mac-ui
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.summary | "\(.total) \(.passed) \(.failed) \(.queued) \(.errors)"')" = "7 4 0 3 0" ]
  [ "$(field '[.details.queued_parts[] | .part] | join(" ")')" = "test-hooks drive-open-sample-sheet drive-sample-transition" ]
  [ "$(field '[.details.queued_parts[] | .user_step] | unique | join(" ")')" = mac-input ]
  [ "$(field '.failures | length')" = 0 ]
  [[ "$(field .message)" == *"4 passed"*"3 of 7 suite(s) queued"*"test-hooks, drive-open-sample-sheet, drive-sample-transition"* ]]

  FAKE_VM_INPUT_DROPPED_RE='open-sample-sheet|sample-transition' FAKE_VM_FAIL_RE='TestHooksTests' mg mac-ui
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .details.queued)" = false ]
  [ "$(field '.summary | "\(.total) \(.passed) \(.failed) \(.queued)"')" = "7 4 1 2" ]
  [ "$(field '[.failures[].id] | join(" ")')" = "suite/test-hooks" ]
  [ "$(field '[.details.queued_parts[] | .part] | join(" ")')" = "drive-open-sample-sheet drive-sample-transition" ]
}

# test_results <dir> <class> <case>...: what a failed run of <class> holds,
# for FAKE_VM_RESULTS: xcresulttool's test list and each failing test's
# activities, shaped as xcresulttool gives them (a failure record with an
# untimed child). free: <class>/testChecksWithoutInput() fails without
# synthesizing input; tap: <class>/testTapsCancel() fails after its tap's
# synthesized event; early: the same test fails before its tap.
test_results() {
  local dir="$1" class="$2" c id nodes='[]' acts='[]'
  shift 2
  mkdir -p "$dir"
  for c in "$@"; do
    case "$c" in
      free) id="$class/testChecksWithoutInput()" ;;
      *) id="$class/testTapsCancel()" ;;
    esac
    nodes="$(jq -c --arg id "$id" --arg m "made-up $c failure" \
      '. + [{nodeType: "Test Case", nodeIdentifier: $id, result: "Failed", children: [{nodeType: "Failure Message", name: $m}]}]' <<<"$nodes")"
    acts="$(jq -c --arg id "$id" --arg c "$c" --arg m "made-up $c failure" '
      def act($t; $s): {title: $t, startTime: $s, isAssociatedWithFailure: false, activityType: "com.apple.dt.xctest.activity-type.internal"};
      def failure($s): {title: $m, startTime: $s, isAssociatedWithFailure: true, childActivities: [{title: "", isAssociatedWithFailure: false}]};
      def tap: act("Tap \"Cancel\" Button"; 200) + {childActivities: [act("Find the \"Cancel\" Button"; 200.1), act("Synthesize event"; 200.5)]};
      . + [{testIdentifier: $id, testRuns: [{activities: ([act("Start Test"; 100), act("Open com.antoinefink.meguri.dev"; 100.2)]
        + (if $c == "free" then [failure(101)] elif $c == "tap" then [tap, act("Waiting 5.0s for Sheet to not exist"; 205.6), failure(210)]
           else [failure(150), tap] end))}]}]' <<<"$acts")"
  done
  jq -n --arg class "$class" --argjson n "$nodes" '{testNodes: [{nodeType: "Unit test bundle", name: "MeguriUITests", children: [
    {nodeType: "Test Suite", name: $class, children: ($n + [{nodeType: "Test Case", nodeIdentifier: "\($class)/testPasses()", result: "Passed"}])}]}]}' \
    >"$dir/tests.json"
  printf '%s\n' "$acts" >"$dir/activities.json"
}

# bats test_tags=gate
@test "a suite whose input macOS dropped still fails on a test the drop does not explain; one the drop explains entirely is queued (U19b)" {
  local r="$BATS_TEST_TMPDIR/results"
  export FAKE_VM_INPUT_DROPPED=1 FAKE_VM_FAIL_RE=TestHooksTests FAKE_VM_RESULTS="$r"

  # A check without input failed, and another test's tap timed out: the
  # check's failure is the app's; the tap's is not recorded.
  test_results "$r" TestHooksTests free tap
  mg mac-ui --suite test-hooks
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .details.queued)" = false ]
  [ "$(field '[.failures[].id] | join(" ")')" = "test-hooks/TestHooksTests/testChecksWithoutInput() suite/test-hooks" ]
  [ "$(field '.failures[0].message')" = "made-up free failure (failed before or without the input macOS dropped)" ]
  [ "$(field '.summary | "\(.failed) \(.queued)"')" = "1 0" ]
  [ "$(field '.user_steps[] | select(.id == "mac-input") | .state')" = missing ]

  # Only the tap's test failed, after its dropped input: queued.
  test_results "$r" TestHooksTests tap
  mg mac-ui --suite test-hooks
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '.summary | "\(.failed) \(.queued)"')" = "0 1" ]

  # The same test failing before its first synthesized event: the app's.
  test_results "$r" TestHooksTests early
  mg mac-ui --suite test-hooks
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(" ")')" = "test-hooks/TestHooksTests/testTapsCancel() suite/test-hooks" ]

  # A failed run whose bundle lists no failing test.
  FAKE_VM_RESULTS="" mg mac-ui --suite test-hooks
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(" ")')" = "test-hooks/xcresult suite/test-hooks" ]

  # The Mac's accessibility suites and performance tests judge their failing
  # tests the same way.
  export FAKE_VM_FAIL_RE='AccessibilityHelperTests|SmokePerfTests'
  test_results "$r" AccessibilityHelperTests free tap
  mg a11y --platform mac
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | join(" ")')" = "test/AccessibilityHelperTests/testChecksWithoutInput()" ]
  [ "$(field .details.queued)" = false ]
  test_results "$r" AccessibilityHelperTests tap
  mg a11y --platform mac
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field '.failures | length')" = 0 ]

  test_results "$r" SmokePerfTests free tap
  mg perf-ui --platform mac --suite SmokePerfTests
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | join(" ")')" = "test/SmokePerfTests/testChecksWithoutInput()" ]
  test_results "$r" SmokePerfTests tap
  mg perf-ui --platform mac --suite SmokePerfTests
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
}

@test "a failing suite in the VM is a test failure, not an error" {
  FAKE_VM_XCODEBUILD_RC=65 mg a11y --platform mac
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .details.mac_runner)" = vm ]
}

# --- the VM is missing: queued -------------------------------------------------------

@test "without the VM config, Mac suites are queued with the mac-vm user step and nothing builds or runs on the host" {
  rm -f "$MEGURI_MAC_VM_CONFIG"
  local args
  for args in "mac-ui" "mac-ui --suite a11y" "a11y --platform mac" \
    "drive $SB/tools/drive-scripts/examples/open-sample-sheet.json --platform mac"; do
    # shellcheck disable=SC2086
    mg $args
    [ "$status" -eq 0 ] || { echo "tools/mg $args exited $status"; return 1; }
    [ "$(field .status)" = warn ]
    [ "$(field .details.queued)" = true ]
    [ "$(field '.user_steps[] | select(.id == "mac-vm") | "\(.state) \(.required)"')" = "missing true" ]
    [[ "$(field .message)" == *"queued"*"no macOS test VM is configured"* ]] || { echo "tools/mg $args: $(field .message)"; return 1; }
  done
  not_called "^fake build"
  not_called "^host-xcodebuild"
  not_called "^(tart|ssh) "
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
}

@test "with the VM's drive disconnected, or the VM gone, Mac suites are queued" {
  mv "$BATS_TEST_TMPDIR/T9" "$BATS_TEST_TMPDIR/T9-unplugged"
  mg mac-ui --suite a11y
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [[ "$(field .details.mac_vm.reason)" == *"$DRIVE/tart"*"is not there"* ]]
  mv "$BATS_TEST_TMPDIR/T9-unplugged" "$BATS_TEST_TMPDIR/T9"

  rm -rf "${FAKE_VM_DIR:?}/meguri-mac"
  mg mac-ui --suite a11y
  [ "$(field .status)" = warn ]
  [[ "$(field .details.mac_vm.reason)" == *"no VM named meguri-mac"* ]]
  not_called "^fake build"
  not_called "^host-xcodebuild"
}

@test "a config that is not valid JSON is an error" {
  echo '{"vms": [' >"$MEGURI_MAC_VM_CONFIG"
  mg mac-ui --suite a11y
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = mac-vm-config ]
  not_called "^fake build"
}

@test "without the VM, MEGURI_MAC_UI_ALLOW_HOST=1 takes the old host path: host lease, hourly gate and the host's xcodebuild" {
  rm -f "$MEGURI_MAC_VM_CONFIG"
  MEGURI_MAC_UI_ALLOW_HOST=1 mg mac-ui --suite a11y
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_runner)" = host ]
  [ "$(field .details.host_lease.job)" = mac-ui ]
  [ "$(jq -r .job "$MEGURI_MAC_UI_DIR/last-start.json")" = mac-ui ]
  called "^host-xcodebuild test-without-building -project app/Meguri.xcodeproj"
  not_called "^vm-xcodebuild"

  # The hourly gate still holds for host runs.
  MEGURI_MAC_UI_ALLOW_HOST=1 mg mac-ui --suite a11y
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [[ "$(field .message)" == *"queued by the hourly Mac UI gate"* ]]
}

@test "with the VM available, MEGURI_MAC_UI_ALLOW_HOST=1 still runs in the VM" {
  MEGURI_MAC_UI_ALLOW_HOST=1 mg mac-ui --suite a11y
  [ "$(field .details.mac_runner)" = vm ]
  not_called "^host-xcodebuild"
}

# --- the VM cannot be reached: queued; it cannot run: error; never the host screen ---------

# bats test_tags=gate
@test "SSH that does not answer queues the suites with the mac-vm step and the reason, never an error; the host screen is not used (U19c)" {
  touch "$FAKE_VM_DIR/ssh-down"
  MEGURI_MAC_VM_BOOT_SECONDS=2 mg mac-ui --suite a11y
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-vm the Mac UI suites" ]
  [[ "$(field '.details.queued_parts[0].reason')" == *"meguri-mac did not answer over SSH in a logged-in session within 2 s"*"Connection refused"* ]]
  [[ "$(field .details.mac_vm.reason)" == *"did not answer over SSH"* ]]
  [ "$(field '.user_steps[] | select(.id == "mac-vm") | "\(.state) \(.required)"')" = "missing true" ]
  [[ "$(field .message)" == *"queued: meguri-mac did not answer over SSH"*"(user step mac-vm)"* ]]
  not_called "^host-xcodebuild"
  not_called "^vm-xcodebuild"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  validate "$result"
}

# bats test_tags=gate
@test "a VM whose shell cannot fork when the job looks for a run left there queues the job, not an error (U19c: U28's merge)" {
  FAKE_VM_FORK_FAIL='mg-mac-vm: stopping' mg a11y --platform mac
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [[ "$(field '.details.queued_parts[0].reason')" == *"meguri-mac did not answer when this job looked for a UI test run an earlier job left there"*"fork failed: resource temporarily unavailable"* ]]
  not_called "^vm-xcodebuild"
  # The lease went back when the job queued.
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
}

# bats test_tags=gate
@test "a test run whose SSH session the VM refuses is queued under its suite; the next suite still runs (U19c)" {
  FAKE_VM_SSH_REFUSE='xcodebuild test-without-building .*TestHooksTests' mg mac-ui --suite test-hooks --suite a11y
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '.summary | "\(.total) \(.passed) \(.failed) \(.queued) \(.errors)"')" = "2 1 0 1 0" ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-vm test-hooks" ]
  [[ "$(field '.details.queued_parts[0].reason')" == *"meguri-mac refused the test run's SSH session (ssh exit 255: exec request failed on channel 0)"* ]]
  [[ "$(field .message)" == *"1 passed; 1 of 2 suite(s) queued: the test VM could not run test-hooks: "*"(user step mac-vm)"* ]]
  called "^vm-xcodebuild .*-only-testing:MeguriUITests/AccessibilityHelperTests"
}

# bats test_tags=gate
@test "a VM near its process limit: the job waits, checking again with a growing delay, and runs once processes free up (U19c)" {
  FAKE_VM_PROCS=2700,2700,2000 MEGURI_MAC_VM_PROCS_POLL_SECONDS=1 mg a11y --platform mac
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field -c '.details.mac_vm.procs | [.user, .max_per_user, .all, .max, .free, .needed]')" = '[2000,2900,2050,4000,900,400]' ]
  [ "$(field '.details.mac_vm.procs.waited_s >= 3')" = true ]
  [[ "$stderr" == *"waiting for free processes: meguri-mac is near its process limit (2700 of 2900 processes of its user, 2750 of 4000 in all): 200 free, a job needs 400"* ]]
  # Checked three times, the second delay twice the first, all before any copy or run.
  local times="$FAKE_VM_DIR/meguri-mac/procs.times"
  [ "$(wc -l <"$times" | tr -d ' ')" = 3 ]
  awk 'NR > 1 { d[NR - 1] = $1 - p } { p = $1 } END { exit !(d[1] >= 0.9 && d[2] >= 1.8 && d[2] >= 1.5 * d[1]) }' "$times"
  called "^vm-xcodebuild"
}

# bats test_tags=gate
@test "a VM that stays near its process limit is queued after the bounded wait, before anything is copied or run there (U19c)" {
  FAKE_VM_PROCS=2800 MEGURI_MAC_VM_PROCS_POLL_SECONDS=1 MEGURI_MAC_VM_PROCS_WAIT_SECONDS=3 mg a11y --platform mac
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '.details.queued_parts[0].user_step')" = mac-vm ]
  [[ "$(field '.details.queued_parts[0].reason')" == "meguri-mac is near its process limit (2800 of 2900 processes of its user, 2850 of 4000 in all): 100 free, a job needs 400, still after "[34]" s" ]]
  [ "$(field '.details.mac_vm.procs.free')" = 100 ]
  not_called "^vm-xcodebuild"
  [ ! -e "$(vm_fs meguri-mac "$SB/build/app/DerivedData/Build/Products")" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]

  # A check that gets no answer within the wait queues the job too.
  : >"$FAKE_VM_DIR/meguri-mac/procs.times"
  FAKE_VM_SSH_REFUSE='^printf "uid' MEGURI_MAC_VM_PROCS_POLL_SECONDS=1 MEGURI_MAC_VM_PROCS_WAIT_SECONDS=2 mg a11y --platform mac
  [ "$(field .status)" = warn ]
  [[ "$(field '.details.queued_parts[0].reason')" == "meguri-mac did not answer: the check of its processes did not run through (ssh exit 255: exec request failed on channel 0), still after "* ]]
  not_called "^vm-xcodebuild"

  # A VM that gives no process limit is not waited for.
  FAKE_VM_MAXPROCPERUID=unknown mg a11y --platform mac
  [ "$(field .status)" = pass ]
  [[ "$stderr" == *"it gave no process limit"*"going on without the process check"* ]]
}

@test "a VM that fails to start is an error with tart's reason; the host screen is not used" {
  echo stopped >"$FAKE_VM_DIR/meguri-mac/state"
  FAKE_TART_RUN_FAILS=1 MEGURI_MAC_VM_BOOT_SECONDS=3 mg a11y --platform mac
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = mac-vm-start ]
  [[ "$(field '.failures[0].message')" == *"The virtual machine failed to start."* ]]
  not_called "^host-xcodebuild"
}

@test "an SSH session lost during a suite is an error, and that run is stopped in the VM at once: the next suite runs alone" {
  FAKE_VM_SSH_DROP=1 mg mac-ui --suite a11y --suite test-hooks
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '[.failures[].id] | join(" ")')" = "mac-vm-ssh suite/a11y" ]
  [ "$(field '.summary | "\(.passed) \(.errors)"')" = "1 1" ]
  called "^vm-xcodebuild .* -only-testing:MeguriUITests/TestHooksTests\$"
  # The lost run was stopped before the next suite started, never beside it.
  local stop second
  stop="$(grep -n '^vm-pkill -f ' "$STUB_LOG" | head -n 1 | cut -d: -f1)"
  second="$(grep -n '^vm-xcodebuild ' "$STUB_LOG" | sed -n 2p | cut -d: -f1)"
  [ -n "$stop" ] && [ -n "$second" ] && [ "$stop" -lt "$second" ] || { cat "$STUB_LOG"; return 1; }
  not_called "^vm-overlap"
  ! vm_tests_running meguri-mac
  not_called "^host-xcodebuild"

  # A run that ends normally leaves nothing to stop.
  : >"$STUB_LOG"
  mg mac-ui --suite a11y
  [ "$(field .status)" = pass ]
  not_called "^vm-pkill"
}

@test "a lost run that cannot be stopped ends the remaining suites, and the VM gets no new run until it is stopped" {
  # The session drops and SSH stops answering: the run cannot be stopped,
  # and the VM cannot be reached: the suites are queued (U19c).
  FAKE_VM_SSH_DROP=down mg mac-ui --suite a11y --suite test-hooks --suite agent-driver
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]
  [ "$(field '[.details.queued_parts[] | "\(.user_step) \(.part)"] | join(", ")')" = "mac-vm a11y, mac-vm test-hooks, mac-vm agent-driver" ]
  [[ "$(field '.details.queued_parts[1].reason')" == "not run: the test run that lost its SSH session may still be running in the VM meguri-mac, which did not answer"* ]]
  [ "$(field '.summary | "\(.queued) \(.errors)"')" = "3 0" ]
  [ "$(grep -c '^vm-xcodebuild ' "$STUB_LOG")" = 1 ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
  vm_tests_running meguri-mac

  # SSH is back, but the run left there ignores every signal: the next job
  # starts no run beside it.
  rm "$FAKE_VM_DIR/ssh-down"
  : >"$STUB_LOG"
  FAKE_VM_UNKILLABLE=1 mg a11y --platform mac
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = mac-vm-stop ]
  [[ "$(field '.failures[0].message')" == *"still running"* ]]
  called "^vm-pkill -9 -f "
  not_called "^vm-xcodebuild"
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]

  # Once it can be stopped, the next job stops it first, then runs alone.
  : >"$STUB_LOG"
  mg mac-ui --suite a11y
  [ "$(field .status)" = pass ]
  [ "$(field .details.mac_vm.stopped_left_run)" = true ]
  called "^vm-pkill -f "
  not_called "^vm-overlap"
}

@test "a passing run whose result bundle does not come back from the VM is an error, never a pass" {
  # The SSH call that looks for the bundle fails as ssh: the VM could not be
  # reached, so the suite is queued (U19c), never a pass.
  FAKE_VM_SSH_FAIL='^test -d ' mg mac-ui --suite a11y
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.details.queued_parts[] | "\(.user_step) \(.part)"')" = "mac-vm a11y" ]
  [[ "$(field '.details.queued_parts[0].reason')" == *"could not check for "*"/a11y.xcresult in the VM meguri-mac (ssh exit 255"*"closed by remote host"* ]]

  # xcodebuild exits 0 but leaves no result bundle.
  FAKE_VM_NO_BUNDLE=1 mg a11y --platform mac
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = mac-vm-copy ]
  [[ "$(field '.failures[0].message')" == *"xcodebuild passed in the VM meguri-mac but left no result bundle"* ]]

  # The copy back exits 0 but brings no bundle.
  FAKE_VM_SSH_EMPTY_TAR='^tar -C ' mg a11y --platform mac
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = mac-vm-copy ]
  [[ "$(field '.failures[0].message')" == *"brought no result bundle back"* ]]
  not_called "^host-xcodebuild"
}

@test "a cancelled Mac UI run stops its test run in the VM before releasing the VM lease" {
  local job
  FAKE_VM_XCODEBUILD_UNTIL="$BATS_TEST_TMPDIR/never" FAKE_VM_IGNORE_TERM=1 "$SB/tools/mg" mac-ui --suite a11y \
    >"$BATS_TEST_TMPDIR/job.out" 2>"$BATS_TEST_TMPDIR/job.err" 3>&- &
  job=$!
  bg_track "$job"
  wait_for "$BATS_TEST_TMPDIR/never.started"
  kill -TERM "$job"
  wait "$job" || true
  result="$(tail -n 1 "$BATS_TEST_TMPDIR/job.out")"
  [ "$(field '.failures[0].id')" = cancelled ]
  # SIGTERM does not end it; SIGKILL does.
  called "^vm-pkill -f "
  called "^vm-pkill -9 -f "
  ! vm_tests_running meguri-mac
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
}

# bats test_tags=gate
@test "ui-test on the Mac queues only failures explained by dropped input and preserves independent failures" {
  local r="$BATS_TEST_TMPDIR/results"
  export FAKE_VM_INPUT_DROPPED=1 FAKE_VM_FAIL_RE=NavigationLayoutTests FAKE_VM_RESULTS="$r"

  test_results "$r" NavigationLayoutTests tap
  mg ui-test --platform mac --suite NavigationLayoutTests
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  [ "$(field .details.queued)" = true ]
  [ "$(field '.failures | length')" = 0 ]

  test_results "$r" NavigationLayoutTests free tap
  mg ui-test --platform mac --suite NavigationLayoutTests
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .details.queued)" = false ]
  [ "$(field '[.failures[].id] | join(" ")')" = "test/NavigationLayoutTests/testChecksWithoutInput()" ]

  test_results "$r" NavigationLayoutTests early
  mg ui-test --platform mac --suite NavigationLayoutTests
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(" ")')" = "test/NavigationLayoutTests/testTapsCancel()" ]

  FAKE_VM_RESULTS="" mg ui-test --platform mac --suite NavigationLayoutTests
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[].id] | join(" ")')" = "test/xcresult" ]
}
