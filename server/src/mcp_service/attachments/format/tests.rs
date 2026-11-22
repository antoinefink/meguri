// SPDX-License-Identifier: MIT
use super::*;
use crate::mcp_service::attachment_formats as fixtures;
#[test]
fn expanded_type_matrix_requires_complete_structures_and_detects_original_bytes() {
    let cases = [
        ("gif", fixtures::gif()),
        ("webp", fixtures::webp()),
        ("avif", fixtures::heif(b"avif")),
        ("heic", fixtures::heif(b"heic")),
        ("heif", fixtures::heif(b"mif1")),
        ("wav", fixtures::wav()),
        ("flac", fixtures::flac()),
        ("aac", fixtures::adts()),
        ("m4a", fixtures::m4a(false)),
        ("aiff", fixtures::aiff(false)),
        ("aifc", fixtures::aiff(true)),
        ("caf", fixtures::caf()),
        ("ogg", fixtures::ogg(true)),
        ("ogg", fixtures::ogg(false)),
        ("pdf", fixtures::pdf()),
    ];
    for (ext, bytes) in cases {
        assert_eq!(detect(&bytes), Some(ext), "{ext}");
        assert_eq!(
            detect(&bytes[..bytes.len() - if ext == "pdf" { 2 } else { 1 }]),
            None,
            "truncated {ext}"
        );
        let mut bad_signature = bytes.clone();
        bad_signature[0] ^= 0x80;
        assert_eq!(detect(&bad_signature), None, "signature {ext}");
        let mut appended = bytes.clone();
        appended.extend_from_slice(b"<script>active</script>");
        assert_eq!(detect(&appended), None, "trailing content {ext}");
        // Every short prefix must fail without indexing outside the input.
        for n in 0..bytes.len().min(16) {
            assert_eq!(detect(&bytes[..n]), None, "short {ext}/{n}");
        }
    }
}
#[test]
fn adts_accepts_one_complete_frame_and_rejects_partial_frames() {
    let bytes = fixtures::adts();
    let first = &bytes[..bytes.len() / 2];
    let tiny = fixtures::adts_frame(&[0x01, 0x40, 0x22, 0x80]);
    for frame in [first, tiny.as_slice()] {
        assert_eq!(detect(frame), Some("aac"), "complete isolated frame");
        assert_eq!(detect(&frame[..frame.len() - 1]), None, "partial frame");
    }
    assert_eq!(detect(&bytes[..first.len() + 1]), None, "partial next header");
}
#[test]
fn container_lengths_and_audio_only_tracks_are_enforced() {
    for mut bytes in [
        fixtures::wav(),
        fixtures::webp(),
        fixtures::aiff(false),
        fixtures::aiff(true),
    ] {
        bytes[4..8].fill(0xff);
        assert_eq!(detect(&bytes), None);
    }
    assert_eq!(
        detect(&fixtures::m4a(true)),
        None,
        "video M4A brand is insufficient"
    );
    for mut bytes in [
        fixtures::heif(b"avif"),
        fixtures::heif(b"heic"),
        fixtures::m4a(false),
    ] {
        bytes[..4].copy_from_slice(&1u32.to_be_bytes());
        bytes[8..16].fill(0xff);
        assert_eq!(detect(&bytes), None, "oversized extended box");
    }
    let mut ogg = fixtures::ogg(true);
    *ogg.last_mut().unwrap() ^= 1;
    assert_eq!(detect(&ogg), None, "CRC covers payload");
    let mut aac = fixtures::adts();
    aac[4] = 0xff;
    assert_eq!(detect(&aac), None, "ADTS declared frame extends past input");
    let mut flac = fixtures::flac();
    flac[4] = 0xff;
    assert_eq!(detect(&flac), None, "reserved FLAC block");
}
#[test]
fn pdf_active_names_are_decoded_and_strings_are_data() {
    for active in [
        b"/JavaScript".as_slice(),
        b"/J#53",
        b"/OpenAction",
        b"/AA",
        b"/Launch",
        b"/EmbeddedFile",
        b"/Encrypt",
        b"/XFA",
    ] {
        assert_eq!(detect(&fixtures::pdf_with(active)), None);
    }
    assert_eq!(
        detect(&fixtures::pdf_with(b"/Title (A /JavaScript example)")),
        Some("pdf")
    );
    let mut wrong_xref = fixtures::pdf();
    let at = wrong_xref
        .windows(9)
        .rposition(|v| v == b"startxref")
        .unwrap()
        + 10;
    wrong_xref[at] = b'9';
    assert_eq!(detect(&wrong_xref), None);
    for bytes in [
        b"<svg xmlns='http://www.w3.org/2000/svg'/>".as_slice(),
        b"<!doctype html><html>active</html>",
        b"%PDF-1.7\n/Catalog /Pages\n%%EOF",
    ] {
        assert_eq!(detect(bytes), None);
    }
}
#[test]
fn references_use_detected_type_despite_advisory_metadata() {
    use crate::mcp_service::attachments::Validated;
    for (bytes, extension, template) in [
        (fixtures::gif(), "gif", "<img src=\"NAME\">"),
        (fixtures::wav(), "wav", "[sound:NAME]"),
        (
            fixtures::ogg(true),
            "ogg",
            "<audio controls src=\"NAME\"></audio>",
        ),
        (
            fixtures::pdf(),
            "pdf",
            "<a href=\"NAME\">Open PDF</a><object data=\"NAME\" hidden></object>",
        ),
    ] {
        let value = Validated::new(bytes.clone()).unwrap();
        assert_eq!(value.bytes, bytes);
        assert!(value.name.ends_with(&format!(".{extension}")));
        assert_eq!(value.reference(), template.replace("NAME", &value.name));
        assert_eq!(value.name, Validated::new(bytes).unwrap().name);
    }
}

