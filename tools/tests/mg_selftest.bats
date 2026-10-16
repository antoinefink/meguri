#!/usr/bin/env bats
# tools/mg self-tests, run in the dev container by `tools/mg selftest`.
# Each test runs a fresh copy of tools/ and docker/ inside a throwaway git
# repo, so its results never land in the real build/ directory. Host-only
# behavior (macOS bash, real doctor, Docker from the host) is covered by
# mg_host_selftest.sh.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/core"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/core/rust-toolchain.toml" "$t/core/"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  export CARGO_TARGET_DIR="$BATS_TEST_TMPDIR/target" CARGO_NET_OFFLINE=true
}

# mg <args>: run the sandbox's tools/mg. Sets status, output (stdout), stderr,
# lines, and result (the last stdout line).
mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }

validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }

add_fixture_jobs() {
  cat >"$SB/tools/mg.d/zz-fixture.sh" <<'EOF'
mg_job fx-pass host fx_pass "fixture: pass with counts, artifact, detail"
mg_job fx-warn host fx_warn "fixture: warn"
mg_job fx-fail host fx_fail "fixture: fail"
mg_job fx-error host fx_error "fixture: error"
mg_job fx-crash host fx_crash "fixture: non-zero exit, no status"
mg_job fx-pass-exit host fx_pass_exit "fixture: pass then non-zero exit"
mg_job fx-warn-exit host fx_warn_exit "fixture: warn then non-zero exit"
mg_job fx-failure-only host fx_failure_only "fixture: failure, no status"
mg_job fx-args host fx_args "fixture: echo args"
mg_job fx-sleep host fx_sleep "fixture: sleep in a child process until stopped"
fx_sleep() {
  echo "fx-sleep started"
  sleep 60 &
  echo "$!" >"$MG_RUN_DIR/sleep.pid"
  wait "$!"
  : >"$MG_RUN_DIR/finished"
  mg_status pass
}
fx_pass() {
  echo "hello from fx-pass"
  : >"$MG_RUN_DIR/shot.png"
  mg_artifact png "$MG_RUN_DIR/shot.png" screenshot
  mg_count total 2
  mg_count passed 2
  mg_detail answer 42
  mg_message "fixture passed"
  mg_status pass
}
fx_warn() { mg_status warn; }
fx_fail() { mg_failure broken "it broke"; mg_status fail; }
fx_error() { mg_status error; }
fx_crash() { echo "crashing" >&2; return 3; }
fx_pass_exit() { mg_status pass; return 7; }
fx_warn_exit() { mg_status warn; return 7; }
fx_failure_only() { mg_failure broken "it broke" "tools/mg fx-failure-only --again"; }
fx_args() { mg_detail args "$(mg_json_array "$@")"; mg_status pass; }
EOF
}

# Host command stubs for doctor. Every call is logged to $STUB_LOG. Commands
# that could change system state are stubbed too, so a call to them shows up
# in the log instead of failing silently with "command not found".
make_stubs() {
  STUBS="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$STUBS"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  : >"$STUB_LOG"
  cat >"$STUBS/_stub" <<'EOF'
#!/usr/bin/env bash
name="$(basename "$0")"
printf '%s\n' "$name $*" >>"$STUB_LOG"
case "$name $*" in
  "uname -s") echo Darwin ;;
  "uname -m") echo arm64 ;;
  "uname -sm") echo "Darwin arm64" ;;
  "sw_vers -productVersion") echo 27.0 ;;
  "xcode-select -p") echo /Applications/Xcode.app/Contents/Developer ;;
  "xcodebuild -version") printf 'Xcode 27.0\nBuild version 27A266a\n' ;;
  "xcodebuild -license check") exit "${STUB_LICENSE_RC:-0}" ;;
  "xcrun simctl list runtimes --json")
    if [ "${STUB_NO_IOS27:-0}" = 1 ]; then
      echo '{"runtimes":[{"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-18-2","version":"18.2","buildversion":"22C150","isAvailable":true,"platform":"iOS"}]}'
    else
      echo '{"runtimes":[{"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-27-0","version":"27.0","buildversion":"24A434","isAvailable":true,"platform":"iOS"}]}'
    fi ;;
  "defaults read com.apple.dt.Xcode IDEProvisioningTeamByIdentifier")
    if [ "${STUB_ACCOUNT:-1}" = 1 ]; then
      printf '{\n    "A1" = (\n        {\n            teamID = DNQ34W43XX;\n        }\n    );\n}\n'
    else
      echo "The domain/default pair does not exist" >&2
      exit 1
    fi ;;
  "defaults read com.apple.dt.Xcode IDEProvisioningTeams")
    echo "The domain/default pair does not exist" >&2
    exit 1 ;;
  "docker info --format {{json .}}")
    # Like the real CLI: with the daemon down it still prints the client-side
    # JSON, with an empty ServerVersion, and exits 1.
    if [ "${STUB_DOCKER:-1}" = 1 ]; then
      echo "{\"ServerVersion\":\"29.8.2\",\"NCPU\":${STUB_DOCKER_CPUS:-16},\"MemTotal\":${STUB_DOCKER_MEM:-46278615040}}"
    else
      echo '{"ServerVersion":"","NCPU":0,"MemTotal":0}'
      exit 1
    fi ;;
  "docker compose version --short") echo 5.5.0 ;;
  "tart list --format json")
    echo "[{\"Source\": \"local\", \"Name\": \"meguri-mac\", \"Running\": true, \"State\": \"${STUB_VM_STATE:-running}\"}]" ;;
  "vm-ssh xcodebuild -version")
    [ "${STUB_VM_SSH:-1}" = 1 ] || exit 255
    printf 'Xcode %s\nBuild version %s\n' "${STUB_VM_XCODE:-27.0}" 27A266a ;;
  "vm-ssh xcrun simctl list runtimes --json")
    if [ "${STUB_VM_RUNTIME:-1}" = 1 ]; then
      echo '{"runtimes":[{"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-27-0","version":"27.0","buildversion":"24A434","isAvailable":true,"platform":"iOS"}]}'
    else
      echo '{"runtimes":[]}'
    fi ;;
  "rustup toolchain list") echo 1.99.0-aarch64-apple-darwin ;;
  "rustup +1.99.0 target list --installed")
    printf '%s\n' ${STUB_RUST_TARGETS-aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim} ;;
  "rustup +1.99.0 component list --installed")
    printf 'cargo-aarch64-apple-darwin\nclippy-aarch64-apple-darwin\nrustfmt-aarch64-apple-darwin\n' ;;
  "df -Pk "*)
    printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1s1 971350180 13631488 251658240 6%% /\n' ;;
  *) exit 97 ;;
esac
EOF
  chmod +x "$STUBS/_stub"
  local c
  for c in uname sw_vers xcode-select xcodebuild xcrun defaults docker rustup df \
    sudo launchctl open softwareupdate security tccutil systemsetup networksetup spctl pmset devicectl; do
    ln -s _stub "$STUBS/$c"
  done
  export PATH="$STUBS:$PATH"
  # The Mac UI test VM (U8b): its drive's tart and SSH wrappers, and the
  # host's config naming them.
  mkdir -p "$BATS_TEST_TMPDIR/T9/bin"
  ln -s "$STUBS/_stub" "$BATS_TEST_TMPDIR/T9/bin/tart"
  ln -s "$STUBS/_stub" "$BATS_TEST_TMPDIR/T9/bin/vm-ssh"
  export MEGURI_MAC_VM_CONFIG="$BATS_TEST_TMPDIR/mac-vm.json"
  jq -n --arg d "$BATS_TEST_TMPDIR/T9/bin" '{tart: "\($d)/tart", ssh: "\($d)/vm-ssh", vms: ["meguri-mac"]}' >"$MEGURI_MAC_VM_CONFIG"
}

# Doctor may only issue these read-only host commands.
assert_read_only_calls() {
  [ -s "$STUB_LOG" ]
  local bad
  bad="$(grep -Ev \
    -e '^uname -[a-z]+$' \
    -e '^sw_vers -productVersion$' \
    -e '^xcode-select -p$' \
    -e '^xcodebuild -(version|license check)$' \
    -e '^xcrun simctl list runtimes --json$' \
    -e '^defaults read com\.apple\.dt\.Xcode [A-Za-z]+$' \
    -e '^docker info --format \{\{json \.\}\}$' \
    -e '^docker compose version --short$' \
    -e '^rustup (toolchain list|\+[0-9.]+ (target|component) list --installed)$' \
    -e '^df -Pk /' \
    -e '^tart list --format json$' \
    -e '^vm-ssh xcodebuild -version$' \
    -e '^vm-ssh xcrun simctl list runtimes --json$' \
    "$STUB_LOG" || true)"
  if [ -n "$bad" ]; then
    echo "doctor issued commands outside the read-only allowlist:"
    echo "$bad"
    return 1
  fi
}

write_crate() { # write_crate <name> <lib.rs contents>
  mkdir -p "$SB/core/crates/$1/src"
  printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n' "$1" >"$SB/core/crates/$1/Cargo.toml"
  printf '%s\n' "$2" >"$SB/core/crates/$1/src/lib.rs"
}

write_workspace() {
  printf '[workspace]\nresolver = "3"\nmembers = ["crates/*"]\n' >"$SB/core/Cargo.toml"
}

write_file() { # write_file <path under the sandbox> <contents>
  mkdir -p "$(dirname "$SB/$1")"
  printf '%s\n' "$2" >"$SB/$1"
}

# copy_core: the real core/ workspace (without build output) in the sandbox,
# with its crates.io dependencies in CARGO_HOME. Fetching is a no-op once the
# shared cargo home has them. Builds share one target directory per file.
copy_core() {
  rm -rf "$SB/core"
  (cd "$REPO_ROOT" && tar cf - --exclude=core/target core) | (cd "$SB" && tar xf -)
  CARGO_NET_OFFLINE=false cargo fetch --locked --manifest-path "$SB/core/Cargo.toml" >&2
  export CARGO_TARGET_DIR="$BATS_FILE_TMPDIR/core-target"
}

# vendor_crate <name> <license>: a stand-in for a crates.io crate, outside the workspace.
vendor_crate() {
  write_file "vendor/$1/Cargo.toml" "[package]
name = \"$1\"
version = \"0.1.0\"
edition = \"2024\"
license = \"$2\""
  write_file "vendor/$1/src/lib.rs" ""
}

# add_dep <crate dir under core/> <dependency line>: under [dependencies].
add_dep() {
  local manifest="$SB/core/$1/Cargo.toml"
  if grep -q '^\[dependencies\]$' "$manifest"; then
    sed -i "/^\[dependencies\]\$/a $2" "$manifest"
  else
    printf '\n[dependencies]\n%s\n' "$2" >>"$manifest"
  fi
}

failure_ids() { jq -r '[.failures[].id] | sort | join("\n")' "$result"; }

# --- dispatcher ------------------------------------------------------------

@test "unknown job exits non-zero with an error result listing the known jobs" {
  mg nosuchjob
  [ "$status" -eq 2 ]
  [ "${#lines[@]}" -eq 1 ]
  [[ "$result" == "$SB/build/mg/_dispatch/"*/result.json ]]
  [ "$(field .status)" = error ]
  [ "$(field .exit_code)" = 2 ]
  [ "$(field .job)" = nosuchjob ]
  [ "$(field '.failures[0].id')" = unknown-job ]
  jq -e '.details.known_jobs | (index("doctor") and index("core-test") and index("selftest") and index("dev-image"))' "$result"
  validate "$result"
}

@test "every registered job's dry-run result validates against the schema" {
  run "$SB/tools/mg" --list
  [ "$status" -eq 0 ]
  local listing="$output" name placement _desc expected
  for expected in doctor core-test core-lint core-deny selftest selftest-host dev-image; do
    cut -f1 <<<"$listing" | grep -qx "$expected"
  done
  local results=()
  while IFS=$'\t' read -r name placement _desc; do
    mg "$name" --dry-run
    [ "$status" -eq 0 ] || { echo "$name --dry-run exited $status"; return 1; }
    [ "$(field .job)" = "$name" ]
    [ "$(field .dry_run)" = true ]
    [ "$(field .status)" = pass ]
    [ "$(field .details.job.placement)" = "$placement" ]
    results+=("$result")
  done <<<"$listing"
  [ "${#results[@]}" -ge 5 ]
  validate "${results[@]}"
}

@test "results record the commit and whether the tree was dirty" {
  mg doctor --dry-run
  [ "$(field .commit.sha)" = "$(git -C "$SB" rev-parse HEAD)" ]
  [ "$(field .commit.dirty)" = false ]
  echo "# local edit" >>"$SB/docker/compose.yml"
  mg doctor --dry-run
  [ "$(field .commit.dirty)" = true ]
}

@test "a job family dropped into tools/mg.d is reachable without editing the dispatcher" {
  add_fixture_jobs
  run "$SB/tools/mg" --list
  cut -f1 <<<"$output" | grep -qx fx-pass
  mg fx-pass
  [ "$status" -eq 0 ]
  [ "$(field .job)" = fx-pass ]
}

