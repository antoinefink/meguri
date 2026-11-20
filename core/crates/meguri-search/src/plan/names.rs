//! The name and id terms (`specs/search/names-and-ids.md`): `tag:`, `deck:`,
//! `preset:`, `note:`, `card:`, `nid:`, `cid:`, `did:`, `mid:` and `dupe:`.
//!
//! Names are matched inside the statement, against the rows it reads
//! (`decks`, `deck_config`, `notetypes`, `templates`), so the result always
//! equals the rules on the collection state the statement sees (N1). Only the
//! tag terms test an expression per note (slow, N7); the name subqueries run
//! their functions once per deck, preset or note type row.

use meguri_model::ids::NotetypeId;
use meguri_text::normalize::{nfc, remove_ascii_controls};

use crate::ast::{IdKind, TemplateRef, TextMode};
use crate::context::{ContextReads, ContextValue};
use crate::error::ErrorKind;
use crate::pattern::{self, Wildcards};

use super::Rows;
use super::sql::{Param, Params, TermEnv, TermSql};

/// The deck level separator of stored names (N3.1).
const LEVEL: char = '\u{1f}';

fn fast(sql: String) -> TermSql {
    TermSql {
        sql,
        slow: false,
        reads: ContextReads::NONE,
    }
}

/// N2: `tag:` in its three modes.
pub(super) fn tag(mode: TextMode, value: &str, params: &mut Params) -> Result<TermSql, ErrorKind> {
    let value = nfc(value);
    let regex_call = |column: &str, params: &mut Params, expression: String| {
        let pattern = params.bind(Param::Text(expression));
        TermSql {
            sql: format!("meguri_regex({pattern}, {column})"),
            slow: true,
            reads: ContextReads::NONE,
        }
    };
    let (column, converted) = match mode {
        // N2.3: none of the special values apply.
        TextMode::Regex => {
            let expression = format!("(?i){value}");
            pattern::compile(&expression, &value)?;
            let pattern = params.bind(Param::Text(expression));
            return Ok(TermSql {
                sql: format!("meguri_regex_tags({pattern}, n.tags)"),
                slow: true,
                reads: ContextReads::NONE,
            });
        }
        TextMode::Normal => ("n.tags".to_owned(), value.clone()),
        // N2.4: the steps of N2.2 on the value as read, then the transform
        // before the conversion.
        TextMode::NoCombining => (
            format!("meguri_transform(n.tags, {})", crate::sqlfns::TRANSFORM_COMBINING_FREE),
            std::borrow::Cow::Owned(pattern::combining_free(&value).into_owned()),
        ),
    };
    // N2.2 steps 1-3.
    match value.as_ref() {
        "none" => return Ok(fast("n.tags = ''".to_owned())),
        "*" => return Ok(fast("1".to_owned())),
        text if text.contains(' ') => return Ok(fast("0".to_owned())),
        _ => {}
    }
    let expression = format!("(?i).* {}(::| ).*", pattern::glob_to_regex(&converted, Wildcards::TAG));
    pattern::compile(&expression, &value)?;
    Ok(regex_call(&column, params, expression))
}

/// A card is in one of `decks` (an SQL id set) through `did`, or through
/// `odid` when it is in a filtered deck (N3.3, R1056).
fn in_decks(decks: &str) -> String {
    format!("c.did IN ({decks}) OR (c.odid != 0 AND c.odid IN ({decks}))")
}

/// N3: `deck:`.
pub(super) fn deck(value: &str, env: &TermEnv<'_>, params: &mut Params) -> Result<TermSql, ErrorKind> {
    let value = nfc(value);
    match value.as_ref() {
        "*" => return Ok(fast("1".to_owned())),
        "filtered" => return Ok(fast("c.odid != 0".to_owned())),
        "current" => return Ok(current_deck(env, params)),
        _ => {}
    }
    // N3.3 steps 1-2: the converted expression, cut into levels.
    let converted = pattern::glob_to_regex(&value, Wildcards::TEXT);
    let levels: Vec<String> = converted
        .split("::")
        .map(|piece| {
            let piece = nfc(piece);
            let piece = remove_ascii_controls(&piece);
            let piece = piece.trim_matches(|c: char| c.is_whitespace() || c == ':');
            if piece.is_empty() { "blank".to_owned() } else { piece.to_owned() }
        })
        .collect();
    let expression = format!("(?i)^{}($|{LEVEL})", levels.join(&LEVEL.to_string()));
    pattern::compile(&expression, &value)?;
    let pattern = params.bind(Param::Text(expression));
    Ok(fast(in_decks(&format!(
        "SELECT id FROM decks WHERE meguri_regex({pattern}, name)"
    ))))
}

/// N3.4: the current deck's tree, by the stored name of the current deck or
/// `Default` when it does not exist. The name is read in the statement, so
/// the expression is built there too; an escaped name holds no wildcard.
fn current_deck(env: &TermEnv<'_>, params: &mut Params) -> TermSql {
    let id = env.ctx.current_deck.0;
    let head = params.bind(Param::Text("(?i)^".to_owned()));
    let tail = params.bind(Param::Text(format!("($|{LEVEL})")));
    let fallback = params.bind(Param::Text("Default".to_owned()));
    let expression = format!(
        "{head} || meguri_regex_escape(COALESCE((SELECT name FROM decks WHERE id = {id}), {fallback})) || {tail}"
    );
    TermSql {
        sql: in_decks(&format!("SELECT id FROM decks WHERE meguri_regex({expression}, name)")),
        slow: false,
        reads: ContextReads::NONE.with(ContextValue::CurrentDeck),
    }
}

