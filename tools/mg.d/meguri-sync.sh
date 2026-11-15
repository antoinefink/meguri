# Meguri Sync's standalone MIT workspace. Never invoke the reference family.
mg_job sync-server-test docker:dev mg_sync_server_test \
  "Account/service tests: locked nextest, zero retries; -- [-E|--filter-expr <expr>] [--test <binary>] (repeatable, separate values only)"
mg_job sync-server-lint docker:dev mg_sync_server_lint \
  "Standalone server clippy correctness/safety on all targets/features, locked"
mg_job sync-server-deps docker:dev mg_sync_server_deps \
  "Server graph/deny/notices evidence, locked; --resolve initial lock or --update <package> <exact-version>"
mg_job sync-server-format docker:dev mg_sync_server_format \
  "Optional server rustfmt check; --write explicitly applies formatting in Docker"

mg_sync_server__env() {
  export CARGO_TARGET_DIR=/cargo-target/server
  if [ ! -f server/Cargo.toml ] || [ ! -f server/rust-toolchain.toml ]; then
    mg_failure workspace-missing "server manifest/toolchain is missing"
    mg_status error
    return 1
  fi
  # All cargo invocations run in server/ and use its toolchain pin.
  mg_detail environment "$(cd server && jq -nc --arg rustc "$(rustc -V)" --arg nextest "$(cargo nextest --version)" \
    --arg target "$CARGO_TARGET_DIR" '{rustc:$rustc,nextest:$nextest,target:$target}')"
}
mg_sync_server__locked() {
  if [ ! -f server/Cargo.lock ]; then
    mg_failure lock-missing "Generate the initial server lock explicitly with tools/mg sync-server-deps --resolve; normal checks never resolve it"
    mg_status fail
    return 1
  fi
}
mg_sync_server_test() {
  [ "${1:-}" != -- ] || shift
  # Only separate filter operands are forwarded. No Cargo/config/output/policy
  # switches (or libtest arguments after another --) may change this receipt.
  local filters=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -E|--filter-expr|--test)
        if [ "$#" -lt 2 ] || [ -z "$2" ] || [[ "$2" = -* ]]; then
          mg_failure usage "filters require a nonempty separate value"; mg_status error; return 0
        fi
        filters+=("$1" "$2"); shift 2 ;;
      *)
        mg_failure usage "sync-server-test supports only -- [-E|--filter-expr <expr>] [--test <binary>]"; mg_status error; return 0 ;;
    esac
  done
  mg_sync_server__env && mg_sync_server__locked || return 0
  local binary="$MG_RUN_DIR/binary.jsonl" out="$MG_RUN_DIR/nextest.jsonl" rc
  # Integration tests spawn this exact feature-gated executable for lock/kill
  # evidence. Default production build is checked separately by clippy.
  (cd server && cargo build --locked --bin meguri-sync-server --features test-hooks --message-format json-render-diagnostics) >"$binary"
  rc=$?
  mg_artifact server-build-jsonl "$binary"
  if [ "$rc" -ne 0 ]; then
    mg_failure "build-exit-$rc" "server binary build failed; see log.txt"; mg_status fail; return 0
  fi
  export MEGURI_SERVER_TEST_BINARY
  MEGURI_SERVER_TEST_BINARY="$(jq -r 'select(.reason=="compiler-artifact" and .target.name=="meguri-sync-server" and .executable!=null) | .executable' "$binary" | tail -n 1)"
  if [ ! -x "$MEGURI_SERVER_TEST_BINARY" ]; then
    mg_failure binary-missing "cargo did not report an executable"; mg_status fail; return 0
  fi
  (cd server && NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --locked --workspace --all-targets --features test-hooks \
    --retries 0 --no-fail-fast --no-tests=fail --message-format libtest-json-plus --message-format-version 0.1 ${filters[0]+"${filters[@]}"}) >"$out"
  rc=$?
  mg_artifact nextest-jsonl "$out"
  local report="$MG_RUN_DIR/nextest-evidence.json" diagnostics="$MG_RUN_DIR/nextest-evidence.log"
  local passed=0 failed=0 skipped=0 evidence_ok=false counts
  # Parse one JSON object per line, retaining good completed events even when
  # another line is broken. Per-binary state permits interleaved suites/tests.
  # Ignored tests may have no started event; executed tests must have one.
  if jq -Rnc '
    def count: if type == "number" then . >= 0 and . <= 2147483647 and floor == . else false end;
    def text: if type == "string" then length > 0 else false end;
    def test_name: if type == "string" then test("^[^$]+\\$.+$") else false end;
    def bad($m): .errors |= (if length < 20 then . + [$m[0:500]] else . end);
    reduce inputs as $line (
      {line:0, suites:{}, tests:{}, passed:0, failed:0, skipped:0, errors:[], failures:[]};
      .line += 1 |
      (try ($line | fromjson) catch null) as $e |
      if ($e | type) != "object" then bad("invalid JSON object at line \(.line)")
      elif $e.type == "suite" then
        if ($e.nextest | type) != "object" then bad("missing suite identity")
        elif ([$e.nextest.crate, $e.nextest.test_binary, $e.nextest.kind] | all(.[]; text)) then
          ($e.nextest.crate + "::" + $e.nextest.test_binary + "$") as $key |
          if $e.event == "started" then
            if .suites[$key] != null then bad("duplicate suite start: " + $key)
            else
              .suites[$key] = {count:$e.test_count, kind:$e.nextest.kind, passed:0, failed:0, skipped:0, done:false} |
              if ($e.test_count | count) then . else bad("invalid suite test_count: " + $key) end
            end
          elif $e.event == "ok" or $e.event == "failed" then
            if .suites[$key] == null or .suites[$key].done then bad("missing start or duplicate suite completion: " + $key)
            else
              .suites[$key].done = true |
              if $e.nextest.kind != .suites[$key].kind then bad("inconsistent suite identity: " + $key) else . end |
              if ([$e.passed,$e.failed,$e.ignored,$e.measured,$e.filtered_out] | all(.[]; count)) then
                if $e.measured != 0 or
                   $e.passed != .suites[$key].passed or $e.failed != .suites[$key].failed or
                   $e.ignored != .suites[$key].skipped or
                   ($e.passed + $e.failed + $e.ignored) != .suites[$key].count
                then bad("inconsistent suite counts: " + $key) else . end
              else bad("invalid suite counts: " + $key) end |
              if $e.event == "failed" or $e.failed != 0 then bad("failed suite: " + $key) else . end
            end
          else bad("unsupported suite event") end
        else bad("missing suite identity") end
      elif $e.type == "test" then
        if ($e.name | test_name) then
          $e.name as $name | (($name | split("$")[0]) + "$") as $key |
          if .suites[$key] == null or .suites[$key].done then bad("test outside active suite: " + $name) else . end |
          if $e.event == "started" then
            if .tests[$name] != null then bad("duplicate or late test start: " + $name)
            else .tests[$name] = {done:false} end
          elif $e.event == "ok" or $e.event == "failed" or $e.event == "ignored" then
            if .tests[$name].done == true then bad("duplicate test completion: " + $name)
            else
              if .tests[$name] == null and $e.event != "ignored" then bad("test completion without start: " + $name) else . end |
              .tests[$name] = {done:true} |
              (if $e.event == "ok" then "passed" elif $e.event == "failed" then "failed" else "skipped" end) as $count |
              .[$count] += 1 |
              if .suites[$key] != null then .suites[$key][$count] += 1 else . end |
              if $e.event == "failed" and (.failures | length) < 50 then
                .failures += [{id:($name | .[0:500]),
                  message:(if ($e.stdout | type) == "string" then $e.stdout[0:2000] else "failed" end),
                  repro:"tools/mg sync-server-test"}]
              else . end
            end
          else bad("unsupported test event") end
        else bad("missing fully qualified test name") end
      else bad("unsupported nextest record") end
    ) |
    if (.suites | length) == 0 then bad("no suites") else . end |
    if any(.suites[]; .done != true) then bad("incomplete suites") else . end |
    if any(.tests[]; .done != true) then bad("incomplete tests") else . end |
    {passed,failed,skipped,errors,failures}
  ' "$out" >"$report" 2>"$diagnostics"; then
    # Check every extraction before passing numbers to shell arithmetic or MG.
    if counts="$(jq -er '[.passed,.failed,.skipped] | if all(.[]; type == "number" and . >= 0 and . <= 2147483647 and floor == .) then @tsv else error("invalid counts") end' "$report")" &&
       IFS=$'\t' read -r passed failed skipped <<<"$counts" &&
       jq -c '.failures[]' "$report" >>"$MG_STATE/failures.jsonl"; then
      evidence_ok=true
    fi
  fi
  mg_artifact nextest-evidence "$report"
  mg_artifact nextest-evidence-log "$diagnostics"
  mg_count total "$((passed+failed+skipped))"; mg_count passed "$passed"; mg_count failed "$failed"; mg_count skipped "$skipped"
  if [ "$evidence_ok" != true ] || ! jq -e '.errors == []' "$report" >/dev/null 2>&1; then
    mg_failure nextest-evidence "invalid or incomplete nextest evidence; inspect retained JSONL, evidence report and log"
    evidence_ok=false
  fi
  if [ "$rc" -ne 0 ] || [ "$failed" -ne 0 ] || [ "$passed" -eq 0 ]; then
    mg_failure "nextest-exit-$rc" "nextest failed or reported zero executed tests; inspect retained JSONL/log"
    evidence_ok=false
  fi
  if [ "$evidence_ok" = true ]; then
    mg_message "sync-server-test: $passed passed, $skipped skipped (zero retries)"; mg_status pass
  else mg_status fail; fi
}
mg_sync_server_lint() {
  [ "$#" -eq 0 ] || { mg_failure usage "sync-server-lint takes no arguments"; mg_status error; return 0; }
  mg_sync_server__env && mg_sync_server__locked || return 0
  local rc=0
  (cd server && cargo clippy --locked --workspace --all-targets --all-features -- -D clippy::correctness -D clippy::suspicious) >"$MG_RUN_DIR/clippy.log" 2>&1 || rc=$?
  # Also prove that the default production graph builds without testing seams.
  (cd server && cargo clippy --locked --workspace --all-targets --no-default-features -- -D clippy::correctness -D clippy::suspicious) >>"$MG_RUN_DIR/clippy.log" 2>&1 || rc=$?
  mg_artifact clippy-log "$MG_RUN_DIR/clippy.log"
  if [ "$rc" -eq 0 ]; then mg_status pass; else mg_failure clippy "server clippy failed; see clippy.log"; mg_status fail; fi
}
mg_sync_server_format() {
  local fmt_args=(--all -- --check)
  if [ "${1:-}" = --write ] && [ "$#" -eq 1 ]; then fmt_args=(--all)
  elif [ "$#" -ne 0 ]; then mg_failure usage "sync-server-format [--write]"; mg_status error; return 0; fi
  mg_sync_server__env || return 0
  if (cd server && cargo fmt "${fmt_args[@]}"); then mg_status pass; else mg_failure rustfmt "server rustfmt failed; see log.txt"; mg_status fail; fi
}
mg_sync_server_deps() {
  local LC_ALL=C
  local resolve=false update_package= update_version=
  local package_pattern='^[A-Za-z0-9][A-Za-z0-9_-]*$'
  local version_pattern='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?(\+[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?$'
  if [ "${1:-}" = --resolve ] && [ "$#" -eq 1 ]; then resolve=true
  elif [ "${1:-}" = --update ] && [ "$#" -eq 3 ] && [[ "$2" =~ $package_pattern ]] && [[ "$3" =~ $version_pattern ]]; then
    update_package="$2"; update_version="$3"
  elif [ "$#" -ne 0 ]; then mg_failure usage "sync-server-deps [--resolve | --update <package> <exact-version>]"; mg_status error; return 0; fi
  mg_sync_server__env || return 0
  if [ "$resolve" = true ]; then
    if [ -f server/Cargo.lock ]; then mg_failure lock-exists "--resolve is initial generation only; inspect existing lock explicitly"; mg_status fail; return 0; fi
    if ! (cd server && cargo generate-lockfile); then mg_failure resolve "initial server lock generation failed"; mg_status fail; return 0; fi
  fi
  mg_sync_server__locked || return 0
  if [ -n "$update_package" ]; then
    local update_rc=0
    (cd server && cargo update --package "$update_package" --precise "$update_version") || update_rc=$?
    if [ "$update_rc" -ne 0 ]; then
      if cp server/Cargo.lock "$MG_RUN_DIR/Cargo.lock"; then
        mg_artifact cargo-lock "$MG_RUN_DIR/Cargo.lock" "lockfile after failed cargo update"
      else
        mg_failure lockfile-artifact "could not retain Cargo.lock after failed cargo update; see log.txt"
      fi
      mg_failure "cargo-update-exit-$update_rc" "cargo update failed; inspect retained lock and log.txt"; mg_status fail; return 0
    fi
    mg_message "sync-server-deps: updated $update_package to $update_version; checking resolved graph"
  fi
  local rc=0 edge
  cp server/Cargo.lock "$MG_RUN_DIR/Cargo.lock" || rc=1
  cp server/deny.toml "$MG_RUN_DIR/deny.toml" || rc=1
  cp server/about.toml "$MG_RUN_DIR/about.toml" || rc=1
  (cd server && cargo metadata --locked --format-version 1 --all-features) >"$MG_RUN_DIR/metadata.json" || rc=1
  for edge in normal build dev; do
    (cd server && cargo tree --locked --all-features --edges "$edge,features") >"$MG_RUN_DIR/tree-$edge.txt" || rc=1
    mg_artifact "tree-$edge" "$MG_RUN_DIR/tree-$edge.txt"
  done
  (cd server && cargo deny --locked --format json --config deny.toml check) >"$MG_RUN_DIR/deny.stdout" 2>"$MG_RUN_DIR/deny.jsonl" || rc=1
  # cargo-about's built-in JSON renderer avoids introducing a notice framework.
  (cd server && cargo about generate --locked --all-features --config about.toml --format json) >"$MG_RUN_DIR/notices.json" 2>"$MG_RUN_DIR/about.log" || rc=1
  mg_artifact cargo-lock "$MG_RUN_DIR/Cargo.lock"
  mg_artifact deny-config "$MG_RUN_DIR/deny.toml"
  mg_artifact about-config "$MG_RUN_DIR/about.toml"
  mg_artifact cargo-metadata "$MG_RUN_DIR/metadata.json"
  mg_artifact cargo-deny "$MG_RUN_DIR/deny.jsonl"
  mg_artifact cargo-notices "$MG_RUN_DIR/notices.json"
  mg_artifact cargo-about-log "$MG_RUN_DIR/about.log"
  if [ "$rc" -eq 0 ]; then mg_status pass; else mg_failure dependencies "one or more dependency evidence/audit commands failed; inspect retained artifacts"; mg_status fail; fi
}