@test "exit code mirrors the job status" {
  add_fixture_jobs
  local results=()
  mg fx-pass
  [ "$status" -eq 0 ] && [ "$(field .status)" = pass ] && results+=("$result")
  mg fx-warn
  [ "$status" -eq 0 ] && [ "$(field .status)" = warn ] && results+=("$result")
  mg fx-fail
  [ "$status" -eq 1 ] && [ "$(field .status)" = fail ] && results+=("$result")
  [ "$(field '.failures[0].repro')" = "tools/mg fx-fail" ]
  mg fx-error
  [ "$status" -eq 2 ] && [ "$(field .status)" = error ] && results+=("$result")
  mg fx-crash
  [ "$status" -eq 2 ] && [ "$(field .status)" = error ] && results+=("$result")
  [ "$(field '.failures[0].id')" = job-exit ]
  mg fx-failure-only
  [ "$status" -eq 1 ] && [ "$(field .status)" = fail ] && results+=("$result")
  [ "$(field '.failures[0].repro')" = "tools/mg fx-failure-only --again" ]
  [ "${#results[@]}" -eq 6 ]
  validate "${results[@]}"
}

@test "a run writes counts, artifacts, details and its log into its run directory" {
  add_fixture_jobs
  mg fx-pass
  local run_dir
  run_dir="$(dirname "$result")"
  [[ "$run_dir" == "$SB/build/mg/fx-pass/"* ]]
  [ "${#lines[@]}" -eq 1 ]
  [[ "$stderr" == *"hello from fx-pass"* ]]
  grep -q "hello from fx-pass" "$run_dir/log.txt"
  [ "$(field -c .summary)" = '{"total":2,"passed":2,"failed":0,"skipped":0}' ]
  jq -e --arg p "$run_dir/shot.png" '[.artifacts[] | select(.kind == "png" and .path == $p and .label == "screenshot")] | length == 1' "$result"
  jq -e --arg p "$run_dir/log.txt" '[.artifacts[] | select(.kind == "log" and .path == $p)] | length == 1' "$result"
  [ "$(field .details.answer)" = 42 ]
  [ "$(field .message)" = "fixture passed" ]
  [ "$(field .placement)" = host ]
  [ "$(field .service)" = null ]
  [ ! -e "$run_dir/.mg" ]
  [ "$(readlink "$SB/build/mg/fx-pass/latest")" = "$(basename "$run_dir")" ]
  validate "$result"
}

@test "a job reporting pass then exiting non-zero reports an error" {
  add_fixture_jobs
  mg fx-pass-exit
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field .exit_code)" -eq 2 ]
  [ "$(field '.failures[0].id')" = job-exit ]
  [[ "$(field '.failures[0].message')" == *"reported pass but exited with code 7"* ]]
  validate "$result"
}

@test "a job reporting warn then exiting non-zero reports an error" {
  add_fixture_jobs
  mg fx-warn-exit
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field .exit_code)" -eq 2 ]
  [ "$(field '.failures[0].id')" = job-exit ]
  [[ "$(field '.failures[0].message')" == *"reported warn but exited with code 7"* ]]
  validate "$result"
}

@test "dry runs do not move latest" {
  add_fixture_jobs
  mg fx-pass
  local real
  real="$(basename "$(dirname "$result")")"
  mg fx-pass --dry-run
  [ "$(basename "$(dirname "$result")")" != "$real" ]
  [ "$(readlink "$SB/build/mg/fx-pass/latest")" = "$real" ]
}

@test "arguments reach the job intact; --dry-run after -- is passed on" {
  add_fixture_jobs
  mg fx-args -E "two words" -c "it's" -- --dry-run
  [ "$status" -eq 0 ]
  [ "$(field .dry_run)" = false ]
  [ "$(field -c .details.args)" = '["-E","two words","-c","it'"'"'s","--","--dry-run"]' ]
  [ "$(field -c .args)" = "$(field -c .details.args)" ]
  [ "$(field .message)" = "fx-args: pass" ]
}

# SIGTERM goes to tools/mg alone, not to its process group, so tools/mg must
# stop the job's processes itself. (bats starts it in the background, where
# SIGINT is ignored; the host self-test kills a docker job's process group.)
@test "a run stopped by SIGTERM stops the job's processes and writes an error result" {
  add_fixture_jobs
  local out="$BATS_TEST_TMPDIR/out" pid rc=0 sleeper="" start run_dir
  "$SB/tools/mg" fx-sleep >"$out" 2>"$BATS_TEST_TMPDIR/err" 3>&- &
  pid=$!
  for _ in $(seq 100); do
    sleeper="$(cat "$SB"/build/mg/fx-sleep/*/sleep.pid 2>/dev/null)" && [ -n "$sleeper" ] && break
    sleep 0.1
  done
  [ -n "$sleeper" ]
  start=$SECONDS
  kill -TERM "$pid"
  wait "$pid" || rc=$?
  [ "$rc" -eq 2 ]
  [ $((SECONDS - start)) -lt 10 ]
  for _ in $(seq 20); do
    kill -0 "$sleeper" 2>/dev/null || break
    sleep 0.1
  done
  if kill -0 "$sleeper" 2>/dev/null; then
    echo "the job's sleep ($sleeper) still runs"
    kill "$sleeper"
    return 1
  fi
  [ "$(wc -l <"$out")" -eq 1 ]
  result="$(cat "$out")"
  run_dir="$(dirname "$result")"
  [[ "$run_dir" == "$SB/build/mg/fx-sleep/"* ]]
  [ "$(field .status)" = error ]
  [ "$(field .exit_code)" = 2 ]
  jq -e '.failures[] | select(.id == "cancelled" and (.message | contains("SIGTERM")) and .repro == "tools/mg fx-sleep")' "$result"
  [ ! -e "$run_dir/finished" ]
  [ ! -e "$run_dir/.mg" ]
  grep -q "fx-sleep started" "$run_dir/log.txt"
  [ "$(readlink "$SB/build/mg/fx-sleep/latest")" = "$(basename "$run_dir")" ]
  validate "$result"
}

@test "a job registered twice makes every run an error" {
  echo 'mg_job doctor host true "duplicate"' >"$SB/tools/mg.d/zz-dup.sh"
  mg doctor --dry-run
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = registry ]
  [[ "$(field '.failures[0].message')" == *"registered twice"* ]]
  validate "$result"
}

@test "a docker job run outside any container needs Docker" {
  export MG_CONTAINER=""
  command -v docker && skip "this container has a docker CLI"
  mg core-test
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field .placement)" = docker ]
  [ "$(field '.failures[0].id')" = docker-unreachable ]
}

@test "a docker job is refused inside a different container" {
  export MG_CONTAINER=oracle
  mg core-test
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = wrong-container ]
}

# --- doctor (host commands stubbed) ----------------------------------------

@test "doctor without an Xcode account warns, lists the account step and only reads host state" {
  make_stubs
  export STUB_ACCOUNT=0
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  jq -e '.user_steps[] | select(.id == "xcode-account" and .state == "missing" and .required == true)' "$result"
  jq -e '.details.checks[] | select(.id == "xcode-account" and .status == "warn")' "$result"
  [ "$(field '[.details.checks[] | select(.status == "fail")] | length')" = 0 ]
  [ "$(field .details.environment.xcode_version)" = 27.0 ]
  [ "$(field .details.environment.xcode_build)" = 27A266a ]
  [[ "$stderr" == *"xcode-account"* ]]
  assert_read_only_calls
  validate "$result"
}

@test "doctor on a fully set-up Mac passes and still lists the steps it cannot verify" {
  make_stubs
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field '[.user_steps[] | select(.state == "missing" and .required)] | length')" = 0 ]
  jq -e '.user_steps[] | select(.id == "uitest-automation" and .state == "unverified")' "$result"
  jq -e '.user_steps[] | select(.id == "xcode-27-1" and .state == "missing" and .required == false)' "$result"
  [ "$(field .summary.total)" = "$(field '.details.checks | length')" ]
  assert_read_only_calls
  validate "$result"
}

@test "doctor reports the Mac UI test VM: configured, drive connected, present, running, SSH and the host's Xcode" {
  make_stubs
  mg doctor
  [ "$(field .status)" = pass ]
  jq -e '.details.checks[] | select(.id == "mac-vm/meguri-mac" and .status == "pass" and (.message | contains("running, SSH answers, Xcode 27.0 (27A266a) as on this Mac")))' "$result"
  [ "$(field -c '.details.mac_vm.vms[0] | [.drive_connected, .present, .state, .ssh, .xcode, .xcode_matches_host, .sim_runtime]')" = '[true,true,"running",true,"27.0 (27A266a)",true,true]' ]
  jq -e '.details.checks[] | select(.id == "mac-vm/meguri-mac/simulators" and .status == "pass" and (.message | contains("iOS 27.0 (24A434) simulator runtime")))' "$result"
  [ "$(field '[.user_steps[] | select(.id == "mac-vm")] | length')" = 0 ]

  # A VM without the pinned simulator runtime warns, naming the script that
  # adds it (not a user step: an agent runs it).
  STUB_VM_RUNTIME=0 mg doctor
  [ "$(field .status)" = warn ]
  jq -e '.details.checks[] | select(.id == "mac-vm/meguri-mac/simulators" and .status == "warn" and (.message | contains("tools/vm/add-sim-runtime.sh")))' "$result"
  [ "$(field '.details.mac_vm.vms[0].sim_runtime')" = false ]
  [ "$(field '[.user_steps[] | select(.id == "mac-vm")] | length')" = 0 ]

  # A stopped VM is fine: Mac UI jobs start it. Doctor never does.
  : >"$STUB_LOG"
  STUB_VM_STATE=stopped mg doctor
  [ "$(field .status)" = pass ]
  jq -e '.details.checks[] | select(.id == "mac-vm/meguri-mac" and .status == "pass" and (.message | contains("stopped")))' "$result"
  [ -z "$(grep -E '^(tart run|vm-ssh)' "$STUB_LOG" || true)" ]

  # SSH that does not answer, or another Xcode in the VM, warns.
  STUB_VM_SSH=0 mg doctor
  [ "$(field .status)" = warn ]
  jq -e '.details.checks[] | select(.id == "mac-vm/meguri-mac" and .status == "warn" and (.message | contains("SSH does not answer")))' "$result"
  STUB_VM_XCODE=27.1 mg doctor
  [ "$(field .status)" = warn ]
  jq -e '.details.checks[] | select(.id == "mac-vm/meguri-mac" and (.message | contains("Xcode 27.1 (27A266a) in the VM, 27.0 (27A266a) on this Mac")))' "$result"
  [ "$(field '.details.mac_vm.vms[0].xcode_matches_host')" = false ]
  assert_read_only_calls
  validate "$result"
}

@test "doctor without the VM, or with its drive disconnected, warns and lists the mac-vm step" {
  make_stubs
  rm "$MEGURI_MAC_VM_CONFIG"
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  jq -e '.details.checks[] | select(.id == "mac-vm" and .status == "warn" and (.message | contains("no macOS test VM is configured")))' "$result"
  [ "$(field '.user_steps[] | select(.id == "mac-vm") | "\(.state) \(.required)"')" = "missing true" ]
  [ "$(field .details.mac_vm.configured)" = false ]

  jq -n --arg d "$BATS_TEST_TMPDIR/T9/bin" '{tart: "\($d)/tart", ssh: "\($d)/vm-ssh", vms: ["meguri-mac"]}' >"$MEGURI_MAC_VM_CONFIG"
  rm -rf "$BATS_TEST_TMPDIR/T9"
  mg doctor
  [ "$(field .status)" = warn ]
  jq -e '.details.checks[] | select(.id == "mac-vm/meguri-mac" and .status == "warn" and (.message | contains("connect the drive that holds the VM")))' "$result"
  [ "$(field '.details.mac_vm.vms[0].drive_connected')" = false ]
  [ "$(field '.user_steps[] | select(.id == "mac-vm") | .state')" = missing ]
  assert_read_only_calls
  validate "$result"
}

@test "doctor fails and lists the step when the Xcode license is not accepted" {
  make_stubs
  export STUB_LICENSE_RC=1
  mg doctor
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  jq -e '.user_steps[] | select(.id == "xcode-license" and .state == "missing")' "$result"
  jq -e '.failures[] | select(.id == "xcode-license" and .message == "the Xcode license is not accepted" and .repro == "tools/mg doctor")' "$result"
  assert_read_only_calls
  validate "$result"
}

@test "doctor fails and lists the step without an iOS 27 simulator runtime" {
  make_stubs
  export STUB_NO_IOS27=1
  mg doctor
  [ "$status" -eq 1 ]
  jq -e '.user_steps[] | select(.id == "ios-runtime" and .state == "missing")' "$result"
  jq -e '.details.ios_runtimes | length == 1' "$result"
  jq -e '.failures[] | select(.id == "ios-runtime" and .message == "no available iOS 27 simulator runtime" and .repro == "tools/mg doctor")' "$result"
  assert_read_only_calls
  validate "$result"
}

@test "doctor fails when Docker is unreachable" {
  make_stubs
  export STUB_DOCKER=0
  mg doctor
  [ "$status" -eq 1 ]
  jq -e '.details.checks[] | select(.id == "docker" and .status == "fail")' "$result"
  jq -e '.failures[] | select(.id == "docker" and .message == "Docker is not reachable; start Docker Desktop" and .repro == "tools/mg doctor")' "$result"
  # A stopped daemon is not a resource problem.
  [ "$(field '[.details.checks[] | select(.id == "docker-resources")] | length')" = 0 ]
  [ "$(field '[.user_steps[] | select(.id == "docker-resources")] | length')" = 0 ]
  assert_read_only_calls
  validate "$result"
}

