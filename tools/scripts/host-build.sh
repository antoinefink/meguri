# Host builds at a lower scheduling class with a bounded job count. Sourced by
# tools/mg.d (bridge.sh, app.sh, xcode.sh) and tools/tests/xcode_project_check.sh.
# macOS /bin/bash 3.2 safe.

# mg_build_jobs: parallel jobs for a host build: MEGURI_BUILD_JOBS, else half
# the cores (all of them on CI), at least 1.
mg_build_jobs() {
  local n="${MEGURI_BUILD_JOBS:-}" cores
  case "$n" in
    '' | *[!0-9]* | 0)
      cores="$(getconf _NPROCESSORS_ONLN 2>/dev/null)" || cores=2
      n=$((${cores:-2} / 2))
      [ -z "${CI:-}" ] || n="${cores:-2}"
      [ "$n" -ge 1 ] || n=1
      ;;
  esac
  printf '%s\n' "$n"
}

# mg_host_build <program> [args...]: a host build (xcodebuild, swift build, a
# script running cargo) under taskpolicy's utility QoS clamp, which every child
# inherits, so the owner's interactive work wins the cores (not on CI); with
# mg_build_jobs jobs: xcodebuild -jobs (after the action), swift build -j (last),
# CARGO_BUILD_JOBS for cargo, MEGURI_BUILD_JOBS for scripts.
mg_host_build() {
  local jobs action
  jobs="$(mg_build_jobs)"
  case "$1 ${2:-}" in
    'xcodebuild '?*) action="$2" && shift 2 && set -- xcodebuild "$action" -jobs "$jobs" ${1+"$@"} ;;
    'swift build') set -- "$@" -j "$jobs" ;;
  esac
  if [ -z "${CI:-}" ] && command -v taskpolicy >/dev/null 2>&1; then
    set -- taskpolicy -c utility "$@"
  fi
  MEGURI_BUILD_JOBS="$jobs" CARGO_BUILD_JOBS="$jobs" "$@"
}
