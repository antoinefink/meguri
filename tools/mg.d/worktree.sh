# Role worktrees (U2, KTD1, R1955, R1958). Sourced by tools/mg.
#
# `tools/mg worktree new --role <role> <name>` creates a git worktree for one
# agent session. Implementer and fixer worktrees are sparse: oracle/ is never
# on disk, and .claude/settings.implementer.json is rendered into the
# worktree's .claude/settings.local.json as its deny profile. refs/ and
# local-data/ are untracked, so no worktree has them. Each worktree gets a
# session record (session.sh).

mg_job worktree host mg_worktree \
  "Role worktrees (KTD1): tools/mg worktree new --role <role> <name> [--base <ref>] [--branch <branch>] [--path <dir>]"

MG_WORKTREE_ROLES="implementer fixer spec-writer oracle-maintainer infra"

# Sparse-checkout patterns per role; empty means a full checkout.
mg__worktree_sparse() {
  case "$1" in
    implementer | fixer) printf '%s\n' '/*' '!/oracle/' ;;
  esac
}

mg__worktree_profile() {
  case "$1" in
    implementer | fixer) echo .claude/settings.implementer.json ;;
  esac
}

mg_worktree() {
  local sub="${1:-}"
  [ "$#" -gt 0 ] && shift
  if [ "$sub" != new ]; then
    mg_failure usage "usage: tools/mg worktree new --role <role> <name> [--base <ref>] [--branch <branch>] [--path <dir>]"
    mg_status error
    return 0
  fi
  local role="" name="" base=HEAD branch="" path=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --role) role="${2:-}"; shift 2 || shift ;;
      --base) base="${2:-}"; shift 2 || shift ;;
      --branch) branch="${2:-}"; shift 2 || shift ;;
      --path) path="${2:-}"; shift 2 || shift ;;
      -*)
        mg_failure usage "unknown option '$1'"
        mg_status error
        return 0
        ;;
      *)
        if [ -n "$name" ]; then
          mg_failure usage "one worktree name expected, got '$name' and '$1'"
          mg_status error
          return 0
        fi
        name="$1"
        shift
        ;;
    esac
  done
  case " $MG_WORKTREE_ROLES " in
    *" $role "*) [ -n "$role" ] ;;
    *) false ;;
  esac || {
    mg_failure bad-role "--role must be one of: $MG_WORKTREE_ROLES (critic sessions use a packet, U10)"
    mg_status error
    return 0
  }
  if ! printf '%s' "$name" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$'; then
    mg_failure bad-name "worktree name '$name' must match [A-Za-z0-9][A-Za-z0-9._-]*"
    mg_status error
    return 0
  fi
  local common main base_sha
  common="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir)" || {
    mg_failure not-a-repo "$MG_ROOT is not a git checkout"
    mg_status error
    return 0
  }
  main="$(dirname "$common")"
  [ -n "$branch" ] || branch="unit/$name"
  [ -n "$path" ] || path="$(dirname "$main")/$(basename "$main")-wt/$name"
  if [ -e "$path" ]; then
    mg_failure path-exists "$path already exists"
    mg_status error
    return 0
  fi
  local new_branch=true
  if git -C "$MG_ROOT" show-ref --verify -q "refs/heads/$branch"; then
    new_branch=false
    base="$branch"
  fi
  if ! base_sha="$(git -C "$MG_ROOT" rev-parse --verify -q "$base^{commit}")"; then
    mg_failure bad-base "'$base' is not a commit"
    mg_status error
    return 0
  fi

  mkdir -p "$(dirname "$path")"
  if [ "$new_branch" = true ]; then
    git -C "$MG_ROOT" worktree add --no-checkout -b "$branch" "$path" "$base_sha" >&2
  else
    git -C "$MG_ROOT" worktree add --no-checkout "$path" "$branch" >&2
  fi || {
    mg_failure worktree-add "git worktree add failed; see log.txt"
    mg_status error
    return 0
  }
  path="$(cd "$path" && pwd -P)"

  local sparse sparse_json='[]' excluded_json='[]'
  sparse="$(mg__worktree_sparse "$role")"
  if [ -n "$sparse" ]; then
    # Per-worktree sparse settings: git sets extensions.worktreeConfig and keeps
    # core.sparseCheckout out of the main checkout's config. The patterns are
    # read into an array: unquoted, `/*` would expand as a pathname.
    local pats=() line
    while IFS= read -r line; do
      [ -n "$line" ] && pats[${#pats[@]}]="$line"
    done <<EOF
$sparse
EOF
    git -C "$path" sparse-checkout set --no-cone "${pats[@]}" >&2 || {
      mg__worktree_abort "$path" "$branch" "$new_branch" sparse "git sparse-checkout set failed; see log.txt"
      return 0
    }
    sparse_json="$(printf '%s\n' "$sparse" | jq -R . | jq -sc .)"
    excluded_json="$(printf '%s\n' "$sparse" | sed -n 's|^!/||p' | jq -R . | jq -sc .)"
  fi
  git -C "$path" checkout -q >&2 || {
    mg__worktree_abort "$path" "$branch" "$new_branch" checkout "checking out the worktree failed; see log.txt"
    return 0
  }
  local ex
  while IFS= read -r ex; do
    if [ -n "$ex" ] && [ -e "$path/$ex" ]; then
      mg__worktree_abort "$path" "$branch" "$new_branch" sparse-leak "$ex is present in the $role worktree"
      return 0
    fi
  done <<EOF
$(jq -r '.[]' <<<"$excluded_json")
EOF

  local profile profile_sha="" profile_json=null
  profile="$(mg__worktree_profile "$role")"
  if [ -n "$profile" ]; then
    if [ ! -f "$path/$profile" ]; then
      mg__worktree_abort "$path" "$branch" "$new_branch" no-profile "$profile is missing at $base"
      return 0
    fi
    mkdir -p "$path/.claude"
    # Deny rules name absolute paths: the main checkout and the directory that
    # holds the worktrees.
    sed -e "s|@MAIN_CHECKOUT@|$main|g" -e "s|@WORKTREES@|$(dirname "$path")|g" \
      "$path/$profile" >"$path/.claude/settings.local.json"
    if ! jq -e '.permissions.deny | length > 0' "$path/.claude/settings.local.json" >/dev/null; then
      mg__worktree_abort "$path" "$branch" "$new_branch" bad-profile "$profile did not render to a settings file with deny rules"
      return 0
    fi
    profile_sha="$(shasum -a 256 "$path/.claude/settings.local.json" 2>/dev/null || sha256sum "$path/.claude/settings.local.json")"
    profile_sha="${profile_sha%% *}"
    profile_json="$(jq -nc --arg p "$profile" --arg s "$profile_sha" '{source: $p, installed: ".claude/settings.local.json", sha256: $s}')"
  fi

  local id record
  id="$(mg_session_new_id)"
  record="$(mg_session_record_json "$id" "$role" worktree "$name" "$base_sha" \
    "$(jq -nc --arg branch "$branch" --arg path "$path" --argjson sparse "$sparse_json" \
      --argjson excluded "$excluded_json" --argjson profile "$profile_json" \
      '{branch: $branch, worktree: $path, sparse: $sparse, excluded_paths: $excluded, deny_profile: $profile}')")"
  jq . <<<"$record" >"$(git -C "$path" rev-parse --absolute-git-dir)/meguri-session.json"
  mg_session_append "$record"
  mg_detail session "$record"
  mg_detail worktree "$(jq -nc --arg p "$path" --arg b "$branch" '{path: $p, branch: $b}')"
  mg_message "worktree $name at $path (role $role, branch $branch, session $id)"
  mg_status pass
}

# mg__worktree_abort <path> <branch> <created branch?> <failure id> <message>
mg__worktree_abort() {
  git -C "$MG_ROOT" worktree remove --force "$1" >&2 2>&1 || rm -rf "$1"
  git -C "$MG_ROOT" worktree prune >&2 2>&1
  [ "$3" = true ] && git -C "$MG_ROOT" branch -D "$2" >&2 2>&1
  mg_failure "$4" "$5"
  mg_status error
}
