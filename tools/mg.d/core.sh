# Rust core jobs. Sourced by tools/mg; they run inside the `dev` container.

mg_job core-test docker:dev mg_core_test \
  "Rust tests: cargo nextest on core/ (no retries), canary_* first; --canaries only; a bare run and --quick leave out the three stress cases (hosted shards: [--partition hash:M/N] [--exclude-list <file> | --only-list <file>] [--archive <file>]; --build-archive <file> builds quick's tests once for them), --stress requires those three (--stress --case session|suspend|merge: one of them), building only their test binaries; args after -- go to nextest"
mg_job core-lint docker:dev mg_core_lint \
  "Rust lint: clippy -D warnings on all targets and features, layering-check (edges.toml), and no AnkiWeb name in meguri-sync built without its features; source formatting is optional"
mg_job core-deny docker:dev mg_core_deny \
  "Rust dependency gate: cargo deny check on core/ (licenses, advisories, bans, sources); args after -- name checks"
mg_job u38-wiring docker:dev mg_u38_wiring \
  "Host provider milestone check: --milestone <id>; pending providers must belong to later milestones"

mg_u38_wiring() {
  if [ "$#" -ne 2 ] || [ "$1" != --milestone ] || [ -z "$2" ]; then
    mg_failure usage "usage: tools/mg u38-wiring --milestone <id>" "tools/mg --list"
    mg_status error
    return 0
  fi
  local target="$2" manifest="$MG_RUN_DIR/manifest.json"
  mg_milestone_gate__load "$manifest" || return 0
  mg_artifact manifest "$manifest" "validated milestone manifest for pending provider ownership"
  export MEGURI_WIRING_TARGET="$target" MEGURI_WIRING_MANIFEST="$manifest"
  mg_core_test -- --test host_it --run-ignored only -E 'package(meguri-host) & test(=ports::milestone_provider_owners)'
}

# Canary test binaries (tests/canary_*.rs) run first and alone in a bare
# core-test; when one fails, the other tests do not run (AE82 ordering).
# The canary phase builds only the packages that hold a canary, so a build
# error elsewhere (say, code broken by a pin bump) cannot hide the pin canary.
# nextest rejects a binary() filter that matches no binary, so the canary
# phase runs only when a canary exists.
MG_CORE_CANARIES='binary(/^canary_/)'

# Exact identities shared by quick's exclusion and stress's selection, one
# line per case: <case> <package> <test binary> <test>. Keep nextest's
# existing scheduling overrides; neither mode changes the profile.
MG_CORE_STRESS_CASES='session meguri-engine engine_it session_predict::c8_all_four_actual_choices_in_ten_thousand_random_multi_answer_sessions
suspend meguri-sync sync_it normal_sync::large::suspend_two_seconds_with_five_queued_answers_rolls_back_merge_at_150k
merge meguri-sync sync_it normal_sync::large::merge_150k_updates_preserves_real_overlay_answers_and_write_budget_at_150k'

# mg__core_stress <case|all> <filter|names|targets>: the selected cases'
# nextest filter, their `<package>::<binary>$<test>` names (JSON array), or
# their test binaries (`--test <binary>` arguments, each once).
mg__core_stress() {
  awk -v want="$1" -v what="$2" '
    want == "all" || $1 == want {
      n++
      filter = filter (n > 1 ? " | " : "") "(package(=" $2 ") & binary(=" $3 ") & test(=" $4 "))"
      names = names (n > 1 ? "," : "") "\"" $2 "::" $3 "$" $4 "\""
      if (!($3 in seen)) { seen[$3] = 1; targets = targets (targets == "" ? "" : " ") "--test " $3 }
    }
    END {
      if (n == 0) exit 1
      if (what == "filter") print filter
      else if (what == "names") print "[" names "]"
      else print targets
    }' <<<"$MG_CORE_STRESS_CASES"
}
MG_CORE_STRESS="$(mg__core_stress all filter)"

# mg__core_stress_present: the filter of the stress cases whose package has
# their test binary in this workspace, or nothing. nextest rejects a
# binary() filter that matches no binary, so a bare run in a workspace
# without them must not name them.
mg__core_stress_present() {
  local have
  have="$( (cd core && cargo metadata --no-deps --format-version 1) |
    jq -r '.packages[] | .name as $p | .targets[] | select(any(.kind[]; . == "test")) | $p + " " + .name')" || return 0
  awk 'NR == FNR { have[$0] = 1; next }
    ($2 " " $3) in have {
      n++
      filter = filter (n > 1 ? " | " : "") "(package(=" $2 ") & binary(=" $3 ") & test(=" $4 "))"
    }
    END { if (n) print filter }' <(printf '%s\n' "$have") <(printf '%s\n' "$MG_CORE_STRESS_CASES")
}

# mg__core_canary_packages: workspace packages with a canary_* test target, one per line.
mg__core_canary_packages() {
  (cd core && cargo metadata --no-deps --format-version 1) |
    jq -r '.packages[] | select(any(.targets[]; any(.kind[]; . == "test") and (.name | startswith("canary_")))) | .name' |
    sort -u
}

