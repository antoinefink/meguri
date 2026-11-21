//! Pure, connection-local SQL functions: the card-data functions
//! (search/card-extractors X1-X8), the expression functions of
//! `text-matching.md` T14 and the name and id helpers of `names-and-ids.md`
//! N7. No SQL against collection tables or persistent schema objects lives
//! here.
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use meguri_model::card_data::extractors::Fields;
use meguri_model::proto::BlobMessage;
use meguri_model::proto::deck::{DeckKind, DeckKindMessage};
use regex::Regex;
use rusqlite::{
    Connection,
    functions::{Context, FunctionFlags},
    types::ValueRef,
};

use crate::pattern::Like;

/// Register on each writer and reader through storage's C5 initializers.
pub fn register(conn: &Connection) -> rusqlite::Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS;
    conn.create_scalar_function("meguri_card_f32", 2, flags, |ctx| {
        let key = match ctx.get_raw(1) {
            ValueRef::Text(b"s") => 0,
            ValueRef::Text(b"d") => 1,
            ValueRef::Text(b"dr") => 2,
            ValueRef::Text(b"decay") => 3,
            _ => {
                return Err(rusqlite::Error::UserFunctionError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid card-data scalar key",
                ))));
            }
        };
        let fields = data(ctx);
        Ok(sql_real(
            [
                fields.stability,
                fields.difficulty,
                fields.desired_retention,
                fields.decay,
            ][key],
        ))
    })?;
    conn.create_scalar_function("meguri_original_position", 1, flags, |ctx| {
        Ok(data(ctx).position.map(i64::from))
    })?;
    conn.create_scalar_function("meguri_retrievability", 5, flags, |ctx| {
        Ok(sql_real(retrievability(ctx)))
    })?;
    conn.create_scalar_function("meguri_relative_retrievability", 5, flags, |ctx| {
        Ok(sql_real(relative(ctx)))
    })?;
    // P15: never an error; data or a key that is not TEXT reads as no keys.
    conn.create_scalar_function("meguri_custom_data", 2, flags, |ctx| {
        let (ValueRef::Text(data), ValueRef::Text(key)) = (ctx.get_raw(0), ctx.get_raw(1)) else {
            return Ok(None);
        };
        let (Ok(data), Ok(key)) = (std::str::from_utf8(data), std::str::from_utf8(key)) else {
            return Ok(None);
        };
        Ok(crate::custom_data::custom_data(data, key))
    })?;
    register_text(conn, flags)
}

