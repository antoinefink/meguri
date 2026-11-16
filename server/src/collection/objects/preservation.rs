// SPDX-License-Identifier: MIT
//! Server-only overlay from model/protobuf-records P4/P5 and schema11-json S4–S10.
//! The model owns decoding, encoding, captured unknowns and original bytes. This
//! adapter applies only differences from the stored record's JSON projection;
//! a lossy projection alone must not change a stored known value either.
use crate::Result;
use meguri_model::{
    deck::Deck,
    deck_config::DeckConfig,
    notetype::Notetype,
    proto::{BlobMessage, ProtoBlob, deck::{DayLimit, DeckKind, DeckCommon}},
    schema11,
};
use super::super::{model_adapter::blob, schema::invalid};

// All destinations are public model fields. In particular, cloning a nested
// message retains its private unknown-field capture; replacing it would not.
macro_rules! fields {
    ($stored:expr, $projected:expr, $incoming:expr; $($field:ident),+ $(,)?) => {
        $(if $projected.$field != $incoming.$field {
            $stored.$field.clone_from(&$incoming.$field);
        })+
    };
}

// Start with the incoming known message, restoring stored values whenever the
// wire projection did not change them. This also protects lossy enum/ordinal
// projections. Floats use bits rather than PartialEq (P4).
macro_rules! keep {
    ($out:expr, $old:expr, $projected:expr, $incoming:expr; $($field:ident),+ $(,)?) => {
        $(if $projected.$field == $incoming.$field {
            $out.$field.clone_from(&$old.$field);
        })+
    };
}
macro_rules! keep_floats {
    ($out:expr, $old:expr, $projected:expr, $incoming:expr; $($field:ident),+ $(,)?) => {
        $(if $projected.$field.to_bits() == $incoming.$field.to_bits() {
            $out.$field = $old.$field;
        })+
    };
}

fn overlay<'a,M:BlobMessage>(old:&ProtoBlob<M>, desired:&M, unknowns:impl Iterator<Item=&'a [u8]>, other:Option<&[u8]>)->Result<ProtoBlob<M>> {
    let mut bytes=desired.encode();
    if let Some(other)=other {
        // Public messages have an opaque `other` getter but no setter. P2/P7
        // permits appending the last occurrence of bytes field 255. The public
        // decoder, not a server parser, resolves it and canonicalizes on edits.
        bytes.extend_from_slice(&[0xfa,0x0f]);
        let mut len=other.len();
        while len>=128 {bytes.push((len as u8&127)|128);len>>=7;}
        bytes.push(len as u8);bytes.extend_from_slice(other);
    }
    for bytes_field in unknowns {bytes.extend_from_slice(bytes_field);}
    let mut out=old.clone();
    *out=blob::<M>(&bytes)?.into_message();
    Ok(out)
}