mg__core_environment() {
  mg_detail environment "$(
    cd core 2>/dev/null || cd .
    jq -nc \
      --arg container "${MG_CONTAINER:-}" \
      --arg os "$(uname -sm)" \
      --arg rustc "$(rustc -V 2>&1)" \
      --arg nextest "$(cargo nextest --version 2>&1 | head -n 1)" \
      --arg deny "$(cargo deny --version 2>&1 | head -n 1)" \
      '{container: $container, os: $os, rustc: $rustc, nextest: $nextest, cargo_deny: $deny}'
  )"
}

# mg__nextest <jsonl> <package scope and nextest args>: one nextest run in
# core/. The caller passes the scope: --workspace or -p <package>...
mg__nextest() {
  local out="$1"
  shift
  (
    cd core &&
      NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run \
        --retries 0 --no-fail-fast \
        --message-format libtest-json-plus --message-format-version 0.1 \
        ${1+"$@"} >"$out"
  )
}

# mg__core_list <file>: the hosted timing-sensitive list (.github/
# core-timing-sensitive.txt). One test per line, `<binary id> <test name>`
# as nextest prints them, an optional `# reason`. Prints `<binary id>\t<test
# name>` per test; returns 1 on a malformed line, so no line can widen or
# narrow the selection beyond exact identities.
mg__core_list() {
  local line binary test rest n=0
  local binary_re='^[A-Za-z0-9_-]+(::[A-Za-z0-9_-]+)?(/[A-Za-z0-9_-]+)?$' test_re='^[A-Za-z0-9_]+(::[A-Za-z0-9_]+)*$'
  while IFS= read -r line || [ -n "$line" ]; do
    n=$((n + 1))
    line="${line%%#*}"
    binary="" test="" rest=""
    read -r binary test rest <<<"$line"
    [ -n "$binary" ] || continue
    if [ -n "$rest" ] || ! [[ "$binary" =~ $binary_re ]] || ! [[ "$test" =~ $test_re ]]; then
      echo "line $n: expected '<binary id> <test name>  # reason', got: $line" >&2
      return 1
    fi
    printf '%s\t%s\n' "$binary" "$test"
  done <"$1"
}

# mg__core_settle <filter>: under --only-list, build the selected tests, then
# flush the build's writeback, so the listed wall-clock tests start on a
# settled disk (as the settled-disk wrapper does for the WAL tests). Reads the
# caller's list_mode and features. A build failure is left to the run itself.
mg__core_settle() {
  [ "$list_mode" = only ] || return 0
  # The listing also names each selected test's binary (mg_core_test).
  (cd core && cargo nextest list ${scope[0]+"${scope[@]}"} --message-format json -E "$1") \
    >"$MG_RUN_DIR/nextest-list.json" || echo '{}' >"$MG_RUN_DIR/nextest-list.json"
  mg_artifact nextest-list-json "$MG_RUN_DIR/nextest-list.json"
  sync
}