# MemTotal is the Linux VM's total after the kernel's reservation: a VM given
# exactly 16 GB reports about 0.9 GiB less (OrbStack: 44 GiB assigned, 43.1 GiB
# MemTotal).
@test "doctor accepts a Docker VM given exactly the documented 12 CPUs and 16 GB" {
  make_stubs
  export STUB_DOCKER_CPUS=12 STUB_DOCKER_MEM=16213720000
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  jq -e '.details.checks[] | select(.id == "docker-resources" and .status == "pass")' "$result"
  [ "$(field '[.user_steps[] | select(.id == "docker-resources")] | length')" = 0 ]
  assert_read_only_calls
  validate "$result"
}

@test "doctor warns and lists the docker-resources step for a Docker VM given 8 GB" {
  make_stubs
  export STUB_DOCKER_CPUS=12 STUB_DOCKER_MEM=8217301504
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  jq -e '.details.checks[] | select(.id == "docker-resources" and .status == "warn" and (.message | contains("16 GB")))' "$result"
  jq -e '.user_steps[] | select(.id == "docker-resources" and .state == "missing" and .required == true)' "$result"
  assert_read_only_calls
  validate "$result"
}

@test "doctor warns about missing Rust targets without making it a user step" {
  make_stubs
  export STUB_RUST_TARGETS="aarch64-apple-darwin"
  mg doctor
  [ "$status" -eq 0 ]
  [ "$(field .status)" = warn ]
  jq -e '.details.checks[] | select(.id == "rust" and .status == "warn" and (.message | contains("aarch64-apple-ios-sim")))' "$result"
  [ "$(field '[.user_steps[] | select(.state == "missing" and .required)] | length')" = 0 ]
  assert_read_only_calls
}

# --- core-test (runs cargo in this container) -------------------------------

@test "core-test with no Cargo workspace reports zero tests and passes inside the dev container" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  mg core-test
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .placement)" = docker ]
  [ "$(field .service)" = dev ]
  [ "$(field .summary.total)" = 0 ]
  [ "$(field .details.environment.container)" = dev ]
  [[ "$(field .details.environment.os)" == Linux* ]]
  validate "$result"
}

@test "core-test with an empty workspace reports zero tests and passes" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate empty ""
  mg core-test
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field -c .summary)" = '{"total":0,"passed":0,"failed":0,"skipped":0}' ]
  validate "$result"
}

@test "core-test reports each failing test with a repro command that reruns it" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate fx '#[cfg(test)]
mod tests {
    #[test]
    fn adds() { assert_eq!(1 + 1, 2); }
    #[test]
    fn breaks() { assert_eq!(1 + 1, 3, "deliberate"); }
    #[test]
    #[ignore]
    fn later() {}
}'
  mg core-test
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field -c .summary)" = '{"total":3,"passed":1,"failed":1,"skipped":1}' ]
  [ "$(field '.failures | length')" = 1 ]
  [[ "$(field '.failures[0].id')" == *'$tests::breaks' ]]
  [[ "$(field '.failures[0].message')" == *deliberate* ]]
  local repro
  repro="$(field '.failures[0].repro')"
  [ "$repro" = "tools/mg core-test -- -E 'package(fx) & test(=tests::breaks)'" ]
  jq -e '[.artifacts[] | select(.kind == "nextest-jsonl")] | length == 1' "$result"
  validate "$result"

  run --separate-stderr bash -c "cd '$SB' && $repro"
  [ "$status" -eq 1 ]
  result="${lines[${#lines[@]} - 1]}"
  [ "$(field -c .summary)" = '{"total":1,"passed":0,"failed":1,"skipped":0}' ]
}

@test "core-test fails when a filter matches no tests" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate fx '#[cfg(test)] mod tests { #[test] fn adds() { assert_eq!(1 + 1, 2); } }'
  mg core-test -- -E 'package(fx) & test(=tests::renamed)'
  [ "$status" -ne 0 ]
  [ "$(field .status)" != pass ]
  [ "$(field .summary.total)" -eq 0 ]
  [ "$(field '.failures[0].id')" = no-tests-matched ]
  validate "$result"
}

@test "core-test with a filter fails when core/ has no Cargo workspace yet" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  [ ! -e "$SB/core/Cargo.toml" ]
  mg core-test -- -E 'package(nonexistent)'
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field .summary.total)" -eq 0 ]
  [ "$(field '.failures[0].id')" = no-tests-matched ]
  [ "$(field '.failures[0].repro')" = "tools/mg core-test -- -E 'package(nonexistent)'" ]
  validate "$result"
}

@test "core-test reports a build failure as a failed run" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate broken 'pub fn f() -> u32 { "not a number" }'
  mg core-test
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [[ "$(field '.failures[0].id')" == nextest-exit-* ]]
  validate "$result"
}

@test "core-test runs canary_* test binaries first, alone, and stops when one fails" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate fx '#[cfg(test)] mod tests { #[test] fn unit_breaks() { panic!("unit test ran"); } }'
  write_file core/crates/fx/tests/canary_pins.rs '#[test] fn pin_drifted() { panic!("pin drifted"); }
#[test] fn pin_held() {}'
  write_file core/crates/fx/tests/other.rs '#[test] fn other_breaks() { panic!("other test ran"); }'
  mg core-test
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(failure_ids)" = 'fx::canary_pins$pin_drifted' ]
  [[ "$(field '.failures[0].message')" == *"pin drifted"* ]]
  [ "$(field '.failures[0].repro')" = "tools/mg core-test -- -E 'package(fx) & test(=pin_drifted)'" ]
  [ "$(field -c .summary)" = '{"total":2,"passed":1,"failed":1,"skipped":0}' ]
  [[ "$(field .message)" == *canary* ]]
  jq -e '[.artifacts[] | select(.kind == "nextest-canary-jsonl")] | length == 1' "$result"
  validate "$result"
}

@test "core-test builds only the canary packages first, so a build error elsewhere cannot hide a failing canary" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate fx ''
  write_file core/crates/fx/tests/canary_pins.rs '#[test] fn pin_drifted() { panic!("pin drifted"); }'
  write_crate broken 'pub fn f() -> u32 { "not a number" }'
  mg core-test
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = 'fx::canary_pins$pin_drifted' ]
  [[ "$(field .message)" == *canary* ]]
  validate "$result"

  # Once the canary passes, the rest of the workspace builds and the error shows.
  write_file core/crates/fx/tests/canary_pins.rs '#[test] fn pin_held() {}'
  mg core-test
  [ "$status" -eq 1 ]
  [[ "$(failure_ids)" == nextest-exit-* ]]
  [ "$(field .summary.passed)" = 1 ]
  validate "$result"
}

@test "core-test runs the other tests once the canaries pass, each test once" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  write_workspace
  write_crate fx '#[cfg(test)] mod tests { #[test] fn unit() {} }'
  write_file core/crates/fx/tests/canary_pins.rs '#[test] fn pin_held() {}'
  write_file core/crates/fx/tests/other.rs '#[test] fn other_breaks() { panic!("other test ran"); }'
  mg core-test
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = 'fx::other$other_breaks' ]
  [ "$(field -c .summary)" = '{"total":3,"passed":2,"failed":1,"skipped":0}' ]
  validate "$result"
}

# --- core-lint and core-deny on a copy of the real workspace -----------------

@test "core-lint and core-deny pass without a Cargo workspace" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  mg core-lint
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  validate "$result"
  mg core-deny
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  validate "$result"
}

@test "core-lint fails naming the forbidden edges: tokio in sched, engine in sched, stats in package" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  vendor_crate tokio MIT
  add_dep crates/meguri-sched 'tokio = { path = "../../../vendor/tokio" }'
  add_dep crates/meguri-sched 'meguri-engine.workspace = true'
  add_dep crates/meguri-package 'meguri-stats.workspace = true'
  mg core-lint
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(failure_ids)" = "layering/edge/meguri-package -> meguri-stats
layering/edge/meguri-sched -> meguri-engine
layering/restricted/meguri-sched -> tokio" ]
  local m
  m="$(jq -r '.failures[] | select(.id == "layering/restricted/meguri-sched -> tokio") | .message' "$result")"
  [[ "$m" == *"meguri-sched depends on tokio"* ]]
  m="$(jq -r '.failures[] | select(.id == "layering/edge/meguri-sched -> meguri-engine") | .message' "$result")"
  [[ "$m" == *"edges.toml row 6"*"(row 7)"* ]]
  m="$(jq -r '.failures[] | select(.id == "layering/edge/meguri-package -> meguri-stats") | .message' "$result")"
  [[ "$m" == *"edges.toml row 8"*"(row 8)"*siblings* ]]
  [ "$(field '.failures[0].repro')" = "tools/mg core-lint" ]
  [ "$(field -c .summary)" = '{"total":3,"passed":2,"failed":1,"skipped":0}' ]
  validate "$result"
}

@test "core-lint reports SystemTime::now outside the clock module and unformatted code" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  printf '%s\n' '' 'pub fn stamp() -> std::time::SystemTime {' '    std::time::SystemTime::now()' '}' \
    >>"$SB/core/crates/meguri-text/src/lib.rs"
  printf '%s\n' 'pub fn  spaced ( ) { }' >>"$SB/core/crates/meguri-search/src/lib.rs"
  mg core-lint
  [ "$status" -eq 1 ]
  local ids
  ids="$(failure_ids)"
  [[ "$ids" == *"clippy/clippy::disallowed_methods/crates/meguri-text/src/lib.rs:"* ]]
  [[ "$ids" == *"fmt/crates/meguri-search/src/lib.rs"* ]]
  [ "$(field '.failures | length')" -eq 2 ]
  [[ "$(jq -r '.failures[] | select(.id | startswith("clippy/")) | .message' "$result")" == *"std::time::SystemTime::now"* ]]
  validate "$result"
}

@test "core-deny fails naming the crate and license when a GPL-3.0 crate joins the graph" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  mg core-deny -- licenses
  [ "$status" -eq 0 ] || { jq . "$result"; return 1; }
  vendor_crate gpl-fixture GPL-3.0-only
  add_dep crates/meguri-template 'gpl-fixture = { path = "../../../vendor/gpl-fixture" }'
  mg core-deny -- licenses
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(failure_ids)" = "deny/rejected/gpl-fixture@0.1.0" ]
  [[ "$(field '.failures[0].message')" == *"gpl-fixture@0.1.0"*"GPL-3.0-only"* ]]
  [ "$(field '.failures[0].repro')" = "tools/mg core-deny -- licenses" ]
  validate "$result"
}

@test "core/deny.toml fails an unsound advisory on a crate the workspace reaches only transitively" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  # The workspace reaches ppv-lite86 only through rand -> rand_chacha.
  grep -q '^name = "ppv-lite86"$' "$SB/core/Cargo.lock"
  # A fixture advisory database. core-deny always fetches its databases, so
  # this runs cargo deny --offline, which reads the database for a URL from
  # db-path/<name>-<hash of the URL> (cargo-deny 0.20.2) without fetching.
  local dbs="$BATS_TEST_TMPDIR/advisory-dbs" db
  db="$dbs/advisory-db-e5f9bee2060c5996"
  mkdir -p "$db/crates/ppv-lite86"
  cat >"$db/crates/ppv-lite86/RUSTSEC-2026-9001.md" <<'EOF'
```toml
[advisory]
id = "RUSTSEC-2026-9001"
package = "ppv-lite86"
date = "2026-01-01"
informational = "unsound"
url = "https://fixture.invalid/RUSTSEC-2026-9001"

[versions]
patched = []
```

# Fixture: unsound advisory on a transitive crate

core-deny self-test fixture.
EOF
  git -C "$db" init -q
  git -C "$db" add -A
  git -C "$db" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  sed -i "/^\[advisories\]\$/a db-urls = [\"https://fixture.invalid/advisory-db\"]\ndb-path = \"$dbs\"" "$SB/core/deny.toml"

  run --separate-stderr bash -c "cd '$SB/core' && cargo deny --offline --format json --color never check advisories"
  [ "$status" -eq 1 ] || { echo "$stderr"; return 1; }
  [ "$(grep '^{' <<<"$stderr" | jq -sc '[.[] | select(.type == "diagnostic" and .fields.severity == "error")
    | {code: .fields.code, crate: .fields.graphs[0].Krate.name, id: .fields.advisory.id}]')" \
    = '[{"code":"unsound","crate":"ppv-lite86","id":"RUSTSEC-2026-9001"}]' ]
}

# --- core-fuzz (cargo stubbed; runs as if in the fuzz container) -------------

# fuzz_sandbox: fuzz targets alpha and beta, and a cargo stub. `cargo fuzz run`
# leaves a finding under -artifact_prefix for the targets in $STUB_CRASH and
# exits with $STUB_RUN_EXIT (default 0) for the others; every call is logged.
fuzz_sandbox() {
  local stubs="$BATS_TEST_TMPDIR/fuzz-stubs"
  mkdir -p "$stubs" "$SB/core/fuzz/fuzz_targets"
  : >"$SB/core/fuzz/fuzz_targets/alpha.rs"
  : >"$SB/core/fuzz/fuzz_targets/beta.rs"
  cat >"$stubs/cargo" <<'EOF'
#!/bin/bash
echo "$*" >>"$STUB_LOG"
[ "$2" = fuzz ] || exit 97
case "$3" in
  --version) echo "cargo-fuzz 0.0.0-stub" ;;
  build) exit "${STUB_BUILD_EXIT:-0}" ;;
  run)
    target="$6"
    for a in "$@"; do
      case "$a" in -artifact_prefix=*) prefix="${a#-artifact_prefix=}" ;; esac
    done
    if [[ " ${STUB_CRASH:-} " == *" $target "* ]]; then
      printf 'boom' >"${prefix}crash-0123"
      echo "==1== ERROR: libFuzzer: deadly signal"
      exit 1
    fi
    echo "stat::number_of_executed_units: 42"
    exit "${STUB_RUN_EXIT:-0}" ;;
