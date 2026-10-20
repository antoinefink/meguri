# Master-only source isolation for Codex implementers/fixers (U2).
mg_job cleanroom host mg_cleanroom \
  "Isolated Codex runner: --check probes boundaries; --prompt <file> --session <id> [--effort high|xhigh] [--seed-patch <own-code patch>]; exports changes.patch, never edits master"
mg_job cleanroom-image host mg_cleanroom_image \
  "Build the version-pinned isolated Codex runtime from a digest-pinned Linux base"

mg_cleanroom_image() {
  local project rc
  project="$(mg_compose_project)"
  BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker build --progress plain \
    -f "$MG_ROOT/docker/cleanroom.Dockerfile" \
    -t "$project-cleanroom" "$MG_ROOT"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    mg_failure cleanroom-image "isolated Codex runtime build failed"
    mg_status error
  else
    mg_status pass
  fi
}

mg_cleanroom_cleanup() {
  rm -f "$MG_RUN_DIR/codex-home/auth.json"
  docker rm -f "meguri-cleanroom-$MG_RUN_ID" >/dev/null 2>&1 || true
}

mg_cleanroom() {
  local check=false prompt="" session="" seed="" effort=high role rc image before
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --check) check=true; shift ;;
      --prompt|--session|--effort|--seed-patch)
        [ "$#" -ge 2 ] || break
        case "$1" in --prompt) prompt="$2" ;; --session) session="$2" ;; --effort) effort="$2" ;; --seed-patch) seed="$2" ;; esac
        shift 2 ;;
      *) break ;;
    esac
  done
  if [ "$#" -ne 0 ] || ! mg_word_in "$effort" "high xhigh" ||
    { [ "$check" = true ] && { [ -n "$prompt" ] || [ -n "$session" ] || [ -n "$seed" ]; }; } ||
    { [ -n "$seed" ] && [ ! -f "$seed" ]; } ||
    { [ "$check" = false ] && { [ ! -f "$prompt" ] || [ -z "$session" ]; }; }; then
    mg_failure usage "usage: tools/mg cleanroom --check | --prompt <file> --session <id> [--effort high|xhigh] [--seed-patch <own-code patch>]"
    mg_status error; return 0
  fi
  image="$(mg_compose_project)-cleanroom"
  if ! docker image inspect "$image" >/dev/null 2>&1; then
    mg_failure cleanroom-image "isolated runtime unavailable; run tools/mg cleanroom-image"
    mg_status error; return 0
  fi
  if [ "$check" = false ]; then
    role="$(jq -rs --arg s "$session" '[.[] | select(.session == $s)] | if length == 1 then .[0].role else "" end' "$MG_ROOT/$MG_SESSIONS_FILE")"
    if ! mg_word_in "$role" "implementer fixer" ||
      [ "$(git -C "$MG_ROOT" branch --show-current)" != master ] ||
      [ -n "$(git -C "$MG_ROOT" status --porcelain)" ]; then
      mg_failure source-state "run requires clean master and a recorded implementer/fixer session"
      mg_status error; return 0
    fi
  fi
  mkdir -p "$MG_RUN_DIR/codex-home"
  cp "$MG_TOOLS/cleanroom/config.toml" "$MG_RUN_DIR/codex-home/config.toml"
  if [ "$check" = true ]; then
    mkdir "$MG_RUN_DIR/source"
  else
    before="$(git -C "$MG_ROOT" rev-parse HEAD)"
    if ! python3 "$MG_TOOLS/cleanroom/source_view.py" prepare "$MG_ROOT" "$MG_RUN_DIR/source" >"$MG_RUN_DIR/source-view.json"; then
      mg_failure source-view "source view refused unsafe input; no Codex run started"
      mg_status error; return 0
    fi
    if [ "$before" != "$(git -C "$MG_ROOT" rev-parse HEAD)" ] || [ -n "$(git -C "$MG_ROOT" status --porcelain)" ]; then
      mg_failure source-changed "master changed during source-view preparation"
      mg_status error; return 0
    fi
    cp "$prompt" "$MG_RUN_DIR/prompt.md"
    if [ -n "$seed" ]; then
      cp "$seed" "$MG_RUN_DIR/seed.patch"
      mg_artifact patch "$MG_RUN_DIR/seed.patch" "reviewed own-code input, validated before Codex starts"
    fi
    mg_detail source_commit "\"$before\""
  fi
  mg_on_exit mg_cleanroom_cleanup
  if [ "$check" = false ] && ! install -m 600 "$HOME/.codex/auth.json" "$MG_RUN_DIR/codex-home/auth.json"; then
    mg_failure authentication "Codex authentication unavailable; no run started"
    mg_status error; return 0
  fi
  # Permit nested unprivileged namespaces for bubblewrap, retaining no outer
  # SYS_ADMIN capability, a read-only root and only filtered bind mounts.
  # Mapping UID 0 into a new user namespace requires SETFCAP on Linux >= 5.12.
  docker run --rm --init --read-only --cap-drop ALL --cap-add SETFCAP \
    --security-opt seccomp=unconfined \
    --security-opt systempaths=unconfined \
    --label "meguri.mg.run-id=$MG_RUN_ID" --name "meguri-cleanroom-$MG_RUN_ID" \
    --tmpfs /tmp:exec,mode=1777 --tmpfs /git:mode=0700 \
    -v "$MG_RUN_DIR/source:/work" -v "$MG_RUN_DIR:/codex-run" \
    -v "$MG_TOOLS/cleanroom:/runner:ro" \
    -w /work "$image" /bin/bash /runner/run.sh "$([ "$check" = true ] && echo --check || echo --run)" "$effort"
  rc=$?
  rm -f "$MG_RUN_DIR/codex-home/auth.json"
  if [ "$rc" -ne 0 ]; then
    mg_failure runner "isolated runner or boundary probe failed; no fallback was attempted"
    mg_status error; return 0
  fi
  mg_artifact log "$MG_RUN_DIR/log.txt" "clean-room boundary probes"
  if [ "$check" = false ]; then
    mg_artifact patch "$MG_RUN_DIR/changes.patch" "review before applying to master"
    mg_artifact text "$MG_RUN_DIR/codex.out" "Codex implementation report"
    mg_artifact json "$MG_RUN_DIR/source-view.json" "permitted source view"
  fi
  mg_message "cleanroom: boundary probes passed$([ "$check" = true ] || echo '; changes exported for review')"
  mg_status pass
}
