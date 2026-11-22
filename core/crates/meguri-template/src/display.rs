//! Display-only HTML (render/display-transform). Comparable card output
//! remains unchanged; inserted TTS text and replay markup are never rescanned.
use crate::{
    av_tags::{AvSide, AvTag},
    card::{BlankKind, CardRender, Outcome},
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Platform {
    #[default]
    Mac,
    Ios,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderHost {
    pub platform: Platform,
    pub hide_play_buttons: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplaySides {
    pub question: String,
    pub answer: String,
    /// Positive transform ownership, separate from authored HTML attributes.
    pub question_replay_ids: Vec<String>,
    pub answer_replay_ids: Vec<String>,
}

fn button(command: &str, platform: Platform, id: &str) -> String {
    // Original Meguri graphic, compact as D4 requires. The command
    // comes only from a q/a marker followed by ASCII digits, never HTML.
    let action = match platform {
        Platform::Mac => format!("pycmd('{command}')"),
        Platform::Ios => format!("window.webkit.messageHandlers.meguri.postMessage('{command}')"),
    };
    format!(
        r##"<a class="replay-button soundLink" id="{id}" data-meguri-replay="true" aria-label="Play audio" href=# onclick="{action}; return false;" draggable="false"><svg class="playImage" aria-hidden="true" focusable="false" viewBox="0 0 48 48" version="1.1"><rect x="3" y="3" width="42" height="42" rx="12" fill="none" stroke="currentColor" stroke-width="2" /><path d="M18 14L34 24L18 34Z" fill="currentColor" /></svg></a>"##
    )
}

pub fn markers(text: &str, question: &[AvTag], answer: &[AvTag], host: RenderHost) -> String {
    markers_for_document(text, question, answer, host, 'q').0
}

fn markers_for_document(
    text: &str,
    question: &[AvTag],
    answer: &[AvTag],
    host: RenderHost,
    side: char,
) -> (String, Vec<String>) {
    // Resolve TTS first for reservation only: its authored markup can also
    // occupy IDs. The output pass still substitutes each original marker once.
    let mut has_replay = false;
    let authored = replace_markers(text, question, answer, |_| {
        has_replay = true;
        String::new()
    });
    if host.hide_play_buttons || !has_replay {
        return (authored, Vec::new());
    }
    let mut occupied = meguri_text::description::element_ids(&authored);
    let mut ordinal = 0;
    let mut ids = Vec::new();
    let html = replace_markers(text, question, answer, |command| {
        let id = loop {
            let candidate = format!("review.card.replay.{side}{ordinal}");
            ordinal += 1;
            if occupied.insert(candidate.clone()) {
                break candidate;
            }
        };
        let html = button(command, host.platform, &id);
        ids.push(id);
        html
    });
    (html, ids)
}

fn replace_markers(text: &str, question: &[AvTag], answer: &[AvTag], mut replay: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(relative) = text[at..].find('[') {
        let start = at + relative;
        out.push_str(&text[at..start]);
        let tail = &text[start..];
        let bytes = tail.as_bytes();
        if tail.starts_with("[anki:play:") && matches!(bytes.get(11), Some(b'q' | b'a')) && bytes.get(12) == Some(&b':')
        {
            let mut end = 13;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end > 13 && bytes.get(end) == Some(&b']') {
                let entries = if bytes[11] == b'q' { question } else { answer };
                let entry = tail[13..end].parse::<usize>().ok().and_then(|index| entries.get(index));
                if let Some(AvTag::Tts { content, .. }) = entry {
                    out.push_str(&crate::media_display::encode_for_display(content));
                } else {
                    out.push_str(&replay(&tail[6..end]));
                }
                at = start + end + 1;
                continue;
            }
        }
        out.push('[');
        at = start + 1;
    }
    out.push_str(&text[at..]);
    out
}
fn wrap(css: &str, text: &str) -> String {
    format!("<style>{css}</style>{text}")
}
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}
fn blank_notice(kind: BlankKind, ord: u32) -> String {
    let (message, anchor) = match kind {
        BlankKind::EmptyFront => ("This card’s front shows no field content.".into(), "front-of-card-is-blank"),
        BlankKind::MissingCloze => (
            format!("This card’s note has no cloze {}.", u64::from(ord) + 1),
            "no-cloze-filter-on-cloze-note-type",
        ),
    };
    format!(
        "<div>{message}<br><a href='https://docs.ankiweb.net/templates/errors.html#{anchor}'>More information</a></div>"
    )
}
/// Review/preview composition only. Browser consumers use comparable raw
/// texts and metadata directly and never add notices or this transform.
pub fn compose(card: &CardRender, css: &str, ord: u32, host: RenderHost) -> DisplaySides {
    if let Outcome::Error { side, error } = &card.outcome {
        let header = match side {
            AvSide::Question => "The front template has an error:",
            AvSide::Answer => "The back template has an error:",
        };
        let notice = format!(
            "{header}<br>{}<br><a href='https://docs.ankiweb.net/templates/errors.html#template-syntax-error'>More information</a>",
            escape(&error.to_string())
        );
        let html = wrap("", &notice);
        return DisplaySides {
            question: html.clone(),
            answer: html,
            ..DisplaySides::default()
        };
    }
    let transform = |text: &str, side| {
        let whole = wrap(css, text);
        markers_for_document(
            &crate::media_display::encode_for_display(&whole),
            &card.question_av,
            &card.answer_av,
            host,
            side,
        )
    };
    let (mut question, question_replay_ids) = transform(card.question.as_deref().unwrap_or_default(), 'q');
    let (answer, answer_replay_ids) = if let Outcome::Blank(kind) = card.outcome {
        let notice = blank_notice(kind, ord);
        question.push_str(&notice);
        transform(&notice, 'a')
    } else {
        transform(card.answer.as_deref().unwrap_or_default(), 'a')
    };
    DisplaySides {
        question,
        answer,
        question_replay_ids,
        answer_replay_ids,
    }
}
