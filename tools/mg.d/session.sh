# Agent session records (U2, R1958). Sourced by tools/mg.
#
# docs/process/provenance/sessions.jsonl is append-only: one JSON object per
# agent session, with its role, base commit, what it could read (sparse
# exclusions, deny-profile hash) and the version of its role instructions.
# `tools/mg worktree new` appends a record for each worktree it creates and
# also writes it to <worktree git dir>/meguri-session.json, which marks the
# worktree's role for other jobs (the similarity scan refuses implementer
# worktrees). `tools/mg session record` covers sessions without a worktree.

mg_job session host mg_session \
  "Agent session records (R1958): tools/mg session record --role <role> [--name <name>] [--note <text>]"

MG_ROLES="spec-writer implementer fixer oracle-maintainer infra critic integrator"
MG_SESSIONS_FILE="docs/process/provenance/sessions.jsonl"

mg_role_valid() {
  case " $MG_ROLES " in *" $1 "*) [ -n "$1" ] ;; *) return 1 ;; esac
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

# mg_session_append <record JSON>: one line, appended (O_APPEND keeps
# concurrent appends whole). sessions.jsonl merges with the union driver
# (.gitattributes), so records added on different branches never conflict.
mg_session_append() {
  local file="$MG_ROOT/$MG_SESSIONS_FILE"
  mkdir -p "$(dirname "$file")"
  jq -c . <<<"$1" >>"$file"
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
  mg_session_append "$record"
  mg_detail session "$record"
  mg_message "session $id recorded (role $role); use 'Session: $id' in its commits"
  mg_status pass
}
