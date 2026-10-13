# Code-similarity scan against Anki releases (U2, R1957). Sourced by tools/mg.
#
# Host jobs: they need refs/ of this checkout (the Anki corpus), so they run
# from the main checkout and refuse to run in an implementer worktree. Each one
# starts the `similarity` service itself with the corpus mounted read-only at
# /refs, labelled with the run id so a cancelled run's container is removed.
# MG_SIMILARITY_REFS overrides the corpus directory (scanner development only).

mg_job similarity host mg_similarity \
  "Similarity scan of Meguri files against every Anki tag since 2.1.50 (R1957); --base <ref> changed files only, --rev <ref> that commit's tree, -- <paths> given files"
mg_job similarity-nightly host mg_similarity_nightly \
  "JPlag comparison of Meguri with selected Anki tags (nightly second opinion); report stays under build/"
mg_job similarity-selftest host mg_similarity_selftest \
  "Similarity scanner tests and calibration against the Anki corpus (tools/similarity/tests)"

# mg__similarity_guard: records an error and returns 1 in an implementer or
# fixer worktree (R1955: their environment never holds Anki's source).
mg__similarity_guard() {
  local role
  role="$(mg_session_role)"
  case "$role" in
    implementer | fixer)
      mg_failure implementer-worktree "this is a $role worktree: the similarity scan runs only from the main checkout, which holds refs/; the orchestrator passes back its implementer view" "tools/mg $MG_JOB (from the main checkout)"
      mg_message "$MG_JOB: refused in a $role worktree (R1955)"
      mg_status error
      return 1
      ;;
  esac
}

# mg__similarity_corpus: prints the corpus directory (holding anki/), or
# nothing when there is none.
mg__similarity_corpus() {
  local refs="${MG_SIMILARITY_REFS:-$MG_ROOT/refs}"
  [ -d "$refs/anki" ] && (cd "$refs" && pwd -P)
  return 0
}

mg__similarity_no_corpus() {
  mg_failure no-corpus "no Anki repository at ${MG_SIMILARITY_REFS:-refs}/anki; run from the main checkout, whose refs/ holds the clone" "tools/mg $MG_JOB (from the main checkout)"
  mg_message "$MG_JOB: no Anki corpus"
  mg_status error
}

# mg__similarity_run <refs dir> <command...>: run a command in the similarity
# service with the corpus mounted.
mg__similarity_run() {
  local refs="$1"
  shift
  if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
    mg_failure docker-unreachable "Docker is not reachable; start Docker Desktop" "tools/mg doctor"
    mg_status error
    return 125
  fi
  echo "tools/mg: building image for service 'similarity'" >&2
  if ! mg_compose --progress plain build similarity >&2; then
    mg_failure image-build "building the 'similarity' image failed; see log.txt" \
      "docker compose -p $(mg_compose_project) -f docker/compose.yml build similarity"
    mg_status error
    return 125
  fi
  mg_compose run --rm -T --no-deps --label "meguri.mg.run-id=$MG_RUN_ID" \
    -v "$refs:/refs:ro" similarity "$@" </dev/null
}

# Files a full scan considers: tracked plus untracked, not ignored.
mg__similarity_all_files() {
  git -C "$1" ls-files -z --cached --others --exclude-standard | tr '\0' '\n'
}

