//! Typed editor shapes (render/image-occlusion-format F6, F7).

#[derive(Clone, Debug, PartialEq)]
pub enum ShapeKind {
    Rect { width: f64, height: f64 },
    Ellipse { rx: f64, ry: f64 },
    Polygon { points: Vec<(f64, f64)> },
    Text { text: String, scale: f64, fs: Option<f64> },
}

impl ShapeKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Rect { .. } => "rect",
            Self::Ellipse { .. } => "ellipse",
            Self::Polygon { .. } => "polygon",
            Self::Text { .. } => "text",
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub number: u16,
    pub left: f64,
    pub top: f64,
    /// Degrees, with no normalization applied during editing.
    pub angle: f64,
    pub fill: Option<String>,
    pub kind: ShapeKind,
    pub unknown_props: Vec<(String, String)>,
    pub(crate) source: Option<Box<Source>>,
}

#[derive(Clone, Debug)]
pub(crate) struct Source {
    pub text: String,
    pub shape: Shape,
    pub hide_all: bool,
}

// Source spelling is deliberately excluded (F11.3).
impl PartialEq for Shape {
    fn eq(&self, other: &Self) -> bool {
        self.number == other.number
            && self.left == other.left
            && self.top == other.top
            && self.angle == other.angle
            && self.fill == other.fill
            && self.kind == other.kind
            && self.unknown_props == other.unknown_props
    }
}

impl Shape {
    pub fn new(number: u16, left: f64, top: f64, kind: ShapeKind) -> Self {
        Self {
            number,
            left,
            top,
            angle: 0.,
            fill: None,
            kind,
            unknown_props: Vec::new(),
            source: None,
        }
    }

    /// Exact cloze text, available only on a shape read from a field.
    pub fn source(&self) -> Option<&str> {
        self.source.as_ref().map(|s| s.text.as_str())
    }

    pub(crate) fn retain_source(&mut self, text: &str, hide_all: bool) {
        self.source = Some(Box::new(Source {
            text: text.to_owned(),
            shape: self.clone(),
            hide_all,
        }));
    }

    /// Give an edited value the provenance of a shape from this document.
    /// The writer checks equality before it can reuse that source.
    pub fn with_source_from(mut self, original: &Self) -> Self {
        self.source = original.source.clone();
        self
    }
}
