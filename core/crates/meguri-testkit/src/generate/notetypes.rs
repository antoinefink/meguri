//! Note types with Meguri's own names, templates and CSS (R1959), sized as
//! the profile's note types are: field and template counts, template,
//! script and CSS bytes. Every front template shows the first field, which
//! is never empty, so each template of a normal note type makes a card;
//! cloze note types show `{{cloze:Text}}`.

use meguri_model::ids::NotetypeId;
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::notetype::{CardRequirement, FieldConfig, NotetypeConfig, TemplateConfig};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::profile::{NotetypeShape, Profile};
use super::rng::Rng;
use super::{Ids, Timeline, WorstCase};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Cloze,
}

#[derive(Clone, Debug)]
pub struct GenNotetype {
    pub record: Notetype,
    pub kind: Kind,
    /// Index of its shape in the profile's note types.
    pub shape: usize,
}

impl GenNotetype {
    pub fn fields(&self) -> usize {
        self.record.fields.len()
    }

    pub fn templates(&self) -> usize {
        self.record.templates.len()
    }
}

pub fn field_names(kind: Kind, count: usize) -> Vec<String> {
    (0..count)
        .map(|i| match (kind, i) {
            (Kind::Normal, 0) => "Front".to_owned(),
            (Kind::Normal, 1) => "Back".to_owned(),
            (Kind::Cloze, 0) => "Text".to_owned(),
            (Kind::Cloze, 1) => "Extra".to_owned(),
            (_, i) => format!("Field {}", i + 1),
        })
        .collect()
}

const CSS_BASE: &str = ".card {\n  font-family: system-ui, sans-serif;\n  font-size: 22px;\n  line-height: 1.4;\n  text-align: center;\n}\n.mg-rule {\n  border: 0;\n  border-top: 1px solid #8884;\n}\n";

/// Meguri's base front and back for template `ord`.
pub fn base_templates(kind: Kind, fields: &[String], ord: usize) -> (String, String) {
    match kind {
        Kind::Normal => {
            let back = fields.get(1).unwrap_or(&fields[0]);
            (
                format!(
                    "<div class=\"mg-front\">{{{{{}}}}}</div>\n<div class=\"mg-label\">side {}</div>",
                    fields[0],
                    ord + 1
                ),
                format!("{{{{FrontSide}}}}\n<hr class=\"mg-rule\">\n<div class=\"mg-back\">{{{{{back}}}}}</div>"),
            )
        }
        Kind::Cloze => {
            let mut back = format!("<div class=\"mg-cloze\">{{{{cloze:{}}}}}</div>", fields[0]);
            if let Some(extra) = fields.get(1) {
                back.push_str(&format!("\n<div class=\"mg-extra\">{{{{{extra}}}}}</div>"));
            }
            (
                format!("<div class=\"mg-cloze\">{{{{cloze:{}}}}}</div>", fields[0]),
                back,
            )
        }
    }
}

/// `text` grown to `target` bytes: a script element of `script` bytes of
/// body, then an HTML comment of filler.
pub fn sized(mut text: String, target: u64, script: u64) -> String {
    if script > 0 {
        // The script's body is `PRE`, the filler and `POST`.
        const PRE: &str = "/* meguri synthetic */ var mgFill = \"";
        const POST: &str = "\";";
        let fill = script.saturating_sub((PRE.len() + POST.len()) as u64);
        text.push_str("\n<script>");
        text.push_str(PRE);
        text.push_str(&filler_ascii(fill as usize));
        text.push_str(POST);
        text.push_str("</script>");
    }
    let have = text.len() as u64;
    let frame = "\n<!-- mg-fill  -->".len() as u64;
    if target > have + frame {
        text.push_str("\n<!-- mg-fill ");
        text.push_str(&filler_ascii((target - have - frame) as usize));
        text.push_str(" -->");
    }
    text
}

/// `len` bytes of letters in a fixed pattern.
pub fn filler_ascii(len: usize) -> String {
    const PATTERN: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    (0..len).map(|i| char::from(PATTERN[(i * 7 + i / 26) % 26])).collect()
}