mg_similarity() {
  local base="" rev="" rebuild="" paths=() a
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --base) base="${2:-}"; shift 2 || shift ;;
      --base=*) base="${1#--base=}"; shift ;;
      --rev) rev="${2:-}"; shift 2 || shift ;;
      --rev=*) rev="${1#--rev=}"; shift ;;
      --rebuild-index) rebuild=--rebuild; shift ;;
      --) shift; for a in "$@"; do paths[${#paths[@]}]="$a"; done; break ;;
      *)
        mg_failure usage "unknown argument '$1'; usage: tools/mg similarity [--base <ref>] [--rev <ref>] [--rebuild-index] [-- paths...]"
        mg_status error
        return 0
        ;;
    esac
  done
  mg__similarity_guard || return 0
  local refs
  refs="$(mg__similarity_corpus)"
  [ -n "$refs" ] || { mg__similarity_no_corpus; return 0; }

  local list="$MG_RUN_DIR/files.txt" root_rel="." rel="${MG_RUN_DIR#"$MG_ROOT"/}"
  : >"$list"
  if [ -n "$rev" ]; then
    # Scan a commit's tree: materialize its files (and its interop vocabulary)
    # under the run directory.
    if ! git -C "$MG_ROOT" rev-parse --verify -q "$rev^{commit}" >/dev/null; then
      mg_failure bad-rev "'$rev' is not a commit"
      mg_status error
      return 0
    fi
    if [ "${#paths[@]}" -gt 0 ]; then
      printf '%s\n' "${paths[@]}" >"$list"
    elif [ -n "$base" ]; then
      git -C "$MG_ROOT" diff --name-only --no-renames --diff-filter=d "$base" "$rev" >"$list" || {
        mg_failure bad-base "git diff $base $rev failed"
        mg_status error
        return 0
      }
    else
      git -C "$MG_ROOT" ls-tree -r --name-only "$rev" >"$list"
    fi
    # Through a private index, so the checkout's own index and files stay
    # untouched; paths that do not exist at the commit are skipped.
    local index="$MG_RUN_DIR/rev-index"
    mkdir -p "$MG_RUN_DIR/tree"
    GIT_INDEX_FILE="$index" git -C "$MG_ROOT" read-tree "$rev" || {
      mg_failure bad-rev "git read-tree $rev failed"
      mg_status error
      return 0
    }
    GIT_INDEX_FILE="$index" git -C "$MG_ROOT" ls-files | grep -Fx -f "$list" |
      { cat; GIT_INDEX_FILE="$index" git -C "$MG_ROOT" ls-files -- 'specs/*/interop-vocab.txt'; } |
      sort -u | tr '\n' '\0' |
      GIT_INDEX_FILE="$index" git -C "$MG_ROOT" checkout-index -z --stdin --prefix="$MG_RUN_DIR/tree/"
    rm -f "$index"
    root_rel="$rel/tree"
  elif [ "${#paths[@]}" -gt 0 ]; then
    printf '%s\n' "${paths[@]}" >"$list"
  elif [ -n "$base" ]; then
    if ! git -C "$MG_ROOT" rev-parse --verify -q "$base^{commit}" >/dev/null; then
      mg_failure bad-base "'$base' is not a commit"
      mg_status error
      return 0
    fi
    { git -C "$MG_ROOT" diff --name-only --no-renames --diff-filter=d "$base"; git -C "$MG_ROOT" ls-files --others --exclude-standard; } |
      sort -u >"$list"
  else
    mg__similarity_all_files "$MG_ROOT" >"$list"
  fi

  local out="$MG_RUN_DIR/scan" rc
  mg__similarity_run "$refs" python3 -m meguri_similarity scan \
    --refs /refs --cache /work/build/similarity-index \
    --root "/work/$root_rel" --vocab-root "/work/$root_rel" \
    --files-from "/work/$rel/files.txt" --out "/work/$rel/scan" $rebuild
  rc=$?
  [ "$rc" -eq 125 ] && return 0
  mg__similarity_record "$out/summary.json" "$rc"
}

# mg__similarity_record <summary.json> <scanner exit code>
mg__similarity_record() {
  local summary="$1" rc="$2"
  if [ ! -f "$summary" ]; then
    mg_failure scanner-exit "the scanner exited with code $rc without a summary; see log.txt"
    mg_status error
    return 0
  fi
  local dir
  dir="$(dirname "$summary")"
  mg_artifact similarity-implementer "$dir/implementer.md" "implementer view"
  mg_artifact similarity-implementer-json "$dir/implementer.json"
  mg_artifact similarity-reviewer "$dir/reviewer.md" "reviewer view (names Anki paths; human review only)"
  mg_artifact similarity-reviewer-json "$dir/reviewer.json"
  mg_count total "$(jq '.counts.scanned' "$summary")"
  mg_count passed "$(jq '.counts.clean + .counts.allowlisted' "$summary")"
  mg_count failed "$(jq '.counts.flagged + .counts.errors' "$summary")"
  mg_count skipped "$(jq '.counts.skipped' "$summary")"
  mg_count allowlisted "$(jq '.counts.allowlisted' "$summary")"
  mg_detail corpus "$(jq -c '.corpus' "$summary")"
  mg_detail timing "$(jq -c '.timing' "$summary")"
  mg_detail counts "$(jq -c '.counts' "$summary")"
  jq -c '.flags[] | {
      id: ("similarity:" + .path + ":" + .channel),
      message: (.path + " lines " + ([.regions[] | "\(.[0])-\(.[1])"] | join(", ")) + ": " + (.reasons | join("; ")) + "; rewrite this region from the spec"),
      repro: ("tools/mg similarity -- " + (.path | @sh))}' "$summary" >>"$MG_STATE/failures.jsonl"
  jq -c '.errors[] | {id: "similarity:error", message: ., repro: "tools/mg similarity"}' "$summary" >>"$MG_STATE/failures.jsonl"
  local status note
  status="$(jq -r '.status' "$summary")"
  note="$(jq -r '.corpus | if (.missing_boundary_tags | length) > 0 then "; corpus lacks tags " + (.missing_boundary_tags | join(", ")) else "" end' "$summary")"
  mg_message "$(jq -r '"similarity: \(.counts.scanned) files scanned, \(.counts.flagged) flagged, \(.counts.allowlisted) allowlisted; corpus \(.corpus.tags) tags \(.corpus.first_tag)..\(.corpus.last_tag)"' "$summary")$note"
  case "$status" in
    pass | fail | error) mg_status "$status" ;;
    *) mg_status error ;;
  esac
}

