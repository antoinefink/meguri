# Shared by the host smoke test and the synthetic doctor regressions.
# Full result schema validation remains in validate_result.py.
assert_doctor_result() { # <result.json> <observed exit code>
  # A diagnostic failure is valid evidence; an execution error is not.
  # Do not turn this into a readiness gate or exempt individual tools.
  jq -se --arg rc "$2" '
    def text: type == "string" and length > 0;
    def check($id): [.details.checks[] | select(.id == $id)][0].status;
    def step($id; $state):
      any(.user_steps[]; .id == $id and .state == $state and .required == true);
    length == 1 and (.[0] | . as $result |
      [.details.checks[] | select(.status == "pass")] as $passed |
      [.details.checks[] | select(.status == "warn")] as $warned |
      [.details.checks[] | select(.status == "fail") | {id, message, repro: "tools/mg doctor"}] as $failed |
      .job == "doctor" and .dry_run == false and
      .placement == "host" and .service == null and
      (.details.checks | type == "array" and length > 0) and
      all(.details.checks[];
        (.id | text) and (.message | text) and
        (.status == "pass" or .status == "warn" or .status == "fail")) and
      ([.details.checks[].id] | length == (unique | length)) and
      (["host", "xcode-select", "xcode", "xcode-license", "ios-runtime",
        "xcode-account", "docker", "rust", "disk", "host-session", "mac-input"]
        - [.details.checks[].id] | length == 0) and
      any(.details.checks[]; .id == "mac-vm" or (.id | startswith("mac-vm/"))) and
      .summary == {total: (.details.checks | length), passed: ($passed | length),
        warned: ($warned | length), failed: ($failed | length), skipped: 0} and
      .status == (if ($failed | length) > 0 then "fail" elif ($warned | length) > 0 then "warn" else "pass" end) and
      .exit_code == (if .status == "fail" then 1 else 0 end) and
      (.exit_code | tostring) == $rc and
      (.failures | sort_by(.id)) == ($failed | sort_by(.id)) and
      (.details.environment | type == "object") and
      (.details.environment as $env |
        all(["macos", "arch", "developer_dir", "xcode_version", "xcode_build",
          "docker_server", "docker_compose", "rust_channel"][];
          . as $key | ($env | has($key)) and
            ($env[$key] | . == null or type == "string"))) and
      (if check("xcode") == "pass" then
        (.details.environment.xcode_version | text) and
        (.details.environment.xcode_build | text) else true end) and
      (.user_steps | type == "array" and length > 0) and
      ([.user_steps[].id] | length == (unique | length)) and
      step("uitest-automation"; "unverified") and
      all(["xcode-select", "xcode-license", "ios-runtime"][];
        . as $id | $result | if check($id) == "fail" then step($id; "missing") else true end) and
      all(["host-session", "mac-input", "docker-resources", "mac-vm"][];
        . as $id | $result | if check($id) == "warn" then step($id; "missing") else true end) and
      (if check("xcode-account") == "warn" then step("xcode-account"; "unverified") else true end)
    )
  ' "$1" >/dev/null
}
