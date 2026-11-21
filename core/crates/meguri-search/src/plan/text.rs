//! The text terms (`specs/search/text-matching.md`, `fields.md`,
//! `case-folding.md`): plain text, `nc:`, `sc:`, `w:`, `re:` and field
//! terms.
//!
//! Field names, excluded fields, cloze note types and run patterns come from
//! the note types the plan was given (T15), so every pattern is checked and
//! compiled while the plan is built (T3.3, T12.2). Wildcard patterns run on
//! `meguri_like` in Unicode mode and on SQLite's `LIKE` in AsciiCompat mode;
//! expressions run on `meguri_regex` and `meguri_regex_fields`.

use std::collections::BTreeMap;

use meguri_text::normalize::nfc;

use crate::ast::TextMode;
use crate::context::{ContextReads, ContextValue, FoldMode};
use crate::error::{ErrorKind, SearchFailure};
use crate::notetypes::{NotetypeShape, SearchNotetypes, runs};
use crate::pattern::{self, LIKE_LIMIT, Wildcards};
use crate::sqlfns::{TRANSFORM_COMBINING_FREE, TRANSFORM_STRIP_CLOZE};

use super::sql::{Param, Params, TermEnv, TermSql};

/// The field separator of `notes.flds`.
const SEPARATOR: char = '\u{1f}';

/// T4: the value as the term tests it.
fn normalized(value: &str, env: &TermEnv<'_>) -> String {
    if env.ctx.normalize_note_text {
        nfc(value).into_owned()
    } else {
        value.to_owned()
    }
}

/// Unicode mode runs on `meguri_like`; AsciiCompat on SQLite's `LIKE`
/// (T14), which is Anki's comparison exactly (C6).
fn unicode(env: &TermEnv<'_>) -> bool {
    match env.ctx.fold {
        FoldMode::Unicode => true,
        #[cfg(feature = "test-hooks")]
        FoldMode::AsciiCompat => false,
    }
}

/// `text` (an SQL expression) through `meguri_transform` with `flags`.
fn transformed(text: &str, flags: i64) -> String {
    if flags == 0 {
        text.to_owned()
    } else {
        format!("meguri_transform({text}, {flags})")
    }
}

const SORT_TEXT: &str = "CAST(n.sfld AS TEXT)";
const FIELD_STRING: &str = "n.flds";

/// SQL id lists of note types.
fn id_list<'a>(types: impl IntoIterator<Item = &'a NotetypeShape>) -> String {
    types
        .into_iter()
        .map(|shape| shape.id.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// The wildcard tests of one term: each distinct pattern bound once.
struct PatternTests<'p> {
    params: &'p mut Params,
    unicode: bool,
    bound: BTreeMap<String, String>,
    /// Every test calls a Meguri function per row.
    slow: bool,
}

impl PatternTests<'_> {
    /// T3.3: a pattern over the limit fails the search.
    fn check(pattern: &str) -> Result<(), ErrorKind> {
        if pattern.len() > LIKE_LIMIT {
            return Err(ErrorKind::SearchFailed(SearchFailure::PatternTooLong));
        }
        Ok(())
    }

    /// `text LIKE pattern` in the fold mode.
    fn test(&mut self, text: &str, pattern: &str) -> String {
        let placeholder = match self.bound.get(pattern) {
            Some(placeholder) => placeholder.clone(),
            None => {
                let placeholder = self.params.bind(Param::Text(pattern.to_owned()));
                self.bound.insert(pattern.to_owned(), placeholder.clone());
                placeholder
            }
        };
        if self.unicode {
            self.slow = true;
            format!("meguri_like({text}, {placeholder}, 1)")
        } else {
            format!("{text} LIKE {placeholder} ESCAPE '\\'")
        }
    }
}

