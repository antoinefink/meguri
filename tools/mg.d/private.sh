# The private collection vault (U15, R1938–R1943). Sourced by tools/mg.
#
#   tools/mg private status                 what the vault has: source, copies, token set
#   tools/mg private copy                   a copy in build/private/<run>/copy, and the token set
#   tools/mg private suite [--record <name>] [--keep]
#                                           the private suite on a fresh copy; summary of pass counts
#   tools/mg private scan-outputs [path...] files (default: the staged lines) holding a private token
#
# A host job. The only source is local-data/anki/ of this checkout (the main
# checkout: no worktree has local-data/), and of it only collection.anki2,
# its -wal and collection.media/ are opened (meguri_testkit::private::vault).
# status, copy and suite refuse when a CI marker is set (R1939) and in
# implementer and fixer worktrees (R1955). meguri-cli is built in the dev
# container and runs in the `private` service: no network, the source
# mounted read-only at /vault for that run, local-data/ hidden. Copies, logs
# and the token set stay in build/private/ (gitignored); job results and logs
# carry counts, sizes, hashes and run paths only, never collection text.
#
# The token set (build/private/tokens/tokens.txt) is rebuilt from each copy:
# meguri-cli private tokens lists the candidate strings, and
# tools/scripts/private-scan/private_scan.py keeps those not already in the
# repository. scan-outputs and the pre-commit hook (tools/hooks/pre-commit)
# check files against it and name files and lines, never a token.
#
# suite runs the checks meguri-cli registers (U249 adds the R1942 checks),
# scans the summary and only then, with --record, writes it to
# conformance/reports/private/<name>.json. Its copy is deleted at the end
# unless --keep.

mg_job private host mg_private \
  "Private collection vault (local-data/anki/, read-only, never off this Mac): status | copy | suite [--record <name>] [--keep] | scan-outputs [paths...] (default: the staged lines); copies and the token set stay in build/private/"

# Environment variables that mark a CI or other remote machine. Keep in step
# with CI_MARKERS in core/crates/meguri-testkit/src/private/vault.rs
# (tools/tests/private.bats compares them).
MG_PRIVATE_CI_MARKERS="CI CONTINUOUS_INTEGRATION BUILDKITE BITBUCKET_BUILD_NUMBER CIRCLECI CI_XCODE_PROJECT CODEBUILD_BUILD_ID DRONE GITHUB_ACTIONS GITLAB_CI JENKINS_URL TEAMCITY_VERSION TF_BUILD TRAVIS"
MG_PRIVATE_SOURCE=local-data/anki
MG_PRIVATE_DIR=build/private
MG_PRIVATE_TOKENS=build/private/tokens/tokens.txt
MG_PRIVATE_REPORTS=conformance/reports/private
MG_PRIVATE_SCAN=tools/scripts/private-scan/private_scan.py
# meguri-cli as the dev service builds it (profile `fixtures`), seen from the
# private service, which mounts the same target volume.
MG_PRIVATE_CLI=/cargo-target/fixtures/meguri-cli

mg__private_usage() {
  mg_failure usage "$1; usage: tools/mg private status | copy | suite [--record <name>] [--keep] | scan-outputs [paths...]"
  mg_message "private: usage error"
  mg_status error
}

# mg__private_ci_marker: prints the first CI marker set to a non-empty value.
mg__private_ci_marker() {
  local name value
  for name in $MG_PRIVATE_CI_MARKERS; do
    eval "value=\${$name:-}"
    if [ -n "$value" ]; then
      printf '%s\n' "$name"
      return 0
    fi
  done
  return 0
}

# mg__private_guard: records an error and returns 1 when the vault may not
# be used here.
mg__private_guard() {
  local marker role
  marker="$(mg__private_ci_marker)"
  if [ -n "$marker" ]; then
    mg_failure ci-marker "refused: the CI marker $marker is set; the private collection is used only on this Mac, never on a remote machine (R1939)"
    mg_message "private: refused on a CI machine ($marker)"
    mg_status error
    return 1
  fi
  role="$(mg_session_role)"
  case "$role" in
    implementer | fixer)
      mg_failure implementer-worktree "this worktree's role is $role: implementers and fixers never read the private collection (R1955); the vault runs from the main checkout" "tools/mg private (from the main checkout)"
      mg_message "private: refused in a worktree of role $role"
      mg_status error
      return 1
      ;;
  esac
}

# mg__private_source_ok: whether the source holds a regular collection file.
# Only stats it; the vault opens it, in the container.
mg__private_source_ok() {
  [ -f "$MG_ROOT/$MG_PRIVATE_SOURCE/collection.anki2" ] && [ ! -L "$MG_ROOT/$MG_PRIVATE_SOURCE/collection.anki2" ] &&
    [ ! -L "$MG_ROOT/$MG_PRIVATE_SOURCE" ]
}

