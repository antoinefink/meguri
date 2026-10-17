//! Expected strings and errors come from render/image-occlusion-format F7–F12.
use meguri_template::image_occlusion::{self as io, Shape, ShapeKind};

fn rect(number: u16) -> Shape {
    Shape::new(
        number,
        0.2325,
        0.3261,
        ShapeKind::Rect {
            width: 0.202,
            height: 0.0975,
        },
    )
}

#[test]
fn numbers_use_exact_binary_values_and_half_up_rounding_f8() {
    for (value, expected) in [
        (0., ".0000"),
        (-0., ".0000"),
        (0.2081, ".2081"),
        (0.20814, ".2081"),
        (0.20815, ".2082"),
        (12.34565, "12.3456"),
        (0.03125, ".0313"),
        (0.96875, ".9688"),
        (0.5, ".5"),
        (0.202, ".202"),
        (1., "1."),
        (0.99995, "1."),
        (10., "10."),
        (1.5, "1.5"),
        (0.00004, "."),
        (1e-14, "."),
        (0.00005, ".0001"),
        (-0.25, "-0.25"),
        (-1., "-1."),
        (-0.00001, "-0."),
        (-1e-14, "-0."),
    ] {
        assert_eq!(io::format_number(value).unwrap(), expected, "{value:?}");
    }
}

#[test]
fn canonical_rect_and_angle_f10() {
    let mut shape = rect(1);
    let expected = "{{c1::image-occlusion:rect:left=.2325:top=.3261:width=.202:height=.0975:oi=1}}";
    assert_eq!(io::write_shape(&shape, true).unwrap(), expected);
    shape.angle = 90.;
    assert_eq!(
        io::write_shape(&shape, true).unwrap(),
        "{{c1::image-occlusion:rect:left=.2325:top=.3261:angle=2500:width=.202:height=.0975:oi=1}}"
    );
}

#[test]
fn angle_table_and_integer_roundtrips_f9() {
    for (d, s) in [
        (0., None),
        (360., None),
        (720., None),
        (0.01, None),
        (-0.01, None),
        (90., Some(2500)),
        (450., Some(2500)),
        (45., Some(1250)),
        (30., Some(833)),
        (1., Some(28)),
        (12.5, Some(347)),
        (180., Some(5000)),
        (359.99, Some(10000)),
        (-90., Some(-2500)),
        (179.982, Some(5000)),
        (-179.982, Some(-4999)),
        (539.982, Some(4999)),
    ] {
        assert_eq!(io::write_angle(d).unwrap(), s, "{d}");
    }
    for n in -9999..=9999 {
        assert_eq!(
            io::write_angle(io::read_angle(f64::from(n))).unwrap(),
            (n != 0).then_some(n)
        );
    }
}

#[test]
fn strict_checks_in_spec_order_f7() {
    use io::ErrorKind::*;
    for (field, kind, index) in [
        ("{{c1::image-occlusion:rect:left=abc}}", InvalidValue, Some(0)),
        (
            "{{c1::image-occlusion:rect:left=.1:top=.1:width=.1}}",
            MissingKey,
            Some(0),
        ),
        (
            "{{c1::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1}}<br>{{c2::image-occlusion:rect}}",
            MalformedShape,
            Some(1),
        ),
        ("{{c1::image-occlusion:star:left=.1:top=.1}}", UnknownShape, Some(0)),
        (
            "{{c1,2::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1}}",
            MultipleNumbers,
            Some(0),
        ),
        (
            "{{c1::image-occlusion:rect:left=.1:left=.2:top=.1:width=.1:height=.1}}",
            DuplicateKey,
            Some(0),
        ),
        (
            "{{c1::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1{{c2::x}}}}",
            NestedCloze,
            Some(0),
        ),
        ("{{c1::x}}", ExtraContent, None),
        ("{{c1::image-occlusion:rect:left=abc}}junk", ExtraContent, None),
        ("{{c3::image-occlusion:rect:left=.1", ExtraContent, None),
        ("{{c1::image-occlusion:star:a=1\\x}}", MalformedShape, Some(0)),
        ("{{c1::image-occlusion:rect:left=.1:left=abc}}", DuplicateKey, Some(0)),
    ] {
        let error = io::parse(field).unwrap_err();
        assert_eq!((error.kind, error.shape), (kind, index), "{field}");
    }
    for field in ["", " \t\n\r\x0c<br><BR/><br />"] {
        let doc = io::parse(field).unwrap();
        assert!(doc.shapes.is_empty());
        assert!(!doc.hide_all);
    }
}

