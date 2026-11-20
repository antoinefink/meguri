# Agent session records (U2, R1958). Sourced by tools/mg.
#
# One file per agent session, docs/process/provenance/sessions/<id>.json: one
# JSON object with its role, base commit, what it could read (sparse
# exclusions, deny-profile hash) and the version of its role instructions.
# Files are only ever added: two branches never touch the same file, so their
# merges (GitHub's included) never conflict on records. Every record is
# committed where it is made, that file only, so no checkout is left with an
# uncommitted record. docs/process/provenance/sessions.jsonl is the archive of
# the records made before the switch, one per line; nothing here appends to
# it. Readers take both (mg_session_records); tools/gate/static-checks/
# sessions.py checks both. `tools/mg worktree new` commits a record for each
# worktree it creates as that branch's first commit, and also writes it to
# <worktree git dir>/meguri-session.json, which marks the worktree's role for
# other jobs (the similarity scan refuses implementer worktrees).
# `tools/mg session record` covers sessions without a worktree, on the current
# branch of the checkout it runs in.

mg_job session host mg_session \
  "Agent session records (R1958): tools/mg session record --role <role> [--name <name>] [--note <text>]"

MG_ROLES="spec-writer implementer fixer oracle-maintainer infra critic integrator"
MG_SESSIONS_ARCHIVE="docs/process/provenance/sessions.jsonl"
MG_SESSIONS_DIR="docs/process/provenance/sessions"
MG_SESSIONS_PY="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/gate/static-checks/sessions.py"
# A session name goes into its record, its commit subject and, for a
# worktree, a branch and a path: plain words only.
MG_SESSION_NAME_RE='^[A-Za-z0-9][A-Za-z0-9 ._-]{0,79}$'

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

# mg_plain_name <value> <ERE>: the whole value, one line, matches the pattern.
mg_plain_name() {
  case "$1" in *$'\n'* | *$'\r'*) return 1 ;; esac
  printf '%s' "$1" | LC_ALL=C grep -Eq "$2"
}

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

# mg_session_store <checkout> <record JSON>: writes the record to
# $MG_SESSIONS_DIR/<id>.json in the checkout and commits it on the checkout's
# current branch, that path only (other staged changes stay staged; the
# commit goes through the commit-msg and pre-commit hooks like any other).
# Records made in parallel (critic sessions, spec writers in the main
# checkout) would race on git's index.lock, so the ledgers' lock
# (mg__ledger_lock) covers write and commit; a commit that fails is tried
# again, since another git command may hold index.lock for a moment; after
# the last try the file is unstaged and removed. When the
# pre-commit private-token scan refuses the record, the file is removed and
# the failure is session-private: the caller picks another name (or note, or
# path); neither the hook nor this message names the word it matched. On
# failure sets MG_SESSION_FAILURE (failure id) and MG_SESSION_MESSAGE and
# returns 1.
mg_session_store() {
  local dir="$1" id role name rel file line tries=0 err
  id="$(jq -r .session <<<"$2")"
  role="$(jq -r .role <<<"$2")"
  name="$(jq -r '.name // ""' <<<"$2")"
  rel="$MG_SESSIONS_DIR/$id.json"
  file="$dir/$rel"
  MG_SESSION_FAILURE=session-ledger
  MG_SESSION_MESSAGE="could not write the record $rel; see log.txt"
  mg_plain_name "$id" '^[A-Za-z0-9][A-Za-z0-9._-]{2,}$' || return 1
  line="$(jq -c . <<<"$2")" && [ -n "$line" ] || return 1
  mg__ledger_lock "$dir" || return 1
  if [ -e "$file" ] || git -C "$dir" cat-file -e "HEAD:$rel" 2>/dev/null; then
    mg__session_unlock
    MG_SESSION_MESSAGE="$rel already exists; records are never rewritten"
    return 1
  fi
  err="$(mktemp "${TMPDIR:-/tmp}/meguri-session.XXXXXX")" || {
    mg__session_unlock
    return 1
  }
  MG__SESSION_DROP="$rel" MG__SESSION_DROP_DIR="$dir" MG__SESSION_DROP_ERR="$err"
  # Cancelled before the record is committed: nothing stays behind.
  trap 'mg__session_drop; mg__session_unlock; exit 143' TERM
  trap 'mg__session_drop; mg__session_unlock; exit 129' HUP
  trap 'mg__session_drop; mg__session_unlock' EXIT
  if ! { mkdir -p "$(dirname "$file")" && printf '%s\n' "$line" >"$file"; }; then
    mg__session_drop
    mg__session_unlock
    return 1
  fi
  until mg__ledger_commit "$dir" "$rel" "Record session $id ($role${name:+, $name})

Role: infra
Session: $id" 2>"$err"; do
    cat "$err" >&2
    if grep -q '^pre-commit: staged lines hold' "$err"; then
      mg__session_drop
      mg__session_unlock
      MG_SESSION_FAILURE=session-private
      MG_SESSION_MESSAGE="the private-token scan (pre-commit) refused the session record: its name, note, branch or path holds a word from the private collection; nothing was recorded, so run again with another name (and --note, --branch or --path when given). The scan never says which word, and this message does not either"
      return 1
    fi
    tries=$((tries + 1))
    if [ "$tries" -ge 3 ]; then
      # Committed after all, by a commit of its own: stored.
      if mg__session_committed "$dir" "$rel"; then
        MG__SESSION_DROP=""
        rm -f "$err"
        mg__session_unlock
        return 0
      fi
      mg__session_drop
      mg__session_unlock
      MG_SESSION_FAILURE=session-commit
      MG_SESSION_MESSAGE="could not commit the record $rel, so nothing was recorded (see log.txt)"
      return 1
    fi
    sleep 1
  done
  cat "$err" >&2
  MG__SESSION_DROP=""
  rm -f "$err"
  mg__session_unlock
}

