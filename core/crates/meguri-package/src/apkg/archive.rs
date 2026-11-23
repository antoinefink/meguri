//! A deck package's archive in its three formats (`package/container` P1 to
//! P8, P17): which format, the collection member, the media map and the
//! media payloads. Every read is bounded and streams; no member name ever
//! becomes a path.
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::inspection_zip::{self, NeededArchive};
use crate::{Error, Result, cancelled, corrupt, media_map, reader};

const META_LIMIT: u64 = 64 * 1024;
const MAP_LIMIT: u64 = 64 * 1024 * 1024;
const COLLECTION_LIMIT: u64 = 8 * 1024 * 1024 * 1024;
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// P1's formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageFormat {
    Latest,
    Legacy2,
    Legacy1,
}

impl PackageFormat {
    pub fn name(self) -> &'static str {
        match self {
            PackageFormat::Latest => "latest",
            PackageFormat::Legacy2 => "legacy2",
            PackageFormat::Legacy1 => "legacy1",
        }
    }
}

/// One media map entry, in map order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MediaEntry {
    /// The name as the map gives it (latest, safe by P6) or as F1 keeps it
    /// (legacy, P17).
    pub name: String,
    /// The zip member that holds the payload.
    pub member: u32,
    /// The declared size: the map's (latest) or the member's uncompressed
    /// size in the zip directory (legacy); 0 for a member the archive lacks.
    pub size: u64,
    /// The map's SHA-1 (latest only).
    pub sha1: Option<[u8; 20]>,
}

pub(crate) struct DeckArchive {
    zip: NeededArchive,
    pub format: PackageFormat,
    collection: usize,
    /// Decoded bytes read so far, against P8's 16 GiB.
    total: u64,
}

impl DeckArchive {
    /// P2, P3 and the choice of the collection member.
    /// `charged` is what earlier reads of this package already decoded: P8's
    /// 16 GiB covers every entry read from one package, across reopenings.
    pub fn open(path: &Path, check: &(dyn Fn() -> bool + Sync), charged: u64) -> Result<DeckArchive> {
        let mut zip = inspection_zip::open_with_legacy(File::open(path)?, check)?;
        let mut total = charged;
        let version = match zip.meta {
            Some(index) => {
                let member = zip.archive.by_index(index)?;
                let mut bytes = Vec::new();
                reader::bounded_read(member, META_LIMIT, &mut total, check, |part| bytes.extend_from_slice(part))?;
                Some(reader::meta_version(&bytes)?)
            }
            None => None,
        };
        let format = match version {
            Some(3) => PackageFormat::Latest,
            Some(2) => PackageFormat::Legacy2,
            Some(1) => PackageFormat::Legacy1,
            Some(_) => return Err(Error::TooNew),
            None if zip.legacy21.is_some() => PackageFormat::Legacy2,
            None => PackageFormat::Legacy1,
        };
        let collection = match format {
            PackageFormat::Latest => zip.collection.ok_or_else(|| corrupt("missing collection.anki21b"))?,
            PackageFormat::Legacy2 => zip
                .legacy21
                .take()
                .ok_or_else(|| corrupt("missing collection.anki21"))??,
            PackageFormat::Legacy1 => zip
                .legacy2
                .take()
                .ok_or_else(|| corrupt("missing collection.anki2"))??,
        };
        Ok(DeckArchive {
            zip,
            format,
            collection,
            total,
        })
    }

    /// Decoded bytes charged so far against P8's total.
    pub fn charged(&self) -> u64 {
        self.total
    }

    /// P10's E for the extraction preflight: what the first zstd frame
    /// declares, else the larger of four times the member and `live` (latest);
    /// the member's uncompressed size in the directory (legacy).
    pub fn collection_estimate(&mut self, live: u64) -> Result<u64> {
        let (compressed, declared) = self.zip.sizes[self.collection];
        if self.format != PackageFormat::Latest {
            return Ok(declared);
        }
        let mut member = self.zip.archive.by_index(self.collection)?;
        let mut header = [0; 18];
        let mut filled = 0;
        while filled < header.len() {
            let read = member.read(&mut header[filled..]).map_err(reader::decode_error)?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        let known = if header[..filled].starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            zstd::zstd_safe::get_frame_content_size(&header[..filled]).map_err(|_| corrupt("zstd first-frame header"))?
        } else {
            None
        };
        Ok(match known {
            Some(size) => size,
            None => compressed.saturating_mul(4).max(live),
        })
    }

    /// Streams the collection member into `output` within P8's limits.
    pub fn extract_collection(&mut self, output: &mut File, check: &(dyn Fn() -> bool + Sync)) -> Result<u64> {
        let (compressed, _) = self.zip.sizes[self.collection];
        let limit = reader::decoded_limit(compressed, COLLECTION_LIMIT);
        let member = self.zip.archive.by_index(self.collection)?;
        let total = &mut self.total;
        let written = if self.format == PackageFormat::Latest {
            reader::zstd_read_fallible(member, limit, total, check, |part| {
                meguri_model::faultpoint!("package.inspect.write")?;
                output.write_all(part).map_err(Into::into)
            })?
        } else {
            let mut seen = 0_usize;
            let mut head = [0_u8; 16];
            let length = reader::bounded_read_fallible(member, limit, total, check, |part| {
                if seen < head.len() {
                    let take = part.len().min(head.len() - seen);
                    head[seen..seen + take].copy_from_slice(&part[..take]);
                    seen += take;
                    if seen == head.len() && &head != SQLITE_HEADER {
                        return Err(corrupt("legacy collection is not a SQLite file"));
                    }
                }
                meguri_model::faultpoint!("package.inspect.write")?;
                output.write_all(part).map_err(Into::into)
            })?;
            if seen < head.len() {
                return Err(corrupt("legacy collection is not a SQLite file"));
            }
            length
        };
        output.flush()?;
        Ok(written)
    }

