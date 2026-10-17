//! Image-occlusion storage, without rendering or editor UI.
//!
//! Behavior: `specs/render/image-occlusion-format.md` F1–F12. Use
//! [`parse_raw`] for compatibility and [`parse`] before editing. Preserve
//! the returned [`Document`] and each shape's source across edits.
mod format;
pub mod ordinals;
mod shapes;

pub use format::{
    RawField, RawNumber, RawShape, format_number, parse, parse_number, parse_raw, read_angle, write, write_angle,
    write_shape,
};
pub use shapes::{Shape, ShapeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    ExtraContent,
    MultipleNumbers,
    NestedCloze,
    MalformedShape,
    UnknownShape,
    DuplicateKey,
    InvalidValue,
    MissingKey,
    InvalidSize,
    UnwritableShape,
}

impl ErrorKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::ExtraContent => "extra_content",
            Self::MultipleNumbers => "multiple_numbers",
            Self::NestedCloze => "nested_cloze",
            Self::MalformedShape => "malformed_shape",
            Self::UnknownShape => "unknown_shape",
            Self::DuplicateKey => "duplicate_key",
            Self::InvalidValue => "invalid_value",
            Self::MissingKey => "missing_key",
            Self::InvalidSize => "invalid_size",
            Self::UnwritableShape => "unwritable_shape",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// IO cloze index for reading; input shape index for writing.
    pub shape: Option<usize>,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.kind.name())?;
        if let Some(index) = self.shape {
            write!(f, ": shape {index}")?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug)]
pub struct Document {
    pub shapes: Vec<Shape>,
    pub hide_all: bool,
    original: String,
    original_shapes: Vec<Shape>,
    original_mode: bool,
}
impl Document {
    /// F11: compare against the original reading, even if this document's
    /// public editor state has since changed.
    pub fn write(&self, shapes: &[Shape], hide_all: bool, width: f64, height: f64) -> Result<String, Error> {
        format::check_size(width, height)?;
        if hide_all == self.original_mode && shapes == self.original_shapes {
            return Ok(self.original.clone());
        }
        write(shapes, hide_all, width, height)
    }
}
