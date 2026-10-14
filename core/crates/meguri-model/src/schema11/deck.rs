//! Deck objects (`specs/model/schema11-json.md` S7 to S9, S11).

use serde_json::value::RawValue;

use super::{ObjectReader, Result, nullable, other_from_members, other_members, truncate_u32, write_other};
use crate::deck::{Deck, human_name, native_name};
use crate::error::{ModelError, Schema11Item};
use crate::ids::DeckId;
use crate::json::{self, ObjectWriter};
use crate::proto::ProtoBlob;
use crate::proto::deck::{DayLimit, DeckCommon, DeckKind, DeckKindMessage, FilteredDeck, NormalDeck, SearchTerm};
use crate::time::TimestampSecs;
use crate::usn::Usn;

const ITEM: Schema11Item = Schema11Item::Deck;

/// S3 reserved keys, for normal and filtered decks.
const RESERVED: [&str; 20] = [
    "browserCollapsed",
    "collapsed",
    "conf",
    "desc",
    "desiredRetention",
    "dyn",
    "extendNew",
    "extendRev",
    "id",
    "lrnToday",
    "mod",
    "name",
    "newLimit",
    "newLimitToday",
    "newToday",
    "revToday",
    "reviewLimit",
    "reviewLimitToday",
    "timeToday",
    "usn",
];

const COMMON_KEYS: [&str; 12] = [
    "id",
    "mod",
    "name",
    "usn",
    "lrnToday",
    "revToday",
    "newToday",
    "timeToday",
    "collapsed",
    "browserCollapsed",
    "desc",
    "dyn",
];
const NORMAL_KEYS: [&str; 8] = [
    "conf",
    "extendNew",
    "extendRev",
    "reviewLimit",
    "newLimit",
    "reviewLimitToday",
    "newLimitToday",
    "desiredRetention",
];
const FILTERED_KEYS: [&str; 8] = [
    "resched",
    "terms",
    "separate",
    "delays",
    "previewDelay",
    "previewAgainSecs",
    "previewHardSecs",
    "previewGoodSecs",
];

/// S7 output: the common keys, the `other` members, then the keys of the
/// deck's kind. `other` members the object writes anywhere are left out, so
/// no key appears twice.
pub fn deck_to_json(deck: &Deck) -> String {
    let common: &DeckCommon = &deck.common;
    let day = common.last_day_studied as i32;
    let mut out = ObjectWriter::new();
    out.int("id", deck.id.0);
    out.int("mod", deck.mtime.0);
    out.string("name", &human_name(&deck.name));
    out.int("usn", deck.usn.0);
    write_day_counter(out.key("lrnToday"), day, common.learning_studied);
    write_day_counter(out.key("revToday"), day, common.review_studied);
    write_day_counter(out.key("newToday"), day, common.new_studied);
    write_day_counter(out.key("timeToday"), day, common.milliseconds_studied);
    out.bool("collapsed", common.study_collapsed);
    out.bool("browserCollapsed", common.browser_collapsed);
    let mut written: Vec<&str> = COMMON_KEYS.to_vec();
    match &deck.kind.kind {
        DeckKind::Normal(normal) => {
            out.string("desc", &normal.description);
            if normal.markdown_description {
                out.bool("md", true);
                written.push("md");
            }
            out.int("dyn", 0);
            written.extend(NORMAL_KEYS);
            write_other(&mut out, &other_members(common.other()), &RESERVED, &written);
            write_normal(&mut out, normal);
        }
        DeckKind::Filtered(filtered) => {
            out.string("desc", "");
            out.int("dyn", 1);
            written.extend(FILTERED_KEYS);
            write_other(&mut out, &other_members(common.other()), &RESERVED, &written);
            write_filtered(&mut out, filtered);
        }
    }
    out.finish()
}

/// S7: `[day, amount]`.
fn write_day_counter(out: &mut String, day: i32, amount: i32) {
    out.push('[');
    json::push_int(out, day);
    out.push(',');
    json::push_int(out, amount);
    out.push(']');
}