/// T7.2 and F4: the pattern of a run from `start` to `end` of a note type
/// with `count` fields.
fn run_pattern(pattern: &str, start: usize, end: usize, count: usize) -> String {
    let before = format!("%{SEPARATOR}").repeat(start);
    let after = format!("{SEPARATOR}%").repeat(count.saturating_sub(end + 1));
    format!("{before}{pattern}{after}")
}

/// Which note types a plain, `nc:` or `sc:` term tests (T9.1, T9.4).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kinds {
    All,
    Cloze,
    /// `-sc:`: the types not of the cloze kind.
    NotCloze,
}

impl Kinds {
    fn admits(self, shape: &NotetypeShape) -> bool {
        match self {
            Kinds::All => true,
            Kinds::Cloze => shape.cloze,
            Kinds::NotCloze => !shape.cloze,
        }
    }
}

/// T5.1 and T7.2 for a term whose pattern (`%` around it, transformed as the
/// term requires) is `pattern`, on texts through `meguri_transform` with
/// `flags`. Statement checks first, then the pattern checks.
fn substring(
    pattern: &str,
    flags: i64,
    kinds: Kinds,
    env: &TermEnv<'_>,
    params: &mut Params,
) -> Result<(String, bool), ErrorKind> {
    let notetypes = env.notetypes;
    let excluding = notetypes.any_excluded();
    if excluding && !notetypes.types().iter().any(NotetypeShape::has_included) {
        return Err(ErrorKind::SearchFailed(SearchFailure::NoSearchableField));
    }
    PatternTests::check(pattern)?;
    // The run patterns of every type the term tests, types without notes
    // included; grouped by what they test.
    let mut groups: BTreeMap<(Vec<String>, bool), Vec<&NotetypeShape>> = BTreeMap::new();
    for shape in notetypes.types().iter().filter(|s| kinds.admits(s) && s.has_excluded()) {
        let count = shape.fields.len();
        let patterns: Vec<String> = shape
            .included_runs()
            .into_iter()
            .map(|(start, end)| run_pattern(pattern, start, end, count))
            .collect();
        for run in &patterns {
            PatternTests::check(run)?;
        }
        if patterns.is_empty() {
            continue;
        }
        groups
            .entry((patterns, shape.sort_field_included()))
            .or_default()
            .push(shape);
    }
    let sort_text = transformed(SORT_TEXT, flags);
    let field_string = transformed(FIELD_STRING, flags);
    let mut wildcards = PatternTests {
        params,
        unicode: unicode(env),
        bound: BTreeMap::new(),
        slow: flags != 0,
    };
    let mut clauses = Vec::new();
    // The types without excluded fields: one run of every field (T5.1).
    // Bound only when used: an unused last parameter would not bind.
    let whole = |wildcards: &mut PatternTests<'_>| {
        format!(
            "({} OR {})",
            wildcards.test(&sort_text, pattern),
            wildcards.test(&field_string, pattern)
        )
    };
    // T7.1: with an excluded field anywhere, a note whose `mid` names no note
    // type matches none of these terms, so the types without excluded fields
    // are named (`IN`); without one, it is tested like any note, and `-sc:`
    // leaves out only the cloze types.
    let without_excluded =
        |kinds: Kinds| id_list(notetypes.types().iter().filter(|s| kinds.admits(s) && !s.has_excluded()));
    match kinds {
        Kinds::All | Kinds::NotCloze if !excluding => {
            let cloze = id_list(notetypes.types().iter().filter(|s| s.cloze));
            if kinds == Kinds::All || cloze.is_empty() {
                clauses.push(whole(&mut wildcards));
            } else {
                clauses.push(format!("(n.mid NOT IN ({cloze}) AND {})", whole(&mut wildcards)));
            }
        }
        _ => {
            let types = without_excluded(kinds);
            if !types.is_empty() {
                clauses.push(format!("(n.mid IN ({types}) AND {})", whole(&mut wildcards)));
            }
        }
    }
    for ((patterns, sort_included), shapes) in &groups {
        let mut tests: Vec<String> = patterns.iter().map(|run| wildcards.test(&field_string, run)).collect();
        if *sort_included {
            tests.push(wildcards.test(&sort_text, pattern));
        }
        clauses.push(format!(
            "(n.mid IN ({}) AND ({}))",
            id_list(shapes.iter().copied()),
            tests.join(" OR ")
        ));
    }
    let slow = wildcards.slow && !clauses.is_empty();
    Ok((joined_or(clauses), slow))
}