#[test]
fn decimal_grammar_points_and_equal_values_f7() {
    for text in [
        ".2081", "0.2081", "+.2081", "1.", "1", "1.0000", "-0.25", ".", "-.", "+.", "-0.", ".0000", "1e-3", ".5E2",
    ] {
        assert!(io::parse_number(text).is_some(), "{text}");
    }
    for text in [
        "abc", "", " 1", "1 ", "1,5", "0x10", "inf", "Infinity", "NaN", "1e", ".e5", "e5", "1e400", "--1", "1_0",
    ] {
        assert!(io::parse_number(text).is_none(), "{text}");
    }
    let a = io::parse("{{c1::image-occlusion:rect:left=0.2081:top=.3:width=.1:height=.1}}").unwrap();
    let b = io::parse("{{c1::image-occlusion:rect:left=.2081:top=.3:width=.1:height=.1}}").unwrap();
    assert_eq!(a.shapes, b.shapes);
    for points in ["", " .1,.2", ".1,.2 ", ".1,.2  .3,.4", ".1", ".1,.2,.3"] {
        let field = format!("{{{{c1::image-occlusion:polygon:left=0:top=0:points={points}}}}}");
        assert_eq!(io::parse(&field).unwrap_err().kind, io::ErrorKind::InvalidValue);
    }
}

#[test]
fn shape_writer_table_escaping_defaults_and_unwritable_f10() {
    let mut label = Shape::new(
        3,
        0.1,
        0.1,
        ShapeKind::Text {
            text: "a:b\\c".into(),
            scale: 1.,
            fs: Some(0.0666667),
        },
    );
    let text = io::write_shape(&label, true).unwrap();
    assert_eq!(
        text,
        "{{c0::image-occlusion:text:left=.1:top=.1:text=a\\:b\\\\c:scale=1.:fs=.0667:oi=1}}"
    );
    let doc = io::parse(&text).unwrap();
    assert!(matches!(&doc.shapes[0].kind,ShapeKind::Text{text,..} if text=="a:b\\c"));
    label.kind = ShapeKind::Text {
        text: "a}}b".into(),
        scale: 1.,
        fs: None,
    };
    assert_eq!(
        io::write_shape(&label, false).unwrap_err().kind,
        io::ErrorKind::UnwritableShape
    );
    label.kind = ShapeKind::Text {
        text: "{{c2::x}}".into(),
        scale: 1.,
        fs: None,
    };
    assert!(io::write_shape(&label, false).is_err());
    let mut r = rect(1);
    r.fill = Some("#ffeba2".into());
    assert!(!io::write_shape(&r, false).unwrap().contains("fill="));
    r.fill = Some("#FFEBA2".into());
    assert!(io::write_shape(&r, false).unwrap().contains("fill=#FFEBA2"));
    r.unknown_props = vec![("zz".into(), "q:r".into()), ("empty".into(), "".into())];
    assert!(io::write_shape(&r, false).unwrap().contains(":zz=q\\:r}}"));
    r.unknown_props = vec![("zz".into(), "x}".into())];
    assert!(io::write_shape(&r, false).is_err());
    r.left = f64::NAN;
    assert!(io::write_shape(&r, false).is_err());
}

