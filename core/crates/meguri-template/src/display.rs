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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplaySides {
    pub question: String,
    pub answer: String,
}

fn button(command: &str, platform: Platform) -> String {
    // Original Meguri graphic, compact as D4 requires. The command
    // comes only from a q/a marker followed by ASCII digits, never HTML.
    let action = match platform {
        Platform::Mac => format!("pycmd('{command}')"),
        Platform::Ios => format!("window.webkit.messageHandlers.meguri.postMessage('{command}')"),
    };
    format!(
        r##"<a class="replay-button soundLink" href=# onclick="{action}; return false;" draggable="false"><svg class="playImage" viewBox="0 0 48 48" version="1.1"><rect x="3" y="3" width="42" height="42" rx="12" fill="none" stroke="currentColor" stroke-width="2" /><path d="M18 14L34 24L18 34Z" fill="currentColor" /></svg></a>"##
    )
}

pub fn markers(text: &str, question: &[AvTag], answer: &[AvTag], host: RenderHost) -> String {
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
                } else if !host.hide_play_buttons {
                    out.push_str(&button(&tail[6..end], host.platform));
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
        BlankKind::EmptyFront => ("The front of this card is blank.".into(), "front-of-card-is-blank"),
        BlankKind::MissingCloze => (
            format!(
                "No cloze {} found on card.\nPlease either add a cloze deletion, or use the Empty Cards tool.",
                u64::from(ord) + 1
            ),
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
        };
    }
    let transform = |text: &str| {
        let whole = wrap(css, text);
        markers(
            &crate::media_display::encode_for_display(&whole),
            &card.question_av,
            &card.answer_av,
            host,
        )
    };
    let mut question = transform(card.question.as_deref().unwrap_or_default());
    let answer = if let Outcome::Blank(kind) = card.outcome {
        let notice = blank_notice(kind, ord);
        question.push_str(&notice);
        transform(&notice)
    } else {
        transform(card.answer.as_deref().unwrap_or_default())
    };
    DisplaySides { question, answer }
}
