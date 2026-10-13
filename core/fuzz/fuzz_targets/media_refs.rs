//! Media reference extraction, the IRI encoding of displayed names and the
//! media file name transform never panic, on any input
//! (specs/text/media-references.md, media-filenames.md).

#![no_main]

use libfuzzer_sys::fuzz_target;
use meguri_text::{iri, media_name, media_refs};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);

    let mut names = 0;
    for tag in media_refs::media_tags(&text) {
        assert_eq!(&text[tag.start..tag.start + tag.matched.len()], tag.matched);
        assert!(tag.matched.contains(tag.raw_name), "{text:?}");
        names += 1;
    }
    for sound in media_refs::sound_tags(&text) {
        assert_eq!(&text[sound.start..sound.start + sound.matched.len()], sound.matched);
        names += 1;
    }
    let references = media_refs::references(&text);
    assert_eq!(references.len(), names, "{text:?}");
    assert!(media_refs::local_references(&text).len() <= names);

    let encoded = iri::encode_for_display(&text);
    let _ = iri::decode_for_saving(&encoded);
    let _ = iri::decode_for_saving(&text);

    let name = media_name::safe_media_name(&text);
    assert!(name.len() <= 120, "{text:?} -> {name:?}");
});