esac
EOF
  chmod +x "$stubs/cargo"
  export PATH="$stubs:$PATH" MG_CONTAINER=fuzz MG_FUZZ_TOOLCHAIN=nightly-stub STUB_LOG="$BATS_TEST_TMPDIR/cargo.log"
  : >"$STUB_LOG"
}

@test "core-fuzz rejects bad usage before building anything" {
  fuzz_sandbox
  local args
  for args in "--seconds 0" "--seconds 000" "--seconds -5" "--seconds 1.5" "--seconds abc" "--seconds 12345678" \
    "--seconds" "--frobnicate" "gamma" "alpha --seconds 0"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg core-fuzz $args
    [ "$status" -eq 2 ] || { echo "$args"; jq . "$result"; return 1; }
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = usage ]
    validate "$result"
  done
  [ ! -s "$STUB_LOG" ]
}

@test "core-fuzz runs every target for the given seconds and passes without findings" {
  fuzz_sandbox
  mg core-fuzz --seconds 007
  [ "$status" -eq 0 ] || { jq . "$result"; return 1; }
  [ "$(field .summary.total)" -eq 2 ]
  [ "$(field .summary.passed)" -eq 2 ]
  [ "$(field '.details.targets | map("\(.target)=\(.runs)") | join(" ")')" = "alpha=42 beta=42" ]
  [ "$(grep -c '^+nightly-stub fuzz run -O -a .*-max_total_time=7 ' "$STUB_LOG")" -eq 2 ]
  [ "$(grep -c '^+nightly-stub fuzz build -O -a$' "$STUB_LOG")" -eq 1 ]
  [ ! -e "$(dirname "$result")/findings" ]
  validate "$result"

  : >"$STUB_LOG"
  mg core-fuzz beta
  [ "$status" -eq 0 ]
  [ "$(field .summary.total)" -eq 1 ]
  grep -q -- '-max_total_time=600 ' "$STUB_LOG"
  ! grep -q ' alpha ' "$STUB_LOG"
}

@test "core-fuzz fails a target that finds an input, keeping the input, and on a failed run or build" {
  fuzz_sandbox
  export STUB_CRASH=beta
  mg core-fuzz --seconds 5
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(failure_ids)" = "fuzz/beta" ]
  [ "$(field '.failures[0].repro')" = "tools/mg core-fuzz --seconds 5 beta" ]
  [ "$(cat "$(dirname "$result")/findings/beta/crash-0123")" = boom ]
  [ "$(field '[.artifacts[] | select(.kind == "finding")] | length')" -eq 1 ]
  validate "$result"

  export STUB_CRASH="" STUB_RUN_EXIT=3
  mg core-fuzz --seconds 5 alpha
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = "fuzz/alpha-exit-3" ]

  export STUB_RUN_EXIT=0 STUB_BUILD_EXIT=101
  : >"$STUB_LOG"
  mg core-fuzz --seconds 5
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = "fuzz-build" ]
  ! grep -q ' fuzz run ' "$STUB_LOG"
}

# --- fixtures (cargo and meguri-cli stubbed; runs as if in the dev container) --

# fixtures_sandbox: a user shape, worst case `wide`, recipe `tiny`, a cargo
# stub whose build exits $STUB_BUILD_EXIT and whose metadata names a target
# directory holding a meguri-cli stub. The stub logs its arguments, reports
# $STUB_ELAPSED_MS and $STUB_PEAK_KB, exits $STUB_CLI_EXIT, and reports
# `cached` when its --out directory holds a manifest.json.
fixtures_sandbox() {
  local stubs="$BATS_TEST_TMPDIR/fixtures-stubs" target="$BATS_TEST_TMPDIR/fixtures-target"
  local testkit="$SB/core/crates/meguri-testkit"
  mkdir -p "$stubs" "$target/fixtures" "$testkit/profiles/worst-cases" "$testkit/fixtures/recipes"
  printf '{"format": 1}\n' >"$testkit/profiles/user-shape.json"
  printf '{}\n' >"$testkit/profiles/worst-cases/wide.json"
  printf 'recipe: tiny\n' >"$testkit/fixtures/recipes/tiny.yaml"
  cat >"$stubs/cargo" <<STUB
#!/bin/bash
echo "cargo \$*" >>"\$STUB_LOG"
case "\$1" in
  build) exit "\${STUB_BUILD_EXIT:-0}" ;;
  metadata) printf '{"target_directory": "%s"}\n' "$target" ;;
  *) exit 97 ;;
esac
STUB
  cat >"$target/fixtures/meguri-cli" <<'STUB'
#!/bin/bash
echo "meguri-cli $*" >>"$STUB_LOG"
out=""
while [ "$#" -gt 0 ]; do
  [ "$1" = --out ] && out="$2"
  shift
done
cached=false
[ -f "$out/manifest.json" ] && cached=true
mkdir -p "$out" && printf '{}\n' >"$out/manifest.json"
printf '{"dir": "%s", "cached": %s, "elapsed_ms": %s, "peak_rss_kb": %s, "manifest": {"counts": {"cards": 1}}}\n' \
  "$out" "$cached" "${STUB_ELAPSED_MS:-1000}" "${STUB_PEAK_KB:-50000}"
exit "${STUB_CLI_EXIT:-0}"
STUB
  chmod +x "$stubs/cargo" "$target/fixtures/meguri-cli"
  export PATH="$stubs:$PATH" MG_CONTAINER=dev STUB_LOG="$BATS_TEST_TMPDIR/fixtures.log"
  : >"$STUB_LOG"
}

@test "fixtures rejects bad usage before building anything" {
  fixtures_sandbox
  local args
  for args in "" "--seed" "--seed x 10k" "--media all 10k" "--frobnicate 10k" "lots" "10k nosuch"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg fixtures $args
    [ "$status" -eq 2 ] || { echo "$args"; jq . "$result"; return 1; }
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = usage ]
    validate "$result"
  done
  [ ! -s "$STUB_LOG" ]
}

@test "fixtures generates sizes, worst cases and recipes into build/fixtures and reuses them" {
  fixtures_sandbox
  local shape
  shape="$(sha256sum <"$SB/core/crates/meguri-testkit/profiles/user-shape.json" | cut -c1-16)"
  mg fixtures 10k worst-cases recipes --seed 3
  [ "$status" -eq 0 ] || { jq . "$result"; return 1; }
  [ "$(field .summary.total)" -eq 3 ]
  [ "$(field '[.details.fixtures[] | "\(.target)=\(.kind)"] | join(" ")')" = "10k=size wide=worst-case tiny=recipe" ]
  grep -qx "meguri-cli generate 10k --seed 3 --out build/fixtures/$shape-3-10k --cache" "$STUB_LOG"
  grep -qx "meguri-cli generate worst-case wide --seed 3 --out build/fixtures/$shape-3-wide --cache" "$STUB_LOG"
  grep -qx "meguri-cli generate recipe tiny --seed 3 --out build/fixtures/recipes/tiny-3 --cache" "$STUB_LOG"
  [ "$(grep -c '^cargo build --profile fixtures -p meguri-cli$' "$STUB_LOG")" -eq 1 ]
  [ "$(field '[.artifacts[] | select(.kind == "fixture")] | length')" -eq 3 ]
  validate "$result"

  mg fixtures 10k --seed 3
  [ "$status" -eq 0 ]
  [ "$(field '.details.fixtures[0].cached')" = true ]
}

@test "fixtures fails a run over the 10-minute or 2 GB budget, a failed generation and a failed build" {
  fixtures_sandbox
  STUB_ELAPSED_MS=600001 mg fixtures 1m
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = fixtures/1m/time ]
  rm -rf "$SB/build/fixtures"
  STUB_PEAK_KB=2097153 mg fixtures 1m
  [ "$(failure_ids)" = fixtures/1m/memory ]
  STUB_CLI_EXIT=1 mg fixtures 500k
  [ "$(failure_ids)" = fixtures/500k ]
  : >"$STUB_LOG"
  STUB_BUILD_EXIT=101 mg fixtures 10k
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = fixtures-build ]
  ! grep -q '^meguri-cli' "$STUB_LOG"
}

# --- fixtures-check (the fixtures and oracle jobs stubbed) -------------------

# fixtures_check_sandbox: a user shape on day 500, worst cases `due` (a due
# deck, limit 9999) and `big` (large fields), and stubs that replace the
# fixtures and oracle jobs' functions: fixtures reports the summaries in
# $STUB_DIR/summaries.jsonl, oracle reports $STUB_DIR/<dir name>.json. The
# recipes' launch run (app build and xcodebuild) reports $STUB_DIR/launch.json,
# or cannot run with STUB_LAUNCH=broken.
fixtures_check_sandbox() {
  local testkit="$SB/core/crates/meguri-testkit"
  mkdir -p "$testkit/profiles/worst-cases" "$testkit/fixtures/recipes"
  printf '{"totals": {"span_days": 500}}\n' >"$testkit/profiles/user-shape.json"
  printf '{"due_deck": {"review_limit": 9999}}\n' >"$testkit/profiles/worst-cases/due.json"
  printf '{"large_fields": {"bytes": 1048576, "notes": 2}}\n' >"$testkit/profiles/worst-cases/big.json"
  export STUB_DIR="$BATS_TEST_TMPDIR/fixtures-check" STUB_LOG="$BATS_TEST_TMPDIR/fixtures-check.log" MG_CONTAINER=dev
  mkdir -p "$STUB_DIR"
  : >"$STUB_LOG"
  cat >"$SB/tools/mg.d/zz-stub.sh" <<'STUB'
mg_fixtures() {
  echo "fixtures $*" >>"$STUB_LOG"
  mg_detail fixtures "$(jq -sc . "$STUB_DIR/summaries.jsonl")"
  mg_status "${STUB_FIXTURES_STATUS:-pass}"
}
mg_oracle() {
  echo "oracle $*" >>"$STUB_LOG"
  [ -f "$STUB_DIR/$(basename "$2").json" ] || { mg_failure oracle_error "no report"; mg_status error; return 0; }
  mg_detail check "$(jq -c . "$STUB_DIR/$(basename "$2").json")"
  mg_status pass
}
mg__fixtures_launch_run() {
  echo "launch $1" >>"$STUB_LOG"
  [ "${STUB_LAUNCH:-}" != broken ] || { mg_failure fixtures-check/launch "xcodebuild could not run"; return 1; }
  cp "$STUB_DIR/launch.json" "$2.json"
}
STUB
  # One collection of each kind, each as the oracle should see it.
  local good='{"changed_tables": ["col", "config"], "check_db": {"card_properties": 0, "future_ids": 0}, "counts": {"cards": 10, "decks": 2, "graves": 0, "notes": 8, "notetypes": 1, "presets": 2, "revlog": 40, "tags": 3}, "decks": {}, "render": {}, "timing": {"days_elapsed": 500}}'
  local m='{"counts": {"cards": 10, "decks": 2, "notes": 8, "notetypes": 1, "presets": 2, "revlog": 40, "tags": 3}}'
  {
    jq -nc --argjson m "$m" '{target: "10k", kind: "size", dir: "build/fixtures/s-1-10k", manifest: $m}'
    jq -nc --argjson m "$m" '{target: "due", kind: "worst-case", dir: "build/fixtures/s-1-due", manifest: $m}'
    jq -nc --argjson m "$m" '{target: "big", kind: "worst-case", dir: "build/fixtures/s-1-big", manifest: $m, large_field_notes: [123, 456]}'
    jq -nc --argjson m "$m" '{target: "r", kind: "recipe", dir: "build/fixtures/recipes/r-1", manifest: $m}'
  } >"$STUB_DIR/summaries.jsonl"
  jq -c . <<<"$good" >"$STUB_DIR/s-1-10k.json"
  jq -c '.decks = {Due: {learning: 0, new: 0, review: 10}}' <<<"$good" >"$STUB_DIR/s-1-due.json"
  jq -c '.render = {"123": [{answer_bytes: 1048800, card: 124, ord: 0, question_bytes: 30, status: "ok"}]}' <<<"$good" >"$STUB_DIR/s-1-big.json"
  jq -c '.timing.days_elapsed = 120' <<<"$good" >"$STUB_DIR/r-1.json"
  printf '{"result": "Passed", "passed": 1, "failed": 0, "skipped": 0, "messages": []}\n' >"$STUB_DIR/launch.json"
}

@test "fixtures-check runs the oracle check on every collection with the due deck and a large-field note" {
  fixtures_check_sandbox
  mg fixtures-check
  [ "$status" -eq 0 ] || { jq . "$result"; cat "$STUB_LOG"; return 1; }
  [ "$(field .summary.total)" -eq 5 ]
  grep -qx "fixtures 10k 100k worst-cases recipes --seed 1" "$STUB_LOG"
  grep -qx "oracle check $SB/build/fixtures/s-1-10k" "$STUB_LOG"
  grep -qx "oracle check $SB/build/fixtures/s-1-due --deck Due" "$STUB_LOG"
  grep -qx "oracle check $SB/build/fixtures/s-1-big --render-note 123" "$STUB_LOG"
  grep -qx "oracle check $SB/build/fixtures/recipes/r-1" "$STUB_LOG"
  [ "$(field '[.artifacts[] | select(.kind == "oracle-check")] | length')" -eq 4 ]
  # The recipe, launched in the app.
  grep -qx "launch $SB/build/fixtures/recipes/r-1" "$STUB_LOG"
  [ "$(field '[.artifacts[] | select(.kind == "launch")] | length')" -eq 1 ]
  validate "$result"
  mg fixtures-check 10k --seed 4
  grep -qx "fixtures 10k --seed 4" "$STUB_LOG"
}