pub(crate) fn notetype(old:&Notetype, incoming:&mut Notetype)->Result<()> {
    let projected=schema11::notetype_from_json(&schema11::notetype_to_json(old)).map_err(|_|invalid())?;
    let (s,p,n)=(&old.config,&projected.config,&incoming.config);
    let mut config=(**n).clone();
    keep!(config,s,p,n; kind,sort_field_idx,css,target_deck_id_unused,latex_pre,latex_post,latex_svg,original_stock_kind,original_id);
    // Requirements correspond by array position, like fields/templates. An
    // explicitly removed entry has no successor; surviving entries retain
    // unknown fields even when one of their known values changes.
    for (index,requirement) in config.reqs.iter_mut().enumerate() {
        if let (Some(stored),Some(before))=(s.reqs.get(index),p.reqs.get(index)) {
            let mut out=stored.clone();
            fields!(out,before,requirement; card_ord,kind,field_ords);
            *requirement=out;
        }
    }
    incoming.config=overlay(s,&config,s.unknown_fields().iter().map(|f|f.bytes()),(p.other()==n.other()).then(||s.other()))?;
    for ((stored,before),field) in old.fields.iter().zip(&projected.fields).zip(&mut incoming.fields) {
        let (s,p,n)=(&stored.config,&before.config,&field.config);
        let mut config=(**n).clone();
        keep!(config,s,p,n; sticky,rtl,font_name,font_size,description,plain_text,collapsed,exclude_from_search,id,tag,prevent_deletion);
        field.config=overlay(s,&config,s.unknown_fields().iter().map(|f|f.bytes()),(p.other()==n.other()).then(||s.other()))?;
    }
    for ((stored,before),template) in old.templates.iter().zip(&projected.templates).zip(&mut incoming.templates) {
        let (s,p,n)=(&stored.config,&before.config,&template.config);
        let mut config=(**n).clone();
        keep!(config,s,p,n; q_format,a_format,q_format_browser,a_format_browser,target_deck_id,browser_font_name,browser_font_size,id);
        template.config=overlay(s,&config,s.unknown_fields().iter().map(|f|f.bytes()),(p.other()==n.other()).then(||s.other()))?;
    }
    Ok(())
}

pub(crate) fn preset(old:&DeckConfig, incoming:&mut DeckConfig)->Result<()> {
    let projected=schema11::deck_config_from_json(&schema11::deck_config_to_json(old)).map_err(|_|invalid())?;
    let (s,p,n)=(&old.config,&projected.config,&incoming.config);
    let mut config=(**n).clone();
    keep!(config,s,p,n; new_per_day,reviews_per_day,maximum_review_interval,minimum_lapse_interval,
        graduating_interval_good,graduating_interval_easy,new_card_insert_order,leech_action,leech_threshold,
        disable_autoplay,cap_answer_time_to_secs,show_timer,skip_question_when_replaying_answer,
        bury_new,bury_reviews,bury_interday_learning,new_mix,interday_learning_mix,new_card_sort_order,
        review_order,new_card_gather_priority,new_per_day_minimum,question_action,stop_timer_on_answer,
        answer_action,wait_for_audio,param_search,ignore_revlogs_before_date);
    keep_floats!(config,s,p,n; initial_ease,easy_multiplier,hard_multiplier,lapse_multiplier,interval_multiplier,
        desired_retention,historical_retention,seconds_to_show_question,seconds_to_show_answer);
    for (out,before,next,stored) in [
        (&mut config.learn_steps,&p.learn_steps,&n.learn_steps,&s.learn_steps),
        (&mut config.relearn_steps,&p.relearn_steps,&n.relearn_steps,&s.relearn_steps),
        (&mut config.easy_days_percentages,&p.easy_days_percentages,&n.easy_days_percentages,&s.easy_days_percentages),
    ] {
        if before.iter().map(|v|v.to_bits()).eq(next.iter().map(|v|v.to_bits())) {out.clone_from(stored);}
    }
    // The public codec carries all three independent FSRS lists, including the
    // read-only model lists. Do not use the optimizer setter or merge lists.
    let mut preserved=overlay(s,&config,s.unknown_fields().iter().map(|f|f.bytes()),(p.other()==n.other()).then(||s.other()))?;
    // Encoding implicit -0.0 omits it (P3). Restore desired float bits after
    // the public decode so a column-only edit still gets P5's original bytes.
    preserved.initial_ease=config.initial_ease;
    preserved.easy_multiplier=config.easy_multiplier;
    preserved.hard_multiplier=config.hard_multiplier;
    preserved.lapse_multiplier=config.lapse_multiplier;
    preserved.interval_multiplier=config.interval_multiplier;
    preserved.desired_retention=config.desired_retention;
    preserved.historical_retention=config.historical_retention;
    preserved.seconds_to_show_question=config.seconds_to_show_question;
    preserved.seconds_to_show_answer=config.seconds_to_show_answer;
    incoming.config=preserved;
    Ok(())
}

