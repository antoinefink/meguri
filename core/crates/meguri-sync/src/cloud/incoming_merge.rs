//! K15 ordinary, part-wise merges. Storage and transport never choose winners.
use super::{
    Error,
    entity_codec::{self as c, Payload as P},
};
use std::collections::{BTreeMap, BTreeSet};

fn part<T: Clone + Eq>(b: Option<&T>, l: &T, s: &T, later_local: bool) -> (T, bool) {
    if l == s {
        return (s.clone(), false);
    }
    if b == Some(l) {
        return (s.clone(), false);
    }
    if b == Some(s) {
        return (l.clone(), false);
    }
    (if later_local { l.clone() } else { s.clone() }, true)
}
fn stamp(p: &P) -> i64 {
    match p {
        P::Note(n) => n.modified,
        P::Card(c) => c.modified,
        P::Deck(d) => d.mtime_secs,
        P::Preset(p) => p.mtime_secs,
        P::Notetype(n) => n.mtime_secs,
        P::Config(c) => c.mtime_secs,
        _ => 0,
    }
}
pub(super) fn stamp_local(p: &mut P, now: i64) {
    match p {
        P::Note(n) => n.modified = now,
        P::Card(c) => c.modified = now,
        P::Deck(d) => d.mtime_secs = now,
        P::Preset(p) => p.mtime_secs = now,
        P::Notetype(n) => n.mtime_secs = now,
        P::Config(c) => c.mtime_secs = now,
        _ => {}
    }
}
fn tags(bytes: &[u8]) -> Result<BTreeMap<String, String>, Error> {
    Ok(std::str::from_utf8(bytes)
        .map_err(|_| Error::InvalidData)?
        .split_whitespace()
        .map(|s| (meguri_text::fold::folded(s), s.to_owned()))
        .collect())
}
fn merge_tags(b: Option<&[u8]>, l: &[u8], s: &[u8], later: bool) -> Result<Vec<u8>, Error> {
    let b = tags(b.unwrap_or_default())?;
    let l = tags(l)?;
    let s = tags(s)?;
    let all: BTreeSet<_> = l.keys().chain(s.keys()).cloned().collect();
    let mut result = Vec::new();
    for k in all {
        if b.contains_key(&k) && (!l.contains_key(&k) || !s.contains_key(&k)) {
            continue;
        }
        let spelling = if later {
            l.get(&k).or(s.get(&k))
        } else {
            s.get(&k).or(l.get(&k))
        };
        if let Some(spelling) = spelling {
            result.push(spelling.clone());
        }
    }
    Ok(if result.is_empty() {
        Vec::new()
    } else {
        format!(" {} ", result.join(" ")).into_bytes()
    })
}

