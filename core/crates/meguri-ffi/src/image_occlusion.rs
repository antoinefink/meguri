//! U63's pure editor services (render/image-occlusion-format F13).
//! Source indexes refer to the immutable document returned by parse.
use crate::{CoreError, callback};
use meguri_engine::image_occlusion as io;
use meguri_model::notetype::NotetypeField;
use meguri_model::proto::{ProtoBlob, notetype::FieldConfig};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct IoFieldRoles {
    pub occlusions: u32,
    pub image: u32,
    pub header: u32,
    pub back_extra: u32,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct IoPoint {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct IoProperty {
    pub key: String,
    pub value: String,
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum IoShapeKind {
    Rect { width: f64, height: f64 },
    Ellipse { rx: f64, ry: f64 },
    Polygon { points: Vec<IoPoint> },
    Text { text: String, scale: f64, fs: Option<f64> },
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct IoShape {
    pub number: u16,
    pub left: f64,
    pub top: f64,
    pub angle: f64,
    pub fill: Option<String>,
    pub kind: IoShapeKind,
    pub unknown_properties: Vec<IoProperty>,
    /// None for new shapes. Preserved across editing, reordering and deletion.
    pub source_index: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct IoGroup {
    pub number: u16,
    pub members: Vec<u32>,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct IoReading {
    pub shapes: Vec<IoShape>,
    pub hide_all: bool,
    pub groups: Vec<IoGroup>,
}

#[derive(Debug, uniffi::Object)]
pub struct ImageOcclusionDocument {
    document: io::Document,
}

fn invalid(message: impl ToString) -> CoreError {
    CoreError::InvalidArgument {
        message: message.to_string(),
    }
}

#[uniffi::export]
pub fn image_occlusion_detect(
    original_stock_kind: i32,
    tags: Vec<Option<u32>>,
) -> Result<Option<IoFieldRoles>, CoreError> {
    callback::enter("image_occlusion_detect");
    if original_stock_kind != 6 {
        return Ok(None);
    }
    let fields: Vec<_> = tags
        .into_iter()
        .enumerate()
        .map(|(i, tag)| NotetypeField {
            ord: i as u32,
            name: String::new(),
            prior_ord: None,
            config: {
                let mut config = FieldConfig::default();
                config.tag = tag;
                ProtoBlob::new(config)
            },
        })
        .collect();
    Ok(io::field_roles(&fields).map(|r| IoFieldRoles {
        occlusions: r.occlusions as u32,
        image: r.image as u32,
        header: r.header as u32,
        back_extra: r.back_extra as u32,
    }))
}

#[uniffi::export]
pub fn image_occlusion_parse(field: String) -> Result<Arc<ImageOcclusionDocument>, CoreError> {
    callback::enter("image_occlusion_parse");
    Ok(Arc::new(ImageOcclusionDocument {
        document: io::parse(&field).map_err(invalid)?,
    }))
}

#[uniffi::export]
impl ImageOcclusionDocument {
    pub fn reading(&self) -> Result<IoReading, CoreError> {
        callback::enter("image_occlusion_reading");
        Ok(IoReading {
            shapes: self
                .document
                .shapes
                .iter()
                .enumerate()
                .map(|(i, s)| from_shape(s, Some(i as u32)))
                .collect(),
            hide_all: self.document.hide_all,
            groups: io::ordinals::groups(&self.document.shapes)
                .into_iter()
                .map(|(number, members)| IoGroup {
                    number,
                    members: members.into_iter().map(|i| i as u32).collect(),
                })
                .collect(),
        })
    }
}

/// Pass the original document to preserve unchanged bytes, including field
/// separators. None writes new shapes using F10/F11.
#[uniffi::export]
pub fn image_occlusion_write(
    original: Option<Arc<ImageOcclusionDocument>>,
    shapes: Vec<IoShape>,
    hide_all: bool,
    width: f64,
    height: f64,
) -> Result<String, CoreError> {
    callback::enter("image_occlusion_write");
    let values = shapes
        .iter()
        .map(|s| {
            let value = to_shape(s);
            match (s.source_index, original.as_ref()) {
                (Some(i), Some(doc)) => doc
                    .document
                    .shapes
                    .get(i as usize)
                    .map(|source| value.with_source_from(source))
                    .ok_or_else(|| invalid("invalid source_index")),
                (Some(_), None) => Err(invalid("source_index needs original document")),
                (None, _) => Ok(value),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    match original {
        Some(doc) => doc.document.write(&values, hide_all, width, height),
        None => io::write(&values, hide_all, width, height),
    }
    .map_err(invalid)
}

#[uniffi::export]
pub fn image_occlusion_allocate(shapes: Vec<IoShape>) -> Result<u16, CoreError> {
    callback::enter("image_occlusion_allocate");
    io::ordinals::allocate(&shapes.iter().map(to_shape).collect::<Vec<_>>())
        .ok_or_else(|| invalid("no unused cloze number"))
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum IoNumberAction {
    Group { selected: Vec<u32> },
    Ungroup { number: u16 },
    Duplicate { selected: Vec<u32> },
}

/// Group/ungroup return the entire document; duplicate returns its new
/// copies in editor order, ready for the caller to insert and record in undo.
#[uniffi::export]
pub fn image_occlusion_numbers(shapes: Vec<IoShape>, action: IoNumberAction) -> Result<Vec<IoShape>, CoreError> {
    callback::enter("image_occlusion_numbers");
    let mut values: Vec<_> = shapes.iter().map(to_shape).collect();
    match action {
        IoNumberAction::Group { selected } => io::ordinals::group(
            &mut values,
            &selected.into_iter().map(|i| i as usize).collect::<Vec<_>>(),
        ),
        IoNumberAction::Ungroup { number } => {
            io::ordinals::ungroup(&mut values, number).ok_or_else(|| invalid("no unused cloze number"))?;
        }
        IoNumberAction::Duplicate { selected } => {
            return io::ordinals::duplicate(&values, &selected.into_iter().map(|i| i as usize).collect::<Vec<_>>())
                .map(|copies| copies.iter().map(|s| from_shape(s, None)).collect())
                .ok_or_else(|| invalid("no unused cloze number"));
        }
    }
    Ok(values
        .iter()
        .zip(shapes)
        .map(|(s, original)| from_shape(s, original.source_index))
        .collect())
}

fn to_shape(shape: &IoShape) -> io::Shape {
    let kind = match &shape.kind {
        IoShapeKind::Rect { width, height } => io::ShapeKind::Rect {
            width: *width,
            height: *height,
        },
        IoShapeKind::Ellipse { rx, ry } => io::ShapeKind::Ellipse { rx: *rx, ry: *ry },
        IoShapeKind::Polygon { points } => io::ShapeKind::Polygon {
            points: points.iter().map(|p| (p.x, p.y)).collect(),
        },
        IoShapeKind::Text { text, scale, fs } => io::ShapeKind::Text {
            text: text.clone(),
            scale: *scale,
            fs: *fs,
        },
    };
    let mut value = io::Shape::new(shape.number, shape.left, shape.top, kind);
    value.angle = shape.angle;
    value.fill = shape.fill.clone();
    value.unknown_props = shape
        .unknown_properties
        .iter()
        .map(|p| (p.key.clone(), p.value.clone()))
        .collect();
    value
}

fn from_shape(shape: &io::Shape, source_index: Option<u32>) -> IoShape {
    IoShape {
        number: shape.number,
        left: shape.left,
        top: shape.top,
        angle: shape.angle,
        fill: shape.fill.clone(),
        source_index,
        kind: match &shape.kind {
            io::ShapeKind::Rect { width, height } => IoShapeKind::Rect {
                width: *width,
                height: *height,
            },
            io::ShapeKind::Ellipse { rx, ry } => IoShapeKind::Ellipse { rx: *rx, ry: *ry },
            io::ShapeKind::Polygon { points } => IoShapeKind::Polygon {
                points: points.iter().map(|&(x, y)| IoPoint { x, y }).collect(),
            },
            io::ShapeKind::Text { text, scale, fs } => IoShapeKind::Text {
                text: text.clone(),
                scale: *scale,
                fs: *fs,
            },
        },
        unknown_properties: shape
            .unknown_props
            .iter()
            .map(|(key, value)| IoProperty {
                key: key.clone(),
                value: value.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ffi_preserves_source_and_exposes_errors_and_number_rules() {
        let field = " {{c2::image-occlusion:rect:left=0.3:top=0.1:width=0.2:height=0.08:oi=1}}<BR/>";
        let doc = image_occlusion_parse(field.into()).unwrap();
        let reading = doc.reading().unwrap();
        assert!(reading.hide_all);
        assert_eq!(
            image_occlusion_write(Some(doc.clone()), reading.shapes.clone(), true, 1000., 1000.).unwrap(),
            field
        );
        assert_eq!(image_occlusion_allocate(reading.shapes.clone()).unwrap(), 1);
        let copies =
            image_occlusion_numbers(reading.shapes.clone(), IoNumberAction::Duplicate { selected: vec![0] }).unwrap();
        assert_eq!(copies[0].number, 1);
        assert_eq!(copies[0].source_index, None);
        let mut moved = reading.shapes;
        moved[0].left = 0.4;
        assert_eq!(
            image_occlusion_write(Some(doc), moved, true, 1000., 1000.).unwrap(),
            "{{c2::image-occlusion:rect:left=.4:top=.1:width=.2:height=.08:oi=1}}<br>"
        );
        assert!(
            image_occlusion_parse("{{c1::image-occlusion:rect:left=abc}}".into())
                .unwrap_err()
                .to_string()
                .contains("invalid_value: shape 0")
        );
        assert_eq!(
            image_occlusion_detect(6, vec![Some(3), Some(2), Some(1), Some(0)])
                .unwrap()
                .unwrap()
                .occlusions,
            3
        );
        assert_eq!(image_occlusion_detect(5, vec![None; 4]).unwrap(), None);
    }
}