@test "fixtures-check fails on Check Database problems, changed tables, counts, the day, the due count and the render" {
  fixtures_check_sandbox
  jq -c '.check_db.card_properties = 2 | .changed_tables += ["notes"] | .counts.revlog = 39 | .counts.graves = 1' \
    "$STUB_DIR/s-1-10k.json" >"$STUB_DIR/x" && mv "$STUB_DIR/x" "$STUB_DIR/s-1-10k.json"
  jq -c '.decks.Due.review = 9' "$STUB_DIR/s-1-due.json" >"$STUB_DIR/x" && mv "$STUB_DIR/x" "$STUB_DIR/s-1-due.json"
  jq -c '.render."123"[0].answer_bytes = 1048576' "$STUB_DIR/s-1-big.json" >"$STUB_DIR/x" && mv "$STUB_DIR/x" "$STUB_DIR/s-1-big.json"
  jq -c '.timing.days_elapsed = 121' "$STUB_DIR/r-1.json" >"$STUB_DIR/x" && mv "$STUB_DIR/x" "$STUB_DIR/r-1.json"
  printf '{"result": "Failed", "passed": 0, "failed": 1, "skipped": 0, "messages": ["recipe r: installed files differ"]}\n' >"$STUB_DIR/launch.json"
  mg fixtures-check
  [ "$status" -eq 1 ]
  [ "$(field '.summary.failed')" -eq 5 ]
  local messages
  messages="$(field '[.failures[] | "\(.id): \(.message)"] | join("\n")')"
  grep -qx "fixtures-check/10k: Check Database reported 2 of card_properties" <<<"$messages"
  grep -qx "fixtures-check/10k: Check Database changed the notes table" <<<"$messages"
  grep -qx "fixtures-check/10k: the oracle counts 39 revlog, the manifest 40" <<<"$messages"
  grep -qx "fixtures-check/10k: the oracle counts 1 graves" <<<"$messages"
  grep -qx "fixtures-check/due: deck Due shows 9 reviews, want 10" <<<"$messages"
  grep -qx "fixtures-check/big: card 124 of note 123 rendered ok with 1048576 answer bytes" <<<"$messages"
  grep -qx "fixtures-check/r: the oracle is on day 121, the collection pinned on day 120" <<<"$messages"
  grep -qx "fixtures-check/launch: recipe r: installed files differ" <<<"$messages"
  grep -qx "fixtures-check/launch: the launch suite ended Failed, 1 cases failed" <<<"$messages"
  grep -qx "fixtures-check/launch: 0 launch cases passed for 1 recipes" <<<"$messages"
  validate "$result"
}

@test "fixtures-check fails a skipped launch suite and a launch run that cannot run, and launches nothing without recipes" {
  fixtures_check_sandbox
  printf '{"result": "Skipped", "passed": 0, "failed": 0, "skipped": 1, "messages": []}\n' >"$STUB_DIR/launch.json"
  mg fixtures-check
  [ "$status" -eq 1 ]
  [ "$(field '[.failures[] | "\(.id): \(.message)"] | join("\n")')" = "$(printf '%s\n' \
    "fixtures-check/launch: the launch suite ended Skipped, 0 cases failed" \
    "fixtures-check/launch: 1 launch cases were skipped: a skipped check is no check" \
    "fixtures-check/launch: 0 launch cases passed for 1 recipes")" ]
  STUB_LAUNCH=broken mg fixtures-check
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = fixtures-check/launch ]
  [ "$(field '.summary.failed')" -eq 1 ]
  # No recipe among the collections: no launch run.
  jq -c 'select(.kind != "recipe")' "$STUB_DIR/summaries.jsonl" >"$STUB_DIR/x" && mv "$STUB_DIR/x" "$STUB_DIR/summaries.jsonl"
  : >"$STUB_LOG"
  mg fixtures-check 10k
  [ "$status" -eq 0 ] || { jq . "$result"; return 1; }
  [ "$(field .summary.total)" -eq 3 ]
  ! grep -q '^launch' "$STUB_LOG"
}

@test "fixtures-check stops when generating fails or the oracle gives no report or one it cannot read, and rejects bad usage" {
  fixtures_check_sandbox
  STUB_FIXTURES_STATUS=fail mg fixtures-check
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = fixtures ]
  ! grep -q '^oracle' "$STUB_LOG"
  rm "$STUB_DIR/s-1-due.json"
  jq -c 'if .target == "big" then del(.large_field_notes) else . end' "$STUB_DIR/summaries.jsonl" >"$STUB_DIR/x"
  mv "$STUB_DIR/x" "$STUB_DIR/summaries.jsonl"
  # A report the assertions cannot read is a failure, never a pass.
  jq -c 'del(.changed_tables)' "$STUB_DIR/s-1-10k.json" >"$STUB_DIR/x" && mv "$STUB_DIR/x" "$STUB_DIR/s-1-10k.json"
  jq -c 'del(.check_db)' "$STUB_DIR/r-1.json" >"$STUB_DIR/x" && mv "$STUB_DIR/x" "$STUB_DIR/r-1.json"
  STUB_LAUNCH=broken mg fixtures-check
  [ "$status" -eq 1 ]
  [ "$(field .summary.passed)" -eq 0 ]
  [ "$(failure_ids)" = "$(printf 'fixtures-check/10k/report\nfixtures-check/big/render\nfixtures-check/due/oracle\nfixtures-check/launch\nfixtures-check/r/report')" ]
  local args
  for args in "--seed" "--seed x" "--frobnicate"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg fixtures-check $args
    [ "$status" -eq 2 ] || { echo "$args"; jq . "$result"; return 1; }
    [ "$(field '.failures[0].id')" = usage ]
  done
}

# --- bridge ----------------------------------------------------------------

@test "swift-test --package without a name stops with a usage error" {
  # timeout exits 124 if the job is still parsing its arguments.
  run --separate-stderr timeout 20 "$SB/tools/mg" swift-test --package
  result="${lines[${#lines[@]} - 1]:-}"
  [ "$status" -eq 2 ] || { echo "exit $status; stderr: $stderr"; return 1; }
  [ "$(field .status)" = error ]
  [ "$(failure_ids)" = usage ]
  validate "$result"
}

# --- lint-ui first (AE90) ----------------------------------------------------

# lint_first_sandbox: a card face that applies glass, with SwiftLint, the core
# build and the project check stubbed. Every stubbed call is logged to
# $STUB_LOG. SwiftLint prints the report $STUB_SWIFTLINT_REPORT names; with
# STUB_DOCKER=0 the daemon is down. The core build and the project check fail
# at once, so a run that gets past lint stops there.
lint_first_sandbox() {
  STUBS="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$STUBS"
  export STUB_LOG="$BATS_TEST_TMPDIR/stub.log"
  : >"$STUB_LOG"
  mkdir -p "$SB/app"
  cp "$REPO_ROOT/app/.swiftlint.yml" "$SB/app/"
  write_file app/Packages/MeguriKit/Package.swift '// swift-tools-version: 6.2'
  write_file app/Packages/MeguriKit/Sources/CardRendering/CardFace.swift 'import SwiftUI

struct CardFace: View {
    let html: String

    var body: some View {
        Text(html).glassEffect()
    }
}'
  # The report SwiftLint gives for that file (shape of SwiftLint 0.65.1).
  echo '[{"character":19,"file":"/work/app/Packages/MeguriKit/Sources/CardRendering/CardFace.swift","line":7,"reason":"Liquid Glass and materials belong to the navigation and control layer (R1996, AE90).","rule_id":"glass-outside-chrome","severity":"Error","type":"Glass outside the control layer"}]' \
    >"$BATS_TEST_TMPDIR/glass.json"
  echo '[]' >"$BATS_TEST_TMPDIR/clean.json"
  cat >"$STUBS/docker" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "docker $*" >>"$STUB_LOG"
case " $* " in
  " info ") [ "${STUB_DOCKER:-1}" = 1 ] || exit 1 ;;
  *" swiftlint lint "*)
    cat "$STUB_SWIFTLINT_REPORT"
    # SwiftLint exits 2 when it found error-level violations.
    [ "$(cat "$STUB_SWIFTLINT_REPORT")" = '[]' ] || exit 2 ;;
  *) exit 97 ;;
esac
EOF
  cat >"$STUBS/_stub" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$(basename "$0") $*" >>"$STUB_LOG"
exit 97
EOF
  chmod +x "$STUBS/docker" "$STUBS/_stub"
  local c s
  for c in swift xcodebuild xcrun cargo; do
    ln -s _stub "$STUBS/$c"
  done
  for s in scripts/build-core-xcframework.sh tests/xcode_project_check.sh; do
    cat >"$SB/tools/$s" <<EOF
printf '%s\n' "${s##*/} \$*" >>"\$STUB_LOG"
exit 1
EOF
  done
  export PATH="$STUBS:$PATH"
}

# stub_calls: the logged calls, one word each (the SwiftLint run as
# `swiftlint`), on one line.
stub_calls() {
  sed -E 's/^docker .* swiftlint lint .*/swiftlint/; s/^docker info$/docker-info/; s/ .*//' "$STUB_LOG" | tr '\n' ' '
}

@test "a lint-ui finding stops swift-test and xcode-check before they build anything (AE90)" {
  lint_first_sandbox
  export STUB_SWIFTLINT_REPORT="$BATS_TEST_TMPDIR/glass.json"
  local job lint
  for job in "swift-test --package MeguriKit" "xcode-check --no-ui"; do
    : >"$STUB_LOG"
    # shellcheck disable=SC2086 # job name and its arguments
    mg $job
    [ "$status" -eq 1 ] || { echo "$job: exit $status; stderr: $stderr"; return 1; }
    [ "$(field .status)" = fail ]
    [ "$(failure_ids)" = "lint-ui
lint/glass-outside-chrome" ]
    [ "$(field '.failures[] | select(.id == "lint/glass-outside-chrome") | .repro')" = "tools/mg lint-ui" ]
    # Lint ran, and nothing after it: no core build, no swift, no xcodebuild.
    [ "$(stub_calls)" = "docker-info swiftlint " ] || { echo "$job: $(stub_calls)"; return 1; }
    lint="$(field '.artifacts[] | select(.kind == "lint-ui") | .path')"
    [ "$(jq -r '.job + " " + .status' "$lint")" = "lint-ui fail" ]
    validate "$result" "$lint"
  done
}

@test "with lint-ui clean, swift-test and xcode-check go on to build after it" {
  lint_first_sandbox
  export STUB_SWIFTLINT_REPORT="$BATS_TEST_TMPDIR/clean.json"
  local job
  for job in "swift-test --package MeguriKit" "xcode-check --no-ui"; do
    : >"$STUB_LOG"
    # shellcheck disable=SC2086
    mg $job
    [ "$(failure_ids)" = xcframework ] || { echo "$job: $(failure_ids); stderr: $stderr"; return 1; }
    [ "$(stub_calls)" = "docker-info swiftlint build-core-xcframework.sh " ] || { echo "$job: $(stub_calls)"; return 1; }
    [ "$(jq -r .status "$(field '.artifacts[] | select(.kind == "lint-ui") | .path')")" = pass ]
  done
}

@test "lint-ui that cannot run stops swift-test and xcode-check with an error" {
  lint_first_sandbox
  export STUB_DOCKER=0 STUB_SWIFTLINT_REPORT="$BATS_TEST_TMPDIR/clean.json"
  local job
  for job in "swift-test --package MeguriKit" "xcode-check --no-ui"; do
    : >"$STUB_LOG"
    # shellcheck disable=SC2086
    mg $job
    [ "$status" -eq 2 ] || { echo "$job: exit $status; stderr: $stderr"; return 1; }
    [ "$(field .status)" = error ]
    [ "$(failure_ids)" = "docker-unreachable
lint-ui" ]
    [ "$(stub_calls)" = "docker-info " ]
  done
}

# --- design-docs -------------------------------------------------------------

# design_fixture: a valid inventory, one reference pattern, and the
# screens.md and status ledger it generates.
design_fixture() {
  mkdir -p "$SB/docs/design/references"
  printf '| Pattern | What |\n|---|---|\n| `list-counts` | rows with counts |\n' >"$SB/docs/design/references/index.md"
  cat >"$SB/docs/design/screen-inventory.yaml" <<'YAML'
version: 1
screens:
  - id: home.deckList
    title: Home and deck list
    units: [U152]
    signature: true
    platforms: [iphone, mac]
    states: [empty, typical]
    variants: full
    references: [list-counts]
    purpose: The deck tree with counts.
YAML
  python3 "$SB/tools/scripts/design-screens.py" write --root "$SB"
  (cd "$SB" && python3 tools/scripts/screen-status/screen_status.py init)
}

@test "design-docs passes when screens.md is generated from the inventory" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  design_fixture
  grep -q '| Home and deck list (signature) | `home.deckList` | U152 | iphone, mac | empty, typical | full covering set | `list-counts` |' "$SB/docs/design/screens.md"
  mg design-docs
  [ "$status" -eq 0 ] || { echo "$stderr"; return 1; }
  [ "$(field .status)" = pass ]
  validate "$result"
}

@test "design-docs fails on a hand-edited screens.md and --write repairs it" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  design_fixture
  echo "edited by hand" >>"$SB/docs/design/screens.md"
  mg design-docs
  [ "$status" -eq 1 ]
  [ "$(failure_ids)" = design-docs/screens.md ]
  mg design-docs --write
  [ "$status" -eq 0 ]
  mg design-docs
  [ "$status" -eq 0 ]
}

