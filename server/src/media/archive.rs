// SPDX-License-Identifier: MIT
//! Untrusted ZIP structure, descriptors and EOF verification belong to rawzip.
//! Application bounds use fixed buffers and the same selected metadata extents;
//! zip is retained only for the download writer.
use super::{blobs::{self, Content, Temporary, invalid}, name};
use crate::{DataRoot, Error, Result, limits::TempBudget};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::{collections::{BTreeMap,BTreeSet}, fs::File, io::{Read,Write}, os::unix::fs::FileExt};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};
const META:u64=50*1024;
const ENTRY:u64=104_857_600;
const DIRECTORY:u64=128*1024;
const BUFFER:usize=65536;
const SEARCH:u64=22+65535;
const FILL:u64=2_621_440;

struct Member { size:u64, file:Option<Temporary>, sha1:String, sha256:Vec<u8> }
struct Directory { start:u64, end:u64, count:u64 }

fn field16(bytes:&[u8], at:usize)->u16 {
    u16::from_le_bytes([bytes[at],bytes[at+1]])
}
fn field32(bytes:&[u8], at:usize)->u32 {
    u32::from_le_bytes([bytes[at],bytes[at+1],bytes[at+2],bytes[at+3]])
}
fn fixed<const N:usize>(file:&File, at:u64, end:u64)->Result<[u8;N]> {
    if at.checked_add(N as u64).is_none_or(|n|n>end) {return Err(invalid());}
    let mut bytes=[0;N];
    file.read_exact_at(&mut bytes,at).map_err(|_|invalid())?;
    Ok(bytes)
}
/// This is an extent check, not another locator or ZIP decoder. Read only the
/// classic footer rawzip already selected. ZIP64/multi-disk end forms are not
/// supported; in particular no untrusted extensible field is copied/allocated.
fn directory(file:&File, length:u64, footer:u64, selected:u64, hint:u64)->Result<Directory> {
    let bytes=fixed::<22>(file,footer,length)?;
    let count=u64::from(field16(&bytes,10));
    let size=u64::from(field32(&bytes,12));
    let start=u64::from(field32(&bytes,16));
    if field32(&bytes,0)!=0x0605_4b50 || field16(&bytes,4)!=0 || field16(&bytes,6)!=0
        || u64::from(field16(&bytes,8))!=count || count==0 || count>26 || hint!=count
        || size>DIRECTORY || size==u64::from(u32::MAX) || start==u64::from(u32::MAX)
        || start!=selected || start.checked_add(size)!=Some(footer)
        || footer.checked_add(22+u64::from(field16(&bytes,20)))!=Some(length) {
        return Err(invalid());
    }
    Ok(Directory {start,end:footer,count})
}
/// Account the extent of the record selected by the lending iterator. The
/// library owns its fields/extra decoding; only three fixed length fields are
/// read here because its public archive API does not expose directory size.
fn record_end(file:&File, at:u64, end:u64)->Result<u64> {
    let bytes=fixed::<46>(file,at,end)?;
    let size=46+u64::from(field16(&bytes,28))+u64::from(field16(&bytes,30))+u64::from(field16(&bytes,32));
    let next=at.checked_add(size).filter(|n|*n<=end).ok_or_else(invalid)?;
    // Only the observed classic header profile plus descriptors/DEFLATE is
    // supported: no spanning, ZIP64 or extra-field-dependent interpretation.
    if field16(&bytes,6)>20 || field16(&bytes,30)!=0 || field16(&bytes,34)!=0 || field32(&bytes,20)==u32::MAX
        || field32(&bytes,24)==u32::MAX || field32(&bytes,42)==u32::MAX {
        return Err(invalid());
    }
    Ok(next)
}

