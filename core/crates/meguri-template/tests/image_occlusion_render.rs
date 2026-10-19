//! I1–I6 literals complement the complete golden adapter run.
use meguri_template::{
    cloze::{self, Side},
    image_occlusion::render::element,
};
#[test]
fn attributes_keep_order_duplicates_raw_values_and_exact_zero_rules() {
    assert_eq!(
        element("cloze", "1,3", "rect:left=.1:left=.2:zz=9:top=.3:points=1,2:text=t"),
        "<div class=\"cloze\" data-ordinal=\"1,3\" data-shape=\"rect\" data-left=\".1\" data-left=\".2\" data-top=\".3\" data-points=\"1,2\" data-text=\"t\" ></div>"
    );
    assert_eq!(
        element(
            "cloze-highlight",
            "2",
            "ellipse:width=0:height=0.0:rx=00:ry=-0:angle=0:fs=0:scale=0"
        ),
        "<div class=\"cloze-highlight\" data-ordinal=\"2\" data-shape=\"ellipse\" data-height=\"0.0\" data-rx=\"00\" data-ry=\"-0\" data-angle=\"0\" data-font-size=\"0\" ></div>"
    );
    assert_eq!(
        element("cloze", "1", "rect"),
        "<div class=\"cloze\" data-ordinal=\"1\" ></div>"
    );
    assert_eq!(
        element("cloze", "1", "star:fill=a&b 'c'"),
        "<div class=\"cloze\" data-ordinal=\"1\" data-fill=\"a&b 'c'\" ></div>"
    );
    assert_eq!(
        element("cloze", "1", "polygon:points=.1,.2  .3 .4,.5,.6 ,.7 .8,"),
        "<div class=\"cloze\" data-ordinal=\"1\" data-shape=\"polygon\" data-points=\".1,.2 .4,.5,.6 ,.7 .8,\" ></div>"
    );
}
#[test]
fn labels_use_attribute_encoding_after_the_codec_unescapes_them() {
    assert_eq!(
        element(
            "cloze",
            "0",
            "text:text=a\\:b<c>\"d\"&e 'f' 日本\\\\x\nline 2:scale=1.5:fs=.0667"
        ),
        "<div class=\"cloze\" data-ordinal=\"0\" data-shape=\"text\" data-text=\"a&#x3A;b&lt;c&gt;&quot;d&quot;&amp;e&#x20;&#x27;f&#x27;&#x20;日本&#x5C;x&#x0A;line&#x20;2\" data-scale=\"1.5\" data-font-size=\".0667\" ></div>"
    );
}
#[test]
fn class_activity_and_nested_children_use_the_existing_cloze_hook() {
    let text = "{{c1,3::image-occlusion:rect:left=.1}}{{c0::image-occlusion:text:text=label}}";
    assert!(cloze::render(text, 1, Side::Front).starts_with("<div class=\"cloze\" data-ordinal=\"1,3\""));
    assert!(cloze::render(text, 3, Side::Back).starts_with("<div class=\"cloze-highlight\" data-ordinal=\"1,3\""));
    assert_eq!(cloze::render(text, 2, Side::Back), "");
    assert_eq!(
        cloze::render("{{c0::image-occlusion:text:text=label}}", 1, Side::Front),
        ""
    );
    assert_eq!(
        cloze::render(
            "{{c1::image-occlusion:rect:left=.1{{c2::ignored}}tail}}",
            1,
            Side::Front
        ),
        "<div class=\"cloze\" data-ordinal=\"1\" data-shape=\"rect\" data-left=\".1\" ></div>"
    );
}
