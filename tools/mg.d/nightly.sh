# Nightly and weekly runs (U19: R1950 T2, R1937; docs/process/tiers.md).
# Sourced by tools/mg.
#
# `tools/mg nightly` runs every T2 run of tools/gate/tiers.yaml with schedule
# nightly (`--weekly` adds the weekly ones) and writes one summary result. One
# seed per night goes to every run that takes {seed} and into the summary, so
# any night replays with `--seed` (R1937). The runs themselves (the
# differential fuzzer, U247, and the others) own their budgets' contents;
# tiers.yaml gives each its time budget. There is no git remote: the nights
# run on this Mac from a launchd entry that Antoine installs (user step
# nightly-scheduler); `--print-schedule` writes that entry and installs nothing.

mg_job nightly host mg_nightly \
  "Nightly T2 runs (R1950, R1937): [--weekly] [--seed N] (one logged seed for every fuzzing run, default random); --print-schedule writes the launchd entries for Antoine to install (user step nightly-scheduler); one summary result with the seed"

MG_NIGHTLY_LABEL="com.meguri.nightly"

# mg_nightly__plist <label> <args...>: a launchd agent that runs tools/mg from
# this checkout at 02:30 (daily) or 04:00 on Sundays (weekly). launchd reads
# a missing calendar key as "every", so each entry names its minute.
mg_nightly__plist() {
  local label="$1" weekly="$2"
  cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>$MG_ROOT/tools/mg</string>
    <string>nightly</string>$([ "$weekly" = yes ] && printf '\n    <string>--weekly</string>')
  </array>
  <key>WorkingDirectory</key><string>$MG_ROOT</string>
  <key>StartCalendarInterval</key>
  <dict>$([ "$weekly" = yes ] && printf '<key>Weekday</key><integer>0</integer><key>Hour</key><integer>4</integer><key>Minute</key><integer>0</integer>' || printf '<key>Hour</key><integer>2</integer><key>Minute</key><integer>30</integer>')</dict>
  <key>StandardOutPath</key><string>$MG_ROOT/build/mg/nightly/launchd.log</string>
  <key>StandardErrorPath</key><string>$MG_ROOT/build/mg/nightly/launchd.log</string>
</dict>
</plist>
EOF
}

# mg_nightly__scheduler_step: the user step while the entry is not installed.
mg_nightly__scheduler_step() {
  [ -f "$HOME/Library/LaunchAgents/$MG_NIGHTLY_LABEL.plist" ] && return 0
  mg_user_step nightly-scheduler missing false "Approve the nightly scheduler entry" \
    "tools/mg nightly --print-schedule writes $MG_NIGHTLY_LABEL.plist and $MG_NIGHTLY_LABEL.weekly.plist; copy them to ~/Library/LaunchAgents and load them with launchctl"
}

# mg_gate__scheduled <mode> <plan vars...>: load, plan, run and record the
# runs of a scheduled mode (nightly, weekly, milestone, release) on HEAD.
mg_gate__scheduled() {
  local mode="$1" tip config="$MG_RUN_DIR/config.json" plan="$MG_RUN_DIR/plan.json"
  local results="$MG_RUN_DIR/results.jsonl" receipt="$MG_RUN_DIR/receipt.json"
  shift
  tip="$(mg_gate__commit HEAD)"
  mg_gate__load "$config" || return 1
  mg_gate__plan "$config" "" "$mode" "$plan" --var "base=$tip" --var "tip=$tip" "$@" || return 1
  mg_gate__execute "$config" "$plan" "$results"
  mg_artifact results "$results" "job results"
  if ! mg_gate__evaluate "$config" "$plan" "$results" "$receipt" >"$MG_RUN_DIR/evaluate-errors.json"; then
    mg_failure evaluate "evaluating the results failed: $(cat "$MG_RUN_DIR/evaluate-errors.json")"
    mg_status error
    return 1
  fi
  mg_gate__record "$receipt" "$mode"
}

mg_nightly() {
  local mode=nightly seed="" print=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --weekly) mode=weekly; shift ;;
      --seed)
        case "${2:-}" in '' | *[!0-9]*) mg_gate__usage "--seed takes a whole number"; return 0 ;; esac
        seed="$2"
        shift 2
        ;;
      --print-schedule) print=true; shift ;;
      *) mg_gate__usage "unknown argument '$1'; usage: tools/mg nightly [--weekly] [--seed N] [--print-schedule]"; return 0 ;;
    esac
  done
  if [ "$print" = true ]; then
    mg_nightly__plist "$MG_NIGHTLY_LABEL" no >"$MG_RUN_DIR/$MG_NIGHTLY_LABEL.plist"
    mg_nightly__plist "$MG_NIGHTLY_LABEL.weekly" yes >"$MG_RUN_DIR/$MG_NIGHTLY_LABEL.weekly.plist"
    mg_artifact launchd "$MG_RUN_DIR/$MG_NIGHTLY_LABEL.plist" "nightly entry"
    mg_artifact launchd "$MG_RUN_DIR/$MG_NIGHTLY_LABEL.weekly.plist" "weekly entry"
    mg_nightly__scheduler_step
    mg_message "nightly: launchd entries written to ${MG_RUN_DIR#"$MG_ROOT"/}; Antoine installs them (user step nightly-scheduler)"
    mg_status pass
    return 0
  fi
  # R1937: one seed per night, logged, replayable.
  [ -n "$seed" ] || seed="$(( $(od -An -N4 -tu4 /dev/urandom | tr -d ' ') % 2147483647 + 1 ))"
  mg_detail seed "$seed"
  mg_nightly__scheduler_step
  mg_gate__scheduled "$mode" --var "seed=$seed" || return 0
  mg_message "$(cat "$MG_STATE/message") (seed $seed; replay: tools/mg nightly$([ "$mode" = weekly ] && echo " --weekly") --seed $seed)"
}