/// A fixed-input-buffer adapter over flate2's raw DEFLATE state machine. Require
/// StreamEnd as well as the exact compressed extent: output CRC/size alone can
/// miss a truncated final block or unused compressed suffix. rawzip's verifier
/// still owns the decoded CRC/size check.
struct Inflate<R> {
    reader:R,
    decoder:flate2::Decompress,
    input:[u8;BUFFER],
    start:usize,
    end:usize,
    expected:u64,
    eof:bool,
    done:bool,
}
impl<R:Read> Inflate<R> {
    fn new(reader:R, expected:u64)->Self {
        Self {reader,decoder:flate2::Decompress::new(false),input:[0;BUFFER],start:0,end:0,expected,eof:false,done:false}
    }
}
impl<R:Read> Read for Inflate<R> {
    fn read(&mut self, output:&mut [u8])->std::io::Result<usize> {
        if output.is_empty() || self.done {return Ok(0);}
        loop {
            if self.start==self.end && !self.eof {
                self.end=self.reader.read(&mut self.input)?;
                self.start=0;
                self.eof=self.end==0;
            }
            let before_in=self.decoder.total_in(); let before_out=self.decoder.total_out();
            let flush=if self.eof {flate2::FlushDecompress::Finish} else {flate2::FlushDecompress::None};
            let status=self.decoder.decompress(&self.input[self.start..self.end],output,flush)
                .map_err(|_|std::io::Error::other("invalid DEFLATE data"))?;
            let consumed=(self.decoder.total_in()-before_in) as usize;
            let written=(self.decoder.total_out()-before_out) as usize;
            self.start+=consumed;
            if status==flate2::Status::StreamEnd {
                if self.decoder.total_in()!=self.expected {return Err(std::io::Error::other("DEFLATE extent mismatch"));}
                self.done=true;
                return Ok(written);
            }
            if written!=0 {return Ok(written);}
            if consumed==0 && (self.eof || self.start!=self.end) {
                return Err(std::io::Error::other("truncated DEFLATE stream"));
            }
        }
    }
}

fn stage(reader:&mut dyn Read, declared:u64, maximum:u64, remaining:u64, mut file:Option<Temporary>)->Result<(Member,Vec<u8>)> {
    let mut metadata=if file.is_none() {Vec::with_capacity(META as usize)} else {Vec::new()};
    let mut actual=0u64; let mut block=[0;BUFFER];
    let mut sha1=Sha1::new(); let mut sha256=Sha256::new();
    loop {
        let bound=maximum.min(declared).min(remaining);
        let max=(bound.saturating_sub(actual).min((BUFFER-1) as u64)+1) as usize;
        // Reading through zero is essential: the library verifies CRC/size at
        // EOF, including a zero-length _meta. Hints never size an allocation.
        let n=reader.read(&mut block[..max]).map_err(|_|invalid())?;
        if n==0 {break;}
        if n as u64>bound.saturating_sub(actual) {return Err(invalid());}
        actual+=n as u64;
        if let Some(file)=file.as_mut() {file.write_all(&block[..n])?;}
        else {metadata.extend_from_slice(&block[..n]);}
        sha1.update(&block[..n]); sha256.update(&block[..n]);
    }
    if actual!=declared {return Err(invalid());}
    Ok((Member {size:actual,file,sha1:blobs::hex(&sha1.finalize()),sha256:sha256.finalize().to_vec()},metadata))
}

