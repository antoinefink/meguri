//! Pure, connection-local card-data functions (search/card-extractors X1-X8).
//! No SQL against collection tables or persistent schema objects lives here.
use meguri_model::card_data::extractors::Fields;
use rusqlite::{
    Connection,
    functions::{Context, FunctionFlags},
    types::ValueRef,
};

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
    Ok(())
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