/// S8 output.
fn write_normal(out: &mut ObjectWriter, normal: &NormalDeck) {
    out.int("conf", normal.config_id);
    out.int("extendNew", normal.extend_new as i32);
    out.int("extendRev", normal.extend_review as i32);
    match normal.review_limit {
        Some(limit) => out.int("reviewLimit", limit),
        None => out.null("reviewLimit"),
    }
    match normal.new_limit {
        Some(limit) => out.int("newLimit", limit),
        None => out.null("newLimit"),
    }
    write_day_limit(out, "reviewLimitToday", normal.review_limit_today.as_ref());
    write_day_limit(out, "newLimitToday", normal.new_limit_today.as_ref());
    match normal.desired_retention {
        Some(retention) => out.int("desiredRetention", truncate_u32(retention * 100.0)),
        None => out.null("desiredRetention"),
    }
}

fn write_day_limit(out: &mut ObjectWriter, key: &str, limit: Option<&DayLimit>) {
    match limit {
        Some(limit) => {
            let mut object = ObjectWriter::new();
            object.int("limit", limit.limit);
            object.int("today", limit.today);
            out.raw(key, &object.finish());
        }
        None => out.null(key),
    }
}

/// S9 output.
fn write_filtered(out: &mut ObjectWriter, filtered: &FilteredDeck) {
    out.bool("resched", filtered.reschedule);
    let terms = out.key("terms");
    terms.push('[');
    for (i, term) in filtered.search_terms.iter().enumerate() {
        if i > 0 {
            terms.push(',');
        }
        terms.push('[');
        json::push_string(terms, &term.search);
        terms.push(',');
        json::push_int(terms, term.limit as i32);
        terms.push(',');
        json::push_int(terms, term.order);
        terms.push(']');
    }
    terms.push(']');
    out.bool("separate", true);
    if filtered.delays.is_empty() {
        out.null("delays");
    } else {
        out.f32_array("delays", &filtered.delays);
    }
    out.int("previewDelay", filtered.preview_delay);
    out.int("previewAgainSecs", filtered.preview_again_secs);
    out.int("previewHardSecs", filtered.preview_hard_secs);
    out.int("previewGoodSecs", filtered.preview_good_secs);
}

/// S7 input. The name arrives in human form and is stored native (S11). The
/// record's blobs are new (no original bytes).
pub fn deck_from_json(text: &str) -> Result<Deck> {
    let raw = json::parse_value(text).ok_or_else(|| ModelError::invalid_json(ITEM, ""))?;
    deck_from_raw(raw)
}

pub(crate) fn deck_from_raw(raw: &RawValue) -> Result<Deck> {
    let mut r = ObjectReader::new(raw, ITEM, "")?;
    let id = r.strict("id", json::i64_or_string, None)?;
    let mtime = r.strict("mod", json::i64_or_string, Some(0))?;
    let name = r.strict("name", json::string, None)?;
    let usn = r.strict("usn", json::i32_value, None)?;
    let learning = r.strict("lrnToday", day_counter, None)?;
    let review = r.strict("revToday", day_counter, None)?;
    let new = r.strict("newToday", day_counter, None)?;
    let time = r.strict("timeToday", day_counter, None)?;
    let study_collapsed = r.strict("collapsed", json::boolean, None)?;
    let browser_collapsed = r.strict("browserCollapsed", json::boolean, Some(false))?;
    let description = r.strict("desc", json::string, Some(String::new()))?;
    let markdown_description = r.strict("md", json::boolean, Some(false))?;
    let filtered = r.strict("dyn", is_filtered, None)?;
    r.discard("return");
    let kind = if filtered {
        DeckKind::Filtered(filtered_from(&mut r)?)
    } else {
        DeckKind::Normal(normal_from(&mut r, description, markdown_description)?)
    };
    let other = other_from_members(&r.unread());

    // S7.1: the day is the latest of three counters' days; the learning,
    // review and new amounts count only on that day.
    let day = time.0.max(new.0).max(review.0);
    let on_day = |(counter_day, amount): (i32, i32)| if counter_day == day { amount } else { 0 };
    let common = DeckCommon {
        study_collapsed,
        browser_collapsed,
        last_day_studied: day as u32,
        new_studied: on_day(new),
        review_studied: on_day(review),
        learning_studied: on_day(learning),
        milliseconds_studied: time.1,
        other,
        ..DeckCommon::default()
    };
    Ok(Deck {
        id: DeckId(id),
        name: native_name(&name),
        mtime: TimestampSecs(mtime),
        usn: Usn(usn),
        common: ProtoBlob::new(common),
        kind: ProtoBlob::new(DeckKindMessage::new(kind)),
    })
}