/// T14 and N7: the expression and name functions.
fn register_text(conn: &Connection, flags: FunctionFlags) -> rusqlite::Result<()> {
    let last = Mutex::new(None);
    conn.create_scalar_function("meguri_regex", 2, flags, move |ctx| {
        let (Some(regex), Some(text)) = (expression(ctx, &last)?, text(ctx, 1)) else {
            return Ok(None);
        };
        Ok(Some(regex.is_match(&text)))
    })?;
    let last = Mutex::new(None);
    conn.create_scalar_function("meguri_regex_tags", 2, flags, move |ctx| {
        let (Some(regex), Some(tags)) = (expression(ctx, &last)?, text(ctx, 1)) else {
            return Ok(None);
        };
        Ok(Some(tags.split(' ').any(|piece| regex.is_match(piece))))
    })?;
    let last = Mutex::new(None);
    conn.create_scalar_function("meguri_regex_fields", -1, flags, move |ctx| {
        if ctx.len() < 2 {
            return Err(function_error("meguri_regex_fields takes a pattern and a field string"));
        }
        let (Some(regex), Some(flds)) = (expression(ctx, &last)?, text(ctx, 1)) else {
            return Ok(None);
        };
        let fields: Vec<&str> = flds.split('\u{1f}').collect();
        if ctx.len() == 2 {
            return Ok(Some(fields.iter().any(|field| regex.is_match(field))));
        }
        for index in 2..ctx.len() {
            let ord = integer(ctx, index).ok_or_else(|| function_error("field positions must be integers"))?;
            if usize::try_from(ord)
                .ok()
                .and_then(|ord| fields.get(ord))
                .is_some_and(|field| regex.is_match(field))
            {
                return Ok(Some(true));
            }
        }
        Ok(Some(false))
    })?;
    conn.create_scalar_function("meguri_like", 3, flags, |ctx| {
        let unicode = match ctx.get_raw(2) {
            ValueRef::Integer(0) => false,
            ValueRef::Integer(1) => true,
            _ => return Err(function_error("meguri_like mode must be 0 or 1")),
        };
        let Some(text) = text(ctx, 0) else {
            return Ok(None);
        };
        let like = match ctx.get_aux::<Like>(1)? {
            Some(like) => like,
            None => {
                let pattern = match ctx.get_raw(1) {
                    ValueRef::Null => return Ok(None),
                    ValueRef::Text(bytes) => {
                        std::str::from_utf8(bytes).map_err(|_| function_error("pattern is not UTF-8"))?
                    }
                    _ => return Err(function_error("pattern must be text")),
                };
                let like = Like::new(pattern, unicode).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
                ctx.set_aux(1, like)?
            }
        };
        Ok(Some(like.is_match(&text)))
    })?;
    conn.create_scalar_function("meguri_transform", 2, flags, |ctx| {
        let ValueRef::Integer(flags) = ctx.get_raw(1) else {
            return Err(function_error("meguri_transform flags must be an integer"));
        };
        if flags & !(TRANSFORM_COMBINING_FREE | TRANSFORM_STRIP_CLOZE) != 0 {
            return Err(function_error("unsupported meguri_transform flags"));
        }
        let Some(text) = text(ctx, 0) else {
            return Ok(None);
        };
        let text = if flags & TRANSFORM_STRIP_CLOZE != 0 {
            Cow::Owned(crate::pattern::strip_cloze(&text).into_owned())
        } else {
            text
        };
        Ok(Some(if flags & TRANSFORM_COMBINING_FREE != 0 {
            crate::pattern::combining_free(&text).into_owned()
        } else {
            text.into_owned()
        }))
    })?;
    conn.create_scalar_function("meguri_regex_escape", 1, flags, |ctx| {
        Ok(text(ctx, 0).map(|text| regex::escape(&text)))
    })?;
    conn.create_scalar_function("meguri_deck_preset", 1, flags, |ctx| {
        let ValueRef::Blob(bytes) = ctx.get_raw(0) else {
            return Ok(None);
        };
        Ok(match DeckKindMessage::decode(bytes) {
            Ok(DeckKindMessage {
                kind: DeckKind::Normal(normal),
                ..
            }) => Some(normal.config_id),
            _ => None,
        })
    })?;
    conn.create_scalar_function("meguri_first_field_stripped", 1, flags, |ctx| {
        Ok(text(ctx, 0).map(|flds| {
            let first = flds.split('\u{1f}').next().unwrap_or_default();
            meguri_text::html::strip_markup_keep_media(first).into_owned()
        }))
    })?;
    Ok(())
}

/// `meguri_transform`'s bit 0: the combining-free transform (T8.1).
pub const TRANSFORM_COMBINING_FREE: i64 = 1;
/// `meguri_transform`'s bit 1: cloze markup removal (T9.2), before bit 0.
pub const TRANSFORM_STRIP_CLOZE: i64 = 2;

fn function_error(message: &'static str) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(message.into())
}

/// A text argument (T14): TEXT as stored, INTEGER and REAL as SQLite's text
/// for them (T2: the text `CAST(x AS TEXT)` gives); NULL and BLOB give none,
/// so the function gives NULL.
fn text<'a>(ctx: &'a Context<'_>, index: usize) -> Option<Cow<'a, str>> {
    match ctx.get_raw(index) {
        ValueRef::Text(bytes) => Some(String::from_utf8_lossy(bytes)),
        ValueRef::Integer(value) => Some(Cow::Owned(value.to_string())),
        ValueRef::Real(value) => real_text(value).map(Cow::Owned),
        ValueRef::Null | ValueRef::Blob(_) => None,
    }
}

/// SQLite's text for a REAL, exactly as `CAST(x AS TEXT)` gives it (T2),
/// from the same library on a connection of this thread's own: its rendering
/// (up to 17 significant digits, by its own decimal conversion) is not
/// reproduced here. The plan's statements cast `sfld` themselves, so a REAL
/// reaches a function only from other callers.
fn real_text(value: f64) -> Option<String> {
    thread_local! {
        static CAST: Option<Connection> = Connection::open_in_memory().ok();
    }
    CAST.with(|conn| {
        let conn = conn.as_ref()?;
        let mut statement = conn.prepare_cached("SELECT CAST(?1 AS TEXT)").ok()?;
        statement.query_row([value], |row| row.get::<_, String>(0)).ok()
    })
}