mg__private_no_source() {
  mg_failure no-source "no $MG_PRIVATE_SOURCE/collection.anki2 in this checkout; the private collection is in the main checkout's local-data/ (never committed)" "tools/mg private status"
  mg_message "private: no private collection here"
  mg_status error
}

mg__private_docker() {
  if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    mg_status error
    return 1
  fi
  local service
  for service in dev private; do
    echo "tools/mg: building image for service '$service'" >&2
    if ! mg_compose --progress plain build "$service" >&2; then
      mg_failure image-build "building the '$service' image failed; see log.txt" \
        "docker compose -p $(mg_compose_project) -f docker/compose.yml build $service"
      mg_status error
      return 1
    fi
  done
  # meguri-cli, built without the collection in reach.
  if ! mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" dev \
    bash -c 'cd core && cargo build --profile fixtures -p meguri-cli' </dev/null >&2; then
    mg_failure cli-build "building meguri-cli (profile fixtures) failed; see log.txt"
    mg_status error
    return 1
  fi
}

# mg__private_run <script> [args...]: runs `sh -c <script> sh <meguri-cli>
# /vault [args...]` in the private service, the source mounted read-only at
# /vault. Paths the script writes are under /work (the checkout).
mg__private_run() {
  local script="$1"
  shift
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
    -v "$MG_ROOT/$MG_PRIVATE_SOURCE:/vault:ro" \
    private sh -c "$script" sh "$MG_PRIVATE_CLI" /vault ${1+"$@"} </dev/null
}

# mg__private_copy <run dir, relative>: copies the source into <run>/copy and
# rebuilds the token set from it. Sets MG__PRIVATE_COPY (copy.json) and
# MG__PRIVATE_TOKENSET (the token set's counts); records an error and
# returns 1 on failure.
mg__private_copy() {
  local run="$1" rc
  mkdir -p "$MG_ROOT/$run" || {
    mg_failure run-dir "could not create $run"
    mg_status error
    return 1
  }
  chmod 700 "$MG_ROOT/$MG_PRIVATE_DIR" "$MG_ROOT/$run" 2>/dev/null
  mg__private_run '"$1" private copy --source "$2" --out "$3/copy" --access-log "$3/vault-access.log" >"$3/copy.json"' "/work/$run"
  rc=$?
  if [ "$rc" -ne 0 ] || ! jq -e .collection.sha256 "$MG_ROOT/$run/copy.json" >/dev/null 2>&1; then
    mg_failure copy "the vault copy failed (exit $rc); see log.txt"
    mg_status error
    return 1
  fi
  MG__PRIVATE_COPY="$(jq -c . "$MG_ROOT/$run/copy.json")"
  mg__private_run '"$1" private tokens --copy "$3/copy" --out "$3/candidates.jsonl" >"$3/candidates.json"' "/work/$run"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    rm -f "$MG_ROOT/$run/candidates.jsonl"
    mg_failure tokens "listing the token candidates failed (exit $rc); see log.txt"
    mg_status error
    return 1
  fi
  mkdir -p "$MG_ROOT/$(dirname "$MG_PRIVATE_TOKENS")"
  python3 -I -B "$MG_ROOT/$MG_PRIVATE_SCAN" build --candidates "$MG_ROOT/$run/candidates.jsonl" \
    --repo "$MG_ROOT" --out "$MG_ROOT/$MG_PRIVATE_TOKENS" >"$MG_ROOT/$run/token-set.json"
  rc=$?
  rm -f "$MG_ROOT/$run/candidates.jsonl"
  if [ "$rc" -ne 0 ]; then
    mg_failure token-set "building the token set failed (exit $rc); see log.txt"
    mg_status error
    return 1
  fi
  MG__PRIVATE_TOKENSET="$(jq -c . "$MG_ROOT/$run/token-set.json")"
}