@test "design-docs fails when the status ledger lacks an inventory screen and --write adds it" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  design_fixture
  printf 'version: 1\nscreens: {}\n' >"$SB/docs/design/screen-status.yaml"
  mg design-docs
  [ "$status" -eq 1 ]
  field '.failures[].message' | grep -qF 'home.deckList is in screen-inventory.yaml but not in docs/design/screen-status.yaml'
  mg design-docs --write
  [ "$status" -eq 0 ]
  grep -q '^  home.deckList:$' "$SB/docs/design/screen-status.yaml"
  grep -q '^    status: sketch$' "$SB/docs/design/screen-status.yaml"
}

@test "design-docs rejects a status, an unknown state, an unknown reference and a pairwise signature screen" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  design_fixture
  sed -i 's/^    variants: full/    variants: pairwise\n    status: built/; s/\[empty, typical\]/[empty, glorious]/; s/\[list-counts\]/[list-counts, tab-bar]/' \
    "$SB/docs/design/screen-inventory.yaml"
  mg design-docs
  [ "$status" -eq 1 ]
  local messages
  messages="$(field '.failures[].message')"
  grep -qF 'home.deckList: status lives only in docs/design/screen-status.yaml' <<<"$messages"
  grep -qF 'home.deckList: states must be a non-empty list from' <<<"$messages"
  grep -qF "home.deckList: reference 'tab-bar' is not in docs/design/references/index.md" <<<"$messages"
  grep -qF 'home.deckList: a signature screen uses the full covering set (R1972)' <<<"$messages"
}

# --- images ----------------------------------------------------------------

@test "dev-image reports an error when image inspection fails or returns an empty ID" {
  local stubs="$BATS_TEST_TMPDIR/image-stubs" image mode
  mkdir -p "$stubs"
  cat >"$stubs/docker" <<'EOF'
#!/usr/bin/env bash
case "$*" in
  info | compose*" build dev" | build*) exit 0 ;;
  "image inspect "*)
    if [[ "$3" == *"-$STUB_BAD_IMAGE" ]]; then
      [ "$STUB_INSPECT_MODE" = empty ] && exit 0
      echo sha256:fixture
      echo "image inspection failed" >&2
      exit 1
    fi
    echo sha256:fixture ;;
  *) exit 97 ;;
esac
EOF
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  for image in dev fuzz; do
    for mode in fail empty; do
      export STUB_BAD_IMAGE="$image" STUB_INSPECT_MODE="$mode"
      mg dev-image
      [ "$status" -eq 2 ]
      [ "$(field .status)" = error ]
      [ "$(field .exit_code)" -eq 2 ]
      [ "$(field '.failures[0].id')" = image-inspect ]
      validate "$result"
    done
  done
}

# dockerfiles: sets `files` to docker/'s Dockerfiles, plus oracle/'s when this
# checkout has oracle/ (implementer worktrees do not).
dockerfiles() {
  files=("$SB"/docker/*.Dockerfile)
  [ ! -f "$REPO_ROOT/oracle/Dockerfile" ] || files[${#files[@]}]="$REPO_ROOT/oracle/Dockerfile"
}

@test "every Docker base image is pinned by digest and the dev image matches the Rust toolchain file" {
  local f line image stages files
  dockerfiles
  for f in "${files[@]}"; do
    [ -f "$f" ] || { echo "$f does not exist"; return 1; }
    stages=" "
    while read -r line; do
      image="$(awk '{print $2}' <<<"$line")"
      if [[ "$stages" != *" $image "* ]] && [ "$image" != scratch ]; then
        [[ "$image" =~ @sha256:[0-9a-f]{64}$ ]] || { echo "$f: unpinned base image $image"; return 1; }
      fi
      [[ "$line" =~ [[:space:]][Aa][Ss][[:space:]]+([^[:space:]]+) ]] && stages="$stages${BASH_REMATCH[1]} "
    done < <(grep -E '^FROM[[:space:]]' "$f")
  done
  local base channel
  base="$(sed -n 's/^FROM rust:\([0-9.]*\)-slim@.*/\1/p' "$SB/docker/dev.Dockerfile" | head -n 1)"
  channel="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$SB/core/rust-toolchain.toml")"
  [ -n "$base" ]
  [ "${channel%.*}" = "$base" ]
}

@test "every Dockerfile's build context is an allowlist that starts by excluding everything" {
  local f ignore first files
  dockerfiles
  for f in "${files[@]}"; do
    [ -f "$f" ] || { echo "$f does not exist"; return 1; }
    ignore="$f.dockerignore"
    [ -f "$ignore" ] || { echo "$f has no $(basename "$ignore"); the repo root holds local-data/"; return 1; }
    first="$(grep -Ev '^[[:space:]]*(#|$)' "$ignore" | head -n 1)"
    [ "$first" = "*" ] || { echo "$ignore must start with '*', found '$first'"; return 1; }
    if grep -Eq '^!.*(local-data|refs)' "$ignore"; then
      echo "$ignore re-includes a private path"
      return 1
    fi
  done
}

@test "the Dockerfile checks pass, and check every file, in a checkout without oracle/" {
  # Implementer worktrees have no oracle/ (U2's sparse checkout). $SB has none
  # either, so a nested run of the two tests from $SB checks that case.
  [ ! -e "$SB/oracle" ]
  run --separate-stderr bats --formatter tap --filter 'allowlist|pinned by digest' "$SB/tools/tests/mg_selftest.bats"
  echo "$output"
  [ "$status" -eq 0 ]
  [ "$(grep -c '^ok ' <<<"$output")" -eq 2 ]
}

# --- oracle -----------------------------------------------------------------

@test "oracle rejects bad usage before touching Docker" {
  local stubs="$BATS_TEST_TMPDIR/oracle-stubs" args
  mkdir -p "$stubs"
  printf '#!/bin/sh\necho "docker must not run" >&2\nexit 97\n' >"$stubs/docker"
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  for args in "" "frobnicate" "bump" "bump 26.x" "record --jobs 0" "hello --verbose" "build extra" \
    "check" "check a b" "check --bogus a" "check a --now" "hello --platform linux" "compare --platform x" \
    "lock --platform" "lock --platform windows" "lock extra" "time" "time a b" "time --bogus a" "time a --op" \
    "time a --platform windows" "time a --platform"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg oracle $args
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = usage ]
    [[ "$stderr" != *"docker must not run"* ]]
    validate "$result"
  done
}

@test "oracle check and time report a missing collection before touching Docker" {
  local stubs="$BATS_TEST_TMPDIR/oracle-stubs"
  mkdir -p "$stubs" "$BATS_TEST_TMPDIR/empty"
  printf '#!/bin/sh\necho "docker must not run" >&2\nexit 97\n' >"$stubs/docker"
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  for dir in "$BATS_TEST_TMPDIR/empty" "$BATS_TEST_TMPDIR/none"; do
    for job in check time; do
      mg oracle "$job" "$dir" --tz UTC
      [ "$status" -eq 2 ]
      [ "$(field .status)" = error ]
      [ "$(field '.failures[0].id')" = no_collection ]
      [[ "$stderr" != *"docker must not run"* ]]
      validate "$result"
    done
  done
}

@test "oracle test, bump and lock refuse in a checkout without oracle/ and copy nothing" {
  local stubs="$BATS_TEST_TMPDIR/oracle-stubs" impl="$BATS_TEST_TMPDIR/impl" args
  mkdir -p "$stubs" "$SB/oracle/driver"
  printf '#!/bin/sh\necho "docker must not run" >&2\nexit 97\n' >"$stubs/docker"
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  # The main worktree ($SB) has an oracle/ tree; the linked worktree has none.
  printf 'FROM python:3.12-slim\n' >"$SB/oracle/Dockerfile"
  printf 'anki==26.9.3\n' >"$SB/oracle/requirements.in"
  printf 'anki==26.9.3 \\\n' >"$SB/oracle/requirements.lock"
  : >"$SB/oracle/Dockerfile.dockerignore"
  : >"$SB/oracle/entrypoint.sh"
  printf '# driver\n' >"$SB/oracle/driver/worker.py"
  git -C "$SB" worktree add -q "$impl"
  [ ! -e "$impl/oracle" ]
  for args in "test" "test Logs" "bump 26.10.0" "lock"; do
    # shellcheck disable=SC2086 # each case is a word list
    run --separate-stderr "$impl/tools/mg" oracle $args
    result="${lines[${#lines[@]} - 1]:-}"
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = no-oracle-tree ]
    [[ "$stderr" != *"docker must not run"* ]]
    validate "$result"
  done
  [ -z "$(find "$impl/build" -name worker.py)" ]
}

@test "a checkout without oracle/ builds the main worktree's only when its branch has no other oracle/" {
  local stubs="$BATS_TEST_TMPDIR/oracle-stubs" impl="$BATS_TEST_TMPDIR/impl" same="$BATS_TEST_TMPDIR/same" args
  local git=(git -c user.name=selftest -c user.email=selftest@localhost)
  mkdir -p "$stubs" "$SB/oracle/driver"
  cat >"$stubs/docker" <<'STUB'
#!/bin/bash
printf '%s\n' "$*" >>"$STUB_LOG"
case "$1" in
  info) exit 0 ;;
  image) echo sha256:main; exit 0 ;;
  compose) case " $* " in *" build oracle "*) exit 0 ;; esac ;;
esac
exit 97
STUB
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH" STUB_LOG="$BATS_TEST_TMPDIR/docker.log"
  printf 'FROM python:3.12-slim\n' >"$SB/oracle/Dockerfile"
  printf 'anki==26.9.3\n' >"$SB/oracle/requirements.in"
  printf 'anki==26.9.3 \\\n' >"$SB/oracle/requirements.lock"
  : >"$SB/oracle/Dockerfile.dockerignore"
  : >"$SB/oracle/entrypoint.sh"
  printf '# driver\n' >"$SB/oracle/driver/worker.py"
  "${git[@]}" -C "$SB" add oracle
  "${git[@]}" -C "$SB" commit -qm "add the oracle"
  # Two branches checked out without oracle/, like implementer worktrees: one
  # whose spec side changed the driver, one that did not touch oracle/.
  "${git[@]}" -C "$SB" worktree add -q -b unit/changed "$impl"
  printf '# a new op\n' >>"$impl/oracle/driver/worker.py"
  "${git[@]}" -C "$impl" commit -qam "change the driver"
  rm -rf "$impl/oracle"
  "${git[@]}" -C "$SB" worktree add -q -b unit/same "$same"
  rm -rf "$same/oracle"

  # The main worktree's driver is not the changed branch's: refuse rather
  # than run (or record goldens) on another oracle.
  for args in "build" "hello"; do
    : >"$STUB_LOG"
    # shellcheck disable=SC2086 # each case is a word list
    run --separate-stderr "$impl/tools/mg" oracle $args
    result="${lines[${#lines[@]} - 1]:-}"
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = oracle-tree-differs ]
    [[ "$(field '.failures[0].message')" == *"this branch changes oracle/"* ]]
    ! grep -q -- ' build oracle' "$STUB_LOG"
    validate "$result"
  done

  run --separate-stderr "$same/tools/mg" oracle build
  result="${lines[${#lines[@]} - 1]:-}"
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"using the main worktree's"* ]]
  validate "$result"

  # Once the main worktree has the branch's oracle/, the branch builds it.
  "${git[@]}" -C "$SB" merge -q --no-edit unit/changed
  run --separate-stderr "$impl/tools/mg" oracle build
  result="${lines[${#lines[@]} - 1]:-}"
  [ "$status" -eq 0 ]
  validate "$result"
}

# A docker stub for `oracle bump`: the lock resolver writes a lock, builds and
# hello succeed, and the harness writes $STUB_RECORD and $STUB_DIFF as the
# record and diff reports.
bump_fixture() {
  local stubs="$BATS_TEST_TMPDIR/bump-stubs"
  mkdir -p "$stubs" "$SB/oracle/driver" "$SB/conformance/scenarios/smoke" "$SB/conformance/goldens/smoke"
  printf 'FROM python:3.12-slim@sha256:%064d\n' 0 >"$SB/oracle/Dockerfile"
  printf 'anki==26.9.3\n' >"$SB/oracle/requirements.in"
  printf 'anki==26.9.3 \\\n    --hash=sha256:%064d\n' 0 >"$SB/oracle/requirements.lock"
  printf '*\n' >"$SB/oracle/Dockerfile.dockerignore"
  printf '#!/bin/sh\n' >"$SB/oracle/entrypoint.sh"
  printf '# driver\n' >"$SB/oracle/driver/worker.py"
  cat >"$stubs/docker" <<'STUB'
#!/bin/bash
out="" lock="" prev=""
for a in "$@"; do
  if [ "$prev" = -v ]; then
    case "$a" in
      *:/out) out="${a%:/out}" ;;
      *:/lock) lock="${a%:/lock}" ;;
    esac
  fi
  prev="$a"