    /// P5 with P6 (latest) or P17's JSON map (legacy), in map order.
    pub fn media_map(&mut self, check: &(dyn Fn() -> bool + Sync)) -> Result<Vec<MediaEntry>> {
        let Some(index) = self.zip.media else {
            return Ok(Vec::new());
        };
        let (compressed, _) = self.zip.sizes[index];
        let limit = reader::decoded_limit(compressed, MAP_LIMIT);
        let member = self.zip.archive.by_index(index)?;
        let mut bytes = Vec::new();
        if self.format == PackageFormat::Latest {
            reader::zstd_read(member, limit, &mut self.total, check, |part| bytes.extend_from_slice(part))?;
            let mut entries = Vec::new();
            media_map::records(&bytes, check, |record| {
                if entries.len() >= 400_000 {
                    return Err(corrupt("media map entry bound"));
                }
                let mut sha1 = [0; 20];
                sha1.copy_from_slice(record.sha1);
                entries.push(MediaEntry {
                    name: record.name.to_owned(),
                    member: entries.len() as u32,
                    size: u64::from(record.length),
                    sha1: Some(sha1),
                });
                Ok(())
            })?;
            Ok(entries)
        } else {
            // P17: raw JSON, at most P8's 64 MiB whatever the zip method.
            reader::bounded_read(member, MAP_LIMIT, &mut self.total, check, |part| {
                bytes.extend_from_slice(part)
            })?;
            let mut entries = legacy_map(&bytes, check)?;
            for entry in &mut entries {
                if let Some(index) = self.zip.payloads.get(&entry.member) {
                    entry.size = self.zip.sizes[*index].1;
                }
            }
            Ok(entries)
        }
    }

    /// Decodes one payload into `output`, at most to its declared size
    /// (`package/apkg-import` I12.4), and returns its length and SHA-1. A
    /// latest-format payload must have the map's size and SHA-1.
    pub fn read_payload(
        &mut self,
        entry: &MediaEntry,
        check: &(dyn Fn() -> bool + Sync),
        mut output: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<(u64, [u8; 20])> {
        if let Some(fault) = self.zip.bad_payloads.get(&entry.member) {
            return Err(fault.clone());
        }
        let index = *self
            .zip
            .payloads
            .get(&entry.member)
            .ok_or_else(|| corrupt("missing media payload"))?;
        let member = self.zip.archive.by_index(index)?;
        let mut digest = Sha1::new();
        let consume = |part: &[u8]| {
            digest.update(part);
            output(part)
        };
        let length = if self.format == PackageFormat::Latest {
            reader::zstd_read_fallible(member, entry.size, &mut self.total, check, consume)?
        } else {
            reader::bounded_read_fallible(member, entry.size, &mut self.total, check, consume)?
        };
        let sha1: [u8; 20] = digest.finalize().into();
        if let Some(expected) = entry.sha1
            && (length != entry.size || sha1 != expected)
        {
            return Err(corrupt("media length or SHA-1 mismatch"));
        }
        Ok((length, sha1))
    }
}

/// P17's media map: one JSON object from member numbers to names.
fn legacy_map(bytes: &[u8], check: &(dyn Fn() -> bool + Sync)) -> Result<Vec<MediaEntry>> {
    struct Pairs(Vec<(String, String)>);
    impl<'de> serde::Deserialize<'de> for Pairs {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
            struct Visit;
            impl<'de> serde::de::Visitor<'de> for Visit {
                type Value = Pairs;
                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("an object of member numbers and file names")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Pairs, A::Error> {
                    let mut pairs = Vec::new();
                    while let Some(pair) = map.next_entry::<String, String>()? {
                        if pairs.len() >= 400_000 {
                            return Err(serde::de::Error::custom("too many entries"));
                        }
                        pairs.push(pair);
                    }
                    Ok(Pairs(pairs))
                }
            }
            deserializer.deserialize_map(Visit)
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| corrupt("legacy media map is not UTF-8"))?;
    let Pairs(pairs) = serde_json::from_str(text).map_err(|_| corrupt("legacy media map is not a JSON object"))?;
    let mut members = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut entries = Vec::with_capacity(pairs.len());
    for (key, name) in pairs {
        cancelled(check)?;
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
            return Err(corrupt("legacy media map key"));
        }
        // Leading zeros are allowed; a number past any member count names
        // no member and can never be read.
        let digits = key.trim_start_matches('0');
        let member: u32 = if digits.is_empty() {
            0
        } else {
            digits.parse().map_err(|_| corrupt("legacy media map key"))?
        };
        if !members.insert(member) {
            return Err(corrupt("legacy media map repeats a member"));
        }
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(corrupt("unsafe legacy media name"));
        }
        let kept = meguri_text::media_name::safe_media_name(&name);
        if kept.is_empty() {
            return Err(corrupt("unsafe legacy media name"));
        }
        if !names.insert(kept.clone()) {
            return Err(corrupt("legacy media names repeat"));
        }
        entries.push(MediaEntry {
            name: kept,
            member,
            size: 0,
            sha1: None,
        });
    }
    entries.sort_by_key(|entry| entry.member);
    Ok(entries)
}
