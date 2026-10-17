#![no_main]
use libfuzzer_sys::fuzz_target;
use meguri_template::image_occlusion;
fuzz_target!(|text: &str| {
    let _ = image_occlusion::parse_raw(text);
    if let Ok(doc) = image_occlusion::parse(text) {
        assert_eq!(doc.write(&doc.shapes, doc.hide_all, 1024., 768.).unwrap(), text);
    }
});