mg__private_status() {
  local src="$MG_ROOT/$MG_PRIVATE_SOURCE" present=false bytes=0 wal=false media=false copies tokens
  if mg__private_source_ok; then
    present=true
    # ls stats the files without opening them.
    bytes="$(ls -ln "$src/collection.anki2" | awk '{print $5}')"
    [ -f "$src/collection.anki2-wal" ] && wal=true
    [ -d "$src/collection.media" ] && [ ! -L "$src/collection.media" ] && media=true
  fi
  copies="$(
    for d in "$MG_ROOT/$MG_PRIVATE_DIR"/*/copy; do
      [ -d "$d" ] || continue
      d="${d%/copy}"
      printf '%s\n' "${d#"$MG_ROOT"/}"
    done | jq -R . | jq -sc .
  )"
  tokens=null
  if [ -f "$MG_ROOT/$MG_PRIVATE_TOKENS" ]; then
    tokens="$(jq -c --arg p "$MG_PRIVATE_TOKENS" '{path: $p} + .' "$MG_ROOT/$(dirname "$MG_PRIVATE_TOKENS")/meta.json" 2>/dev/null ||
      jq -nc --arg p "$MG_PRIVATE_TOKENS" '{path: $p}')"
  fi
  mg_detail source "$(jq -nc --arg p "$MG_PRIVATE_SOURCE" --argjson present "$present" --argjson bytes "${bytes:-0}" \
    --argjson wal "$wal" --argjson media "$media" \
    '{path: $p, present: $present, collection_bytes: $bytes, wal: $wal, media_folder: $media}')"
  mg_detail copies "$copies"
  mg_detail token_set "$tokens"
  if [ "$present" = true ]; then
    mg_message "private: source present, $(jq length <<<"$copies") copies, token set $([ "$tokens" = null ] && echo missing || echo present)"
  else
    mg_message "private: no source at $MG_PRIVATE_SOURCE"
  fi
  mg_status pass
}

mg__private_copy_job() {
  [ "$#" -eq 0 ] || {
    mg__private_usage "copy takes no arguments"
    return 0
  }
  mg__private_source_ok || {
    mg__private_no_source
    return 0
  }
  mg__private_docker || return 0
  local run="$MG_PRIVATE_DIR/$MG_RUN_ID"
  if ! mg__private_copy "$run"; then
    rm -rf "$MG_ROOT/$run/copy"
    return 0
  fi
  mg_detail copy "$(jq -nc --arg p "$run/copy" --argjson c "$MG__PRIVATE_COPY" '{path: $p} + $c')"
  mg_detail token_set "$(jq -nc --arg p "$MG_PRIVATE_TOKENS" --argjson t "$MG__PRIVATE_TOKENSET" '{path: $p} + $t')"
  mg_artifact copy "$MG_ROOT/$run/copy" "copy of the private collection (never commit or send it)"
  mg_message "private: copy at $run/copy, token set rebuilt"
  mg_status pass
}

# mg__private_scan <json out> <scan args...>: runs the scanner with every
# token set this checkout sees (the main checkout's and its own). Returns
# its exit code, 3 when there is no token set.
mg__private_scan() {
  local out="$1" common main sets=() f
  shift
  common="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || common=""
  main="${common:+$(dirname "$common")}"
  for f in "${main:+$main/$MG_PRIVATE_TOKENS}" "$MG_ROOT/$MG_PRIVATE_TOKENS"; do
    [ -n "$f" ] && [ -f "$f" ] || continue
    [ "${#sets[@]}" -gt 0 ] && [ "${sets[1]}" = "$f" ] && continue
    sets[${#sets[@]}]=--tokens
    sets[${#sets[@]}]="$f"
  done
  [ "${#sets[@]}" -gt 0 ] || return 3
  python3 -I -B "$MG_ROOT/$MG_PRIVATE_SCAN" scan "${sets[@]}" ${1+"$@"} >"$out"
}

# mg__private_report_scan <scan json>: one failure per file holding a token.
mg__private_report_scan() {
  jq -c '.files[]' "$1" | while IFS= read -r file; do
    local path where
    path="$(jq -r .path <<<"$file")"
    where="$(jq -r 'if .binary then "binary file" else "line " + (.lines | map(tostring) | join(", ")) end' <<<"$file")"
    # mg__quote_args starts each word with a space.
    mg_failure "private-token:$path" "$path holds a private token ($where); the token is never named" \
      "tools/mg private scan-outputs$(mg__quote_args "$path")"
  done
}

mg__private_scan_outputs() {
  local out="$MG_RUN_DIR/scan.json" rc
  if [ "$#" -eq 0 ]; then
    mg__private_scan "$out" --staged --repo "$MG_ROOT"
  else
    mg__private_scan "$out" "$@"
  fi
  rc=$?
  case "$rc" in
    0 | 1)
      mg_artifact report "$out" "files holding a private token (no tokens)"
      mg_detail scan "$(jq -c '{files, files_scanned, tokens}' "$out")"
      mg_count total "$(jq .files_scanned "$out")"
      mg_count failed "$(jq '.files | length' "$out")"
      ;;
  esac
  case "$rc" in
    0)
      mg_message "private: no private token in $(jq .files_scanned "$out") files"
      mg_status pass
      ;;
    1)
      mg__private_report_scan "$out"
      mg_message "private: $(jq '.files | length' "$out") files hold a private token (R1940)"
      mg_status fail
      ;;
    3)
      mg_failure no-token-set "no private token set ($MG_PRIVATE_TOKENS of the main checkout or this one); tools/mg private copy builds it" "tools/mg private copy"
      mg_message "private: no token set"
      mg_status error
      ;;
    *)
      mg_failure scan "the scanner failed (exit $rc); see log.txt"
      mg_status error
      ;;
  esac
}

mg__private_suite() {
  local record="" keep=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --record)
        record="${2:-}"
        shift 2 || shift
        ;;
      --keep)
        keep=true
        shift
        ;;
      *)
        mg__private_usage "unknown suite argument '$1'"
        return 0
        ;;
    esac
  done
  if [ -n "$record" ] && ! printf '%s' "$record" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$'; then
    mg__private_usage "--record takes a name of letters, digits, '.', '_' and '-' (such as M1)"
    return 0
  fi
  mg__private_source_ok || {
    mg__private_no_source
    return 0
  }
  mg__private_docker || return 0
  local run="$MG_PRIVATE_DIR/$MG_RUN_ID" rc
  [ "$keep" = true ] || mg_on_exit "rm -rf '$MG_ROOT/$run/copy'"
  mg__private_copy "$run" || return 0
  mg__private_run '"$1" private suite --copy "$3/copy" --out "$3/summary.json" --log "$3/suite.log" >/dev/null' "/work/$run"
  rc=$?
  if [ "$rc" -gt 1 ] || ! jq -e '.report_version' "$MG_ROOT/$run/summary.json" >/dev/null 2>&1; then
    mg_failure suite "the private suite did not run (exit $rc); see log.txt"
    mg_status error
    return 0
  fi
  # The summary leaves build/private/ only after the scan.
  local scan="$MG_RUN_DIR/summary-scan.json" scan_rc
  mg__private_scan "$scan" "$MG_ROOT/$run/summary.json"
  scan_rc=$?
  if [ "$scan_rc" -ne 0 ]; then
    if [ "$scan_rc" -eq 1 ]; then
      mg_failure summary-private-token "the suite's summary holds a private token; nothing was recorded (R1940)"
      mg_status fail
    else
      mg_failure scan "scanning the summary failed (exit $scan_rc); nothing was recorded"
      mg_status error
    fi
    return 0
  fi
  local summary
  summary="$(jq -c . "$MG_ROOT/$run/summary.json")"
  mg_detail summary "$summary"
  mg_detail copy "$(jq -nc --argjson c "$MG__PRIVATE_COPY" --argjson kept "$keep" --arg p "$run/copy" '{path: $p, kept: $kept} + $c')"
  if [ -n "$record" ]; then
    mkdir -p "$MG_ROOT/$MG_PRIVATE_REPORTS"
    cp "$MG_ROOT/$run/summary.json" "$MG_ROOT/$MG_PRIVATE_REPORTS/$record.json"
    mg_artifact report "$MG_ROOT/$MG_PRIVATE_REPORTS/$record.json" "private suite summary (pass counts only)"
  fi
  jq -r '.checks | to_entries[] | select(.value.error == true or (.value.failed // 0) > 0) | .key' <<<"$summary" |
    while IFS= read -r name; do
      mg_failure "check:$name" "private check $name: $(jq -c --arg n "$name" '.checks[$n]' <<<"$summary")"
    done
  mg_count total "$(jq '.checks | length' <<<"$summary")"
  mg_count failed "$(jq '[.checks[] | select(.error == true or (.failed // 0) > 0)] | length' <<<"$summary")"
  if [ "$(jq '.checks | length' <<<"$summary")" -eq 0 ]; then
    mg_message "private: suite ran no check (none registered yet; U249 adds them)"
    mg_status warn
  elif [ "$rc" -eq 0 ]; then
    mg_message "private: suite passed ($(jq '.checks | length' <<<"$summary") checks)"
    mg_status pass
  else
    mg_message "private: suite failed"
    mg_status fail
  fi
}

mg_private() {
  local command="${1:-}"
  [ "$#" -eq 0 ] || shift
  case "$command" in
    scan-outputs)
      mg__private_scan_outputs "$@"
      return 0
      ;;
    status | copy | suite) ;;
    "")
      mg__private_usage "a command is required"
      return 0
      ;;
    *)
      mg__private_usage "unknown command '$command'"
      return 0
      ;;
  esac
  mg__private_guard || return 0
  case "$command" in
    status) mg__private_status ;;
    copy) mg__private_copy_job "$@" ;;
    suite) mg__private_suite "$@" ;;
  esac
}