/// Bind once before streaming. A reset can unlink this directory, but no later
/// member of this upload can resolve or write into the recreated account store.
pub(crate) fn receive_in(reader:Box<dyn Read+Send>,limit:u64,media_directory:File,budget:TempBudget)->Result<Vec<(String,Option<Content>)>> {
    let mut spool=Temporary::create_in(media_directory.try_clone()?,budget.clone())?;
    crate::protocol::framing::decode(reader,&mut spool,limit)?;
    spool.rewind()?;
    let length=spool.file.metadata()?.len();
    // rawzip's file locator builds no entry index. Search is independently
    // bounded; a false candidate is rejected without an unbounded retry loop.
    let mut index_buffer=[0;BUFFER]; let mut local_buffer=[0;BUFFER];
    let archive=rawzip::ZipLocator::new().max_search_space(SEARCH)
        .locate_in_file(spool.file.try_clone()?,&mut index_buffer).map_err(|_|invalid())?;
    let directory=directory(&spool.file,length,archive.eocd_offset(),archive.directory_offset(),archive.entries_hint())?;
    let mut entries=archive.entries(&mut index_buffer);
    let mut members=BTreeMap::new(); let mut meta=None; let mut expanded=0u64;
    let mut next_record=directory.start; let mut extents=Vec::new();
    let mut local_metadata=0u64;
    // next_entry errors are terminal errors, never normal exhaustion. At most
    // 26 admitted records and one bounded lookahead are examined.
    while let Some(header)=entries.next_entry().map_err(|_|invalid())? {
        if members.len() as u64>=directory.count || members.len()>=26
            || header.central_directory_offset()!=next_record {return Err(invalid());}
        next_record=record_end(&spool.file,next_record,directory.end)?;
        // Copy only a bounded account-independent member identifier, never a
        // full attacker-sized path. Logical UTF-8/NFC names live solely in meta.
        let path=header.file_path(); let bytes=path.as_ref();
        if bytes.len()>5 {return Err(invalid());}
        let key=std::str::from_utf8(bytes).map_err(|_|invalid())?.to_owned();
        if (key!="_meta" && !decimal(&key)) || members.contains_key(&key) {return Err(invalid());}
        let flags=header.flags().bits(); let method=header.compression_method();
        let crc=header.crc32(); let declared=header.uncompressed_size_hint();
        let compressed_size=header.compressed_size_hint();
        let offset=header.local_header_offset();
        let kind=(header.external_attributes()>>16)&0o170000;
        let maximum=if key=="_meta" {META} else {ENTRY};
        if flags & !0x080e !=0 || header.is_dir() || (kind!=0 && kind!=0o100000)
            || header.external_attributes()&0x10!=0 || declared>maximum || (key!="_meta" && declared==0)
            || !matches!(method,rawzip::CompressionMethod::STORE|rawzip::CompressionMethod::DEFLATE)
            || (method==rawzip::CompressionMethod::STORE && (flags&6!=0 || compressed_size!=declared)) {
            return Err(invalid());
        }
        if declared>limit.saturating_sub(expanded) {return Err(Error::new("request_too_large"));}
        let entry=archive.get_entry(header.wayfinder()).map_err(|_|invalid())?;
        let (start,end)=entry.compressed_data_range();
        if offset.checked_add(30+key.len() as u64)!=Some(start)
            || end<start || end-start!=compressed_size || end>directory.start {return Err(invalid());}
        // Checking the supported version at the library-selected local header
        // does not locate or parse another member. No local extras are admitted.
        let version=fixed::<6>(&spool.file,offset,start)?;
        if field16(&version,4)>20 {return Err(invalid());}
        local_metadata=local_metadata.checked_add(start-offset).filter(|n|*n<=DIRECTORY).ok_or_else(invalid)?;
        let local=entry.local_header(&mut local_buffer).map_err(|_|invalid())?;
        if local.file_path().as_ref()!=key.as_bytes() || local.flags().bits()!=flags || local.compression_method()!=method {
            return Err(invalid());
        }
        let descriptor=flags&8!=0;
        let local_size=local.uncompressed_size_hint(); let local_compressed=local.compressed_size_hint();
        if (!descriptor && (local.crc32()!=crc || local_size!=declared || local_compressed!=compressed_size))
            || (descriptor && ((local.crc32()!=0 && local.crc32()!=crc)
                || (local_size!=0 && local_size!=declared) || (local_compressed!=0 && local_compressed!=compressed_size))) {
            return Err(invalid());
        }
        // The local header has been bounded and compared before any decoding.
        let file=if key=="_meta" {None} else {Some(Temporary::create_in(media_directory.try_clone()?,budget.clone())?)};
        let mut compressed=entry.reader();
        let (member,metadata)=if method==rawzip::CompressionMethod::STORE {
            stage(&mut entry.verifying_reader(&mut compressed),declared,maximum,limit-expanded,file)?
        } else {
            let decoder=Inflate::new(&mut compressed,compressed_size);
            stage(&mut entry.verifying_reader(decoder),declared,maximum,limit-expanded,file)?
        };
        let parsed=compressed.data_descriptor().map_err(|_|invalid())?;
        let extent_end=if descriptor {
            let parsed=parsed.ok_or_else(invalid)?;
            if parsed.crc32()!=crc || parsed.compressed_size()!=compressed_size || parsed.uncompressed_size()!=declared {return Err(invalid());}
            // rawzip parses descriptor values. This four-byte check accounts
            // only for its classic signed/unsigned extent at the known end of
            // compressed data; it never scans or restricts media payload bytes.
            let prefix=fixed::<4>(&spool.file,end,directory.start)?;
            let size=if field32(&prefix,0)==0x0807_4b50 {16} else {12};
            end.checked_add(size).filter(|n|*n<=directory.start).ok_or_else(invalid)?
        } else {
            if parsed.is_some() {return Err(invalid());}
            end
        };
        extents.push((offset,extent_end));
        expanded+=member.size;
        if key=="_meta" {meta=Some(metadata);}
        members.insert(key,member);
    }
    if members.len() as u64!=directory.count || next_record!=directory.end {return Err(invalid());}
    extents.sort_unstable();
    let mut end=0;
    for (start,next) in extents {
        // Exact coverage rejects overlap, hidden/unindexed members and padding.
        if start!=end || next<start {return Err(invalid());}
        end=next;
    }
    if end!=directory.start {return Err(invalid());}
    let meta=meta.ok_or_else(invalid)?;
    let changes:Vec<(String,Option<String>)>=serde_json::from_slice(&meta).map_err(|_|invalid())?;
    if changes.len()>25 {return Err(invalid());}
    let mut used=BTreeSet::new();
    for (position,(real,entry)) in changes.iter().enumerate() {
        name(real)?;
        if let Some(key)=entry {
            // Numeric positions include deletions; aliases such as 00 do not
            // establish a second mapping for an existing member.
            if *key!=position.to_string() || !used.insert(key.clone()) || !members.contains_key(key) {return Err(invalid());}
        }
    }
    if members.len()!=used.len()+1 {return Err(invalid());}
    // No blob is published before every byte, CRC, name and mapping validates.
    let mut result=Vec::with_capacity(changes.len());
    for (real,key) in changes {
        let content=if let Some(key)=key {
            let member=members.remove(&key).ok_or_else(invalid)?;
            let blob=member.file.ok_or_else(invalid)?.publish()?;
            Some(Content {blob,size:member.size,sha1:member.sha1,sha256:member.sha256})
        } else {None};
        result.push((real,content));
    }
    crate::accounts::phase("media_after_blob_durability")?;
    Ok(result)
}
fn decimal(value:&str)->bool {
    !value.is_empty() && value.len()<=2 && value.bytes().all(|b|b.is_ascii_digit()) && (value=="0" || !value.starts_with('0'))
}