fn joined_or(clauses: Vec<String>) -> String {
    if clauses.is_empty() {
        "0".to_owned()
    } else {
        clauses.join(" OR ")
    }
}

fn reads(values: &[ContextValue]) -> ContextReads {
    values.iter().copied().collect()
}

/// `%`, T3.1 of the value, `%`.
fn substring_pattern(value: &str) -> String {
    format!("%{}%", pattern::like_pattern(value))
}

/// T5: plain text; with ignore accents on, T6 (the `nc:` matching).
pub(super) fn plain(value: &str, env: &TermEnv<'_>, params: &mut Params) -> Result<TermSql, ErrorKind> {
    let value = normalized(value, env);
    let reads = reads(&[
        ContextValue::NormalizeNoteText,
        ContextValue::IgnoreAccents,
        ContextValue::Fold,
    ]);
    let (pattern, flags) = if env.ctx.ignore_accents {
        (
            pattern::combining_free(&substring_pattern(&value)).into_owned(),
            TRANSFORM_COMBINING_FREE,
        )
    } else {
        (substring_pattern(&value), 0)
    };
    let (sql, slow) = substring(&pattern, flags, Kinds::All, env, params)?;
    Ok(TermSql { sql, slow, reads })
}

/// T8: `nc:`.
pub(super) fn no_combining(value: &str, env: &TermEnv<'_>, params: &mut Params) -> Result<TermSql, ErrorKind> {
    let value = normalized(value, env);
    let pattern = pattern::combining_free(&substring_pattern(&value)).into_owned();
    let (sql, slow) = substring(&pattern, TRANSFORM_COMBINING_FREE, Kinds::All, env, params)?;
    Ok(TermSql {
        sql,
        slow,
        reads: reads(&[ContextValue::NormalizeNoteText, ContextValue::Fold]),
    })
}

/// T9: `sc:`; `negated` when a `-` stands directly before it (T9.4).
pub(super) fn strip_cloze(
    value: &str,
    negated: bool,
    env: &TermEnv<'_>,
    params: &mut Params,
) -> Result<TermSql, ErrorKind> {
    if !env.notetypes.types().iter().any(|shape| shape.cloze) {
        return Err(ErrorKind::SearchFailed(SearchFailure::NoClozeNoteType));
    }
    let value = normalized(value, env);
    let (pattern, flags) = if env.ctx.ignore_accents {
        (
            pattern::combining_free(&substring_pattern(&value)).into_owned(),
            TRANSFORM_STRIP_CLOZE | TRANSFORM_COMBINING_FREE,
        )
    } else {
        (substring_pattern(&value), TRANSFORM_STRIP_CLOZE)
    };
    let kinds = if negated { Kinds::NotCloze } else { Kinds::Cloze };
    let (sql, slow) = substring(&pattern, flags, kinds, env, params)?;
    Ok(TermSql {
        sql,
        slow,
        reads: reads(&[
            ContextValue::NormalizeNoteText,
            ContextValue::IgnoreAccents,
            ContextValue::Fold,
        ]),
    })
}