const SEVEN: &str = concat!(
    "{{c1::image-occlusion:rect:left=.05:top=.1:width=.2:height=.08:oi=1}}<br>\n",
    "{{c2::image-occlusion:rect:left=0.3:top=0.1:fill=#ffeba2:width=0.2:height=0.08:oi=1}}<br>\n",
    "{{c3::image-occlusion:ellipse:left=.55:top=.1:rx=.1:ry=.04:oi=1}}<br>\n",
    "{{c4::image-occlusion:rect:left=.05:top=.3:width=.2:height=.08:oi=1}}<br>\n",
    "{{c5::image-occlusion:polygon:left=.3:top=.3:points=.3,.3 .5,.32 .4,.4:oi=1}}<br>\n",
    "{{c6::image-occlusion:rect:left=.55:top=.3:width=.2:height=.08:oi=1}}<br>\n",
    "{{c7::image-occlusion:rect:left=.05:top=.5:angle=833:width=.2:height=.08:fill=#cc3344:oi=1}}<br>\n"
);

#[test]
fn ae38_preserves_whole_field_and_untouched_masks_f11() {
    let doc = io::parse(SEVEN).unwrap();
    assert_eq!(doc.write(&doc.shapes, true, 100., 100.).unwrap(), SEVEN);
    let mut edited = doc.shapes.clone();
    edited[3].left = 0.0625;
    edited[3].top = 0.35;
    edited.remove(5);
    let expected = SEVEN
        .replace("<br>\n", "<br>")
        .replace(
            "{{c4::image-occlusion:rect:left=.05:top=.3:width=.2:height=.08:oi=1}}",
            "{{c4::image-occlusion:rect:left=.0625:top=.35:width=.2:height=.08:oi=1}}",
        )
        .replace(
            "{{c6::image-occlusion:rect:left=.55:top=.3:width=.2:height=.08:oi=1}}<br>",
            "",
        );
    assert_eq!(doc.write(&edited, true, 100., 100.).unwrap(), expected);
    let switched = doc.write(&doc.shapes, false, 1000., 1000.).unwrap();
    assert!(!switched.contains(":oi="));
    assert!(switched.contains("{{c2::image-occlusion:rect:left=.3:top=.1:width=.2:height=.08}}"));
    assert_eq!(edited.iter().map(|s| s.number).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 7]);
}

#[test]
fn drop_only_rewritten_shapes_f11() {
    let small = "{{c1::image-occlusion:rect:left=0:top=0:width=.03:height=.1}}<BR/>";
    let doc = io::parse(small).unwrap();
    assert_eq!(doc.write(&doc.shapes, false, 100., 100.).unwrap(), small);
    let mut edited = doc.shapes.clone();
    edited[0].left = 0.2;
    assert_eq!(doc.write(&edited, false, 100., 100.).unwrap(), "");
    let mut r = rect(2);
    r.fill = Some("transparent".into());
    assert_eq!(io::write(&[r], false, 100., 100.).unwrap(), "");
    assert_eq!(
        doc.write(&doc.shapes, false, 0., 100.).unwrap_err().kind,
        io::ErrorKind::InvalidSize
    );
    for kind in [
        ShapeKind::Ellipse { rx: 0.02, ry: 0.1 },
        ShapeKind::Polygon {
            points: vec![(0., 0.), (0.04, 0.1)],
        },
        ShapeKind::Text {
            text: String::new(),
            scale: 1.,
            fs: None,
        },
    ] {
        assert_eq!(
            io::write(&[Shape::new(1, 0., 0., kind)], false, 100., 100.).unwrap(),
            ""
        );
    }
    let boundary = Shape::new(
        1,
        0.,
        0.,
        ShapeKind::Rect {
            width: 0.05,
            height: 0.05,
        },
    );
    assert!(!io::write(&[boundary], false, 100., 100.).unwrap().is_empty());
}

#[test]
fn ordinals_examples_f12_and_labels_never_make_cards() {
    use io::ordinals::*;
    let mut shapes = vec![rect(1), rect(2), rect(4)];
    assert_eq!(allocate(&shapes), Some(3));
    group(&mut shapes, &[1, 2]);
    assert_eq!(shapes[2].number, 2);
    ungroup(&mut shapes, 2).unwrap();
    assert_eq!(shapes[2].number, 3);
    let mut shapes = vec![rect(1), rect(2), rect(2), rect(2), rect(3)];
    assert_eq!(groups(&shapes).get(&2), Some(&vec![1, 2, 3]));
    let copies = duplicate(&shapes, &[1, 2, 3]).unwrap();
    assert_eq!(copies.iter().map(|s| s.number).collect::<Vec<_>>(), [4, 4, 4]);
    ungroup(&mut shapes, 2).unwrap();
    assert_eq!(shapes.iter().map(|s| s.number).collect::<Vec<_>>(), [1, 2, 4, 5, 3]);
    let label = Shape::new(
        0,
        0.1,
        0.1,
        ShapeKind::Text {
            text: "label".into(),
            scale: 1.,
            fs: None,
        },
    );
    let written = io::write(&[rect(1), rect(2), label], false, 1000., 1000.).unwrap();
    assert_eq!(
        meguri_template::cloze::numbers(&written)
            .into_iter()
            .collect::<Vec<_>>(),
        [1, 2]
    );
}