fn css(target: u64) -> String {
    let mut css = CSS_BASE.to_owned();
    let mut n = 0;
    while (css.len() as u64) < target {
        css.push_str(&format!(".mg-fill-{n} {{ margin: 0; }}\n"));
        n += 1;
    }
    css
}

/// One note type for a profile shape.
pub fn notetype(
    id: NotetypeId,
    name: &str,
    shape: &NotetypeShape,
    template_override: Option<u64>,
    timeline: &Timeline,
    rng: &mut Rng,
) -> Notetype {
    let kind = if shape.kind == "cloze" {
        Kind::Cloze
    } else {
        Kind::Normal
    };
    let names = field_names(kind, shape.fields as usize);
    // A cloze note type has one template.
    let template_count = if kind == Kind::Cloze {
        1
    } else {
        shape.templates as usize
    };
    let per_side_script = shape.script_bytes / (2 * template_count as u64).max(1);
    let fields = names
        .iter()
        .enumerate()
        .map(|(ord, name)| {
            let mut config = FieldConfig::default();
            config.font_name = "Arial".to_owned();
            config.font_size = 20;
            config.id = Some(rng.u64() as i64);
            NotetypeField {
                ord: ord as u32,
                name: name.clone(),
                config: ProtoBlob::new(config),
                prior_ord: None,
            }
        })
        .collect();
    let templates = (0..template_count)
        .map(|ord| {
            let (front, back) = base_templates(kind, &names, ord);
            let [q_bytes, a_bytes] = shape.template_bytes.get(ord).copied().unwrap_or([0, 0]);
            let q_bytes = if ord == 0 {
                template_override.unwrap_or(q_bytes)
            } else {
                q_bytes
            };
            let mut config = TemplateConfig::default();
            config.q_format = sized(front, q_bytes, per_side_script);
            config.a_format = sized(back, a_bytes, per_side_script);
            config.id = Some(rng.u64() as i64);
            NotetypeTemplate {
                ord: ord as u32,
                name: format!("Side {}", ord + 1),
                mtime: EpochSecs(0),
                usn: Usn(0),
                config: ProtoBlob::new(config),
                prior_ord: None,
            }
        })
        .collect();
    // F6.5: sort field 0, no LaTeX, `original_stock_kind` 0 whatever the
    // profile's, and the requirements Anki computes on every save of these
    // templates: each front needs the first field (kind 1, any).
    let mut config = NotetypeConfig::default();
    config.kind = if kind == Kind::Cloze { 1 } else { 0 };
    config.css = css(shape.css_bytes);
    config.reqs = (0..template_count as u32)
        .map(|ord| {
            let mut req = CardRequirement::default();
            req.card_ord = ord;
            req.kind = 1;
            req.field_ords = vec![0];
            req
        })
        .collect();
    Notetype {
        id,
        name: name.to_owned(),
        mtime: EpochSecs(timeline.crt),
        usn: Usn::PENDING,
        config: ProtoBlob::new(config),
        fields,
        templates,
    }
}

/// One note type per profile shape, named `Meguri Synthetic <n>`. The
/// template worst case puts its size on the first note type's first front.
pub fn build(
    profile: &Profile,
    worst: Option<&WorstCase>,
    timeline: &Timeline,
    ids: &mut Ids,
    rng: &mut Rng,
) -> Vec<GenNotetype> {
    profile
        .notetypes
        .iter()
        .enumerate()
        .map(|(i, shape)| {
            let id = NotetypeId(ids.claim(timeline.crt_ms() + i as i64));
            let template_override = if i == 0 {
                worst.and_then(|w| w.template_bytes)
            } else {
                None
            };
            let record = notetype(
                id,
                &format!("Meguri Synthetic {}", i + 1),
                shape,
                template_override,
                timeline,
                rng,
            );
            GenNotetype {
                kind: if shape.kind == "cloze" {
                    Kind::Cloze
                } else {
                    Kind::Normal
                },
                record,
                shape: i,
            }
        })
        .collect()
}