# mg__session_committed <checkout> <path>: HEAD is a commit that adds exactly
# this record, as the working tree holds it.
mg__session_committed() {
  [ -f "$1/$2" ] &&
    [ "$(git -C "$1" rev-parse -q --verify "HEAD:$2" 2>/dev/null)" = "$(git -C "$1" hash-object -- "$1/$2")" ] &&
    [ "$(git -C "$1" diff-tree --no-commit-id --name-only -r HEAD)" = "$2" ]
}

# mg__session_drop: removes the record mg_session_store is writing
# (MG__SESSION_DROP, in checkout MG__SESSION_DROP_DIR) from the index and the
# working tree unless it was committed, and its temporary file
# (MG__SESSION_DROP_ERR).
mg__session_drop() {
  [ -n "${MG__SESSION_DROP:-}" ] || return 0
  local dir="$MG__SESSION_DROP_DIR" rel="$MG__SESSION_DROP"
  MG__SESSION_DROP=""
  rm -f "$MG__SESSION_DROP_ERR"
  mg__session_committed "$dir" "$rel" && return 0
  git -C "$dir" rm -q --cached --ignore-unmatch -- "$rel" >&2 2>&1
  rm -f "$dir/$rel"
}

# mg_session_records <checkout> <commit>: every session record committed at
# <commit>, one compact JSON object per line: the archive's lines, then each
# regular <id>.json of the records directory whose session is its <id>
# (sessions.py --records). Reads git objects only (never a working tree or a
# replace ref), so an uncommitted record never counts. Returns non-zero when
# git cannot read the commit or a record does not parse: callers refuse then,
# never read it as no records.
mg_session_records() {
  python3 -I -B "$MG_SESSIONS_PY" --records "$1" "$2"
}