/// T11.2 and T7.3: an expression tested on the whole field string, or on
/// each included field of the note types with excluded fields.
fn expression_test(
    expression: &str,
    message: &str,
    flags: i64,
    env: &TermEnv<'_>,
    params: &mut Params,
) -> Result<String, ErrorKind> {
    let notetypes = env.notetypes;
    let excluding = notetypes.any_excluded();
    if excluding && let Some(shape) = notetypes.first_without_included() {
        return Err(ErrorKind::SearchFailed(SearchFailure::AllFieldsExcluded {
            notetype: shape.name.clone(),
        }));
    }
    pattern::compile(expression, message)?;
    let field_string = transformed(FIELD_STRING, flags);
    let placeholder = params.bind(Param::Text(expression.to_owned()));
    let whole = format!("meguri_regex({placeholder}, {field_string})");
    if !excluding {
        return Ok(whole);
    }
    // T7.1: a note whose `mid` names no note type matches nothing here.
    let without_excluded = id_list(notetypes.types().iter().filter(|s| !s.has_excluded()));
    let mut clauses = Vec::new();
    if !without_excluded.is_empty() {
        clauses.push(format!("(n.mid IN ({without_excluded}) AND {whole})"));
    }
    let mut groups: BTreeMap<Vec<usize>, Vec<&NotetypeShape>> = BTreeMap::new();
    for shape in notetypes.types().iter().filter(|s| s.has_excluded()) {
        groups.entry(shape.included()).or_default().push(shape);
    }
    for (positions, shapes) in &groups {
        clauses.push(fields_test(&placeholder, &field_string, positions, shapes));
    }
    Ok(joined_or(clauses))
}

/// `meguri_regex_fields` on the listed positions of the note types.
fn fields_test(placeholder: &str, field_string: &str, positions: &[usize], shapes: &[&NotetypeShape]) -> String {
    let ords: String = positions.iter().map(|k| format!(", {k}")).collect();
    format!(
        "(n.mid IN ({}) AND meguri_regex_fields({placeholder}, {field_string}{ords}))",
        id_list(shapes.iter().copied())
    )
}

/// T11: `re:`.
pub(super) fn regex(value: &str, env: &TermEnv<'_>, params: &mut Params) -> Result<TermSql, ErrorKind> {
    let expression = format!("(?i){}", normalized(value, env));
    Ok(TermSql {
        sql: expression_test(&expression, value, 0, env, params)?,
        slow: true,
        reads: reads(&[ContextValue::NormalizeNoteText]),
    })
}

/// T10: `w:`.
pub(super) fn word(value: &str, env: &TermEnv<'_>, params: &mut Params) -> Result<TermSql, ErrorKind> {
    let body = format!(
        r"\b{}\b",
        pattern::glob_to_regex(&normalized(value, env), Wildcards::TEXT)
    );
    let (body, flags) = if env.ctx.ignore_accents {
        (pattern::combining_free(&body).into_owned(), TRANSFORM_COMBINING_FREE)
    } else {
        (body, 0)
    };
    Ok(TermSql {
        sql: expression_test(&format!("(?i){body}"), value, flags, env, params)?,
        slow: true,
        reads: reads(&[ContextValue::NormalizeNoteText, ContextValue::IgnoreAccents]),
    })
}

/// F2: the matching positions of each note type that has one.
fn resolve<'n>(name: &str, notetypes: &'n SearchNotetypes) -> Result<Vec<(&'n NotetypeShape, Vec<usize>)>, ErrorKind> {
    let matches: Box<dyn Fn(&str) -> bool> = if pattern::has_wildcards(name) {
        let expression = format!("^(?i){}$", pattern::glob_to_regex(name, Wildcards::TEXT));
        let regex = pattern::compile(&expression, name)?;
        Box::new(move |field: &str| regex.is_match(field))
    } else {
        let exact = pattern::exact_text(name).into_owned();
        Box::new(move |field: &str| meguri_text::fold::compare_folded(field, &exact).is_eq())
    };
    Ok(notetypes
        .types()
        .iter()
        .filter_map(|shape| {
            let positions: Vec<usize> = (0..shape.fields.len())
                .filter(|&k| matches(&shape.fields[k].name))
                .collect();
            (!positions.is_empty()).then_some((shape, positions))
        })
        .collect())
}

