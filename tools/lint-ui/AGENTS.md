# tools/lint-ui/

- Patterns live in `app/.swiftlint.yml` (custom rules only); scope, allowlist, inline-disable and path normalisation live in `path-rules.sh`. A key `<id>--<variant>` is a second pattern reported as `<id>`: use it when one rule needs other match kinds (strings).
- SwiftLint drops a match when any token in it has an excluded kind, so a pattern that spans a string literal (`.custom("…"`, `Label("…", image:`) never fires with `excluded_match_kinds: [string]`. Keep string literals out of the match with a lookahead.
- Every rule change gets a fixture case: `tests/fixtures/<case>/` is a tree laid out like the repo (`app/…`, optional `tools/lint-ui/allowlist.tsv`) with `expected.tsv` (`rule<TAB>path:line`, empty for clean). `tools/mg lint-ui-selftest` must pass; `tools/mg lint-ui --root tools/lint-ui/tests/fixtures/<case>` shows one case.
- The rule ids are cited by `docs/design/rubric.md`: keep its lint index in step.