/// S7.1: an array whose last element is the amount and the one before it the
/// day, each counting only as an integer within `i64` (then its low 32 bits),
/// else 0. Anything but an array refuses.
fn day_counter(raw: &RawValue) -> Option<(i32, i32)> {
    let items = json::array(raw)?;
    let part = |back: usize| {
        items
            .len()
            .checked_sub(back)
            .and_then(|i| json::i64_value(items[i]))
            .map_or(0, |v| v as i32)
    };
    Some((part(2), part(1)))
}

/// S7.2: `true`/`false`, or an integer from 0 to 255 that is filtered when 1.
fn is_filtered(raw: &RawValue) -> Option<bool> {
    json::boolean(raw).or_else(|| json::u8_value(raw).map(|v| v == 1))
}

/// S8 input.
fn normal_from(r: &mut ObjectReader<'_>, description: String, markdown_description: bool) -> Result<NormalDeck> {
    let config_id = r.strict("conf", json::i64_or_string, None)?;
    let extend_new = r.lenient("extendNew", json::i32_value, Some(0))?;
    let extend_review = r.lenient("extendRev", json::i32_value, Some(0))?;
    let review_limit = r.lenient("reviewLimit", nullable(json::u32_value), Some(None))?;
    let new_limit = r.lenient("newLimit", nullable(json::u32_value), Some(None))?;
    let review_limit_today = r.lenient("reviewLimitToday", nullable(day_limit), Some(None))?;
    let new_limit_today = r.lenient("newLimitToday", nullable(day_limit), Some(None))?;
    let desired_retention = r.lenient("desiredRetention", nullable(json::u32_value), Some(None))?;
    Ok(NormalDeck {
        config_id,
        extend_new: u32::try_from(extend_new).unwrap_or(0),
        extend_review: u32::try_from(extend_review).unwrap_or(0),
        description,
        markdown_description,
        review_limit,
        new_limit,
        review_limit_today,
        new_limit_today,
        desired_retention: desired_retention.map(|percent| percent as f32 / 100.0),
        ..NormalDeck::default()
    })
}

/// S8: an object with `limit` and `today` as `u32`; other members ignored.
fn day_limit(raw: &RawValue) -> Option<DayLimit> {
    let members = json::object(raw)?;
    let member = |name: &str| members.iter().find(|(key, _)| key == name).map(|(_, raw)| *raw);
    Some(DayLimit::new(
        json::u32_value(member("limit")?)?,
        json::u32_value(member("today")?)?,
    ))
}

/// S9 input.
fn filtered_from(r: &mut ObjectReader<'_>) -> Result<FilteredDeck> {
    let reschedule = r.strict("resched", json::bool_any, None)?;
    let search_terms = r.strict("terms", search_terms, None)?;
    r.strict("separate", json::boolean, Some(false))?;
    let delays = r.lenient("delays", nullable(json::f32_array), Some(None))?;
    let preview_delay = r.strict("previewDelay", json::u32_value, Some(0))?;
    let preview_again_secs = r.strict("previewAgainSecs", json::u32_value, Some(0))?;
    let preview_hard_secs = r.strict("previewHardSecs", json::u32_value, Some(0))?;
    let preview_good_secs = r.strict("previewGoodSecs", json::u32_value, Some(0))?;
    Ok(FilteredDeck {
        reschedule,
        search_terms,
        delays: delays.unwrap_or_default(),
        preview_delay,
        preview_hard_secs,
        preview_good_secs,
        preview_again_secs,
        ..FilteredDeck::default()
    })
}

/// S9 `terms`: `[search, limit, order]` per term; a negative limit gives 0.
fn search_terms(raw: &RawValue) -> Option<Vec<SearchTerm>> {
    json::array(raw)?
        .into_iter()
        .map(|term| {
            let parts = json::array(term)?;
            let [search, limit, order] = parts.as_slice() else {
                return None;
            };
            Some(SearchTerm::new(
                json::string(search)?,
                u32::try_from(json::i32_or_string(limit)?).unwrap_or(0),
                json::i32_value(order)?,
            ))
        })
        .collect()
}
