# Git hooks (U2, R1958). Sourced by tools/mg.

mg_job hooks host mg_hooks \
  "Git hooks: tools/mg hooks install adds the commit-msg provenance-trailer check (tools/hooks/commit-msg)"

MG_HOOK_MARKER="meguri: commit-msg shim installed by tools/mg hooks install"

# The installed hook is a shim, so every worktree runs its own checkout's
# tools/hooks/commit-msg and hook updates need no reinstall.
mg__hook_shim() {
  cat <<SHIM
#!/bin/sh
# $MG_HOOK_MARKER
hook="\$(git rev-parse --show-toplevel)/tools/hooks/commit-msg"
[ -x "\$hook" ] || exit 0
exec "\$hook" "\$@"
SHIM
}

mg_hooks() {
  if [ "${1:-}" != install ]; then
    mg_failure usage "usage: tools/mg hooks install"
    mg_status error
    return 0
  fi
  local hooks_path dir target
  hooks_path="$(git -C "$MG_ROOT" config --get core.hooksPath || true)"
  if [ -n "$hooks_path" ]; then
    mg_failure hooks-path "core.hooksPath is set ($hooks_path); call tools/hooks/commit-msg from that directory's commit-msg instead"
    mg_status error
    return 0
  fi
  dir="$(git -C "$MG_ROOT" rev-parse --path-format=absolute --git-common-dir)/hooks"
  target="$dir/commit-msg"
  if [ -e "$target" ] && ! grep -qF "$MG_HOOK_MARKER" "$target"; then
    mg_failure hook-exists "$target exists and is not the Meguri shim; merge it by hand or remove it, then rerun"
    mg_status error
    return 0
  fi
  mkdir -p "$dir"
  mg__hook_shim >"$target.tmp" && chmod +x "$target.tmp" && mv -f "$target.tmp" "$target"
  mg_detail hook "$(jq -nc --arg p "$target" '{path: $p}')"
  mg_message "hooks: commit-msg installed at $target (shared by every worktree)"
  mg_status pass
}