mg_similarity_nightly() {
  local refs rel="${MG_RUN_DIR#"$MG_ROOT"/}" rc
  mg__similarity_guard || return 0
  refs="$(mg__similarity_corpus)"
  [ -n "$refs" ] || { mg__similarity_no_corpus; return 0; }
  mg__similarity_all_files "$MG_ROOT" >"$MG_RUN_DIR/files.txt"
  mg__similarity_run "$refs" python3 -m meguri_similarity jplag \
    --refs /refs --root /work --files-from "/work/$rel/files.txt" --out "/work/$rel/jplag"
  rc=$?
  [ "$rc" -eq 125 ] && return 0
  local summary="$MG_RUN_DIR/jplag/jplag-summary.json"
  if [ ! -f "$summary" ]; then
    mg_failure jplag-exit "the JPlag run exited with code $rc without a summary; see log.txt"
    mg_status error
    return 0
  fi
  local report
  for report in "$MG_RUN_DIR"/jplag/*.jplag; do
    [ -f "$report" ] && mg_artifact jplag-report "$report" "open with: java -jar jplag.jar -M VIEW <file> (embeds Anki sources; main checkout only)"
  done
  mg_detail jplag "$(jq -c . "$summary")"
  jq -c '.languages | to_entries[] | select(.value.error) |
    {id: ("jplag:" + .key), message: .value.error, repro: "tools/mg similarity-nightly"}' "$summary" >>"$MG_STATE/failures.jsonl"
  mg_message "$(jq -r '"similarity-nightly: " + ([.languages | to_entries[] | "\(.key) " + (if .value.skipped then "skipped (\(.value.skipped))" elif .value.error then "error" else "max \(.value.max_similarity * 100 | floor)%" end)] | join(", ")) + "; tags " + (.tags | join(" "))' "$summary")"
  mg_status "$(jq -r '.status' "$summary")"
}

mg_similarity_selftest() {
  local refs tap="$MG_RUN_DIR/similarity-selftest.tap" rel="${MG_RUN_DIR#"$MG_ROOT"/}" rc
  mg__similarity_guard || return 0
  refs="$(mg__similarity_corpus)"
  if [ -z "$refs" ]; then
    # Without a corpus the unit tests still run; calibration tests skip.
    refs="$MG_RUN_DIR/no-corpus"
    mkdir -p "$refs"
  fi
  mg__similarity_run "$refs" python3 tools/similarity/tests/run_tests.py --out "/work/$rel" ${1+"$@"} >"$tap"
  rc=$?
  [ "$rc" -eq 125 ] && return 0
  cat "$tap" >&2
  mg_artifact tap "$tap"
  if [ -f "$MG_RUN_DIR/calibration.json" ]; then
    mg_artifact calibration "$MG_RUN_DIR/calibration.json" "calibration scores"
    mg_detail calibration "$(jq -c . "$MG_RUN_DIR/calibration.json")"
  fi
  mg_tap_report "$tap" "$rc" "tools/mg similarity-selftest -k"
  if [ ! -d "$refs/anki" ] && [ "$(head -n 1 "$MG_STATE/status")" = pass ]; then
    mg_message "$(cat "$MG_STATE/message") (no Anki corpus: calibration skipped; run from the main checkout)"
    mg_status warn
  fi
}