#[test]
fn pdf_inspects_compressed_objects_and_bounds_expansion() {
    use std::io::Write;
    fn object_stream(content: &[u8]) -> Vec<u8> {
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        compressor.write_all(content).unwrap();
        let bytes = compressor.finish().unwrap();
        let mut extra = format!(
            "/Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /Length {} >>\nstream\n",
            bytes.len()
        )
        .into_bytes();
        extra.extend(bytes);
        extra.extend_from_slice(b"\nendstream\n<<");
        fixtures::pdf_with(&extra)
    }
    assert_eq!(
        detect(&object_stream(b"6 0 << /Title (owned fixture) >>")),
        Some("pdf")
    );
    assert_eq!(
        detect(&object_stream(b"6 0 << /S /JavaScript /JS (active) >>")),
        None
    );
    assert_eq!(
        detect(&object_stream(&vec![b' '; 16 * 1024 * 1024 + 1])),
        None
    );
    let mut chained = fixtures::ogg(true);
    chained.extend(fixtures::ogg(false));
    assert_eq!(detect(&chained), Some("ogg"));
}
#[test]
fn realistic_metadata_layouts_and_item_bounds_are_checked() {
    // Real encoders commonly put large artwork/comments before audio data.
    let mut wav = fixtures::wav();
    let mut metadata = b"LIST".to_vec();
    metadata.extend_from_slice(&70000u32.to_le_bytes());
    metadata.resize(70008, 0);
    wav.splice(12..12, metadata);
    let size = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&size.to_le_bytes());
    assert_eq!(detect(&wav), Some("wav"));
    let mut heif = fixtures::heif(b"heic");
    let location = heif.windows(4).position(|v| v == b"iloc").unwrap() + 4;
    heif[location + 14..location + 18].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        detect(&heif),
        None,
        "item extent cannot escape the uploaded bytes"
    );
    // Framing-only ISO brands cannot impersonate an image or audio track.
    assert_eq!(
        detect(&fixtures::iso_box(b"ftyp", b"avif\0\0\0\0mif1")),
        None
    );
}

mod pdf_regressions {
    use super::*;

    const CONTENT: &[u8] = b"BT /F1 12 Tf 20 50 Td (Meguri PDF regression) Tj ET";

