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
  for args in "" "frobnicate" "bump" "bump 26.x" "record --jobs 0" "hello --verbose" "build extra"; do
    # shellcheck disable=SC2086 # each case is a word list
    mg oracle $args
    [ "$status" -eq 2 ]
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = usage ]
    [[ "$stderr" != *"docker must not run"* ]]
    validate "$result"
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
  run) printf 'anki==26.10.0 \\\n    --hash=sha256:%064d\n' 1 >"$lock/requirements.lock"; exit 0 ;;
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
  # The conformance pin is staged from the new oracle's hello.
  [ "$(jq -r '.anki_version + " " + .lock_sha256' "$run_dir/staging/conformance/oracle.json")" = "26.10.0 l" ]
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
