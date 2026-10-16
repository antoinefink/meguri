# Oracle driver: the `answer_labels` observation (U45)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the `anki` Python APIs
behind `driver/observe/labels.py` and to check that the worker's labels are
English with a full stop; meanings in `specs/conformance/observations.md` O10
and `specs/sched/answer-labels.md`, the spec writer's sources in
`sched-next-states.md`. No code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/lang.py` (`current_lang`, `set_lang`), `pylib/anki/_backend.py` (`RustBackend.__init__`) | a backend made without `langs` takes `current_lang`, which is `en` unless something calls `set_lang` (the driver never does) |
| `rslib/i18n/src/lib.rs` (`I18n::new`, `resources_for_js`, `want_comma_as_decimal_separator`) | the backend's language list (the requested languages, then `en-US`); the decimal separator comes from the first language with a number format, so an English list gives a full stop |
| `rslib/src/i18n/service.rs` (`i18n_resources`) | the backend call that reports that list, used for the driver's English check |
| `rslib/src/scheduler/timespan.rs` (`answer_button_time_collapsible`), `ftl/core/scheduling.ftl` | the label's shape the split relies on: the `<` outside the isolation marks around the amount, the suffixes, `(end)` |

Probed in the oracle image (`driver/tests/test_labels.py`): a default
collection's backend reports `en` and `en-US`; a Polish backend's labels do not
split, and the English check refuses that backend first.