/// F2 step 2.
fn is_all_fields(text: &str) -> bool {
    matches!(text, "*" | "_*" | "*_")
}

/// F1-F7: `field:value` in its three modes.
pub(super) fn field(
    name: &str,
    mode: TextMode,
    text: &str,
    env: &TermEnv<'_>,
    params: &mut Params,
) -> Result<TermSql, ErrorKind> {
    let name = nfc(name);
    let value = normalized(text, env);
    let mut reads = reads(&[ContextValue::NormalizeNoteText]);
    if mode != TextMode::Regex {
        reads = reads.with(ContextValue::Fold);
    }
    if is_all_fields(&name) {
        // F7: each field alone, whatever its note type.
        let expression = match mode {
            TextMode::Regex => format!("(?i){value}"),
            TextMode::Normal | TextMode::NoCombining => {
                format!("(?is)^{}$", pattern::glob_to_regex(&value, Wildcards::TEXT))
            }
        };
        pattern::compile(&expression, text)?;
        let placeholder = params.bind(Param::Text(expression));
        return Ok(TermSql {
            sql: format!("meguri_regex_fields({placeholder}, {FIELD_STRING})"),
            slow: true,
            reads,
        });
    }
    let found = resolve(&name, env.notetypes)?;
    if mode == TextMode::Regex {
        // F5: compiled even when no note type has the field (T12.2).
        let expression = format!("(?i){value}");
        pattern::compile(&expression, text)?;
        if found.is_empty() {
            return Ok(TermSql {
                sql: "0".to_owned(),
                slow: false,
                reads,
            });
        }
        let placeholder = params.bind(Param::Text(expression));
        let mut groups: BTreeMap<Vec<usize>, Vec<&NotetypeShape>> = BTreeMap::new();
        for (shape, positions) in found {
            groups.entry(positions).or_default().push(shape);
        }
        let clauses = groups
            .iter()
            .map(|(positions, shapes)| fields_test(&placeholder, FIELD_STRING, positions, shapes))
            .collect();
        return Ok(TermSql {
            sql: joined_or(clauses),
            slow: true,
            reads,
        });
    }
    // F4, F6: the value's pattern on the runs of the matching positions.
    let (pattern, flags) = match mode {
        TextMode::NoCombining => (
            pattern::combining_free(&pattern::like_pattern(&value)).into_owned(),
            TRANSFORM_COMBINING_FREE,
        ),
        _ => (pattern::like_pattern(&value), 0),
    };
    let one_run_each = is_all_fields(text);
    let mut groups: BTreeMap<Vec<String>, Vec<&NotetypeShape>> = BTreeMap::new();
    for (shape, positions) in found {
        let count = shape.fields.len();
        let spans = if one_run_each {
            positions.iter().map(|&k| (k, k)).collect()
        } else {
            runs(&positions)
        };
        let patterns: Vec<String> = spans
            .into_iter()
            .map(|(start, end)| run_pattern(&pattern, start, end, count))
            .collect();
        for run in &patterns {
            PatternTests::check(run)?;
        }
        groups.entry(patterns).or_default().push(shape);
    }
    let field_string = transformed(FIELD_STRING, flags);
    let mut wildcards = PatternTests {
        params,
        unicode: unicode(env),
        bound: BTreeMap::new(),
        slow: flags != 0,
    };
    let clauses: Vec<String> = groups
        .iter()
        .map(|(patterns, shapes)| {
            let tests: Vec<String> = patterns.iter().map(|run| wildcards.test(&field_string, run)).collect();
            format!(
                "(n.mid IN ({}) AND ({}))",
                id_list(shapes.iter().copied()),
                tests.join(" OR ")
            )
        })
        .collect();
    let slow = wildcards.slow && !clauses.is_empty();
    Ok(TermSql {
        sql: joined_or(clauses),
        slow,
        reads,
    })
}