# mg__core_archive_path <file>: a .tar.zst path inside the checkout, relative.
mg__core_archive_path() {
  case "$1" in
    '' | /* | ../* | */../* | *..) return 1 ;;
    *.tar.zst) return 0 ;;
    *) return 1 ;;
  esac
}

# mg__core_build_archive <file>: quick's test binaries (every workspace test,
# with quick's features), built once into a nextest archive that hosted
# shards run with --archive. Builds only: no test runs here.
mg__core_build_archive() {
  local out="$MG_RUN_DIR/nextest-archive.log" rc
  if [ ! -f core/Cargo.toml ]; then
    mg_failure no-workspace "core/ has no Cargo workspace, so there is nothing to archive"
    mg_status fail
    return 0
  fi
  mkdir -p "$(dirname "$1")"
  rm -f "$1"
  (cd core && cargo nextest archive --workspace --features meguri-ffi/test-hooks --archive-file "$MG_ROOT/$1") 2>&1 | tee "$out" >&2
  rc=${PIPESTATUS[0]}
  mg_artifact log "$out" "cargo nextest archive"
  if [ "$rc" -ne 0 ] || [ ! -s "$1" ]; then
    mg_failure "nextest-archive-exit-$rc" "cargo nextest archive exited with code $rc; see log.txt"
    mg_message "core-test: building the test archive failed (exit $rc)"
    mg_status fail
    return 0
  fi
  mg_detail archive "$(jq -nc --arg file "$1" --arg sha "$(sha256sum "$1" | cut -c1-64)" --argjson bytes "$(wc -c <"$1" | tr -d ' ')" \
    '{file: $file, sha256: $sha, bytes: $bytes, features: ["meguri-ffi/test-hooks"]}')"
  mg_message "core-test: test archive $1 built ($(( $(wc -c <"$1") / 1048576 )) MB)"
  mg_status pass
}

# tools/mg core-test [--canaries | --quick [hosted options] | --stress [--case <case>] | -- nextest args]
# A bare run (no arguments) leaves out the stress cases; -- nextest args select
# exactly what they say.
# --canaries runs the canary phase alone (the gate's canary stage, U19) and
# fails when the workspace has no canary. Quick keeps that ordering; stress
# runs only its exact three cases (or the one --case names), and builds only
# their test binaries. Both opt into the FFI test hooks locally.
# Hosted options split quick's ordinary tests across runners; each shard
# still runs the canaries first. `--partition hash:M/N` keeps the tests of
# nextest partition M of N. `--exclude-list <file>` leaves out the listed
# tests; `--only-list <file>` runs exactly them, one at a time after the
# build has been flushed to disk, and fails when a listed test did not run.
mg_core_test() {
  local only_canaries=false mode=default arg stress_case=all
  local partition="" list_mode="" list_file="" listed="" list_expr="" archive=""
  case "${1:-}" in
    --build-archive)
      if [ "$#" -ne 2 ] || ! mg__core_archive_path "$2"; then
        mg_failure usage "--build-archive takes one <file>.tar.zst inside the checkout, given relative to its root, and nothing else" "tools/mg core-test --build-archive build/core-tests.tar.zst"
        mg_status error
        return 0
      fi
      mg__core_environment
      mg__core_build_archive "$2"
      return 0
      ;;
    --canaries|--stress)
      mode="${1#--}"
      shift
      if [ "$mode" = stress ] && [ "${1:-}" = --case ] && [ "$#" -eq 2 ] && [ "$2" != all ] &&
         mg__core_stress "$2" names >/dev/null; then
        stress_case="$2"
        shift 2
      fi
      if [ "$#" -gt 0 ]; then
        mg_failure usage "--$mode takes no other arguments$( [ "$mode" != stress ] || printf ' but one --case session|suspend|merge')" "tools/mg core-test --$mode"
        mg_status error
        return 0
      fi
      [ "$mode" != canaries ] || only_canaries=true
      ;;
    --quick)
      mode=quick
      shift
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --partition)
            if [ -n "$partition" ] || [ "$#" -lt 2 ] || ! [[ "$2" =~ ^hash:([1-9][0-9]*)/([1-9][0-9]*)$ ]] ||
               [ "${BASH_REMATCH[1]}" -gt "${BASH_REMATCH[2]}" ]; then
              mg_failure usage "--partition takes one hash:M/N with 1 <= M <= N" "tools/mg core-test --quick --partition hash:1/4"
              mg_status error
              return 0
            fi
            partition="$2"
            shift 2
            ;;
          --archive)
            if [ -n "$archive" ] || [ "$#" -lt 2 ] || ! mg__core_archive_path "$2" || [ ! -f "$2" ]; then
              mg_failure usage "--archive takes one existing <file>.tar.zst inside the checkout, given relative to its root (tools/mg core-test --build-archive makes it)" "tools/mg core-test --quick --archive build/core-tests.tar.zst"
              mg_status error
              return 0
            fi
            archive="$2"
            shift 2
            ;;
          --exclude-list|--only-list)
            if [ -n "$list_mode" ] || [ "$#" -lt 2 ] || [ -z "$2" ]; then
              mg_failure usage "--quick takes at most one of --exclude-list or --only-list, with a file" "tools/mg core-test --quick --exclude-list .github/core-timing-sensitive.txt"
              mg_status error
              return 0
            fi
            list_mode="${1#--}"
            list_mode="${list_mode%-list}"
            list_file="$2"
            shift 2
            ;;
          *)
            mg_failure usage "--quick takes only --partition hash:M/N, --archive <file> and one of --exclude-list or --only-list <file>" "tools/mg core-test --quick"
            mg_status error
            return 0
            ;;
        esac
      done
      if [ "$list_mode" = only ] && [ -n "$partition" ]; then
        mg_failure usage "--only-list runs its tests one at a time on one runner; it takes no --partition" "tools/mg core-test --quick --only-list $list_file"
        mg_status error
        return 0
      fi
      if [ -n "$list_mode" ]; then
        case "$list_file" in
          /* | ../* | */../* | *..)
            mg_failure usage "the list must be a file inside the checkout, given relative to its root" "tools/mg core-test --quick --$list_mode-list .github/core-timing-sensitive.txt"
            mg_status error
            return 0
            ;;
        esac
        if [ ! -f "$list_file" ]; then
          mg_failure list-missing "no list file at $list_file" "tools/mg core-test --quick --$list_mode-list $list_file"
          mg_status error
          return 0
        fi
        if ! listed="$(mg__core_list "$list_file")"; then
          mg_failure list-format "$list_file has a malformed line (see log.txt); each line is '<binary id> <test name>  # reason'" "tools/mg core-test --quick --$list_mode-list $list_file"
          mg_status error
          return 0
        fi
        if [ -n "$listed" ]; then
          list_expr="$(printf '%s\n' "$listed" | awk -F'\t' '{printf "%s(binary_id(=%s) & test(=%s))", (NR > 1 ? " | " : ""), $1, $2}')"
        fi
      fi
      mg_detail hosted_selection "$(jq -nc --arg partition "$partition" --arg mode "$list_mode" --arg file "$list_file" \
        --arg sha "$( [ -z "$list_file" ] || sha256sum "$list_file" | cut -c1-64)" --arg listed "$listed" \
        --arg archive "$archive" --arg archive_sha "$( [ -z "$archive" ] || sha256sum "$archive" | cut -c1-64)" '
        {partition: (if $partition == "" then null else $partition end),
         archive: (if $archive == "" then null else {file: $archive, sha256: $archive_sha} end),
         list_mode: (if $mode == "" then null else $mode end),
         list_file: (if $file == "" then null else $file end),
         list_sha256: (if $sha == "" then null else $sha end),
         listed: [$listed | splits("\n") | select(. != "") | split("\t") | .[0] + "$" + .[1]]}')"
      ;;
  esac
  [ "${1:-}" = "--" ] && shift
  for arg in ${1+"$@"}; do
    case "$arg" in --canaries|--quick|--stress)
      mg_failure usage "core-test modes must be used alone" "tools/mg core-test --quick"
      mg_status error
      return 0
      ;;
    esac
  done
  mg__core_environment

  # Only a bare run may match no tests (no or empty workspace). With nextest
  # arguments, an empty match must fail, or a stale repro command would pass.
  if [ ! -f core/Cargo.toml ]; then
    if [ "$only_canaries" = true ]; then
      mg_failure no-canaries "core/ has no Cargo workspace, so no canary (tests/canary_*.rs) runs"
      mg_status fail
    elif [ "$#" -eq 0 ] && [ "$mode" = default ]; then
      mg_message "core-test: no Cargo workspace in core/ yet; 0 tests"
      mg_status pass
    else
      mg_failure no-tests-matched "core/ has no Cargo workspace yet, so no test matches the nextest arguments"
      mg_message "core-test: no Cargo workspace in core/ yet; no test matches the nextest arguments"
      mg_status fail
    fi
    return 0
  fi

  local main="$MG_RUN_DIR/nextest.jsonl" canary="$MG_RUN_DIR/nextest-canary.jsonl"
  local rc stopped=false pkg stress_complete=true
  local ordinary_filter="not ($MG_CORE_CANARIES)"
  local files=() canaries=() features=() hosted=() scope=() extract=""
  case "$mode" in
    quick|stress) features=(--features meguri-ffi/test-hooks) ;;
  esac
  # scope: what nextest builds and lists. From an archive, the binaries
  # --build-archive built with quick's features, extracted once beside the
  # Cargo target (on disk, not the /tmp tmpfs) before anything runs, so the
  # settle step's flush comes after the last write; nothing is compiled.
  if [ -n "$archive" ]; then
    local archives="${CARGO_TARGET_DIR:-$MG_RUN_DIR}/nextest-archive" old
    mkdir -p "$archives"
    # A cancelled run cannot remove its extraction: remove those of runs that
    # ended over 10 minutes ago (a result.json that old; a live run has none,
    # and tools/mg writes it after stopping the run's container, which a
    # failed cleanup would leave running at most until then), and any older
    # than a day, which a killed run or a pruned result leaves unmarked.
    for old in "$archives"/*/; do
      [ -d "$old" ] || continue
      old="${old%/}"
      old="${old##*/}"
      [ -z "$(find "$MG_ROOT/build/mg/core-test/$old/result.json" -mmin +10 2>/dev/null)" ] || rm -rf "${archives:?}/$old"
    done
    find "$archives" -mindepth 1 -maxdepth 1 -type d -mmin +1440 -exec rm -rf {} + 2>/dev/null
    extract="$archives/$MG_RUN_ID"
    rm -rf "$extract" && mkdir -p "$extract"
    if ! (cd core && cargo nextest list --archive-file "$MG_ROOT/$archive" --extract-to "$extract" -E 'none()') >/dev/null; then
      rm -rf "$extract"
      mg_failure archive-extract "cargo nextest could not extract $archive; see log.txt" "tools/mg core-test --build-archive $archive"
      mg_message "core-test: the test archive $archive could not be extracted"
      mg_status fail
      return 0
    fi
    scope=(--binaries-metadata "$extract/target/nextest/binaries-metadata.json"
      --cargo-metadata "$extract/target/nextest/cargo-metadata.json"
      --target-dir-remap "$extract/target" --workspace-remap "$MG_ROOT/core")
  else
    scope=(--workspace ${features[0]+"${features[@]}"})
  fi
  if [ "$mode" = quick ]; then
    local quick_filter="not ($MG_CORE_STRESS)"
    case "$list_mode" in
      exclude) [ -z "$list_expr" ] || quick_filter="$quick_filter & not ($list_expr)" ;;
      only) quick_filter="$quick_filter & (${list_expr:-none()})"; hosted=(--test-threads 1) ;;
    esac
    [ -z "$partition" ] || hosted=(--partition "$partition")
    ordinary_filter="$ordinary_filter & $quick_filter"
    set -- --no-tests=pass -E "$quick_filter"
  elif [ "$mode" = stress ]; then
    # shellcheck disable=SC2046 # the --test words of the selected cases
    set -- $(mg__core_stress "$stress_case" targets) -E "$(mg__core_stress "$stress_case" filter)"
  fi
  # A bare run leaves out the three scale cases, as --quick does: they take
  # about 45 minutes and run with --stress. Explicit nextest arguments (the
  # gate's core-test run passes some) select as before, the cases included.
  local bare=false bare_out=""
  if [ "$mode" = default ] && [ "$#" -eq 0 ]; then
    bare_out="$(mg__core_stress_present)"
    if [ -n "$bare_out" ]; then
      bare=true
      ordinary_filter="$ordinary_filter & not ($bare_out)"
    fi
  fi
  if [ "$mode" != stress ] && { [ "$#" -eq 0 ] || [ "$mode" = quick ]; }; then
    while IFS= read -r pkg; do
      if [ -n "$pkg" ]; then canaries+=(-p "$pkg"); fi
    done < <(mg__core_canary_packages)
  fi
  if [ "$only_canaries" = true ] && [ "${#canaries[@]}" -eq 0 ]; then
    mg_failure no-canaries "no workspace package has a tests/canary_*.rs test, so the pin canary is missing (core/AGENTS.md)"
    mg_message "core-test: no canary to run"
    mg_status fail
    return 0
  fi
  if [ "${#canaries[@]}" -gt 0 ]; then
    if [ -n "$archive" ]; then
      mg__nextest "$canary" "${scope[@]}" --no-tests=pass -E "$MG_CORE_CANARIES"
    else
      mg__nextest "$canary" "${canaries[@]}" --no-tests=pass -E "$MG_CORE_CANARIES"
    fi
    rc=$?
    mg_artifact nextest-canary-jsonl "$canary"
    files=("$canary")
    if [ "$rc" -ne 0 ]; then
      stopped=true
    elif [ "$only_canaries" = true ]; then
      :
    else
      mg__core_settle "$ordinary_filter"
      mg__nextest "$main" "${scope[@]}" ${hosted[0]+"${hosted[@]}"} --no-tests=pass -E "$ordinary_filter"
      rc=$?
      mg_artifact nextest-jsonl "$main"
      files=("$canary" "$main")
    fi
  else
    [ "$#" -gt 0 ] || set -- --no-tests=pass
    [ "$bare" = false ] || set -- "$@" -E "not ($bare_out)"
    mg__core_settle "${quick_filter:-}"
    mg__nextest "$main" "${scope[@]}" ${hosted[0]+"${hosted[@]}"} "$@"
    rc=$?
    mg_artifact nextest-jsonl "$main"
    files=("$main")
  fi

  # Test names look like `<crate>::<binary>$<test path>`.
  local counts
  counts="$(cat "${files[@]}" | jq -sc '[.[] | select(.type == "test" and .event != "started")] |
    {passed: map(select(.event == "ok")) | length,
     skipped: map(select(.event == "ignored")) | length,
     failed: map(select(.event != "ok" and .event != "ignored")) | length}')"
  local passed failed skipped
  passed="$(jq -r .passed <<<"$counts")"
  failed="$(jq -r .failed <<<"$counts")"
  skipped="$(jq -r .skipped <<<"$counts")"
  mg_count total $((passed + failed + skipped))
  mg_count passed "$passed"
  mg_count failed "$failed"
  mg_count skipped "$skipped"

  cat "${files[@]}" | jq -c 'select(.type == "test" and .event != "started" and .event != "ok" and .event != "ignored")
    | (.name | split("$")) as $p
    | {id: .name,
       message: ((.stdout // .event) | .[0:2000]),
       repro: ("tools/mg core-test -- -E " + ("package(" + ($p[0] | split("::")[0]) + ") & test(=" + ($p[1] // $p[0]) + ")" | @sh))}' \
    >>"$MG_STATE/failures.jsonl"

  if [ "$rc" -eq 4 ] && [ $((passed + failed + skipped)) -eq 0 ]; then
    mg_failure no-tests-matched "no test matched the nextest arguments; check the filter (a renamed test?)"
  elif [ "$rc" -ne 0 ] && [ "$failed" -eq 0 ]; then
    mg_failure "nextest-exit-$rc" "cargo nextest exited with code $rc before reporting a test failure (build or setup error); see log.txt"
  fi
  if [ "$mode" = stress ]; then
    local stress_tests
    if stress_tests="$(jq -ces --argjson want "$(mg__core_stress "$stress_case" names)" '
      [.[] | select(.type == "test" and .event != "started")] as $tests
      | ([$tests[].name] | sort) as $names
      | select(($tests | length) == ($want | length) and all($tests[]; .event == "ok")
        and $names == ($want | sort))
      | $names' "$main")"; then
      mg_detail stress_tests "$stress_tests"
    else
      stress_complete=false
      mg_failure stress-selection "stress requires exactly the expected cases ($stress_case) to pass, with none missing or skipped" "tools/mg core-test --stress$( [ "$stress_case" = all ] || printf ' --case %s' "$stress_case")"
    fi
  fi
  # --only-list: every listed test must have run exactly once. A renamed or
  # removed test would otherwise leave the list silently checking nothing.
  local listed_complete=true listed_results missing scope_note=""
  if [ "$list_mode" = only ] && [ "$stopped" = false ]; then
    # The JSON names a test `<package>::<binary name>$<test>`: a library's
    # unit tests and an integration test named like the library share that
    # prefix. Find each listed test's binary in nextest's own listing (the
    # settle step), then its one result.
    local listing="$MG_RUN_DIR/nextest-list.json"
    [ -s "$listing" ] || echo '{}' >"$listing"
    if listed_results="$( { [ ! -f "$main" ] || cat "$main"; } | jq -sc --arg listed "$listed" --slurpfile list "$listing" '
      (($list[0] // {})["rust-suites"] // {}) as $suites
      | [.[] | select(.type == "test" and .event != "started")] as $tests
      | [$listed | splits("\n") | select(. != "") | split("\t")]
      | map(.[0] as $b | .[1] as $t | ($b + "$" + $t) as $n | $suites[$b] as $s
        | if $s == null or $s.testcases[$t] == null or $s.testcases[$t]["filter-match"].status != "matches"
          then {name: $n, event: "not-found"}
          else (($b | split("::")[0]) + "::" + $s["binary-name"] + "$" + $t) as $json
            | [$tests[] | select(.name == $json) | .event] as $e
            | {name: $n, event: (if ($e | length) == 1 then $e[0] elif ($e | length) == 0 then "not-run" else "duplicate" end)}
          end)')"; then
      mg_detail listed_results "$listed_results"
      missing="$(jq -r '[.[] | select(.event == "not-found" or .event == "not-run" or .event == "duplicate" or .event == "ignored") | .name] | join(", ")' <<<"$listed_results")"
    else
      missing="(nextest output unreadable)"
    fi
    if [ -n "$missing" ]; then
      listed_complete=false
      mg_failure listed-test-not-run "listed tests did not run exactly once: $missing; a renamed or removed test must be updated in $list_file" \
        "tools/mg core-test --quick --only-list $list_file"
    fi
  fi
  [ -z "$extract" ] || rm -rf "$extract"
  [ "$bare" = false ] || scope_note="; the three --stress cases left out"
  [ -z "$partition" ] || scope_note="; partition $partition"
  [ -z "$archive" ] || scope_note="$scope_note; from $archive"
  [ "$list_mode" != exclude ] || scope_note="$scope_note; $(printf '%s' "$listed" | grep -c . || true) listed tests left to the --only-list run"
  [ "$list_mode" != only ] || scope_note="$scope_note; only the $(printf '%s' "$listed" | grep -c . || true) tests of $list_file, one at a time"
  if [ "$rc" -eq 0 ] && [ "$failed" -eq 0 ] && [ "$only_canaries" = true ]; then
    mg_message "core-test: canaries only: $passed passed, $skipped skipped"
    mg_status pass
  elif [ "$rc" -eq 0 ] && [ "$failed" -eq 0 ] && [ "$stress_complete" = true ] && [ "$listed_complete" = true ]; then
    mg_message "core-test: $passed passed, $skipped skipped$scope_note"
    mg_status pass
  elif [ "$stopped" = true ] && [ "$failed" -gt 0 ]; then
    mg_message "core-test: canary failed ($failed failed, $passed passed); the other tests did not run (nextest exit $rc)"
    mg_status fail
  else
    mg_message "core-test: $failed failed, $passed passed, $skipped skipped (nextest exit $rc)$scope_note"
    mg_status fail
  fi
}

# tools/mg core-lint: clippy, layering-check and the AnkiWeb scan. All three run, so one
# run reports every kind of finding.
mg_core_lint() {
  [ "${1:-}" = "--" ] && shift
  if [ "$#" -gt 0 ]; then
    mg_failure usage "core-lint takes no arguments" "tools/mg core-lint"
    mg_status error
    return 0
  fi
  mg__core_environment
  if [ ! -f core/Cargo.toml ]; then
    mg_message "core-lint: no Cargo workspace in core/ yet; nothing to lint"
    mg_status pass
    return 0
  fi

  local repro="tools/mg core-lint" failed_steps="" passed=0 failed=0 rc n

  # clippy: one failure per distinct diagnostic.
  # Antoine's delivery allowance (2026-11-12): only these three cosmetic style rules.
  local clippy="$MG_RUN_DIR/clippy.jsonl"
  (
    cd core &&
      cargo clippy --workspace --all-targets --all-features --keep-going --message-format json -- -D warnings \
        -A clippy::collapsible_if -A clippy::manual_inspect -A clippy::too_many_arguments
  ) >"$clippy"
  rc=$?
  mg_artifact clippy-jsonl "$clippy"
  local diags
  diags="$(jq -c 'select(.reason == "compiler-message") | .message
      | select((.level == "error" or .level == "warning") and (.spans | length) > 0)' "$clippy" |
    jq -sc 'unique_by(.rendered) | .[]')"
  if [ -n "$diags" ]; then
    jq -r '.rendered' <<<"$diags" >&2
    jq -c --arg repro "$repro" '([.spans[] | select(.is_primary)][0] // .spans[0]) as $s
      | {id: ("clippy/" + (.code.code // .level) + "/" + $s.file_name + ":" + ($s.line_start | tostring)),
         message: (.rendered | .[0:2000]), repro: $repro}' <<<"$diags" >>"$MG_STATE/failures.jsonl"
  fi
  if [ "$rc" -ne 0 ] || [ -n "$diags" ]; then
    [ -n "$diags" ] || mg_failure "clippy-exit-$rc" "cargo clippy exited with code $rc without a diagnostic; see log.txt" "$repro"
    failed_steps="$failed_steps clippy"
  fi

  # layering-check: the KTD2 table in core/tools/layering-check/edges.toml.
  local layering="$MG_RUN_DIR/layering.jsonl"
  if [ ! -f core/tools/layering-check/Cargo.toml ]; then
    mg_failure layering-check-missing "core/tools/layering-check is missing, so the crate layering is unchecked" "$repro"
    failed_steps="$failed_steps layering"
  else
    (cd core && cargo run -q -p layering-check -- --json) >"$layering"
    rc=$?
    mg_artifact layering-jsonl "$layering"
    jq -r '"[\(.rule)] \(.message)"' "$layering" >&2
    jq -c --arg repro "$repro" '{id: ("layering/" + .rule + "/" + .subject), message: .message, repro: $repro}' \
      "$layering" >>"$MG_STATE/failures.jsonl"
    if [ "$rc" -ne 0 ]; then
      [ -s "$layering" ] || mg_failure "layering-check-exit-$rc" "layering-check exited with code $rc; see log.txt" "$repro"
      failed_steps="$failed_steps layering"
    fi
  fi

  # ankiweb-scan (KTD21, specs/sync/login-endpoints.md L3): meguri-sync built
  # without `ankiweb` and `test-hooks` (as the App Store core is) holds no
  # AnkiWeb name in its object code. The build also checks the crate's
  # compile-time assertion that only the self-hosted server is offered.
  local scan="$MG_RUN_DIR/ankiweb-scan-build.jsonl" hits="$MG_RUN_DIR/ankiweb-scan.jsonl" rlib
  if [ -f core/crates/meguri-sync/Cargo.toml ]; then
    (cd core && cargo build -p meguri-sync --lib --message-format json-render-diagnostics) >"$scan"
    rc=$?
    rlib="$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "meguri_sync")
      | .filenames[] | select(endswith(".rlib"))' "$scan" 2>/dev/null | tail -n 1)"
    if [ "$rc" -ne 0 ] || [ -z "$rlib" ]; then
      mg_failure "ankiweb-scan-build" "building meguri-sync without its features failed (cargo exit $rc); see log.txt" "$repro"
      failed_steps="$failed_steps ankiweb"
    else
      python3 tools/scripts/ankiweb-scan.py "$rlib" >"$hits"
      rc=$?
      mg_artifact ankiweb-scan-jsonl "$hits"
      if [ "$rc" -eq 1 ]; then
        jq -c --arg repro "$repro" '{id: ("ankiweb/" + .member + ":" + (.offset | tostring)),
          message: ("meguri-sync built without the ankiweb feature holds \"" + .text + "\" in " + .member
            + " at byte " + (.offset | tostring) + "; gate it behind #[cfg(feature = \"ankiweb\")] (KTD21)"),
          repro: $repro}' "$hits" >>"$MG_STATE/failures.jsonl"
        failed_steps="$failed_steps ankiweb"
      elif [ "$rc" -ne 0 ]; then
        mg_failure "ankiweb-scan-exit-$rc" "tools/scripts/ankiweb-scan.py could not read $rlib; see log.txt" "$repro"
        failed_steps="$failed_steps ankiweb"
      fi
    fi
  fi

  for n in clippy layering ankiweb; do
    case " $failed_steps " in
      *" $n "*) failed=$((failed + 1)) ;;
      *) passed=$((passed + 1)) ;;
    esac
  done
  mg_count total 3
  mg_count passed "$passed"
  mg_count failed "$failed"
  if [ "$failed" -eq 0 ]; then
    mg_message "core-lint: clippy, layering-check and the AnkiWeb scan clean"
    mg_status pass
  else
    mg_message "core-lint: failed:$failed_steps"
    mg_status fail
  fi
}

# tools/mg core-deny [-- checks, e.g. licenses bans]: cargo deny with core/deny.toml.
mg_core_deny() {
  [ "${1:-}" = "--" ] && shift
  mg__core_environment
  if [ ! -f core/Cargo.toml ]; then
    mg_message "core-deny: no Cargo workspace in core/ yet; no dependencies to check"
    mg_status pass
    return 0
  fi
  if [ ! -f core/deny.toml ]; then
    mg_failure deny-config-missing "core/deny.toml is missing, so cargo deny would run with its permissive defaults"
    mg_status fail
    return 0
  fi

  local out="$MG_RUN_DIR/cargo-deny.jsonl" rc
  (cd core && cargo deny --format json --color never check ${1+"$@"}) 2>"$out.raw"
  rc=$?
  grep '^{' "$out.raw" >"$out"
  grep -v '^{' "$out.raw" >&2
  rm -f "$out.raw"
  mg_artifact cargo-deny-jsonl "$out"

  local errors
  errors="$(jq -c --arg repro "$MG_DEFAULT_REPRO" 'select(.type == "diagnostic" and .fields.severity == "error")
    | .fields as $f
    | ($f.graphs[0].Krate // null) as $k
    | (if $k then $k.name + "@" + $k.version else null end) as $crate
    | ([$f.labels[]?.span] | unique | join(", ")) as $spans
    | ([$f.labels[]?.message | select(. != "")] | unique | join("; ")) as $why
    | {id: ("deny/" + ($f.code // "error") + (if $crate then "/" + $crate else "" end)),
       message: ((if $crate then $crate + ": " else "" end) + $f.message
         + (if $spans != "" then ": " + $spans else "" end)
         + (if $why != "" then " (" + $why + ")" else "" end)
         + (if $f.advisory then " [" + $f.advisory.id + "]" else "" end)),
       repro: $repro}' "$out")"
  if [ -n "$errors" ]; then
    printf '%s\n' "$errors" >>"$MG_STATE/failures.jsonl"
    jq -r '.message' <<<"$errors" >&2
  fi

  local summary
  summary="$(jq -c 'select(.type == "summary") | .fields' "$out" | tail -n 1)"
  [ -n "$summary" ] || summary='{}'
  mg_detail checks "$summary"
  mg_count total "$(jq 'length' <<<"$summary")"
  mg_count passed "$(jq '[.[] | select(.errors == 0)] | length' <<<"$summary")"
  mg_count failed "$(jq '[.[] | select(.errors > 0)] | length' <<<"$summary")"
  if [ "$rc" -ne 0 ] && [ -z "$errors" ]; then
    mg_failure "cargo-deny-exit-$rc" "cargo deny exited with code $rc without an error diagnostic; see log.txt"
  fi
  if [ "$rc" -eq 0 ] && [ -z "$errors" ]; then
    mg_message "core-deny: $(jq -r 'keys | join(", ")' <<<"$summary") clean"
    mg_status pass
  else
    mg_message "core-deny: $(jq -r '[to_entries[] | select(.value.errors > 0) | "\(.value.errors) error(s) in \(.key)"] | join(", ")' <<<"$summary") (cargo deny exit $rc)"
    mg_status fail
  fi
}


mg_job core-format docker:dev mg_core_format \
  "Optionally rewrite Rust source formatting with cargo fmt --all; core-lint does not enforce formatting"
mg_core_format() {
  if [ "$#" -ne 0 ]; then
    mg_failure usage "core-format takes no arguments"
    mg_status error
    return 0
  fi
  if (cd core && cargo fmt --all); then
    mg_message "core-format: Rust sources formatted"
    mg_status pass
  else
    mg_failure rustfmt "cargo fmt failed; see log.txt"
    mg_status fail
  fi
}

mg_job core-update docker:dev mg_core_update \
  "Update one Rust dependency: core-update <package> <exact-version>; Cargo may resolve required transitive updates"
mg_core_update() {
  local LC_ALL=C
  local package_pattern='^[A-Za-z0-9][A-Za-z0-9_-]*$'
  local version_pattern='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?(\+[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?$'
  if [ "$#" -ne 2 ] || ! [[ "${1:-}" =~ $package_pattern ]] || ! [[ "${2:-}" =~ $version_pattern ]]; then
    mg_failure usage "usage: tools/mg core-update <package> <exact-version>"
    mg_status error
    return 0
  fi
  if [ ! -f core/Cargo.toml ]; then
    mg_failure workspace-missing "core/Cargo.toml is missing; no dependency was updated"
    mg_status error
    return 0
  fi

  local rc
  if (cd core && cargo update --package "$1" --precise "$2"); then
    rc=0
  else
    rc=$?
  fi
  if [ -f core/Cargo.lock ]; then
    if ! cp core/Cargo.lock "$MG_RUN_DIR/Cargo.lock"; then
      mg_failure lockfile-artifact "could not retain Cargo.lock after cargo update; see log.txt"
      mg_status fail
      return 0
    fi
    mg_artifact cargo-lock "$MG_RUN_DIR/Cargo.lock" "lockfile after cargo update (exit $rc)"
  fi
  if [ "$rc" -eq 0 ]; then
    mg_message "core-update: updated $1 to $2"
    mg_status pass
  else
    mg_failure "cargo-update-exit-$rc" "cargo update exited with code $rc; see log.txt"
    mg_status fail
  fi
}