/// N3.6: `preset:`, the first preset by id whose name equals the value under
/// unicase equality, through the decks whose normal kind names it.
pub(super) fn preset(value: &str, params: &mut Params) -> TermSql {
    let name = params.bind(Param::Text(value.to_owned()));
    let preset = format!("SELECT id FROM deck_config WHERE name = {name} COLLATE unicase ORDER BY id LIMIT 1");
    let decks = format!("SELECT id FROM decks WHERE meguri_deck_preset(kind) = ({preset})");
    fast(format!("c.did IN ({decks}) OR c.odid IN ({decks})"))
}

/// N4's name rule on `table.name`: with wildcards the anchored, folded glob;
/// without, unicase equality with the exact text.
fn name_filter(value: &str, params: &mut Params) -> Result<String, ErrorKind> {
    if pattern::has_wildcards(value) {
        let expression = format!("(?i)^{}$", pattern::glob_to_regex(value, Wildcards::TEXT));
        pattern::compile(&expression, value)?;
        let pattern = params.bind(Param::Text(expression));
        Ok(format!("meguri_regex({pattern}, name)"))
    } else {
        let name = params.bind(Param::Text(pattern::exact_text(value).into_owned()));
        Ok(format!("name = {name} COLLATE unicase"))
    }
}

/// N4.1: `note:`.
pub(super) fn notetype(value: &str, params: &mut Params) -> Result<TermSql, ErrorKind> {
    let value = nfc(value);
    if value.is_empty() {
        return Ok(fast("0".to_owned()));
    }
    let filter = name_filter(&value, params)?;
    Ok(fast(format!("n.mid IN (SELECT id FROM notetypes WHERE {filter})")))
}

/// N4.2: `card:`.
pub(super) fn template(template: &TemplateRef, params: &mut Params) -> Result<TermSql, ErrorKind> {
    Ok(fast(match template {
        TemplateRef::Ordinal(ord) => format!("c.ord = {ord}"),
        TemplateRef::Name(value) => {
            let filter = name_filter(&nfc(value), params)?;
            format!("(n.mid, c.ord) IN (SELECT ntid, ord FROM templates WHERE {filter})")
        }
    }))
}

/// N3.5, N5: the ids of a list, as decimal numbers; runs beyond the signed
/// 64-bit range name no row. The plan raised `search_failed` already for a
/// digit other than ASCII.
fn id_list(list: &str) -> String {
    let mut ids: Vec<i64> = list.split(',').filter_map(|run| run.parse::<i64>().ok()).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

/// N3.5, N5: `did:`, `nid:`, `cid:`.
pub(super) fn ids(kind: IdKind, list: &str, env: &TermEnv<'_>) -> TermSql {
    let ids = id_list(list);
    if ids.is_empty() {
        return fast("0".to_owned());
    }
    fast(match kind {
        IdKind::Decks => format!("c.did IN ({ids}) OR (c.odid != 0 AND c.odid IN ({ids}))"),
        IdKind::Cards => format!("c.id IN ({ids})"),
        // Q5: from the card when cards are tested alone.
        IdKind::Notes if env.rows == Rows::Cards => format!("c.nid IN ({ids})"),
        IdKind::Notes => format!("n.id IN ({ids})"),
    })
}

/// N5: `mid:`.
pub(super) fn notetype_id(id: NotetypeId) -> TermSql {
    fast(format!("n.mid = {}", id.0))
}

/// N6: `dupe:`. The candidates come through the checksum index; their first
/// fields are stripped as the save path strips them and compared exactly.
pub(super) fn duplicates(notetype: NotetypeId, text: &str, env: &TermEnv<'_>, params: &mut Params) -> TermSql {
    let text = if env.ctx.normalize_note_text {
        nfc(text)
    } else {
        std::borrow::Cow::Borrowed(text)
    };
    let stripped = meguri_text::html::strip_markup_keep_media(&text).into_owned();
    let checksum = meguri_text::checksum::checksum_of_sort_text(&stripped);
    let stripped = params.bind(Param::Text(stripped));
    TermSql {
        sql: format!(
            "n.mid = {} AND n.csum = {checksum} AND meguri_first_field_stripped(n.flds) = {stripped}",
            notetype.0
        ),
        slow: false,
        reads: ContextReads::NONE.with(ContextValue::NormalizeNoteText),
    }
}

#[cfg(test)]
mod tests {
    use super::id_list;

    #[test]
    fn id_lists_read_runs_as_decimal_numbers() {
        assert_eq!(id_list("0400,7,7"), "7,400");
        assert_eq!(id_list("99999999999999999999,5"), "5");
        assert_eq!(id_list("9223372036854775807"), "9223372036854775807");
        assert_eq!(id_list("9223372036854775808"), "");
    }
}
