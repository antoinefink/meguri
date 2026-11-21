# Git hooks (U2, R1958; U15, R1940). Sourced by tools/mg.

mg_job hooks host mg_hooks \
  "Git hooks: tools/mg hooks install adds the commit-msg provenance-trailer check (tools/hooks/commit-msg) and the private-token scan of commits and merges (tools/hooks/pre-commit, pre-merge-commit)"

# The hooks tools/mg installs, each as a shim that runs tools/hooks/<name>.
MG_HOOK_NAMES="commit-msg pre-commit pre-merge-commit"

# mg__hook_marker <name>: the line that marks an installed shim as Meguri's.
mg__hook_marker() { printf 'meguri: %s shim installed by tools/mg hooks install\n' "$1"; }

# The installed hook is a shim, so every worktree runs its own checkout's
# tools/hooks/<name> and hook updates need no reinstall; a checkout without
# that file (a branch older than the hook) skips it. `tools/mg worktree new`
# installs them too, so no worktree is made without them.
mg__hook_shim() {
  cat <<SHIM
#!/bin/sh
# $(mg__hook_marker "$1")
hook="\$(git rev-parse --show-toplevel)/tools/hooks/$1"
[ -x "\$hook" ] || exit 0
exec "\$hook" "\$@"
SHIM
}

# mg__hook_install_one <hooks dir> <name>: installs (or refreshes) one shim.
# On failure it returns 1 with MG_HOOKS_FAILURE and MG_HOOKS_MESSAGE set.
mg__hook_install_one() {
  local dir="$1" name="$2" target tmp marker
  target="$dir/$name"
  marker="$(mg__hook_marker "$name")"
  if [ -e "$target" ] && ! grep -qF "$marker" "$target" 2>/dev/null; then
    MG_HOOKS_FAILURE=hook-exists
    MG_HOOKS_MESSAGE="$target exists and is not the Meguri shim; merge it by hand or remove it, then run tools/mg hooks install"
    return 1
  fi
  MG_HOOKS_FAILURE=hook-write
  if ! mkdir -p "$dir"; then
    MG_HOOKS_MESSAGE="could not create $dir"
    return 1
  fi
  if ! tmp="$(mktemp "$target.tmp.XXXXXX")"; then
    MG_HOOKS_MESSAGE="could not create a temporary hook in $dir"
    return 1
  fi
  mg_on_exit "rm -f $(printf '%q' "$tmp")"
  if ! mg__hook_shim "$name" >"$tmp" || ! chmod +x "$tmp"; then
    rm -f "$tmp" 2>/dev/null
    MG_HOOKS_MESSAGE="could not write $tmp"
    return 1
  fi
  if ! mv -f "$tmp" "$target"; then
    rm -f "$tmp" 2>/dev/null
    MG_HOOKS_MESSAGE="could not move $tmp to $target"
    return 1
  fi
  if [ ! -f "$target" ] || [ ! -x "$target" ] || ! grep -qF "$marker" "$target"; then
    MG_HOOKS_MESSAGE="$target is not the executable Meguri shim after the install"
    return 1
  fi
  MG_HOOKS_FAILURE=""
}

# mg_hooks_install: installs (or refreshes) every shim in the repository's
# shared hooks directory. On failure it returns 1 with MG_HOOKS_FAILURE (a
# failure id) and MG_HOOKS_MESSAGE set; MG_HOOKS_DIR is the hooks directory.
mg_hooks_install() {
  MG_HOOKS_FAILURE="" MG_HOOKS_MESSAGE="" MG_HOOKS_DIR=""
  local hooks_path common name
  hooks_path="$(git -C "$MG_ROOT" config --get core.hooksPath || true)"
  if [ -n "$hooks_path" ]; then
    MG_HOOKS_FAILURE=hooks-path
    MG_HOOKS_MESSAGE="core.hooksPath is set ($hooks_path); call tools/hooks/commit-msg, pre-commit and pre-merge-commit from that directory's hooks instead"
    return 1
  fi
  if ! common="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir)"; then
    MG_HOOKS_FAILURE=not-a-repo
    MG_HOOKS_MESSAGE="$MG_ROOT is not a git checkout"
    return 1
  fi
  MG_HOOKS_DIR="$common/hooks"
  for name in $MG_HOOK_NAMES; do
    mg__hook_install_one "$MG_HOOKS_DIR" "$name" || return 1
  done
  # A token set seen here (stat only) is expected from now on: the hooks
  # then refuse a commit from a session that cannot see it, instead of
  # passing unscanned (tools/hooks/pre-commit). `worktree new` runs this, so
  # a set built before the key existed gets it.
  if [ -f "$(dirname "$common")/build/private/tokens/tokens.txt" ] &&
     ! git -C "$MG_ROOT" config meguri.privateTokenSet required; then
    MG_HOOKS_FAILURE=token-set-key
    MG_HOOKS_MESSAGE="a private token set exists but meguri.privateTokenSet could not be set in the repository config"
    return 1
  fi
}

# mg_hooks_check: whether every shim is installed as mg_hooks_install would
# write it and this checkout has the hook each one runs. Read-only. Returns
# 1 with MG_HOOKS_MESSAGE set to what is wrong; MG_HOOKS_DIR is the hooks
# directory.
mg_hooks_check() {
  MG_HOOKS_MESSAGE="" MG_HOOKS_DIR=""
  local hooks_path common name wrong=""
  # Set to an empty value still overrides the hooks directory.
  if hooks_path="$(git -C "$MG_ROOT" config --get core.hooksPath)"; then
    MG_HOOKS_MESSAGE="core.hooksPath is set ($hooks_path), so git does not run the installed shims"
    return 1
  fi
  if ! common="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"; then
    MG_HOOKS_MESSAGE="$MG_ROOT is not a git checkout"
    return 1
  fi
  MG_HOOKS_DIR="$common/hooks"
  for name in $MG_HOOK_NAMES; do
    if [ ! -f "$MG_HOOKS_DIR/$name" ] || [ ! -x "$MG_HOOKS_DIR/$name" ] ||
       [ "$(cat "$MG_HOOKS_DIR/$name" 2>/dev/null)" != "$(mg__hook_shim "$name")" ]; then
      wrong="$wrong $name (shim)"
    elif [ ! -f "$MG_ROOT/tools/hooks/$name" ] || [ ! -x "$MG_ROOT/tools/hooks/$name" ]; then
      wrong="$wrong $name (tools/hooks/$name of this checkout)"
    fi
  done
  if [ -n "$wrong" ]; then
    MG_HOOKS_MESSAGE="missing, not executable or not the current Meguri shim:$wrong"
    return 1
  fi
}

mg_hooks() {
  if [ "${1:-}" != install ]; then
    mg_failure usage "usage: tools/mg hooks install"
    mg_status error
    return 0
  fi
  if ! mg_hooks_install; then
    mg_failure "$MG_HOOKS_FAILURE" "$MG_HOOKS_MESSAGE"
    mg_status error
    return 0
  fi
  # shellcheck disable=SC2086 # one hook name per word
  mg_detail hooks "$(mg_json_array $MG_HOOK_NAMES | jq -c --arg d "$MG_HOOKS_DIR" '[.[] | {name: ., path: ($d + "/" + .)}]')"
  mg_message "hooks: $MG_HOOK_NAMES installed in $MG_HOOKS_DIR (shared by every worktree)"
  mg_status pass
}