#[test]
fn allocation_uses_first_gap_in_every_small_subset_f12() {
    // Exhaust every subset of 1..=8. F12's first gap is one beyond the
    // consecutive occupied low bits; zero, repeats and ordering do not count.
    for occupied in 0u16..256 {
        let mut numbers = vec![0, 0];
        for number in (1..=8).rev() {
            if occupied & (1 << (number - 1)) != 0 {
                numbers.extend([number, number]);
            }
        }
        let shapes: Vec<_> = numbers.into_iter().map(rect).collect();
        let expected = u16::try_from(occupied.trailing_ones()).unwrap() + 1;
        assert_eq!(io::ordinals::allocate(&shapes), Some(expected), "{occupied:#010b}");
    }
    // Check the upper encoding boundary and exhaustion as well as small gaps.
    let all: Vec<_> = (1..=u16::MAX).map(rect).collect();
    assert_eq!(io::ordinals::allocate(&all), None);
    for missing in [1, 32768, u16::MAX] {
        let shapes: Vec<_> = all.iter().filter(|s| s.number != missing).cloned().collect();
        assert_eq!(io::ordinals::allocate(&shapes), Some(missing));
    }
}

#[test]
fn group_and_ungroup_preserve_geometry_and_follow_editor_order_f12() {
    // Expected number lists apply F12: selected minimum, then first member
    // keeps that number and later members take successive available gaps.
    type GroupCase<'a> = (&'a [u16], &'a [usize], &'a [u16], &'a [u16]);
    let cases: &[GroupCase<'_>] = &[
        (&[1, 2, 4], &[1, 2], &[1, 2, 2], &[1, 2, 3]),
        (&[1, 2, 3, 4], &[1, 3], &[1, 2, 3, 2], &[1, 2, 3, 4]),
        (&[1, 2, 2, 2, 3], &[1, 2, 3], &[1, 2, 2, 2, 3], &[1, 2, 4, 5, 3]),
        (&[4, 2, 1, 2, 7], &[0, 2, 4], &[1, 2, 1, 2, 1], &[1, 2, 3, 2, 4]),
        (&[2, 1, 4], &[0, 2], &[2, 1, 2], &[2, 1, 3]),
        // An unselected member of the chosen group participates in ungroup.
        (&[3, 2, 4, 2], &[2, 1], &[3, 2, 2, 2], &[3, 2, 1, 4]),
        (&[1, 2, 4], &[2, 1, 2, 99], &[1, 2, 2], &[1, 2, 3]),
        (&[1, 2, 4], &[1, 1, 99], &[1, 2, 4], &[1, 2, 4]),
        (&[1, 2, 4], &[], &[1, 2, 4], &[1, 2, 4]),
    ];
    for &(numbers, selected, grouped, ungrouped) in cases {
        let original: Vec<_> = numbers
            .iter()
            .enumerate()
            .map(|(i, &number)| {
                let mut shape = rect(number);
                shape.left = f64::from(u32::try_from(i).unwrap()) / 16.;
                shape.unknown_props = vec![("zz".into(), format!("shape {i}"))];
                shape
            })
            .collect();
        let mut shapes = original.clone();
        io::ordinals::group(&mut shapes, selected);
        for (i, &number) in grouped.iter().enumerate() {
            let mut expected = original[i].clone();
            expected.number = number;
            assert_eq!(shapes[i], expected, "group {numbers:?}, {selected:?}, shape {i}");
        }
        let minimum = selected
            .iter()
            .filter_map(|&i| numbers.get(i))
            .min()
            .copied()
            .unwrap_or(1);
        io::ordinals::ungroup(&mut shapes, minimum).unwrap();
        for (i, &number) in ungrouped.iter().enumerate() {
            let mut expected = original[i].clone();
            expected.number = number;
            assert_eq!(shapes[i], expected, "ungroup {numbers:?}, {selected:?}, shape {i}");
        }
    }
}

