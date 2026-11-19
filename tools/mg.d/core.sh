# Rust core jobs. Sourced by tools/mg; they run inside the `dev` container.

mg_job core-test docker:dev mg_core_test \
  "Rust tests: cargo nextest on core/ (no retries), canary_* first; --canaries only, --quick excludes three stress cases, --stress requires those three; args after -- go to nextest"
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
  mg_core_test -- --test ports --run-ignored only -E 'package(meguri-host) & test(=milestone_provider_owners)'
}

# Canary test binaries (tests/canary_*.rs) run first and alone in a bare
# core-test; when one fails, the other tests do not run (AE82 ordering).
# The canary phase builds only the packages that hold a canary, so a build
# error elsewhere (say, code broken by a pin bump) cannot hide the pin canary.
# nextest rejects a binary() filter that matches no binary, so the canary
# phase runs only when a canary exists.
MG_CORE_CANARIES='binary(/^canary_/)'

# Exact identities shared by quick's exclusion and stress's selection. Keep
# nextest's existing scheduling overrides; neither mode changes the profile.
MG_CORE_STRESS='(package(=meguri-engine) & binary(=session_predict) & test(=c8_all_four_actual_choices_in_ten_thousand_random_multi_answer_sessions))
  | (package(=meguri-sync) & binary(=normal_sync) & test(=large::suspend_two_seconds_with_five_queued_answers_rolls_back_merge_at_150k))
  | (package(=meguri-sync) & binary(=normal_sync) & test(=large::merge_150k_updates_preserves_real_overlay_answers_and_write_budget_at_150k))'

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

# tools/mg core-test [--canaries | --quick | --stress | -- nextest args]
# --canaries runs the canary phase alone (the gate's canary stage, U19) and
# fails when the workspace has no canary. Quick keeps that ordering; stress
# runs only its exact three cases. Both opt into the FFI test hooks locally.
mg_core_test() {
  local only_canaries=false mode=default arg
  case "${1:-}" in
    --canaries|--quick|--stress)
      mode="${1#--}"
      shift
      if [ "$#" -gt 0 ]; then
        mg_failure usage "--$mode takes no other arguments" "tools/mg core-test --$mode"
        mg_status error
        return 0
      fi
      [ "$mode" != canaries ] || only_canaries=true
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
  local files=() canaries=() features=()
  case "$mode" in
    quick|stress) features=(--features meguri-ffi/test-hooks) ;;
  esac
  if [ "$mode" = quick ]; then
    ordinary_filter="$ordinary_filter & not ($MG_CORE_STRESS)"
    set -- --no-tests=pass -E "not ($MG_CORE_STRESS)"
  elif [ "$mode" = stress ]; then
    set -- -E "$MG_CORE_STRESS"
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
    mg__nextest "$canary" "${canaries[@]}" --no-tests=pass -E "$MG_CORE_CANARIES"
    rc=$?
    mg_artifact nextest-canary-jsonl "$canary"
    files=("$canary")
    if [ "$rc" -ne 0 ]; then
      stopped=true
    elif [ "$only_canaries" = true ]; then
      :
    else
      mg__nextest "$main" --workspace ${features[0]+"${features[@]}"} --no-tests=pass -E "$ordinary_filter"
      rc=$?
      mg_artifact nextest-jsonl "$main"
      files=("$canary" "$main")
    fi
  else
    [ "$#" -gt 0 ] || set -- --no-tests=pass
    mg__nextest "$main" --workspace ${features[0]+"${features[@]}"} "$@"
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
    if stress_tests="$(jq -ces '
      [.[] | select(.type == "test" and .event != "started")] as $tests
      | ([$tests[].name] | sort) as $names
      | select(($tests | length) == 3 and all($tests[]; .event == "ok")
        and $names == ([
          "meguri-engine::session_predict$c8_all_four_actual_choices_in_ten_thousand_random_multi_answer_sessions",
          "meguri-sync::normal_sync$large::suspend_two_seconds_with_five_queued_answers_rolls_back_merge_at_150k",
          "meguri-sync::normal_sync$large::merge_150k_updates_preserves_real_overlay_answers_and_write_budget_at_150k"
        ] | sort))
      | $names' "$main")"; then
      mg_detail stress_tests "$stress_tests"
    else
      stress_complete=false
      mg_failure stress-selection "stress requires exactly the three expected cases to pass, with none missing or skipped" "tools/mg core-test --stress"
    fi
  fi
  if [ "$rc" -eq 0 ] && [ "$failed" -eq 0 ] && [ "$only_canaries" = true ]; then
    mg_message "core-test: canaries only: $passed passed, $skipped skipped"
    mg_status pass
  elif [ "$rc" -eq 0 ] && [ "$failed" -eq 0 ] && [ "$stress_complete" = true ]; then
    mg_message "core-test: $passed passed, $skipped skipped"
    mg_status pass
  elif [ "$stopped" = true ] && [ "$failed" -gt 0 ]; then
    mg_message "core-test: canary failed ($failed failed, $passed passed); the other tests did not run (nextest exit $rc)"
    mg_status fail
  else
    mg_message "core-test: $failed failed, $passed passed, $skipped skipped (nextest exit $rc)"
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