# mg_ledger_store <checkout> <ledger path> <record JSON> <commit message>:
# appends a record to an append-only ledger under docs/process/provenance/
# (escalations) and commits it, that file only, under the ledgers' lock (the
# one session records take: they commit in the same checkouts). Same failure
# contract as mg_session_store.
mg_ledger_store() {
  local dir="$1" rel="$2" record="$3" message="$4" file="$1/$2" line tries=0
  MG_SESSION_FAILURE=session-ledger
  MG_SESSION_MESSAGE="could not append the record to $rel; see log.txt"
  line="$(jq -c . <<<"$record")" && [ -n "$line" ] || return 1
  mg__ledger_lock "$dir" || return 1
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

# mg__ledger_lock <checkout>: takes the ledgers' lock, a mkdir lock in the
# repository's common git dir, waiting at most MG_SESSION_LOCK_SECONDS;
# mg__session_unlock gives it back. Returns 1 (MG_SESSION_FAILURE,
# MG_SESSION_MESSAGE) when it stays held, or without setting them when the
# git dir cannot be found.
mg__ledger_lock() {
  local lock waited=0
  lock="$(git -C "$1" rev-parse --path-format=absolute --git-common-dir)/meguri-session.lock" || return 1
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
}

# mg_ledger_settle <checkout> <ledger path> <subject filter>: commits the lines
# a mg_ledger_store appended to the checkout's ledger but could not commit (its
# commit failed after the append), so that a caller counts a record only once
# HEAD holds it. Under the ledgers' lock, so a store still running finishes
# first. The lines past HEAD's ledger must be whole JSON lines; the jq
# <subject filter>, given their records as an array, makes the commit's
# subject. A ledger that differs from HEAD's otherwise (a line changed,
# removed or cut short) is left alone. Returns 0 when HEAD's ledger then is
# the working tree's, otherwise 1 (MG_SESSION_FAILURE, MG_SESSION_MESSAGE).
mg_ledger_settle() {
  local dir="$1" rel="$2" subject="$3" file="$1/$2" size=0 records message tries=0
  mg__ledger_settled "$dir" "$rel" && return 0
  MG_SESSION_FAILURE=ledger-unsettled
  MG_SESSION_MESSAGE="could not settle $rel; see log.txt"
  mg__ledger_lock "$dir" || return 1
  if mg__ledger_settled "$dir" "$rel"; then
    mg__session_unlock
    return 0
  fi
  if git -C "$dir" rev-parse -q --verify "HEAD:$rel" >/dev/null 2>&1; then
    size="$(git -C "$dir" show "HEAD:$rel" | wc -c | tr -d ' ')"
  fi
  if [ ! -f "$file" ] || ! head -c "$size" "$file" | cmp -s - <(git -C "$dir" show "HEAD:$rel" 2>/dev/null); then
    mg__session_unlock
    MG_SESSION_MESSAGE="$rel in $dir differs from HEAD's by more than appended lines; settle it by hand (git diff HEAD -- $rel)"
    return 1
  fi
  if [ "$(tail -c 1 "$file" | od -An -tx1 | tr -d ' \n')" != 0a ] ||
    ! records="$(tail -c +$((size + 1)) "$file" | jq -c -s 'if all(type == "object") then . else error("not a record") end' 2>/dev/null)" ||
    ! message="$(jq -r "$subject" <<<"$records" 2>/dev/null)" || [ -z "$message" ]; then
    mg__session_unlock
    MG_SESSION_MESSAGE="the lines appended to $rel in $dir past HEAD's are not whole records; settle it by hand (git diff HEAD -- $rel)"
    return 1
  fi
  message="$message

Commit what was appended to $rel but not committed: the commit of the run
that appended it failed. tools/mg $MG_JOB ${MG_RUN_ID:-} committed it.

Role: infra"
  until mg__ledger_commit "$dir" "$rel" "$message" || mg__ledger_settled "$dir" "$rel"; do
    tries=$((tries + 1))
    if [ "$tries" -ge 3 ]; then
      mg__session_unlock
      MG_SESSION_FAILURE=session-commit
      MG_SESSION_MESSAGE="$rel in $dir has lines appended but not committed, and committing them failed again (see log.txt)"
      return 1
    fi
    sleep 1
  done
  mg__session_unlock
}

# mg__ledger_settled <checkout> <ledger path>: whether HEAD's ledger is the
# working tree's (both absent counts).
mg__ledger_settled() {
  local blob
  blob="$(git -C "$1" rev-parse -q --verify "HEAD:$2" 2>/dev/null)"
  if [ -f "$1/$2" ]; then
    [ -n "$blob" ] && [ "$(git -C "$1" hash-object -- "$1/$2")" = "$blob" ]
  else
    [ -z "$blob" ]
  fi
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
  if [ -n "$name" ] && ! mg_plain_name "$name" "$MG_SESSION_NAME_RE"; then
    mg_failure bad-name "--name must match $MG_SESSION_NAME_RE"
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
    mg_failure "$MG_SESSION_FAILURE" "$MG_SESSION_MESSAGE"
    mg_status error
    return 0
  fi
  mg_detail session "$record"
  mg_message "session $id recorded and committed (role $role); use 'Session: $id' in its commits"
  mg_status pass
}
