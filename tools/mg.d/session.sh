# Agent session records (U2, R1958). Sourced by tools/mg.
#
# docs/process/provenance/sessions.jsonl is append-only: one JSON object per
# agent session, with its role, base commit, what it could read (sparse
# exclusions, deny-profile hash) and the version of its role instructions.
# Every record is committed where it is appended, that file only, so no
# checkout is left with an uncommitted ledger and records reach the
# integration branch with their branch (union merge driver, .gitattributes).
# `tools/mg worktree new` commits a record for each worktree it creates as
# that branch's first commit, and also writes it to <worktree git
# dir>/meguri-session.json, which marks the worktree's role for other jobs (the
# similarity scan refuses implementer worktrees). `tools/mg session record`
# covers sessions without a worktree, on the current branch of the checkout it
# runs in.

mg_job session host mg_session \
  "Agent session records (R1958): tools/mg session record --role <role> [--name <name>] [--note <text>]"

MG_ROLES="spec-writer implementer fixer oracle-maintainer infra critic integrator"
MG_SESSIONS_FILE="docs/process/provenance/sessions.jsonl"

# mg_word_in <value> <space-separated words>: the value is one of the words,
# exactly (a compound value such as "implementer fixer" is none of them).
mg_word_in() {
  local w
  for w in $2; do
    [ "$1" = "$w" ] && return 0
  done
  return 1
}

mg_role_valid() { mg_word_in "$1" "$MG_ROLES"; }

# Role instructions per role (fixers work under the implementer rules).
mg_role_doc() {
  case "$1" in
    fixer) echo docs/process/roles/implementer.md ;;
    spec-writer | implementer | oracle-maintainer | critic) echo "docs/process/roles/$1.md" ;;
    *) echo "" ;;
  esac
}

# Role of this checkout from its session marker; empty in the main checkout
# and in worktrees not made by `tools/mg worktree new`.
mg_session_role() {
  local dir
  dir="$(git -C "${1:-$MG_ROOT}" rev-parse --absolute-git-dir 2>/dev/null)" || return 0
  [ -f "$dir/meguri-session.json" ] && jq -r '.role // empty' "$dir/meguri-session.json" 2>/dev/null
  return 0
}

mg_session_new_id() {
  printf 's-%s-%s\n' "$(date -u +%Y%m%dT%H%M%SZ)" "$(od -An -N4 -tx1 /dev/urandom | tr -d ' \n')"
}

# mg_session_store <checkout> <record JSON>: appends the record to the
# checkout's ledger and commits it on the checkout's current branch, that path
# only (other staged changes stay staged; the commit goes through the
# commit-msg hook like any other). Records made in parallel (critic sessions,
# spec writers in the main checkout) would race on git's index.lock: one run
# fails while another commits its line, or a line stays uncommitted. A mkdir
# lock in the repository's common git dir serializes append and commit across
# all its checkouts, waiting at most MG_SESSION_LOCK_SECONDS. A commit that
# fails is tried again, since another git command may hold index.lock for a
# moment; once HEAD's ledger has the line (a commit that was not a session
# record took it along), the record is stored. On failure sets
# MG_SESSION_FAILURE (failure id) and MG_SESSION_MESSAGE and returns 1.
mg_session_store() {
  local id role name
  id="$(jq -r .session <<<"$2")"
  role="$(jq -r .role <<<"$2")"
  name="$(jq -r '.name // ""' <<<"$2")"
  mg_ledger_store "$1" "$MG_SESSIONS_FILE" "$2" "Record session $id ($role${name:+, $name})

Role: infra
Session: $id"
}

