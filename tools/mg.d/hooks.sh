# Git hooks (U2, R1958). Sourced by tools/mg.

mg_job hooks host mg_hooks \
  "Git hooks: tools/mg hooks install adds the commit-msg provenance-trailer check (tools/hooks/commit-msg)"

MG_HOOK_MARKER="meguri: commit-msg shim installed by tools/mg hooks install"

# The installed hook is a shim, so every worktree runs its own checkout's
# tools/hooks/commit-msg and hook updates need no reinstall. `tools/mg worktree
# new` installs it too, so no worktree is made without it.
mg__hook_shim() {
  cat <<SHIM
#!/bin/sh
# $MG_HOOK_MARKER
hook="\$(git rev-parse --show-toplevel)/tools/hooks/commit-msg"
[ -x "\$hook" ] || exit 0
exec "\$hook" "\$@"
SHIM
}

# mg_hooks_install: installs (or refreshes) the shim in the repository's shared
# hooks directory. On failure it returns 1 with MG_HOOKS_FAILURE (a failure
# id) and MG_HOOKS_MESSAGE set; MG_HOOKS_TARGET is the hook's path.
mg_hooks_install() {
  MG_HOOKS_FAILURE="" MG_HOOKS_MESSAGE="" MG_HOOKS_TARGET=""
  local hooks_path common dir target tmp
  hooks_path="$(git -C "$MG_ROOT" config --get core.hooksPath || true)"
  if [ -n "$hooks_path" ]; then
    MG_HOOKS_FAILURE=hooks-path
    MG_HOOKS_MESSAGE="core.hooksPath is set ($hooks_path); call tools/hooks/commit-msg from that directory's commit-msg instead"
    return 1
  fi
  if ! common="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir)"; then
    MG_HOOKS_FAILURE=not-a-repo
    MG_HOOKS_MESSAGE="$MG_ROOT is not a git checkout"
    return 1
  fi
  dir="$common/hooks"
  target="$dir/commit-msg"
  tmp="$target.tmp"
  MG_HOOKS_TARGET="$target"
  if [ -e "$target" ] && ! grep -qF "$MG_HOOK_MARKER" "$target" 2>/dev/null; then
    MG_HOOKS_FAILURE=hook-exists
    MG_HOOKS_MESSAGE="$target exists and is not the Meguri shim; merge it by hand or remove it, then run tools/mg hooks install"
    return 1
  fi
  MG_HOOKS_FAILURE=hook-write
  if ! mkdir -p "$dir"; then
    MG_HOOKS_MESSAGE="could not create $dir"
    return 1
  fi
  if ! mg__hook_shim >"$tmp" || ! chmod +x "$tmp"; then
    rm -f "$tmp" 2>/dev/null
    MG_HOOKS_MESSAGE="could not write $tmp"
    return 1
  fi
  if ! mv -f "$tmp" "$target"; then
    rm -f "$tmp" 2>/dev/null
    MG_HOOKS_MESSAGE="could not move $tmp to $target"
    return 1
  fi
  if [ ! -f "$target" ] || [ ! -x "$target" ] || ! grep -qF "$MG_HOOK_MARKER" "$target"; then
    MG_HOOKS_MESSAGE="$target is not the executable Meguri shim after the install"
    return 1
  fi
  MG_HOOKS_FAILURE=""
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
  mg_detail hook "$(jq -nc --arg p "$MG_HOOKS_TARGET" '{path: $p}')"
  mg_message "hooks: commit-msg installed at $MG_HOOKS_TARGET (shared by every worktree)"
  mg_status pass
}