#[test]
fn fixed_fields_and_every_unicode_prefix_never_panic_f7_f11() {
    use io::ErrorKind::*;
    let cases = [
        ("", Ok(0)),
        (" \t\n\r\x0c<br><BR/><br />", Ok(0)),
        (SEVEN, Ok(7)),
        ("{{c0::image-occlusion:text:left=.1:top=.1:text=日本\\:語}}", Ok(1)),
        ("{{c0::image-occlusion:text:left=.1:top=.1:text=a\\:b\\\\c}}", Ok(1)),
        (
            "{{c1::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1:zz=q\\:r}}<br />",
            Ok(1),
        ),
        ("{{c1::image-occlusion:rect:left=abc}}", Err(InvalidValue)),
        ("{{c1::image-occlusion:rect:left=.1:top=.1:width=.1}}", Err(MissingKey)),
        ("{{c1::image-occlusion:star:left=.1:top=.1}}", Err(UnknownShape)),
        (
            "{{c1,2::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1}}",
            Err(MultipleNumbers),
        ),
        (
            "{{c1::image-occlusion:rect:left=.1:left=.2:top=.1:width=.1:height=.1}}",
            Err(DuplicateKey),
        ),
        (
            "{{c1::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1{{c2::x}}}}",
            Err(NestedCloze),
        ),
        ("{{c1::image-occlusion:star:a=1\\x}}", Err(MalformedShape)),
        ("{{c1::x}}", Err(ExtraContent)),
        ("\0🦀{{c65536::x}}\\}}", Err(ExtraContent)),
    ];
    for (text, expected) in cases {
        let result = io::parse(text);
        assert_eq!(
            result.as_ref().map(|doc| doc.shapes.len()).map_err(|e| e.kind),
            expected,
            "{text:?}"
        );
        // Include truncated clozes, escapes, properties and Unicode text.
        for end in text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())) {
            let prefix = &text[..end];
            let _ = io::parse_raw(prefix);
            if let Ok(doc) = io::parse(prefix) {
                assert_eq!(doc.write(&doc.shapes, doc.hide_all, 1024., 768.).unwrap(), prefix);
            }
        }
    }
}

#[test]
fn every_binary_exponent_writes_readable_finite_numbers_f8() {
    // Both signs, every exponent, and mantissas at the binade boundaries
    // and midpoint, including subnormals, maximum finite values and NaNs.
    for sign in [0, 1u64 << 63] {
        for exponent in 0..=0x7ffu64 {
            for fraction in [0, 1, (1u64 << 51) - 1, 1u64 << 51, (1u64 << 52) - 1] {
                let bits = sign | (exponent << 52) | fraction;
                let value = f64::from_bits(bits);
                if exponent == 0x7ff {
                    assert!(io::format_number(value).is_err(), "{bits:#018x}");
                } else {
                    let printed = io::format_number(value).unwrap();
                    assert!(io::parse_number(&printed).is_some(), "{bits:#018x} -> {printed}");
                }
            }
        }
    }
}