    // One readable page, a standard font, exact stream lengths and a complete
    // xref table. Object 9 supplies the indirect stream length when requested.
    fn document(catalog_extra: &str, length: Option<&str>, eol: &str, content: &[u8]) -> Vec<u8> {
        let direct = content.len().to_string();
        let length = length.unwrap_or(&direct);
        let mut stream = format!("<< /Length {length}>>\nstream{eol}").into_bytes();
        stream.extend_from_slice(content);
        stream.extend_from_slice(format!("{eol}endstream").as_bytes());
        let objects = [
            format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox 7 0 R /Resources 8 0 R /Contents 5 0 R >>".to_vec(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
            stream,
            b"<< /Producer (Meguri synthetic regression) >>".to_vec(),
            b"[0 0 200 100]".to_vec(),
            b"<< /Font << /F1 4 0 R >> >>".to_vec(),
            content.len().to_string().into_bytes(),
        ];
        let mut pdf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 10\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 10 /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n")
                .as_bytes(),
        );
        pdf
    }

    #[test]
    fn passive_open_action_destinations_are_accepted() {
        for destination in [
            "/OpenAction [3 0 R /Fit]",
            "/OpenAction[3 0 R/FitB]",
            "/OpenAction [3 0 R /XYZ null null 1.25]",
            "/OpenAction [3 0 R /FitH null]",
            "/OpenAction [3 0 R /FitV 20]",
            "/OpenAction [3 0 R /FitBH +20.5]",
            "/OpenAction [3 0 R /FitBV -.5]",
            "/OpenAction [3 0 R /FitR 0 0 200 100]",
            "/Open#41ction% destination\r\n[3% page\r0\x0cR/F#69t]",
        ] {
            assert_eq!(
                detect(&document(destination, None, "\n", CONTENT)),
                Some("pdf"),
                "{destination}"
            );
        }
    }

    #[test]
    fn open_action_dictionaries_bare_names_and_malformed_destinations_are_refused() {
        for action in [
            "/OpenAction",
            "/Open#41ction",
            "/OpenAction << /S /GoTo /D [3 0 R /Fit] >>",
            "/OpenAction << /S /URI /URI (https://example.invalid/) >>",
            r"/Open#41ction << /S /Java#53cript /JS (app.alert\(1\)) >>",
            "/OpenAction 6 0 R",
            "/OpenAction (named destination)",
            "/OpenAction []",
            "/OpenAction [3 0 R /Fit",
            "/OpenAction [3 0 R /Fit 1]",
            "/OpenAction [3 0R /Fit]",
            "/OpenAction [3x 0 R /Fit]",
            "/OpenAction [3 0 Rjunk /Fit]",
            "/OpenAction [3 0 R /XYZ null null]",
            "/OpenAction [3 0 R /XYZ 1e2 null null]",
            "/OpenAction [3 0 R /FitR null 0 200 100]",
            "/OpenAction [3 0 R /FitH 1.2.3]",
            "/OpenAction [3 0 R /Fit << /S /URI >>]",
        ] {
            assert_eq!(
                detect(&document(action, None, "\n", CONTENT)),
                None,
                "{action}"
            );
        }
        for active in [
            "/AA << /WC << /S /JavaScript /JS (active) >> >>",
            "/Names << /JavaScript << /Names [(startup) << /S /JavaScript /JS (active) >>] >> >>",
            "/AcroForm << /XFA (active) >>",
        ] {
            let catalog = format!("/OpenAction [3 0 R /Fit] {active}");
            assert_eq!(
                detect(&document(&catalog, None, "\n", CONTENT)),
                None,
                "{active}"
            );
        }
    }

    #[test]
    fn indirect_lengths_use_pdf_tokens_and_all_stream_line_endings() {
        for length in [
            "9 0 R",
            "9\t0\x0cR",
            "+9 +0 R",
            "9% object\r0% generation\nR",
            "9\0% object\r\n0% generation\r\nR% reference\r\n",
        ] {
            for eol in ["\r", "\n", "\r\n"] {
                assert_eq!(
                    detect(&document("", Some(length), eol, CONTENT)),
                    Some("pdf"),
                    "length {length:?}, eol {eol:?}"
                );
            }
        }
        for eol in ["\r", "\n", "\r\n"] {
            assert_eq!(detect(&document("", None, eol, CONTENT)), Some("pdf"));
            let length = format!("+{}% direct length\r\n", CONTENT.len());
            assert_eq!(
                detect(&document("", Some(&length), eol, CONTENT)),
                Some("pdf")
            );
        }
    }

    #[test]
    fn malformed_lengths_and_truncated_streams_are_refused() {
        for length in [
            "",
            "-1",
            "1.0",
            "null",
            "/Unknown",
            "[]",
            "<<>>",
            "9x",
            "9R",
            "9 0R",
            "9 0 Rjunk",
            "9 0 R#20",
            "9 -1 R",
            "9 65536 R",
            "9 0.0 R",
            "0 0 R",
            "9% object\n0% generation\rRjunk",
            "999999999999999999999999999999999999999",
        ] {
            assert_eq!(
                detect(&document("", Some(length), "\n", CONTENT)),
                None,
                "{length:?}"
            );
        }
        for length in [0, CONTENT.len() - 1, CONTENT.len() + 2, usize::MAX] {
            assert_eq!(
                detect(&document("", Some(&length.to_string()), "\n", CONTENT)),
                None,
                "incorrect direct length {length}"
            );
        }
        let original_length = CONTENT.len().to_string();
        assert_eq!(
            detect(&document(
                "",
                Some(&original_length),
                "\n",
                &CONTENT[..CONTENT.len() - 4]
            )),
            None
        );
        for length in [None, Some("9 0 R")] {
            assert_eq!(detect(&document("", length, " ", CONTENT)), None);
            let pdf = document("", length, "\n", CONTENT);
            assert_eq!(detect(&pdf[..pdf.len() - 3]), None);
        }
    }

    #[test]
    fn pdf_lookahead_remains_bounded() {
        let comments = "% bounded lookahead\n".repeat(RECORDS);
        let destination = format!("/OpenAction {comments}[3 0 R /Fit]");
        assert_eq!(detect(&document(&destination, None, "\n", CONTENT)), None);
        let length = format!("9 {comments}0 R");
        assert_eq!(detect(&document("", Some(&length), "\n", CONTENT)), None);
    }
}
