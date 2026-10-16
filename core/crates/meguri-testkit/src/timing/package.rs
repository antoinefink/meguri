//! The import package (T5.4): a ZIP archive of two stored entries,
//! `collection.anki2` holding the image's bytes and `media` holding `{}`,
//! in that order. Entry times and the other archive metadata are free.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

/// The two bytes of the `media` entry: an empty JSON object.
pub const MEDIA: &[u8] = b"{}";

/// Writes the T5.4 package of `image` to `out`.
pub fn write_import_package(image: &Path, out: &Path) -> io::Result<()> {
    let (size, crc) = {
        let mut crc = Crc32::new();
        let mut size = 0u64;
        let mut file = File::open(image)?;
        let mut buffer = vec![0u8; 1 << 16];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            crc.update(&buffer[..read]);
            size += read as u64;
        }
        (size, crc.finish())
    };
    let mut zip = ZipWriter::new(BufWriter::new(File::create(out)?));
    zip.entry("collection.anki2", size, crc, &mut File::open(image)?)?;
    zip.entry("media", MEDIA.len() as u64, crc32(MEDIA), &mut &MEDIA[..])?;
    zip.finish()?.flush()
}

/// A ZIP writer for stored entries (no compression, no data descriptor,
/// no ZIP64).
struct ZipWriter<W: Write> {
    out: W,
    written: u64,
    central: Vec<u8>,
    entries: u16,
}

/// Version 2.0 for "version made by" and "version needed to extract".
const VERSION: u16 = 20;
/// The MS-DOS date of 1980-01-01, the format's earliest; time 00:00.
const DOS_DATE: u16 = (1 << 5) | 1;

impl<W: Write> ZipWriter<W> {
    fn new(out: W) -> ZipWriter<W> {
        ZipWriter {
            out,
            written: 0,
            central: Vec::new(),
            entries: 0,
        }
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    /// One stored entry of `size` bytes read from `data`.
    fn entry(&mut self, name: &str, size: u64, crc: u32, data: &mut impl Read) -> io::Result<()> {
        let size32 = u32::try_from(size).map_err(|_| too_large(name))?;
        let offset = u32::try_from(self.written).map_err(|_| too_large(name))?;
        let name_len = u16::try_from(name.len()).map_err(|_| too_large(name))?;

        let mut local = Vec::with_capacity(30 + name.len());
        local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local.extend_from_slice(&VERSION.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes()); // flags
        local.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        local.extend_from_slice(&0u16.to_le_bytes()); // time
        local.extend_from_slice(&DOS_DATE.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&size32.to_le_bytes()); // compressed size
        local.extend_from_slice(&size32.to_le_bytes()); // uncompressed size
        local.extend_from_slice(&name_len.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        local.extend_from_slice(name.as_bytes());
        self.put(&local)?;
        let copied = io::copy(&mut data.take(size), &mut self.out)?;
        if copied != size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("{name}: {copied} of {size} bytes"),
            ));
        }
        self.written += copied;

        let c = &mut self.central;
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&VERSION.to_le_bytes()); // made by
        c.extend_from_slice(&VERSION.to_le_bytes()); // needed
        c.extend_from_slice(&0u16.to_le_bytes()); // flags
        c.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        c.extend_from_slice(&0u16.to_le_bytes()); // time
        c.extend_from_slice(&DOS_DATE.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&size32.to_le_bytes());
        c.extend_from_slice(&size32.to_le_bytes());
        c.extend_from_slice(&name_len.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        c.extend_from_slice(&0u16.to_le_bytes()); // comment length
        c.extend_from_slice(&0u16.to_le_bytes()); // disk number start
        c.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        c.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(name.as_bytes());
        self.entries += 1;
        Ok(())
    }

    /// The central directory and its end record.
    fn finish(mut self) -> io::Result<W> {
        let offset = u32::try_from(self.written).map_err(|_| too_large("the archive"))?;
        let size = u32::try_from(self.central.len()).map_err(|_| too_large("the archive"))?;
        let central = std::mem::take(&mut self.central);
        self.put(&central)?;
        let mut end = Vec::with_capacity(22);
        end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes()); // this disk
        end.extend_from_slice(&0u16.to_le_bytes()); // disk of the central directory
        end.extend_from_slice(&self.entries.to_le_bytes());
        end.extend_from_slice(&self.entries.to_le_bytes());
        end.extend_from_slice(&size.to_le_bytes());
        end.extend_from_slice(&offset.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes()); // comment length
        self.put(&end)?;
        Ok(self.out)
    }
}

fn too_large(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{what} is over 4 GiB, which a ZIP without ZIP64 cannot hold"),
    )
}

/// CRC-32 (ISO-HDLC: reflected polynomial 0xEDB88320, the one ZIP uses).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.finish()
}

struct Crc32(u32);

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

impl Crc32 {
    fn new() -> Crc32 {
        Crc32(0xFFFF_FFFF)
    }

    fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = CRC_TABLE[((self.0 ^ u32::from(b)) & 0xFF) as usize] ^ (self.0 >> 8);
        }
    }

    fn finish(&self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}