done
case "$1" in
  info | ps) exit 0 ;;
  image) echo sha256:new; exit 0 ;;
  run)
    case " $* " in
      *aarch64-apple-darwin*) mkdir -p "$lock/macos" && printf 'anki==26.10.0 \\\n    --hash=sha256:%064d\n' 2 >"$lock/macos/requirements-macos.lock" ;;
      *) printf 'anki==26.10.0 \\\n    --hash=sha256:%064d\n' 1 >"$lock/requirements.lock" ;;
    esac
    exit 0 ;;
  compose) ;;
  *) exit 97 ;;
esac
case " $* " in
  *" build oracle "*) exit 0 ;;
  *" harness record "*) printf '%s\n' "$STUB_RECORD" >"$out/record-report.json"; exit "${STUB_RECORD_RC:-0}" ;;
  *" harness diff-report "*)
    printf '%s\n' "$STUB_DIFF" >"$out/bump-report.json"
    echo '# report' >"$out/bump-report.md"
    exit 0 ;;
  *" oracle ") cat >/dev/null; echo '{"ok":true,"protocol":1,"anki_version":"26.10.0","lock_sha256":"l","driver_sha256":"d","tz":"UTC"}'; exit 0 ;;
esac
exit 97
STUB
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
}

# record_report <scenario>=<status>...
record_report() {
  jq -nc '[$ARGS.positional[] | split("=") | {scenario: .[0], status: .[1]} + (if .[1] == "error" then {error: {kind: "anki_error"}} else {} end)]
    | {command: "record", totals: (group_by(.status) | map({(.[0].status): length}) | add // {}), scenario_count: length, scenarios: .}' --args "$@"
}

# diff_report <golden>=<status>...
diff_report() {
  jq -nc '[$ARGS.positional[] | split("=") | {golden: .[0], area: "smoke", status: .[1]}]
    | {oracle: {old: null, new: null}, totals: (group_by(.status) | map({(.[0].status): length}) | add // {}),
       changed_observations: [], scenarios: .}' --args "$@"
}

@test "oracle bump passes only when every scenario and golden was re-recorded" {
  bump_fixture
  # Every scenario re-recorded, one golden changed: adopt after review.
  STUB_RECORD="$(record_report smoke/a=recorded)" STUB_DIFF="$(diff_report smoke/a.json=changed)"
  export STUB_RECORD STUB_DIFF STUB_RECORD_RC=0
  mg oracle bump 26.10.0
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .summary.changed)" -eq 1 ]
  validate "$result"
  local run_dir
  run_dir="$(dirname "$result")"
  # The staged requirements stay for adoption; no copy of the driver does.
  [ -f "$run_dir/staging/oracle/requirements.lock" ] && [ -f "$run_dir/staging/oracle/requirements.in" ]
  # The conformance pin is staged from the new oracle's hello, with the
  # native lane's lock resolved for the same versions.
  [ "$(jq -r '.anki_version + " " + .lock_sha256' "$run_dir/staging/conformance/oracle.json")" = "26.10.0 l" ]
  [ -f "$run_dir/staging/oracle/macos/requirements-macos.lock" ]
  [ "$(jq -r .macos_lock_sha256 "$run_dir/staging/conformance/oracle.json")" = \
    "$(sha256sum "$run_dir/staging/oracle/macos/requirements-macos.lock" | cut -d' ' -f1)" ]
  [ -z "$(find "$run_dir" -name worker.py -o -name Dockerfile -o -name entrypoint.sh)" ]

  # No scenario recorded: nothing proves the new oracle works.
  STUB_RECORD="$(record_report)" STUB_DIFF="$(diff_report smoke/a.json=not_recorded)" STUB_RECORD_RC=1
  mg oracle bump 26.10.0
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | index("no-scenarios") != null')" = true ]
  validate "$result"

  # A golden without a scenario was not re-recorded.
  STUB_RECORD="$(record_report smoke/a=recorded)" STUB_RECORD_RC=0
  STUB_DIFF="$(diff_report smoke/a.json=unchanged smoke/gone.json=not_recorded)"
  mg oracle bump 26.10.0
  [ "$status" -eq 1 ]
  [ "$(field -c '[.failures[].id]')" = '["smoke/gone"]' ]
  validate "$result"

  # A scenario that failed to record is reported once, not again as its golden.
  STUB_RECORD="$(record_report smoke/a=error smoke/b=recorded)" STUB_RECORD_RC=1
  STUB_DIFF="$(diff_report smoke/a.json=not_recorded smoke/b.json=unchanged)"
  mg oracle bump 26.10.0
  [ "$status" -eq 1 ]
  [ "$(field -c '[.failures[].id]')" = '["smoke/a"]' ]
  [ "$(field .summary.failed)" -eq 1 ]
  validate "$result"
}

# Actor scenarios (specs/conformance/sync-ops.md S8.2, S9): a docker stub that
# logs every call to $STUB_LOG, reports every container running and healthy,
# lets a sync-recorder container write $STUB_EXCHANGES to its recording
# directory and answer $STUB_HEALTH, and makes the harness write $STUB_REPORT
# as its report and exit $STUB_RC. A cargo stub stands in for meguri-cli: it
# writes $STUB_PLAN as the T0 plan report, and `conformance to-json` answers
# each scenario file with the value $STUB_PARSED gives its id, as the scenario
# parser would read it; with $STUB_CARGO_RC set it prints nothing and exits
# with that code. The tree has one actor scenario (smoke/sync/pair) and one
# other (smoke/plain).
actor_fixture() {
  local stubs="$BATS_TEST_TMPDIR/actor-stubs"
  mkdir -p "$stubs" "$SB/oracle/driver" "$SB/conformance/scenarios/smoke/sync" "$SB/conformance/goldens/smoke/sync"
  printf 'FROM python:3.12-slim@sha256:%064d\n' 0 >"$SB/oracle/Dockerfile"
  printf 'anki==26.9.3\n' >"$SB/oracle/requirements.in"
  printf 'anki==26.9.3 \\\n    --hash=sha256:%064d\n' 0 >"$SB/oracle/requirements.lock"
  printf '*\n' >"$SB/oracle/Dockerfile.dockerignore"
  printf '#!/bin/sh\n' >"$SB/oracle/entrypoint.sh"
  printf '# driver\n' >"$SB/oracle/driver/worker.py"
  printf 'id: smoke/sync/pair\nclock: {tz: UTC, start: "2030-01-15T10:00:00"}\nactors: [client_a, client_c]\n' \
    >"$SB/conformance/scenarios/smoke/sync/pair.yaml"
  printf 'id: smoke/plain\nclock: {tz: UTC, start: "2030-01-15T10:00:00"}\n' >"$SB/conformance/scenarios/smoke/plain.yaml"
  cat >"$stubs/docker" <<'STUB'
#!/bin/bash
printf '%s\n' "$*" >>"$STUB_LOG"
out="" report="" prev=""
for a in "$@"; do
  [ "$prev" = -v ] && case "$a" in *:/out) out="${a%:/out}" ;; esac
  [ "$prev" = --report ] && report="$a"
  prev="$a"
done
case "$1" in
  info | ps | rm) exit 0 ;;
  image) echo sha256:new; exit 0 ;;
  inspect) echo "true healthy"; exit 0 ;;
  exec) printf '%s\n' "$STUB_HEALTH"; exit 0 ;;
  compose) ;;
  *) exit 97 ;;
esac
case " $* " in
  *" build oracle "*) exit 0 ;;
  *" up "*" sync-faults "*) exit 0 ;;
  *" run -d "*" sync-recorder "*) printf '%s' "$STUB_EXCHANGES" >"$MEGURI_RECORD_DIR/exchanges.jsonl"; exit 0 ;;
  *" harness "*) printf '%s\n' "$STUB_REPORT" >"$out${report#/out}"; exit "${STUB_RC:-0}" ;;
esac
exit 97
STUB
  cat >"$stubs/cargo" <<'STUB'
#!/bin/bash
[ -z "${STUB_CARGO_RC:-}" ] || exit "$STUB_CARGO_RC"
prev="" files=false rc=0 rel=""
for a in "$@"; do
  if [ "$files" = true ]; then
    rel="${a#*/conformance/scenarios/}"
    jq -nc --arg p "$a" --arg id "${rel%.*}" --argjson parsed "$STUB_PARSED" \
      'if $parsed | has($id) then {path: $p, value: $parsed[$id]} else {path: $p, error: "not parsed"} end'
    jq -e --arg id "${rel%.*}" 'has($id)' <<<"$STUB_PARSED" >/dev/null || rc=1
    continue
  fi
  [ "$a" = to-json ] && files=true
  [ "$prev" = --report ] && printf '%s\n' "$STUB_PLAN" >"$a"
  prev="$a"
done
exit "$rc"
STUB
  chmod +x "$stubs/docker" "$stubs/cargo"
  export PATH="$stubs:$PATH" STUB_LOG="$BATS_TEST_TMPDIR/docker.log"
  export STUB_PARSED='{"smoke/sync/pair": {"id": "smoke/sync/pair", "actors": ["client_a", "client_c"]},
    "smoke/plain": {"id": "smoke/plain"}}'
  : >"$STUB_LOG"
}

# harness_report <command> <scenario>=<status>...
harness_report() {
  local cmd="$1"
  shift
  jq -nc --arg cmd "$cmd" '[$ARGS.positional[] | split("=") | {scenario: .[0], status: .[1]}]
    | {command: $cmd, oracle: {anki_version: "26.9.3", driver_sha256: ("d" * 64), lock_sha256: ("l" * 64)},
       totals: (group_by(.status) | map({(.[0].status): length}) | add // {}), scenario_count: length,
       jobs: 1, wall_s: 1, mean_worker_ms: 1, scenarios: .}' --args "$@"
}

@test "oracle record and compare start the fault proxy only for actor scenarios" {
  actor_fixture
  export STUB_REPORT
  STUB_REPORT="$(harness_report record smoke/plain=recorded)"
  mg oracle record smoke/plain
  [ "$status" -eq 0 ]
  ! grep -q 'sync-faults' "$STUB_LOG"
  ! grep -q 'ORACLE_SYNC_FAULTS' "$STUB_LOG"
  validate "$result"

  : >"$STUB_LOG"
  STUB_REPORT="$(harness_report compare smoke/sync/pair=pass)"
  mg oracle compare smoke/sync
  [ "$status" -eq 0 ]
  # The checkout's toxiproxy, healthy (--wait) before the harness starts;
  # the workers get its API, and it stays up for other runs.
  grep -q -- "--profile oracle-sync up --detach --wait --wait-timeout 60 sync-faults" "$STUB_LOG"
  [ "$(grep -n -- ' harness compare ' "$STUB_LOG" | cut -d: -f1)" -gt "$(grep -n -- " up --detach --wait " "$STUB_LOG" | cut -d: -f1)" ]
  grep -q -- "-e ORACLE_SYNC_FAULTS=sync-faults:8474 .* harness compare" "$STUB_LOG"
  ! grep -q -- "^rm " "$STUB_LOG"
  validate "$result"
}

@test "oracle runs read actor scenarios with the scenario parser, however actors is written" {
  actor_fixture
  # A quoted key and a flow mapping parse to the same top-level `actors`
  # member as `actors:`, and hash alike.
  printf '%s\n' 'id: smoke/sync/quoted' 'clock: {tz: UTC, start: "2030-01-15T10:00:00"}' "'actors': [client_a]" \
    >"$SB/conformance/scenarios/smoke/sync/quoted.yaml"
  printf '%s\n' '{id: smoke/sync/flow, clock: {tz: UTC, start: "2030-01-15T10:00:00"}, actors: [client_a]}' \
    >"$SB/conformance/scenarios/smoke/sync/flow.yaml"
  STUB_PARSED="$(jq -c '. + {"smoke/sync/quoted": {id: "smoke/sync/quoted", actors: ["client_a"]},
    "smoke/sync/flow": {id: "smoke/sync/flow", actors: ["client_a"]}}' <<<"$STUB_PARSED")"
  export STUB_PARSED STUB_REPORT
  local id
  for id in smoke/sync/quoted smoke/sync/flow; do
    : >"$STUB_LOG"
    STUB_REPORT="$(harness_report compare "$id=pass")"
    mg oracle compare "$id"
    [ "$status" -eq 0 ]
    grep -q -- "-e ORACLE_SYNC_FAULTS=sync-faults:8474 .* harness compare .*--filter $id" "$STUB_LOG"
    validate "$result"
  done

  # A parser that cannot run leaves the actor scenarios unknown: an error
  # before the image build, not a run without the fault proxy.
  : >"$STUB_LOG"
  export STUB_CARGO_RC=101
  mg oracle compare smoke/sync
  unset STUB_CARGO_RC
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = scenario-parse ]
  ! grep -q -- ' build oracle' "$STUB_LOG"
  ! grep -q -- ' harness ' "$STUB_LOG"
  validate "$result"
}

@test "oracle recordings rejects bad usage and non-actor scenarios before touching Docker" {
  actor_fixture
  local args
  for args in "recordings" "recordings --jobs 2 smoke/sync/pair"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg oracle $args
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
    validate "$result"
  done
  mg oracle recordings smoke/plain
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = no-scenarios ]
  [ ! -s "$STUB_LOG" ]
  validate "$result"
}