/// Returns the result and the losing whole payload only for a copy-bearing conflict.
pub(super) fn merge(base: Option<&P>, local: &P, server: &P) -> Result<(P, Option<P>), Error> {
    if local == server {
        return Ok((server.clone(), None));
    }
    let ld = local.payload_digest().map_err(|_| Error::InvalidData)?;
    let sd = server.payload_digest().map_err(|_| Error::InvalidData)?;
    let later = (stamp(local), ld) > (stamp(server), sd);
    let mut copy = false;
    macro_rules! pick {
        ($b:expr,$l:expr,$s:expr) => {
            part($b, $l, $s, later).0
        };
    }
    macro_rules! copied {
        ($b:expr,$l:expr,$s:expr) => {{
            let (v, c) = part($b, $l, $s, later);
            copy |= c;
            v
        }};
    }
    let result = match (local, server) {
        (P::Note(l), P::Note(s)) => {
            let b = match base {
                Some(P::Note(b)) => Some(b),
                _ => None,
            };
            if l.mid != s.mid || l.nt_struct_gen != s.nt_struct_gen {
                return Err(Error::InvalidData);
            }
            let lf: Vec<_> = l.flds.split(|c| *c == 31).map(Vec::from).collect();
            let sf: Vec<_> = s.flds.split(|c| *c == 31).map(Vec::from).collect();
            let bf: Option<Vec<_>> = b.map(|b| b.flds.split(|c| *c == 31).map(Vec::from).collect());
            if lf.len() != sf.len() || bf.as_ref().is_some_and(|b| b.len() != lf.len()) {
                return Err(Error::InvalidData);
            }
            let mut n = s.clone();
            n.flds = lf
                .iter()
                .zip(&sf)
                .enumerate()
                .map(|(i, (l, s))| copied!(bf.as_ref().map(|b| &b[i]), l, s))
                .collect::<Vec<_>>()
                .join(&31);
            n.tags = merge_tags(b.map(|b| b.tags.as_slice()), &l.tags, &s.tags, later)?;
            n.guid = pick!(b.map(|b| &b.guid), &l.guid, &s.guid);
            n.flags = pick!(b.map(|b| &b.flags), &l.flags, &s.flags);
            n.data = pick!(b.map(|b| &b.data), &l.data, &s.data);
            P::Note(n)
        }
        (P::Card(l), P::Card(s)) => {
            let b = match base {
                Some(P::Card(b)) => Some(b),
                _ => None,
            };
            if (l.nid, l.ord, l.nt_struct_gen) != (s.nid, s.ord, s.nt_struct_gen) {
                return Err(Error::InvalidData);
            }
            let mut r = s.clone();
            let review = l.last_review_id.cmp(&s.last_review_id);
            macro_rules! schedule {
                ($f:ident) => {
                    r.$f = match review {
                        std::cmp::Ordering::Greater => l.$f.clone(),
                        std::cmp::Ordering::Less => s.$f.clone(),
                        _ => pick!(b.map(|b| &b.$f), &l.$f, &s.$f),
                    };
                };
            }
            schedule!(card_type);
            schedule!(ivl);
            schedule!(factor);
            schedule!(reps);
            schedule!(lapses);
            schedule!(left);
            schedule!(data);
            let home = |c: &c::Card| if c.odid != 0 { c.odue.clone() } else { c.due.clone() };
            let lh = home(l);
            let sh = home(s);
            let bh = b.map(home);
            let home = match review {
                std::cmp::Ordering::Greater => lh,
                std::cmp::Ordering::Less => sh,
                _ => pick!(bh.as_ref(), &lh, &sh),
            };
            let placement = |c: &c::Card| (c.did, c.odid, if c.odid != 0 { Some(c.due.clone()) } else { None });
            let bp = b.map(placement);
            let lp = placement(l);
            let sp = placement(s);
            let (did, odid, filtered) = pick!(bp.as_ref(), &lp, &sp);
            r.did = did;
            r.odid = odid;
            r.due = if odid != 0 {
                filtered.unwrap_or(c::Stored::Integer(0))
            } else {
                home.clone()
            };
            r.odue = if odid != 0 { home } else { c::Stored::Integer(0) };
            let hold = |c: &c::Card| {
                if (-3..=-1).contains(&c.queue) {
                    Some(c.queue)
                } else {
                    None
                }
            };
            let bh = b.map(hold);
            let lh = hold(l);
            let sh = hold(s);
            let held = pick!(bh.as_ref(), &lh, &sh);
            let mut q = match review {
                std::cmp::Ordering::Greater => l.queue,
                std::cmp::Ordering::Less => s.queue,
                _ => pick!(b.map(|b| &b.queue), &l.queue, &s.queue),
            };
            if q < 0 {
                q = match r.card_type {
                    0 => 0,
                    1 => 1,
                    2 => 2,
                    3 => 3,
                    _ => return Err(Error::InvalidData),
                };
            }
            r.queue = held.unwrap_or(q);
            r.flags = pick!(b.map(|b| &b.flags), &l.flags, &s.flags);
            r.last_review_id = l.last_review_id.max(s.last_review_id);
            P::Card(r)
        }
        (P::Revlog(l), P::Revlog(s)) => {
            let b = match base {
                Some(P::Revlog(b)) => Some(b),
                _ => None,
            };
            let mut r = s.clone();
            macro_rules! col {
                ($f:ident) => {
                    r.$f = part(b.map(|b| &b.$f), &l.$f, &s.$f, ld > sd).0
                };
            }
            col!(cid);
            col!(ease);
            col!(ivl);
            col!(last_ivl);
            col!(factor);
            col!(time);
            col!(review_type);
            P::Revlog(r)
        }
        (P::Deck(l), P::Deck(s)) => {
            let b = match base {
                Some(P::Deck(b)) => Some(b),
                _ => None,
            };
            let mut r = s.clone();
            let leaf = |d: &c::Deck| d.name.rsplit(|b| *b == 31).next().unwrap_or_default().to_vec();
            let bl = b.map(leaf);
            let ll = leaf(l);
            let sl = leaf(s);
            r.name = pick!(bl.as_ref(), &ll, &sl);
            r.parent_id = pick!(b.map(|b| &b.parent_id), &l.parent_id, &s.parent_id);
            r.common = copied!(b.map(|b| &b.common), &l.common, &s.common);
            r.kind = copied!(b.map(|b| &b.kind), &l.kind, &s.kind);
            P::Deck(r)
        }
        (P::Preset(l), P::Preset(s)) => {
            let b = match base {
                Some(P::Preset(b)) => Some(b),
                _ => None,
            };
            let mut r = s.clone();
            r.name = pick!(b.map(|b| &b.name), &l.name, &s.name);
            r.config = copied!(b.map(|b| &b.config), &l.config, &s.config);
            P::Preset(r)
        }
        (P::Notetype(l), P::Notetype(s)) => {
            let b = match base { Some(P::Notetype(b)) => Some(b), _ => None };
            if !super::row_payload::same_layout(l, s)?
                || b.is_some_and(|b| !super::row_payload::same_layout(b, s).unwrap_or(false))
            { return Err(Error::InvalidData); }
            let mut r = s.clone();
            r.name = copied!(b.map(|b| &b.name), &l.name, &s.name);
            r.config = copied!(b.map(|b| &b.config), &l.config, &s.config);
            let field_names = |n: &c::Notetype| n.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
            let template_names = |n: &c::Notetype| n.templates.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
            let bf = b.map(field_names);
            let bt = b.map(template_names);
            let names = copied!(bf.as_ref(), &field_names(l), &field_names(s));
            for (i, name) in names.into_iter().enumerate() {
                r.fields[i].name = name;
                r.fields[i].config = copied!(b.map(|b| &b.fields[i].config), &l.fields[i].config, &s.fields[i].config);
            }
            let names = copied!(bt.as_ref(), &template_names(l), &template_names(s));
            for (i, name) in names.into_iter().enumerate() {
                r.templates[i].name = name;
                r.templates[i].config = copied!(b.map(|b| &b.templates[i].config), &l.templates[i].config, &s.templates[i].config);
                // Row timestamps are data. Keep the chosen config's source
                // timestamp, with actual later-version ordering for equal blobs.
                if r.templates[i].config == l.templates[i].config
                    && (r.templates[i].config != s.templates[i].config || later)
                { r.templates[i].mtime_secs = l.templates[i].mtime_secs; }
            }
            P::Notetype(r)
        }
        (P::Tag(l), P::Tag(s)) => {
            let b = match base {
                Some(P::Tag(b)) => Some(b),
                _ => None,
            };
            let mut r = s.clone();
            r.tag = part(b.map(|b| &b.tag), &l.tag, &s.tag, ld > sd).0;
            r.collapsed = part(b.map(|b| &b.collapsed), &l.collapsed, &s.collapsed, ld > sd).0;
            r.config = part(b.map(|b| &b.config), &l.config, &s.config, ld > sd).0;
            P::Tag(r)
        }
        (P::Config(l), P::Config(s)) => {
            let b = match base {
                Some(P::Config(b)) => Some(b),
                _ => None,
            };
            let mut r = s.clone();
            r.val = pick!(b.map(|b| &b.val), &l.val, &s.val);
            if matches!(s.key.as_slice(), b"nextPos" | b"lastUnburied")
                && let (Ok(l), Ok(s)) = (
                    std::str::from_utf8(&l.val).unwrap_or("").parse::<i64>(),
                    std::str::from_utf8(&s.val).unwrap_or("").parse::<i64>(),
                )
            {
                r.val = l.max(s).to_string().into_bytes();
            }
            P::Config(r)
        }
        _ => return Err(Error::InvalidData),
    };
    Ok((
        result,
        if copy {
            Some(if later { server.clone() } else { local.clone() })
        } else {
            None
        },
    ))
}
