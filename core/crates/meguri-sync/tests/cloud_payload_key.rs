//! K4.2 media keys use M2 lowercase mapping; payloads retain exact file names.
use meguri_sync::cloud::{
    Error,
    entity_codec::{MediaName, Payload},
    row_payload::payload_key,
};

#[test]
fn media_payload_key_uses_fixed_unicode_lowercase_contract() {
    for (name, expected) in [
        ("Photo.JPG", "photo.jpg"),
        ("photo.jpg", "photo.jpg"),
        ("Straße.PNG", "straße.png"),
        ("İ.PNG", "i\u{307}.png"),
        ("ΟΣ.PNG", "οσ.png"),
        ("ΟΣ", "ος"),
        ("É.PNG", "é.png"),
        ("E\u{301}.PNG", "e\u{301}.png"),
    ] {
        let payload = Payload::MediaName(MediaName {
            name: name.as_bytes().to_vec(),
            ..MediaName::default()
        });
        let original = payload.clone();
        assert_eq!(payload_key(&payload).unwrap(), expected.as_bytes(), "{name}");
        assert_eq!(payload, original);
    }
}

#[test]
fn media_payload_key_refuses_invalid_utf8() {
    for name in [vec![0xff], vec![0xc3], vec![b'A', 0x80, b'.', b'P', b'N', b'G']] {
        let payload = Payload::MediaName(MediaName {
            name,
            ..MediaName::default()
        });
        assert!(matches!(payload_key(&payload), Err(Error::InvalidData)));
    }
}