@test "oracle recordings keeps a recording only when the run reproduced its golden and every exchange was recorded" {
  actor_fixture
  local dest="$SB/conformance/recordings/scenarios/smoke/sync/pair" sha
  sha="$(printf 'a%.0s' $(seq 64))"
  export STUB_PLAN STUB_REPORT STUB_HEALTH STUB_EXCHANGES STUB_RC=0
  STUB_PLAN="$(jq -nc --arg sha "$sha" '{tier: "t0", scenarios: [{scenario: "smoke/sync/pair", status: "live", scenario_sha256: $sha}]}')"
  STUB_REPORT="$(harness_report compare smoke/sync/pair=pass)"
  STUB_HEALTH='{"ok": true, "written": 2, "errors": 0, "last_error": null}'
  STUB_EXCHANGES=$'{"actor":"client_a","seq":1,"step":1}\n{"actor":"client_c","seq":2,"step":7}\n'
  mg oracle recordings smoke/sync
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .summary.passed)" -eq 1 ]
  validate "$result"
  [ "$(cat "$dest/exchanges.jsonl")" = "$(printf '%s' "$STUB_EXCHANGES")" ]
  # recording.json: sorted keys, two-space indents, a final newline (S9).
  [ "$(cat "$dest/recording.json")" = "$(jq -nS --indent 2 --arg sha "$sha" \
    '{oracle: {anki_version: "26.9.3", driver_sha256: ("d" * 64), lock_sha256: ("l" * 64)},
      scenario_id: "smoke/sync/pair", scenario_sha256: $sha}')" ]
  [ "$(tail -c 1 "$dest/recording.json" | od -An -c | tr -d ' ')" = '\n' ]
  # The recorder was this run's and is gone; the harness compared one
  # scenario at a time through it.
  local run_id
  run_id="$(field -r .run_id | tr 'A-Z' 'a-z')"
  grep -q -- "--profile oracle-record run -d --no-deps --name mg-sync-recorder-$run_id-1 --label meguri.mg.run-id=$(field -r .run_id) sync-recorder" "$STUB_LOG"
  grep -q -- "-e ORACLE_SYNC_FAULTS=sync-faults:8474 -e ORACLE_SYNC_RECORDER=mg-sync-recorder-$run_id-1:8080 .* harness compare .*--jobs 1 .*--filter smoke/sync/pair" "$STUB_LOG"
  grep -q -- "^rm -f mg-sync-recorder-$run_id-1$" "$STUB_LOG"

  # A recorder that missed an exchange: the old recording stays as it was.
  STUB_HEALTH='{"ok": false, "written": 1, "errors": 1, "last_error": "no prefix"}'
  STUB_EXCHANGES='{"seq":1}'
  mg oracle recordings smoke/sync/pair
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].id')" = smoke/sync/pair ]
  [[ "$(field '.failures[0].message')" == *"did not record every exchange"* ]]
  [ "$(wc -l <"$dest/exchanges.jsonl" | tr -d ' ')" -eq 2 ]
  validate "$result"

  # A run that differs from the golden records nothing either.
  STUB_HEALTH='{"ok": true, "written": 1, "errors": 0, "last_error": null}'
  STUB_REPORT="$(harness_report compare smoke/sync/pair=fail)" STUB_RC=1
  mg oracle recordings smoke/sync/pair
  [ "$status" -eq 1 ]
  [[ "$(field '.failures[0].message')" == *"did not reproduce the golden"* ]]
  [ "$(wc -l <"$dest/exchanges.jsonl" | tr -d ' ')" -eq 2 ]
  validate "$result"

  # A stale golden: nothing runs.
  : >"$STUB_LOG"
  STUB_PLAN="$(jq -nc '{tier: "t0", scenarios: [{scenario: "smoke/sync/pair", status: "stale"}]}')"
  mg oracle recordings smoke/sync/pair
  [ "$status" -eq 1 ]
  [ "$(field '.failures[0].repro')" = "tools/mg oracle record smoke/sync/pair" ]
  ! grep -q ' harness ' "$STUB_LOG"
  validate "$result"
}

@test "conformance jobs reject bad usage before touching Docker" {
  local stubs="$BATS_TEST_TMPDIR/conf-stubs" job args
  mkdir -p "$stubs"
  printf '#!/bin/sh\necho "docker must not run" >&2\nexit 97\n' >"$stubs/docker"
  printf '#!/bin/sh\necho "cargo must not run" >&2\nexit 97\n' >"$stubs/cargo"
  chmod +x "$stubs/docker" "$stubs/cargo"
  export PATH="$stubs:$PATH"
  for args in "conformance --lane moon" "conformance --jobs 0" "conformance --frobnicate" \
    "conformance --live --lane docker" "goldens-check --base no-such-revision" "goldens-check extra" \
    "harvest-report extra" "harvest-report --live-report" "harvest-report --live-report /etc/hosts" \
    "harvest-inventory --bogus"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg $args
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = usage ]
    [[ "$stderr" != *"must not run"* ]]
    validate "$result"
  done
}

@test "fp-probe rejects bad usage, and any host but an arm64 Mac, before touching Docker" {
  local stubs="$BATS_TEST_TMPDIR/fp-stubs" args
  mkdir -p "$stubs"
  printf '#!/bin/sh\necho "docker must not run" >&2\nexit 97\n' >"$stubs/docker"
  printf '#!/bin/sh\necho "cargo must not run" >&2\nexit 97\n' >"$stubs/cargo"
  chmod +x "$stubs/docker" "$stubs/cargo"
  export PATH="$stubs:$PATH"
  for args in "--jobs 0" "--jobs" "--jobs x" "extra"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg fp-probe $args
    [ "$status" -eq 2 ]
    [ "$(field '.failures[0].id')" = usage ]
    [[ "$stderr" != *"must not run"* ]]
    validate "$result"
  done
  # The self-tests run in the Linux dev container: no native Apple sides.
  mg fp-probe
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = platform ]
  [[ "$stderr" != *"must not run"* ]]
  validate "$result"
}

@test "harvest-inventory refuses in a checkout without oracle/" {
  local stubs="$BATS_TEST_TMPDIR/conf-stubs"
  mkdir -p "$stubs"
  printf '#!/bin/sh\necho "docker must not run" >&2\nexit 97\n' >"$stubs/docker"
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  mg harvest-inventory
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = no-oracle-tree ]
  [[ "$stderr" != *"must not run"* ]]
  validate "$result"
}

# --- sync-rig ----------------------------------------------------------------

@test "sync-rig rejects bad usage before touching Docker" {
  local stubs="$BATS_TEST_TMPDIR/sync-stubs" args
  mkdir -p "$stubs"
  printf '#!/bin/sh\necho "docker must not run" >&2\nexit 97\n' >"$stubs/docker"
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  for args in "" "frobnicate" "up extra" "up --record" "up --record Bad_Name" "up --record -x" \
    "down extra" "status extra"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg sync-rig $args
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = usage ]
    [[ "$stderr" != *"docker must not run"* ]]
    validate "$result"
  done
}

@test "sync-rig up never appends to an existing recording" {
  local stubs="$BATS_TEST_TMPDIR/sync-stubs"
  mkdir -p "$stubs" "$SB/conformance/recordings/sync/taken"
  printf '#!/bin/sh\necho "docker must not run" >&2\nexit 97\n' >"$stubs/docker"
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  printf '{"seq":1}\n' >"$SB/conformance/recordings/sync/taken/exchanges.jsonl"
  mg sync-rig up --record taken
  [ "$status" -eq 2 ]
  [ "$(field '.failures[0].id')" = recording-exists ]
  [[ "$stderr" != *"docker must not run"* ]]
  [ "$(cat "$SB/conformance/recordings/sync/taken/exchanges.jsonl")" = '{"seq":1}' ]
  validate "$result"
}

@test "sync-rig up takes an empty exchanges.jsonl, which the recorder creates at start, as no recording" {
  local stubs="$BATS_TEST_TMPDIR/sync-stubs" dir="$SB/conformance/recordings/sync"
  mkdir -p "$stubs" "$dir/unused" "$dir/other"
  printf '#!/bin/sh\nexit 97\n' >"$stubs/docker"
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  : >"$dir/unused/exchanges.jsonl"
  mg sync-rig up --record unused
  # Past the recording check, the job asks Docker, which this stub refuses.
  [ "$(field -c '[.failures[].id]')" = '["docker-unreachable"]' ]
  validate "$result"
  # Any other file is a recording.
  : >"$dir/other/notes.txt"
  mg sync-rig up --record other
  [ "$(field '.failures[0].id')" = recording-exists ]
  validate "$result"
}

@test "sync-rig status reports a stopped rig as down, and down removes only the rig's services" {
  local stubs="$BATS_TEST_TMPDIR/sync-stubs"
  mkdir -p "$stubs"
  # Docker answers, and compose lists no running container.
  cat >"$stubs/docker" <<STUB
#!/bin/sh
echo "\$*" >>"$BATS_TEST_TMPDIR/docker-calls"
case "\$1" in info | compose | network) exit 0 ;; esac
exit 97
STUB
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
  mg sync-rig status
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field .details.running)" = false ]
  [[ "$(field .message)" == *down* ]]
  validate "$result"
  : >"$BATS_TEST_TMPDIR/docker-calls"
  mg sync-rig down
  [ "$status" -eq 0 ]
  [ "$(field .details.running)" = false ]
  grep -q -- ' rm --stop --force --volumes syncserver toxiproxy mitmdump sync-edge$' "$BATS_TEST_TMPDIR/docker-calls"
  [ "$(grep -c -- ' down' "$BATS_TEST_TMPDIR/docker-calls")" -eq 0 ]
  validate "$result"
}

# A docker stub for a running rig: sync-edge publishes its four ports at
# $STUB_PUBLISH_URL, and the server's environment is $STUB_SERVER_ENV.
running_rig_fixture() {
  local stubs="$BATS_TEST_TMPDIR/sync-stubs"
  mkdir -p "$stubs"
  cat >"$stubs/docker" <<'STUB'
#!/bin/bash
echo "$*" >>"$BATS_TEST_TMPDIR/docker-calls"
case "$1" in
  info) exit 0 ;;
  inspect) [ "${*: -1}" = srv1 ] && printf '%s\n' "$STUB_SERVER_ENV"; exit 0 ;;
  compose) ;;
  *) exit 97 ;;
esac
case " $* " in
  *" ps --quiet --status running syncserver "*) echo srv1 ;;
  *" ps --quiet --status running "*) ;;
  *" ps --format json "*)
    for s in syncserver toxiproxy; do printf '{"Service":"%s","Publishers":[]}\n' "$s"; done
    printf '{"Service":"sync-edge","Publishers":['
    printf '{"URL":"%s","TargetPort":8080,"PublishedPort":40080,"Protocol":"tcp"},' "$STUB_PUBLISH_URL"
    printf '{"URL":"%s","TargetPort":8081,"PublishedPort":40081,"Protocol":"tcp"},' "$STUB_PUBLISH_URL"
    printf '{"URL":"%s","TargetPort":8474,"PublishedPort":40474,"Protocol":"tcp"},' "$STUB_PUBLISH_URL"
    printf '{"URL":"%s","TargetPort":8082,"PublishedPort":40082,"Protocol":"tcp"}]}\n' "$STUB_PUBLISH_URL" ;;
  *" port sync-edge 8080 "*) echo "$STUB_PUBLISH_URL:40080" ;;
  *" port sync-edge 8081 "*) echo "$STUB_PUBLISH_URL:40081" ;;
  *" port sync-edge 8474 "*) echo "$STUB_PUBLISH_URL:40474" ;;
  *" port sync-edge 8082 "*) echo "$STUB_PUBLISH_URL:40082" ;;
esac
exit 0
STUB
  chmod +x "$stubs/docker"
  export PATH="$stubs:$PATH"
}

@test "sync-rig status stops a rig published beyond 127.0.0.1" {
  running_rig_fixture
  export STUB_PUBLISH_URL=0.0.0.0 STUB_SERVER_ENV='["SYNC_USER1=meguri-a:meguri-test-a"]'
  mg sync-rig status
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '[.failures[].id] | index("published-beyond-loopback") != null')" = true ]
  [[ "$(field '.failures[] | select(.id == "published-beyond-loopback") | .message')" == *"sync-edge 0.0.0.0:40080"* ]]
  [[ "$(field .message)" == *"stopped"* ]]
  grep -q -- ' rm --stop --force --volumes syncserver toxiproxy mitmdump sync-edge$' "$BATS_TEST_TMPDIR/docker-calls"
  validate "$result"
}

@test "sync-rig reports the users the running server has, overrides included" {
  running_rig_fixture
  # docker/syncserver.env replaced SYNC_USER1 for compose; the server has no
  # meguri-a, and SYNC_USER6 comes after a gap.
  export STUB_PUBLISH_URL=127.0.0.1 STUB_SERVER_ENV='["PATH=/usr/bin","SYNC_USER2=meguri-b:meguri-test-b","SYNC_USER1=alice:pw","SYNC_USER3=meguri-c:x:y","SYNC_USER4=meguri-d:meguri-test-d","SYNC_USER6=late:pw","SYNC_HOST=0.0.0.0"]'
  printf 'SYNC_USER1=alice:pw\n' >"$SB/docker/syncserver.env"
  mg sync-rig status
  [ "$(field -c .details.users)" = '["alice","meguri-b","meguri-c","meguri-d"]' ]
  [ "$(field .details.endpoints.syncserver)" = "http://127.0.0.1:40080/" ]
  [ "$(field .details.endpoints.toxiproxy_api)" = "http://127.0.0.1:40474/" ]
  [ "$(field '[.failures[].id] | index("published-beyond-loopback")')" = null ]
  [ "$(grep -c -- ' rm ' "$BATS_TEST_TMPDIR/docker-calls")" -eq 0 ]
  validate "$result"
}
