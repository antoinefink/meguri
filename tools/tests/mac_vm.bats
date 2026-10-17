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
  case "${a[$i]}" in --path) path="${a[$((i + 1))]}" ;; --output-path) out="${a[$((i + 1))]}" ;; esac
done
case "$*" in
  "xcresulttool get test-results summary "*) cat "$path/summary.json" ;;
  "xcresulttool get test-results tests "*) echo "{\"testNodes\": []}" ;;
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
# Paths are logged as the VM sees them (without the fake file system's
# prefix).
vm_stubs() {
  stub "$FAKE_VM_BIN/sudo" 'exec "$@"'
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
app="$products/Debug/Meguri.app/Contents/Info.plist"
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
  local p="$MG_ROOT/build/app/DerivedData/Build/Products"
  mkdir -p "$p/$2/Meguri.app/Contents" "$p/$2/MeguriUITests-Runner.app"
  printf 'built from %s\n' "$MG_ROOT" >"$p/$2/Meguri.app/Contents/Info.plist"
  printf '<plist><string>__TESTROOT__/%s/MeguriUITests-Runner.app</string></plist>\n' "$2" >"$p/Meguri_UI_macosx27.0-arm64.xctestrun"
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

# --- the VM cannot run: error, never the host screen ------------------------------------

@test "SSH that does not answer is an error with the reason; the host screen is not used" {
  touch "$FAKE_VM_DIR/ssh-down"
  MEGURI_MAC_VM_BOOT_SECONDS=2 mg mac-ui --suite a11y
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = mac-vm-ssh ]
  [[ "$(field '.failures[0].message')" == *"meguri-mac did not answer over SSH in a logged-in session within 2 s"*"Connection refused"* ]]
  not_called "^host-xcodebuild"
  not_called "^vm-xcodebuild"
  [ ! -e "$MEGURI_HOST_LEASE_DIR" ]
  [ -z "$(ls -d "$BATS_TEST_TMPDIR"/mac-vm.*.lock 2>/dev/null)" ]
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

@test "a lost run that cannot be stopped aborts the remaining suites, and the VM gets no new run until it is stopped" {
  # The session drops and SSH stops answering: the run cannot be stopped.
  FAKE_VM_SSH_DROP=down mg mac-ui --suite a11y --suite test-hooks --suite agent-driver
  [ "$status" -eq 2 ]
  [ "$(field '[.failures[].id] | join(" ")')" = "mac-vm-ssh mac-vm-stop suite/a11y suite/test-hooks suite/agent-driver" ]
  [[ "$(field '.failures[] | select(.id == "suite/test-hooks") | .message')" == *"was not run"* ]]
  [ "$(field .summary.errors)" = 3 ]
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
  # The SSH call that looks for the bundle fails.
  FAKE_VM_SSH_FAIL='^test -d ' mg mac-ui --suite a11y
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '[.failures[].id] | join(" ")')" = "mac-vm-copy suite/a11y" ]
  [[ "$(field '.failures[0].message')" == *"could not check for "*"/a11y.xcresult in the VM meguri-mac (ssh exit 255"*"closed by remote host"* ]]

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
