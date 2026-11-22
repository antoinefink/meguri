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
fn references_use_detected_type() {
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
        document_with_scalar(catalog_extra, length, eol, content, &content.len().to_string())
    }

    fn document_with_scalar(catalog_extra: &str, length: Option<&str>, eol: &str, content: &[u8], scalar: &str) -> Vec<u8> {
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
            scalar.as_bytes().to_vec(),
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
    fn indirect_length_skips_visible_endstream_text_and_uses_the_later_scalar() {
        let content = b"BT /F1 12 Tf 20 50 Td (Visible endstream word and /JavaScript text) Tj ET";
        for eol in ["\r", "\n", "\r\n"] {
            for scalar in [content.len().to_string(), format!("% scalar{eol}+{}% end{eol}", content.len())] {
                assert_eq!(
                    detect(&document_with_scalar("", Some("9 0 R"), eol, content, &scalar)),
                    Some("pdf")
                );
            }
            let embedded = content.windows(9).position(|v| v == b"endstream").unwrap();
            for wrong in [0, 1, embedded, content.len() - 1, content.len() + 3, usize::MAX] {
                assert_eq!(
                    detect(&document_with_scalar("", Some("9 0 R"), eol, content, &wrong.to_string())),
                    None,
                    "wrong indirect length {wrong}"
                );
            }
            // A producer may include some or all of the separator in Length.
            for included in 1..=eol.len() {
                let scalar = (content.len() + included).to_string();
                assert_eq!(detect(&document_with_scalar("", Some("9 0 R"), eol, content, &scalar)), Some("pdf"));
            }
        }
        for scalar in ["-1", "1.0", "null", "[]", "9 0 R", "1 2", "999999999999999999999999999999999999"] {
            assert_eq!(detect(&document_with_scalar("", Some("9 0 R"), "\n", content, scalar)), None);
        }
        for length in ["10 0 R", "9 1 R", "5 0 R"] {
            assert_eq!(detect(&document("", Some(length), "\n", content)), None);
        }
        // Exact boundaries apply equally to direct and indirect declarations.
        for length in [None, Some("9 0 R")] {
            let mut pdf = document("", length, "\n", content);
            let end = pdf.windows(10).position(|v| v == b"endstream\n").unwrap();
            pdf[end + 9] = b'X';
            assert_eq!(detect(&pdf), None, "endstream must be a complete token");
        }
    }

    #[test]
    fn indirect_lengths_require_indexed_objects_outside_stream_data() {
        let mut content = b"BT /F1 12 Tf 20 50 Td (9 0 obj\n0000\nendobj\nendstream and /JavaScript text) Tj ET".to_vec();
        let scalar = content.windows(4).position(|v| v == b"0000").unwrap();
        let size = format!("{:04}", content.len());
        content[scalar..scalar + 4].copy_from_slice(size.as_bytes());
        let original = document("", Some("9 0 R"), "\n", &content);
        assert_eq!(detect(&original), Some("pdf"));
        let table = original.windows(10).position(|v| v == b"xref\n0 10\n").unwrap();
        let entry = table + 10 + 9 * 20;
        let embedded = original.windows(8).position(|v| v == b"9 0 obj\n").unwrap();
        for offset in [0, original.len() + 1, embedded] {
            let mut pdf = original.clone();
            pdf[entry..entry + 10].copy_from_slice(format!("{offset:010}").as_bytes());
            assert_eq!(detect(&pdf), None, "untrusted scalar offset {offset}");
        }
        let mut free = original;
        free[entry + 17] = b'f';
        assert_eq!(detect(&free), None, "free entries cannot provide lengths");
    }

    // Replace the table with an uncompressed or Flate xref stream. All offsets
    // still address actual objects; the Length scalar remains after the page.
    fn with_xref_stream(mut pdf: Vec<u8>, compressed: bool, predictor: bool) -> Vec<u8> {
        use std::io::Write;
        let xref = pdf.windows(10).position(|v| v == b"xref\n0 10\n").unwrap();
        let table = std::str::from_utf8(&pdf[xref..]).unwrap();
        let mut entries = vec![0, 0, 0, 0, 0, 255, 255];
        for line in table.lines().skip(3).take(9) {
            let offset = line[..10].parse::<u32>().unwrap();
            entries.push(1);
            entries.extend_from_slice(&offset.to_be_bytes());
            entries.extend_from_slice(&0u16.to_be_bytes());
        }
        entries.push(1);
        entries.extend_from_slice(&(xref as u32).to_be_bytes());
        entries.extend_from_slice(&0u16.to_be_bytes());
        let filter = if compressed { "/Filter [ /FlateDecode ]" } else { "" };
        let params = if predictor { "/DecodeParms [<< /Predictor 12 /Columns 7 >>]" } else { "" };
        if predictor {
            let mut predicted = Vec::new();
            let mut previous = [0u8; 7];
            for row in entries.chunks_exact(7) {
                predicted.push(2);
                predicted.extend(row.iter().zip(previous).map(|(v, up)| v.wrapping_sub(up)));
                previous.copy_from_slice(row);
            }
            entries = predicted;
        }
        if compressed {
            let mut compressor = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            compressor.write_all(&entries).unwrap();
            entries = compressor.finish().unwrap();
        }
        pdf.truncate(xref);
        pdf.extend_from_slice(format!("10 0 obj\n<< /Type /XRef /Size 11 /Root 1 0 R /W [1 4 2] /Index [0 11] /Length {} {filter} {params} >>\nstream\n", entries.len()).as_bytes());
        pdf.extend_from_slice(&entries);
        pdf.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        pdf
    }

    #[test]
    fn indirect_lengths_resolve_through_plain_and_compressed_xref_streams() {
        let content = b"BT /F1 12 Tf 20 50 Td (Visible endstream word) Tj ET";
        for (compressed, predictor) in [(false, false), (true, false), (true, true)] {
            assert_eq!(detect(&with_xref_stream(document("", Some("9 0 R"), "\n", content), compressed, predictor)), Some("pdf"));
            let bad = document_with_scalar("", Some("9 0 R"), "\n", content, "999999");
            assert_eq!(detect(&with_xref_stream(bad, compressed, predictor)), None);
        }
    }

    fn append_revision(mut pdf: Vec<u8>, content: &[u8], scalar: usize) -> Vec<u8> {
        let marker = pdf.windows(9).rposition(|v| v == b"startxref").unwrap();
        let prev = std::str::from_utf8(&pdf[marker + 9..]).unwrap().split_whitespace().next().unwrap();
        let prev = prev.parse::<usize>().unwrap();
        let size = pdf.windows(6).rposition(|v| v == b"/Size ").unwrap();
        let digits: Vec<u8> = pdf[size + 6..].iter().copied().take_while(u8::is_ascii_digit).collect();
        let size = std::str::from_utf8(&digits).unwrap().parse::<usize>().unwrap();
        let stream = pdf.len();
        pdf.extend_from_slice(b"5 0 obj\n<< /Length 9 0 R >>\nstream\n");
        pdf.extend_from_slice(content);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        let length = pdf.len();
        pdf.extend_from_slice(format!("9 0 obj\n{scalar}\nendobj\n").as_bytes());
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n5 1\n{stream:010} 00000 n \n9 1\n{length:010} 00000 n \ntrailer\n<< /Size {size} /Root 1 0 R /Prev {prev} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        pdf
    }

    #[test]
    fn incremental_stream_replacements_resolve_lengths_in_their_own_revision() {
        let newer = b"BT /F1 12 Tf 20 50 Td (New visible endstream text) Tj ET";
        for stream_xref in [false, true] {
            let original = document("", Some("9 0 R"), "\r\n", CONTENT);
            let original = if stream_xref { with_xref_stream(original, true, true) } else { original };
            let updated = append_revision(original.clone(), newer, newer.len());
            assert_eq!(detect(&updated), Some("pdf"));
            // A third revision also replaces both IDs, without confusing caches.
            assert_eq!(detect(&append_revision(updated, CONTENT, CONTENT.len())), Some("pdf"));
            assert_eq!(detect(&append_revision(original, newer, newer.len() - 1)), None);
        }
        let active = document("/AA << /WC << /S /JavaScript >> >>", Some("9 0 R"), "\n", CONTENT);
        assert_eq!(detect(&append_revision(active, newer, newer.len())), None);
    }

    // Object 9 is the second compressed object, following an ordinary metadata
    // dictionary. Both the ID and nonzero object index must agree with the xref.
    fn object_stream_document(decoded: &[u8], first: usize, count: usize, index: u16,
        compressed: bool, hybrid: bool, length: Option<&str>) -> Vec<u8> {
        use std::io::Write;
        let mut pdf = document("", Some("9 0 R"), "\n", CONTENT);
        let table = pdf.windows(10).position(|v| v == b"xref\n0 10\n").unwrap();
        let offsets: Vec<u32> = std::str::from_utf8(&pdf[table..]).unwrap().lines()
            .skip(3).take(9).map(|line| line[..10].parse().unwrap()).collect();
        pdf.truncate(offsets[8] as usize);
        let stream = pdf.len();
        let bytes = if compressed {
            let mut compressor = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            compressor.write_all(decoded).unwrap();
            compressor.finish().unwrap()
        } else { decoded.to_vec() };
        let direct = bytes.len().to_string();
        let length = length.unwrap_or(&direct);
        let filter = if compressed { "/Filter /FlateDecode" } else { "" };
        pdf.extend_from_slice(format!("10 0 obj\n<< /Type /ObjStm /N {count} /First {first} /Length {length} {filter} >>\nstream\n").as_bytes());
        pdf.extend_from_slice(&bytes);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        let xref = pdf.len();
        let mut entries = Vec::new();
        for object in 0..13 {
            let (kind, position, generation) = match object {
                0 => (0, 0, 65535),
                1..=8 => (1, offsets[object - 1], 0),
                9 => (2, 10, index),
                10 => (1, stream as u32, 0),
                11 => (1, xref as u32, 0),
                12 => (2, 10, 0),
                _ => unreachable!(),
            };
            entries.push(kind);
            entries.extend_from_slice(&position.to_be_bytes());
            entries.extend_from_slice(&generation.to_be_bytes());
        }
        pdf.extend_from_slice(format!("11 0 obj\n<< /Type /XRef /Size 13 /Root 1 0 R /W [1 4 2] /Length {} >>\nstream\n", entries.len()).as_bytes());
        pdf.extend_from_slice(&entries);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        let startxref = if hybrid {
            let table = pdf.len();
            // The hybrid supplement overrides these free table placeholders.
            pdf.extend_from_slice(format!("xref\n9 1\n0000000000 00000 f \n12 1\n0000000000 00000 f \ntrailer\n<< /Size 13 /Root 1 0 R /XRefStm {xref} >>\n").as_bytes());
            table
        } else { xref };
        pdf.extend_from_slice(format!("startxref\n{startxref}\n%%EOF\n").as_bytes());
        pdf
    }

    #[test]
    fn indirect_scalar_lengths_in_bounded_object_streams_are_accepted() {
        let other = "<< /Producer (Meguri object stream regression) >>\n";
        let header = format!("12 0 9 {}\n", other.len());
        let decoded = format!("{header}{other}{}\n", CONTENT.len());
        for compressed in [false, true] {
            for hybrid in [false, true] {
                let pdf = object_stream_document(decoded.as_bytes(), header.len(), 2, 1, compressed, hybrid, None);
                assert_eq!(detect(&pdf), Some("pdf"), "compressed {compressed}, hybrid {hybrid}");
                let newer = b"BT (Incremental object stream replacement) Tj ET";
                assert_eq!(detect(&append_revision(pdf, newer, newer.len())), Some("pdf"));
            }
        }
    }

    #[test]
    fn compressed_lengths_require_bounded_headers_framing_and_acyclic_references() {
        let header = "12 0 9 5\n";
        let decoded = format!("{header}null\n{}\n", CONTENT.len());
        // /First may include leading whitespace; both object slices remain exact.
        for first in [header.len(), header.len() - 1] {
            assert_eq!(detect(&object_stream_document(decoded.as_bytes(), first, 2, 1, false, false, None)), Some("pdf"),
                "first {first}, count 2, index 1");
        }
        for (first, count, index) in [
            (header.len(), 2, 0), (header.len(), 2, 2),
            (header.len() - 2, 2, 1), (header.len() + 1, 2, 1),
            (usize::MAX, 2, 1), (header.len(), 0, 1),
            (header.len(), 1, 1), (header.len(), RECORDS, 1),
        ] {
            assert_eq!(detect(&object_stream_document(decoded.as_bytes(), first, count, index, false, false, None)), None,
                "first {first}, count {count}, index {index}");
        }
        let scalar = CONTENT.len();
        for (header, body) in [
            ("12 0 8 5\n", format!("null\n{scalar}\n")), // wrong ID
            ("9 0 9 5\n", format!("null\n{scalar}\n")), // duplicate ID
            ("12 5 9 0\n", format!("null\n{scalar}\n")), // reversed offsets
            ("12 0 9 999999\n", format!("null\n{scalar}\n")),
            ("12 0 9 5\n", format!("null\n{scalar} 1\n")), // multiple scalar values
            ("12 0 9 5\n", "null\n9 0 R\n".to_string()), // not a scalar
            ("12 0 9 20\n", format!("<< /JavaScript 0 >>\n{scalar}\n")),
        ] {
            let decoded = format!("{header}{body}");
            assert_eq!(detect(&object_stream_document(decoded.as_bytes(), header.len(), 2, 1, false, false, None)), None);
        }
        assert_eq!(detect(&object_stream_document(decoded.as_bytes(), header.len(), 2, 1, false, false, Some("9 0 R"))), None);
        assert_eq!(detect(&object_stream_document(decoded.as_bytes(), header.len(), 2, 1, false, false, Some("12 0 R"))), None);
        let mut wrong_header = object_stream_document(decoded.as_bytes(), header.len(), 2, 1, false, false, None);
        let at = wrong_header.windows(8).position(|v| v == b"10 0 obj").unwrap();
        wrong_header[at + 1] = b'3';
        assert_eq!(detect(&wrong_header), None);
        let original = object_stream_document(decoded.as_bytes(), header.len(), 2, 1, false, false, None);
        let xref = original.windows(8).position(|v| v == b"11 0 obj").unwrap();
        let data = xref + original[xref..].windows(7).position(|v| v == b"stream\n").unwrap() + 7;
        for offset in [0u32, original.len() as u32 + 1] {
            let mut pdf = original.clone();
            let entry = data + 10 * 7 + 1;
            pdf[entry..entry + 4].copy_from_slice(&offset.to_be_bytes());
            assert_eq!(detect(&pdf), None);
        }
        let mut wrong_boundary = original;
        let marker = b"endstream\nendobj\n11 0 obj";
        let end = wrong_boundary.windows(marker.len()).position(|v| v == marker).unwrap();
        wrong_boundary[end + 8] = b'X';
        assert_eq!(detect(&wrong_boundary), None);
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
