//! I1–I2: emit the permissive codec reading, preserving property order.
use super::format::raw_shape;

pub fn element(class: &str, ordinals: &str, text: &str) -> String {
    let mut out = format!("<div class=\"{class}\" data-ordinal=\"{ordinals}\" ");
    if let Some((shape, _)) = raw_shape(text) {
        if matches!(shape.shape.as_str(), "rect" | "ellipse" | "polygon" | "text") {
            attribute(&mut out, "shape", &shape.shape);
        }
        for (key, value) in shape.props {
            if value.is_empty() {
                continue;
            }
            match key.as_str() {
                "left" | "top" | "angle" | "fill" => attribute(&mut out, &key, &value),
                "width" | "height" | "rx" | "ry" | "scale" if value != "0" => attribute(&mut out, &key, &value),
                "oi" => attribute(&mut out, "occludeInactive", &value),
                "fs" => attribute(&mut out, "font-size", &value),
                "text" => attribute(&mut out, "text", &htmlescape::encode_attribute(&value)),
                "points" => {
                    let points = value
                        .split(' ')
                        .filter(|p| p.contains(','))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !points.is_empty() {
                        attribute(&mut out, "points", &points);
                    }
                }
                _ => {}
            }
        }
    }
    out.push_str("></div>");
    out
}
fn attribute(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("data-{key}=\"{value}\" "));
}
