// SPDX-License-Identifier: MIT
//! Fixture-owned format builders. No downloaded or reference assets.
#![allow(dead_code)]
fn chunk(kind: &[u8; 4], data: &[u8], little: bool) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend_from_slice(&if little {
        (data.len() as u32).to_le_bytes()
    } else {
        (data.len() as u32).to_be_bytes()
    });
    out.extend_from_slice(data);
    if data.len() & 1 != 0 {
        out.push(0);
    }
    out
}
pub fn gif() -> Vec<u8> {
    // One white pixel: clear, palette index 1, end, in 3-bit LZW codes.
    b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff\x21\xf9\x04\x01\0\0\0\0\x2c\0\0\0\0\x01\0\x01\0\0\x02\x02\x4c\x01\0\x3b".to_vec()
}
pub fn wav() -> Vec<u8> {
    let mut format = Vec::new();
    for value in [1u16, 1] {
        format.extend_from_slice(&value.to_le_bytes());
    }
    format.extend_from_slice(&8000u32.to_le_bytes());
    format.extend_from_slice(&16000u32.to_le_bytes());
    format.extend_from_slice(&2u16.to_le_bytes());
    format.extend_from_slice(&16u16.to_le_bytes());
    let mut body = b"WAVE".to_vec();
    // Odd-sized ancillary metadata exercises RIFF padding.
    body.extend(chunk(b"JUNK", b"owned", true));
    body.extend(chunk(b"fmt ", &format, true));
    body.extend(chunk(b"data", &[0; 160], true));
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}
pub fn pdf() -> Vec<u8> {
    pdf_with(b"/Producer (Meguri fixture)")
}
pub fn pdf_with(extra: &[u8]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = vec![0];
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents 4 0 R >>"
            .to_vec(),
        b"<< /Length 4 >>\nstream\nq\nQ\nendstream".to_vec(),
        [b"<< ".as_slice(), extra, b" >>"].concat(),
    ];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    out
}
pub fn aiff(compressed: bool) -> Vec<u8> {
    let mut comm = vec![
        0, 1, 0, 0, 0, 80, 0, 16, 0x40, 0x0b, 0xfa, 0, 0, 0, 0, 0, 0, 0,
    ];
    if compressed {
        comm.extend_from_slice(b"NONE\x0enot compressed");
    }
    let mut body = if compressed {
        b"AIFC".to_vec()
    } else {
        b"AIFF".to_vec()
    };
    if compressed {
        body.extend(chunk(b"FVER", &0xa2805140u32.to_be_bytes(), false));
    }
    body.extend(chunk(b"COMM", &comm, false));
    body.extend(chunk(b"SSND", &[0; 168], false));
    let mut out = b"FORM".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend(body);
    out
}
pub fn caf() -> Vec<u8> {
    let mut out = b"caff\0\x01\0\0desc".to_vec();
    out.extend_from_slice(&32u64.to_be_bytes());
    out.extend_from_slice(&8000f64.to_be_bytes());
    out.extend_from_slice(b"lpcm");
    for value in [2u32, 2, 1, 1, 16] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&164u64.to_be_bytes());
    out.extend_from_slice(&[0; 164]);
    out
}
pub fn adts_frame(payload: &[u8]) -> Vec<u8> {
    let size = 7 + payload.len();
    assert!(size > 7 && size <= 0x1fff);
    let mut out = vec![
        0xff,
        0xf1,
        0x50,
        0x40 | ((size >> 11) as u8),
        (size >> 3) as u8,
        ((size & 7) << 5) as u8 | 0x1f,
        0xfc,
    ];
    out.extend_from_slice(payload);
    out
}
pub fn adts() -> Vec<u8> {
    // Two 128-byte AAC-LC mono frames, above the matrix's short-prefix range.
    // Payload is deliberately opaque; encode the actual length in each header.
    let frame = adts_frame(&[0; 121]);
    [frame.as_slice(), frame.as_slice()].concat()
}
pub fn flac() -> Vec<u8> {
    let mut out = b"fLaC\x80\0\0\x22".to_vec();
    let mut info = [0u8; 34];
    info[..4].copy_from_slice(&[0, 16, 0, 16]);
    info[10..18].copy_from_slice(&((8000u64 << 44) | (15u64 << 36) | 16).to_be_bytes());
    out.extend_from_slice(&info);
    // Constant subframe, 16 samples, mono 16-bit, explicit block size, CRCs.
    let mut frame = vec![0xff, 0xf8, 0x64, 0x08, 0, 15];
    let mut crc = 0u8;
    for &byte in &frame {
        crc ^= byte;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 128 != 0 { 7 } else { 0 };
        }
    }
    frame.push(crc);
    frame.extend_from_slice(&[0, 0, 0]);
    let mut crc = 0u16;
    for &byte in &frame {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x8000 != 0 { 0x8005 } else { 0 };
        }
    }
    frame.extend_from_slice(&crc.to_be_bytes());
    out.extend(frame);
    out
}
pub fn ogg(opus: bool) -> Vec<u8> {
    let head = if opus {
        b"OpusHead\x01\x01\0\0\x80\xbb\0\0\0\0\0".to_vec()
    } else {
        let mut p = b"\x01vorbis\0\0\0\0\x01".to_vec();
        p.extend_from_slice(&8000u32.to_le_bytes());
        p.extend_from_slice(&[0; 12]);
        p.extend_from_slice(&[0x86, 1]);
        p
    };
    let tags = if opus {
        b"OpusTags\0\0\0\0\0\0\0\0".to_vec()
    } else {
        b"\x03vorbis\0\0\0\0\0\0\0\0\x01".to_vec()
    };
    let packets = [
        head,
        tags,
        if opus {
            vec![0xf8, 0xff, 0xfe]
        } else {
            b"\x05vorbis\0".to_vec()
        },
        vec![0, 0],
    ];
    let mut out = Vec::new();
    for (seq, payload) in packets.iter().enumerate() {
        let mut page = b"OggS\0".to_vec();
        page.push(if seq == 0 {
            2
        } else if seq == 3 {
            4
        } else {
            0
        });
        page.extend_from_slice(&(if seq == 3 { 960u64 } else { 0 }).to_le_bytes());
        page.extend_from_slice(&17u32.to_le_bytes());
        page.extend_from_slice(&(seq as u32).to_le_bytes());
        page.extend_from_slice(&[0; 4]);
        page.push(1);
        page.push(payload.len() as u8);
        page.extend(payload);
        let mut crc = 0u32;
        for &byte in &page {
            crc ^= u32::from(byte) << 24;
            for _ in 0..8 {
                crc = (crc << 1)
                    ^ if crc & 0x8000_0000 != 0 {
                        0x04c1_1db7
                    } else {
                        0
                    };
            }
        }
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        out.extend(page);
    }
    out
}
pub fn iso_box(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = ((data.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out
}
pub fn m4a(video: bool) -> Vec<u8> {
    let mut out = iso_box(b"ftyp", b"M4A \0\0\0\0M4A isom");
    out.extend(iso_box(b"free", &[0; 5]));
    let mut hdlr = vec![0; 8];
    hdlr.extend_from_slice(if video { b"vide" } else { b"soun" });
    hdlr.extend_from_slice(&[0; 13]);
    let mut entry = vec![0; 28];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes());
    entry[16..18].copy_from_slice(&1u16.to_be_bytes());
    entry[18..20].copy_from_slice(&16u16.to_be_bytes());
    entry[24..28].copy_from_slice(&(8000u32 << 16).to_be_bytes());
    let mut stsd = vec![0; 4];
    stsd.extend_from_slice(&1u32.to_be_bytes());
    stsd.extend(iso_box(b"mp4a", &entry));
    let mut mdia = iso_box(b"hdlr", &hdlr);
    mdia.extend(iso_box(
        b"minf",
        &iso_box(b"stbl", &iso_box(b"stsd", &stsd)),
    ));
    out.extend(iso_box(
        b"moov",
        &iso_box(b"trak", &iso_box(b"mdia", &mdia)),
    ));
    out.extend(iso_box(b"mdat", &[0; 16]));
    out
}
pub fn heif(brand: &[u8; 4]) -> Vec<u8> {
    let avif = brand == b"avif";
    let mut ftyp = brand.to_vec();
    ftyp.extend_from_slice(&[0; 4]);
    ftyp.extend_from_slice(b"mif1");
    let mut out = iso_box(b"ftyp", &ftyp);
    let mut infe = vec![2, 0, 0, 0, 0, 1, 0, 0];
    infe.extend_from_slice(if avif { b"av01" } else { b"hvc1" });
    infe.push(0);
    let mut iinf = vec![0, 0, 0, 0, 0, 1];
    iinf.extend(iso_box(b"infe", &infe));
    let mut props = iso_box(b"ispe", &[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1]);
    let mut config = vec![0; if avif { 4 } else { 23 }];
    config[0] = if avif { 0x81 } else { 1 };
    props.extend(iso_box(if avif { b"av1C" } else { b"hvcC" }, &config));
    let mut meta = vec![0; 4];
    meta.extend(iso_box(b"pitm", &[0, 0, 0, 0, 0, 1]));
    meta.extend(iso_box(
        b"iloc",
        &[
            0, 0, 0, 0, 0x44, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 16,
        ],
    ));
    meta.extend(iso_box(b"iinf", &iinf));
    meta.extend(iso_box(b"iprp", &iso_box(b"ipco", &props)));
    out.extend(iso_box(b"meta", &meta));
    let offset = (out.len() + 8) as u32;
    let iloc = out.windows(4).position(|v| v == b"iloc").unwrap() + 4;
    out[iloc + 14..iloc + 18].copy_from_slice(&offset.to_be_bytes());
    out.extend(iso_box(b"mdat", &[0; 16]));
    out
}
pub fn webp() -> Vec<u8> {
    // Lossless header plus opaque coded data, and padded metadata.
    let mut body = b"WEBP".to_vec();
    body.extend(chunk(b"VP8L", &[0x2f, 0, 0, 0, 0, 0x88, 0x88, 8], true));
    body.extend(chunk(b"EXIF", b"fixture", true));
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}
