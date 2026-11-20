//! The state words and flags (`specs/search/state-and-flags.md` S1, S2, S5,
//! S6): stored `type`, `queue`, `due` and `flags` columns, never slow.

use crate::ast::CardState;
use crate::context::{ContextReads, ContextValue};

use super::sql::{Param, Params, TermEnv, TermSql};

fn stored(sql: &str) -> TermSql {
    TermSql {
        sql: sql.to_owned(),
        slow: false,
        reads: ContextReads::NONE,
    }
}

/// S5's table; `is:due` is S1.
pub(super) fn state(state: CardState, env: &TermEnv<'_>, params: &mut Params) -> TermSql {
    match state {
        CardState::New => stored("c.type = 0"),
        CardState::Learn => stored("c.type IN (1, 3)"),
        CardState::Review => stored("c.type IN (2, 3)"),
        CardState::Suspended => stored("c.queue = -1"),
        // S2: this stored predicate performs no unburying or day query.
        CardState::Buried => stored("c.queue IN (-2, -3)"),
        CardState::BuriedSibling => stored("c.queue = -2"),
        CardState::BuriedManually => stored("c.queue = -3"),
        // S1, S3: compare stored due, never type or odue.
        CardState::Due => {
            let today = params.bind(Param::Integer(i64::from(env.ctx.today)));
            // S3 permits saturating the upper cutoff over the whole i64
            // domain. Compute before SQL so SQLite cannot promote an overflow
            // to REAL.
            let cutoff = env.ctx.now.saturating_add(i64::from(env.ctx.learn_ahead_secs));
            let cutoff = params.bind(Param::Integer(cutoff));
            TermSql {
                sql: format!("(c.queue IN (2, 3) AND c.due <= {today}) OR (c.queue IN (1, 4) AND c.due <= {cutoff})"),
                slow: false,
                reads: ContextReads::NONE
                    .with(ContextValue::Now)
                    .with(ContextValue::Today)
                    .with(ContextValue::LearnAhead),
            }
        }
    }
}

/// S6: the lowest three bits of `flags`.
pub(super) fn flag(flag: u8) -> TermSql {
    TermSql {
        sql: format!("(c.flags & 7) = {flag}"),
        slow: false,
        reads: ContextReads::NONE,
    }
}