/// The compiled expression of argument 0 (T14): SQLite's per-statement
/// auxiliary data when the argument is constant, else the last expression
/// this function compiled (a pattern computed per row, `deck:current`). The
/// plan compiled every pattern already, so a failure here is a programming
/// mistake.
fn expression(ctx: &Context<'_>, last: &Mutex<Option<(String, Arc<Regex>)>>) -> rusqlite::Result<Option<Arc<Regex>>> {
    if let Some(regex) = ctx.get_aux::<Arc<Regex>>(0)? {
        return Ok(Some(Arc::clone(&regex)));
    }
    let pattern = match ctx.get_raw(0) {
        ValueRef::Null => return Ok(None),
        ValueRef::Text(bytes) => std::str::from_utf8(bytes).map_err(|_| function_error("pattern is not UTF-8"))?,
        _ => return Err(function_error("pattern must be text")),
    };
    let mut last = last.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let regex = match last.as_ref() {
        Some((text, regex)) if text == pattern => Arc::clone(regex),
        _ => {
            let regex = Arc::new(Regex::new(pattern).map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?);
            *last = Some((pattern.to_owned(), Arc::clone(&regex)));
            regex
        }
    };
    ctx.set_aux(0, Arc::clone(&regex))?;
    Ok(Some(regex))
}

fn data(ctx: &Context<'_>) -> Fields {
    match ctx.get_raw(0) {
        ValueRef::Text(bytes) => std::str::from_utf8(bytes).map(Fields::parse).unwrap_or_default(),
        _ => Fields::default(),
    }
}

fn integer(ctx: &Context<'_>, index: usize) -> Option<i64> {
    match ctx.get_raw(index) {
        ValueRef::Integer(value) => Some(value),
        _ => None,
    }
}

fn sql_real(value: Option<f32>) -> Option<f64> {
    value.filter(|v| !v.is_nan()).map(f64::from)
}

fn recall(fields: Fields, seconds: u32) -> Option<f32> {
    Some(fsrs::current_retrievability(
        fsrs::MemoryState {
            stability: fields.stability?,
            difficulty: fields.difficulty?,
        },
        seconds as f32 / 86_400.0,
        fields.decay.unwrap_or(0.5),
    ))
}

fn retrievability(ctx: &Context<'_>) -> Option<f32> {
    let due = integer(ctx, 1)?;
    let now = integer(ctx, 4)? as u32;
    let fields = data(ctx);
    let elapsed = match fields.last_review_secs {
        Some(last) => now.saturating_sub(last as u32),
        None => {
            let interval = integer(ctx, 2)? as u32;
            let start = (due as u32).saturating_sub(interval);
            if due > 365_000 {
                now.saturating_sub(start)
            } else {
                (integer(ctx, 3)? as u32).saturating_sub(start).wrapping_mul(86_400)
            }
        }
    };
    recall(fields, elapsed)
}

fn relative(ctx: &Context<'_>) -> Option<f32> {
    let due = integer(ctx, 1)?;
    let interval = integer(ctx, 2)?;
    let now = integer(ctx, 4)? as u32;
    let elapsed = if due > 365_000 {
        now.saturating_sub(due as u32)
    } else {
        (integer(ctx, 3)? as u32)
            .saturating_sub(due.saturating_sub(interval) as u32)
            .wrapping_mul(86_400)
    };
    let fields = data(ctx);
    if let (Some(_), Some(_), Some(target)) = (fields.stability, fields.difficulty, fields.desired_retention) {
        let elapsed = fields
            .last_review_secs
            .map_or(elapsed, |last| now.saturating_sub(last as u32));
        let r = recall(fields, elapsed)?.max(0.0001);
        let power = -1.0f32 / fields.decay.unwrap_or(0.5);
        Some(-(r.powf(power) - 1.0) / (target.max(0.0001).powf(power) - 1.0))
    } else {
        Some(-((elapsed / 86_400) as f32 + 0.001) / (interval as f32).max(1.0))
    }
}