fn day_limit(stored: &mut Option<DayLimit>, projected: &Option<DayLimit>, incoming: &Option<DayLimit>) {
    match (stored.as_mut(), projected, incoming) {
        (Some(s), Some(p), Some(n)) => { fields!(s, p, n; limit, today); }
        (_, None, None) => {}
        _ => *stored = incoming.clone(),
    }
}

fn common(old: &ProtoBlob<DeckCommon>, projected: &DeckCommon, incoming: &DeckCommon) -> Result<ProtoBlob<DeckCommon>> {
    let mut out = old.clone();
    fields!(out, projected, incoming;
        study_collapsed, browser_collapsed, last_day_studied, new_studied,
        review_studied, learning_studied, milliseconds_studied);
    if projected.other() != incoming.other() {
        // `other` has no public setter. Build the desired known message with
        // the public codec, then append the exact captured unknown wire entries
        // (P2/P4). No local protobuf parser or model implementation is copied.
        let mut known = incoming.clone();
        known.study_collapsed = out.study_collapsed;
        known.browser_collapsed = out.browser_collapsed;
        known.last_day_studied = out.last_day_studied;
        known.new_studied = out.new_studied;
        known.review_studied = out.review_studied;
        known.learning_studied = out.learning_studied;
        known.milliseconds_studied = out.milliseconds_studied;
        let mut bytes = known.encode();
        for field in old.unknown_fields().iter() { bytes.extend_from_slice(field.bytes()); }
        let replacement: ProtoBlob<DeckCommon> = blob(&bytes)?;
        // Keep the original-byte owner even if the new `other` representation
        // turns out to have identical content.
        *out = replacement.into_message();
    }
    Ok(out)
}

pub(crate) fn deck(old: &Deck, incoming: &mut Deck) -> Result<()> {
    let projected = schema11::deck_from_json(&schema11::deck_to_json(old)).map_err(|_| invalid())?;
    incoming.common = common(&old.common, &projected.common, &incoming.common)?;
    let mut out = old.kind.clone();
    match (&mut out.kind, &projected.kind.kind, &incoming.kind.kind) {
        (DeckKind::Normal(s), DeckKind::Normal(p), DeckKind::Normal(n)) => {
            fields!(s, p, n; config_id, extend_new, extend_review, description,
                markdown_description, review_limit, new_limit);
            day_limit(&mut s.review_limit_today, &p.review_limit_today, &n.review_limit_today);
            day_limit(&mut s.new_limit_today, &p.new_limit_today, &n.new_limit_today);
            // P4 equality for floats is bitwise, including optional -0.0.
            if p.desired_retention.map(f32::to_bits) != n.desired_retention.map(f32::to_bits) {
                s.desired_retention = n.desired_retention;
            }
        }
        (DeckKind::Filtered(s), DeckKind::Filtered(p), DeckKind::Filtered(n)) => {
            fields!(s, p, n; reschedule, preview_delay, preview_hard_secs,
                preview_good_secs, preview_again_secs);
            if !p.delays.iter().map(|v| v.to_bits()).eq(n.delays.iter().map(|v| v.to_bits())) {
                s.delays = n.delays.clone();
            }
            // S9 carries an ordered array. Corresponding existing positions
            // keep their captures; explicitly removed entries have no successor.
            for (index, term) in n.search_terms.iter().enumerate() {
                if let (Some(stored), Some(before)) = (s.search_terms.get_mut(index), p.search_terms.get(index)) {
                    fields!(stored, before, term; search, limit, order);
                } else {
                    s.search_terms.push(term.clone());
                }
            }
            s.search_terms.truncate(n.search_terms.len());
        }
        // A changed one-of removes its former member; retain the envelope's
        // unknowns, while the intentionally replaced member has no successor.
        _ => out.kind = incoming.kind.kind.clone(),
    }
    incoming.kind = out;
    Ok(())
}