#[test]
fn writer_ellipse_polygon_and_text_examples_f10() {
    for (shape, mode, expected) in [
        (
            Shape::new(2, 0.1, 0.2, ShapeKind::Ellipse { rx: 0.05, ry: 0.08 }),
            false,
            "{{c2::image-occlusion:ellipse:left=.1:top=.2:rx=.05:ry=.08:fill=#ff0000}}",
        ),
        (
            Shape::new(
                3,
                0.3,
                0.4,
                ShapeKind::Polygon {
                    points: vec![(0.3, 0.45), (0.5, 0.4), (0.35, 0.6)],
                },
            ),
            true,
            "{{c3::image-occlusion:polygon:left=.3:top=.4:points=.3,.45 .5,.4 .35,.6:oi=1}}",
        ),
        (
            Shape::new(
                0,
                0.2,
                0.7,
                ShapeKind::Text {
                    text: "line 1\nline 2".into(),
                    scale: 0.75,
                    fs: Some(0.04),
                },
            ),
            false,
            "{{c0::image-occlusion:text:left=.2:top=.7:text=line 1\nline 2:scale=.75:fs=.04}}",
        ),
        (
            Shape::new(
                0,
                0.6,
                0.6,
                ShapeKind::Text {
                    text: "日本:語".into(),
                    scale: 1.,
                    fs: Some(0.05),
                },
            ),
            true,
            "{{c0::image-occlusion:text:left=.6:top=.6:text=日本\\:語:scale=1.:fs=.05:oi=1}}",
        ),
    ] {
        let mut shape = shape;
        if shape.number == 2 {
            shape.fill = Some("#ff0000".into());
        }
        assert_eq!(io::write_shape(&shape, mode).unwrap(), expected);
    }
    let doc = io::parse("{{c6::image-occlusion:rect:zz=q\\:r:left=.1:top=.1:width=.1:height=.1}}").unwrap();
    let mut shape = doc.shapes[0].clone();
    shape.left = 0.2;
    assert_eq!(
        io::write_shape(&shape, true).unwrap(),
        "{{c6::image-occlusion:rect:left=.2:top=.1:width=.1:height=.1:zz=q\\:r:oi=1}}"
    );
}

#[test]
fn labels_occupy_numbers_but_are_never_group_members_f12() {
    let label = Shape::new(
        3,
        0.,
        0.,
        ShapeKind::Text {
            text: "legacy".into(),
            scale: 1.,
            fs: None,
        },
    );
    let mut shapes = vec![rect(1), rect(2), label, rect(4)];
    assert_eq!(io::ordinals::allocate(&shapes), Some(5));
    io::ordinals::group(&mut shapes, &[1, 2]);
    assert_eq!(shapes.iter().map(|s| s.number).collect::<Vec<_>>(), [1, 2, 3, 4]);
    io::ordinals::group(&mut shapes, &[1, 2, 3]);
    assert_eq!(shapes.iter().map(|s| s.number).collect::<Vec<_>>(), [1, 2, 3, 2]);
    let copied = io::ordinals::duplicate(&shapes, &[1, 2, 3]).unwrap();
    assert_eq!(copied.iter().map(|s| s.number).collect::<Vec<_>>(), [4, 0, 4]);
    assert_eq!(io::ordinals::groups(&shapes).get(&2), Some(&vec![1, 3]));
}

#[test]
fn writer_reports_input_index_and_preserves_tiny_unmodified_neighbors_f11() {
    let field = "{{c1::image-occlusion:rect:left=0:top=0:width=.001:height=.001}}\t<br/>{{c2::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1}}";
    let doc = io::parse(field).unwrap();
    let mut changed = doc.shapes.clone();
    changed[1].left = 0.2;
    assert_eq!(
        doc.write(&changed, false, 100., 100.).unwrap(),
        "{{c1::image-occlusion:rect:left=0:top=0:width=.001:height=.001}}<br>{{c2::image-occlusion:rect:left=.2:top=.1:width=.1:height=.1}}<br>"
    );
    changed[1].angle = f64::INFINITY;
    let e = doc.write(&changed, false, 100., 100.).unwrap_err();
    assert_eq!((e.kind, e.shape), (io::ErrorKind::UnwritableShape, Some(1)));
    for size in [f64::NAN, f64::INFINITY, -1., 0.] {
        assert_eq!(
            doc.write(&doc.shapes, false, size, 100.).unwrap_err().kind,
            io::ErrorKind::InvalidSize
        );
    }
    for tiny in [f64::from_bits(1), -f64::from_bits(1)] {
        assert_eq!(io::format_number(tiny).unwrap(), if tiny < 0. { "-0." } else { "." });
    }
    let max = io::format_number(f64::MAX).unwrap();
    assert_eq!(io::parse_number(&max), Some(f64::MAX));
}