pub(crate) fn download(root:&DataRoot,id:&str,budget:TempBudget,requested:Vec<(String,Content)>)->Result<Temporary> {
    let mut opened=Vec::with_capacity(requested.len());
    // Check the entire request, including beyond the returned fill prefix.
    for (name,content) in requested { opened.push((name,root.media_blob(id,&content.blob)?,content)); }
    let mut output=Temporary::create(root,id,budget)?;
    let mut meta=BTreeMap::new(); let mut size=0u64;
    {
        let mut archive=ZipWriter::new(&mut output);
        let options=SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (index,(real,mut file,content)) in opened.into_iter().enumerate() {
            blobs::verify(&mut file,&content)?;
            let key=index.to_string();
            archive.start_file(&key,options).map_err(|_|Error::new("storage_unavailable"))?;
            let written=std::io::copy(&mut file,&mut archive)?;
            if written!=content.size {return Err(blobs::integrity());}
            size+=written;
            meta.insert(key,real);
            if size>FILL {break;}
        }
        archive.start_file("_meta",options).map_err(|_|Error::new("storage_unavailable"))?;
        let bytes=serde_json::to_vec(&meta).map_err(|_|Error::new("internal_error"))?;
        archive.write_all(&bytes)?;
        archive.finish().map_err(|_|Error::new("storage_unavailable"))?;
    }
    output.rewind()?;
    Ok(output)
}