# mg_ledger_store <checkout> <ledger path> <record JSON> <commit message>:
# mg_session_store for any append-only ledger under docs/process/provenance/
# (sessions, escalations). One lock for all of them: they commit in the same
# checkouts. Same failure contract as mg_session_store.
mg_ledger_store() {
  local dir="$1" rel="$2" record="$3" message="$4" file="$1/$2" line lock waited=0 tries=0
  MG_SESSION_FAILURE=session-ledger
  MG_SESSION_MESSAGE="could not append the record to $rel; see log.txt"
  line="$(jq -c . <<<"$record")" && [ -n "$line" ] || return 1
  lock="$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir)/meguri-session.lock" || return 1
  trap 'mg__session_unlock; exit 143' TERM
  trap 'mg__session_unlock; exit 129' HUP
  trap 'mg__session_unlock' EXIT
  until mkdir "$lock" 2>/dev/null; do
    if [ "$waited" -ge $((${MG_SESSION_LOCK_SECONDS:-60} * 10)) ]; then
      trap - TERM HUP EXIT
      MG_SESSION_FAILURE=session-lock
      MG_SESSION_MESSAGE="$lock has been held for ${MG_SESSION_LOCK_SECONDS:-60} s; if no tools/mg session record, worktree new or escalation is running, remove that directory and run again"
      return 1
    fi
    sleep 0.1
    waited=$((waited + 1))
  done
  MG__SESSION_LOCK="$lock"
  if ! { mkdir -p "$(dirname "$file")" && printf '%s\n' "$line" >>"$file"; }; then
    mg__session_unlock
    return 1
  fi
  until mg__ledger_commit "$dir" "$rel" "$message" || mg__ledger_in_head "$dir" "$rel" "$line"; do
    tries=$((tries + 1))
    if [ "$tries" -ge 3 ]; then
      mg__session_unlock
      MG_SESSION_FAILURE=session-commit
      MG_SESSION_MESSAGE="appended the record to $rel but could not commit it (see log.txt)"
      return 1
    fi
    sleep 1
  done
  mg__session_unlock
}

mg__session_unlock() {
  [ -z "${MG__SESSION_LOCK:-}" ] || rmdir "$MG__SESSION_LOCK" 2>/dev/null
  MG__SESSION_LOCK=""
  trap - TERM HUP EXIT
}

mg__ledger_commit() {
  git -C "$1" add -- "$2" >&2 &&
    printf '%s\n' "$3" | git -C "$1" commit -q -F - -- "$2" >&2
}

mg__ledger_in_head() {
  git -C "$1" show "HEAD:$2" 2>/dev/null | grep -Fxq -- "$3"
}

# mg_session_record_json <id> <role> <kind> <name> <base commit> [extra JSON object]
mg_session_record_json() {
  local doc blob="" extra="${6:-}"
  [ -n "$extra" ] || extra='{}'
  doc="$(mg_role_doc "$2")"
  if [ -n "$doc" ]; then
    blob="$(git -C "$MG_ROOT" rev-parse -q --verify "$5:$doc" 2>/dev/null || true)"
  fi
  jq -nc --arg id "$1" --arg role "$2" --arg kind "$3" --arg name "$4" --arg base "$5" \
    --arg doc "$doc" --arg blob "$blob" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson extra "$extra" \
    '{session: $id, role: $role, kind: $kind, name: $name, base_commit: $base,
      role_doc: (if $doc == "" then null else $doc end),
      role_doc_blob: (if $blob == "" then null else $blob end),
      created_at: $at} + $extra'
}

mg_session() {
  local sub="${1:-}"
  [ "$#" -gt 0 ] && shift
  case "$sub" in
    record) ;;
    *)
      mg_failure usage "usage: tools/mg session record --role <role> [--name <name>] [--note <text>]"
      mg_status error
      return 0
      ;;
  esac
  local role="" name="" note=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --role) role="${2:-}"; shift 2 || shift ;;
      --name) name="${2:-}"; shift 2 || shift ;;
      --note) note="${2:-}"; shift 2 || shift ;;
      *)
        mg_failure usage "unknown argument '$1'"
        mg_status error
        return 0
        ;;
    esac
  done
  if ! mg_role_valid "$role"; then
    mg_failure bad-role "--role must be one of: $MG_ROLES"
    mg_status error
    return 0
  fi
  local id base record
  id="$(mg_session_new_id)"
  base="$(git -C "$MG_ROOT" rev-parse --verify -q HEAD || echo "")"
  record="$(mg_session_record_json "$id" "$role" session "$name" "$base" \
    "$(jq -nc --arg note "$note" --arg checkout "$MG_ROOT" \
      '{checkout: $checkout} + (if $note == "" then {} else {note: $note} end)')")"
  if ! mg_session_store "$MG_ROOT" "$record"; then
    case "$MG_SESSION_FAILURE" in
      session-commit) MG_SESSION_MESSAGE="$MG_SESSION_MESSAGE; commit that file by hand" ;;
    esac
    mg_failure "$MG_SESSION_FAILURE" "$MG_SESSION_MESSAGE"
    mg_status error
    return 0
  fi
  mg_detail session "$record"
  mg_message "session $id recorded and committed (role $role); use 'Session: $id' in its commits"
  mg_status pass
}
