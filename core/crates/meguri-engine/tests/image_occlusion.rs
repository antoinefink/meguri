//! render/image-occlusion-format F2/F13. Does not need a collection for reads.
use meguri_engine::{image_occlusion as io, notetypes::stock::StockKind};
use meguri_model::{
    ids::{NoteId, NotetypeId},
    note::Note,
    rng::SeededRandom,
};

#[test]
fn detection_checks_stock_kind_and_uses_tags_independent_of_cloze_kind() {
    let mut nt = StockKind::ImageOcclusion.notetype(&SeededRandom::new(63));
    assert_eq!(io::detect(&nt).unwrap().occlusions, 0);
    nt.fields.reverse();
    let roles = io::detect(&nt).unwrap();
    assert_eq!(
        (roles.occlusions, roles.image, roles.header, roles.back_extra),
        (4, 3, 2, 1)
    );
    nt.config.kind = 0;
    assert!(io::detect(&nt).is_some());
    nt.fields[1].config.tag = None; // incomplete set falls back entirely
    let roles = io::detect(&nt).unwrap();
    assert_eq!(
        (roles.occlusions, roles.image, roles.header, roles.back_extra),
        (0, 1, 2, 3)
    );
    nt.fields.truncate(3);
    assert!(io::detect(&nt).is_none());
    let basic = StockKind::BasicOptionalReversed.notetype(&SeededRandom::new(63));
    assert!(io::detect(&basic).is_none());
}

#[test]
fn get_reports_unreadable_masks_without_changing_note() {
    let mut nt = StockKind::ImageOcclusion.notetype(&SeededRandom::new(63));
    nt.id = NotetypeId(2);
    let text =
        "{{c1::image-occlusion:rect:left=abc}}\u{1f}<img src=\"a&amp;b.PNG\">\u{1f}header\u{1f}back\u{1f}comments";
    let note = Note {
        id: NoteId(1),
        notetype_id: nt.id,
        fields: text.into(),
        ..Note::default()
    };
    let reading = io::get(&note, &nt).unwrap();
    assert_eq!(reading.image_name.as_deref(), Some("a&amp;b.PNG"));
    assert_eq!(reading.document.unwrap_err().kind, io::ErrorKind::InvalidValue);
    assert_eq!(note.fields, text);
    nt.config.original_stock_kind = 5;
    assert_eq!(io::get(&note, &nt).unwrap_err().to_string(), "not_image_occlusion");
}

#[test]
fn image_name_is_literal_first_matching_tag_last_matching_attribute_f13() {
    for (html, name) in [
        ("<img src='single.png'><img src=\"second.png\">", Some("second.png")),
        ("<IMG src=\"upper.png\">", None),
        ("<image src=\"x.png\">", None),
        ("<img src=\"\"><img data-src = \"a&amp;b.png\">", Some("a&amp;b.png")),
        ("<img src=\"a.png\" src = \"b.png\"><img src=\"c.png\">", Some("b.png")),
        ("<img other='>' src=\"after.png\">", None),
        ("<img\n src = \"日本.png\">", Some("日本.png")),
    ] {
        assert_eq!(io::image_name(html), name, "{html}");
    }
}

#[test]
fn update_delegates_only_the_role_selected_field_to_the_normal_save() {
    let mut nt = StockKind::ImageOcclusion.notetype(&SeededRandom::new(63));
    nt.id = NotetypeId(2);
    nt.fields.swap(0, 3);
    let note = Note {
        id: NoteId(1),
        notetype_id: nt.id,
        fields: "back\u{1f}<img src=\"a.png\">\u{1f}header\u{1f}old\u{1f}comments".into(),
        tags: " tag ".into(),
        data: "keep".into(),
        ..Note::default()
    };
    let written = io::update(&note, &nt, "new", Ok).unwrap();
    let mut expected = note.clone();
    expected.fields = "back\u{1f}<img src=\"a.png\">\u{1f}header\u{1f}new\u{1f}comments".into();
    assert_eq!(written, expected);
    assert_eq!(io::update(&note, &nt, "old", Ok).unwrap(), note);
    nt.config.original_stock_kind = 5;
    let mut called = false;
    let error = io::update(&note, &nt, "new", |_| {
        called = true;
        Ok(())
    })
    .unwrap_err();
    assert!(!called);
    assert!(error.to_string().contains("not_image_occlusion"));
}
