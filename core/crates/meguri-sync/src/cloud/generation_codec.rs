//! Pure K19.2 generation wire shapes. No layout operations, state transitions,
//! record I/O or claim policy. Counters and signed identifiers are caller facts.
use super::{
    control_codec::DecodeOutcome,
    entity_codec::{
        AssetInput, AssetProblem, CodecError, ReceiveError, Refusal, sha256,
        wire::{Reader, Value, Writer},
    },
};

type Result<T> = std::result::Result<T, CodecError>;

pub const MAX_GENERATION_BYTES: usize = 786_432;
/// K7 A2: maximum bytes in one CKAsset, including a MemberList's wire headers.
pub const MAX_MEMBER_LIST_BYTES: u64 = 104_857_600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GenerationState {
    Claimed = 1,
    Complete = 2,
    Abandoned = 3,
}
impl TryFrom<u64> for GenerationState {
    type Error = CodecError;
    fn try_from(value: u64) -> Result<Self> {
        match value {
            1 => Ok(Self::Claimed),
            2 => Ok(Self::Complete),
            3 => Ok(Self::Abandoned),
            _ => Err(CodecError::NonCanonical),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Member {
    pub record_name: String,
    pub erev: u64,
}
#[derive(Clone, PartialEq, Eq)]
pub struct MemberList {
    pub members: Vec<Member>,
}
/// A fully validated MemberList borrowing the caller's immutable asset bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MemberListView<'a> {
    bytes: &'a [u8],
    len: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MemberRef<'a> {
    pub record_name: &'a str,
    pub erev: u64,
}
pub struct MemberIter<'a> {
    reader: Reader<'a>,
}
impl std::fmt::Debug for MemberListView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemberListView")
    }
}
impl std::fmt::Debug for MemberRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemberRef")
    }
}
impl<'a> MemberListView<'a> {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Members and their names borrow the validated asset, without allocation.
    pub fn iter(&self) -> MemberIter<'a> {
        MemberIter {
            reader: Reader::new(self.bytes),
        }
    }
}
impl<'a> Iterator for MemberIter<'a> {
    type Item = MemberRef<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        // Only a fully validated, immutable view can construct this iterator.
        let (_, value) = self.reader.next().expect("validated MemberList")?;
        Some(read_member_ref(value.bytes().expect("validated member field")).expect("validated member"))
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct LayoutMap {
    pub notetype_id: i64,
    pub from_struct_gen: u64,
    pub to_struct_gen: u64,
    pub field_map: Vec<i32>,
    pub template_map: Vec<i32>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct TypeChange {
    pub old_notetype_id: i64,
    pub old_struct_gen: u64,
    pub new_notetype_id: i64,
    pub new_struct_gen: u64,
    pub field_map: Vec<i32>,
    pub template_map: Vec<i32>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Generation {
    pub generation_id: [u8; 16],
    /// K19.1 codes 1 through 10, strictly ascending. No operation is inferred.
    pub reasons: Vec<u32>,
    pub state: GenerationState,
    pub anchors: Vec<String>,
    pub members: Vec<Member>,
    pub maps: Vec<LayoutMap>,
    pub type_changes: Vec<TypeChange>,
    pub members_sha256: Option<[u8; 32]>,
    pub members_size: Option<u64>,
}

// Record names and identities are opaque in diagnostics, like the control codec.
macro_rules! opaque_debug {
    ($($name:ident),+ $(,)?) => {$(
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(stringify!($name))
            }
        }
    )+};
}
opaque_debug!(Member, MemberList, LayoutMap, TypeChange, Generation);

trait Canonical: Sized {
    const LIMIT: u64 = MAX_GENERATION_BYTES as u64;
    fn write(&self, w: &mut Writer) -> Result<()>;
    fn read(bytes: &[u8]) -> Result<Self>;
    fn encode_canonical(&self) -> Result<Vec<u8>> {
        let mut count = Writer::counting();
        self.write(&mut count)?;
        if count.len as u64 > Self::LIMIT {
            return Err(CodecError::Oversize);
        }
        let mut w = Writer::with_capacity(count.len);
        self.write(&mut w)?;
        Ok(w.finish())
    }
    fn decode_canonical(bytes: &[u8]) -> Result<Self> {
        if bytes.len() as u64 > Self::LIMIT {
            return Err(CodecError::Oversize);
        }
        let value = Self::read(bytes)?;
        if value.encode_canonical()? != bytes {
            return Err(CodecError::NonCanonical);
        }
        Ok(value)
    }
}

impl Generation {
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.encode_canonical()
    }
    pub fn decode(bytes: &[u8]) -> Result<DecodeOutcome<Self>> {
        // Probe only format, before the V1 cap or interpretation/allocation.
        let mut probe = Reader::new(bytes);
        let format = loop {
            let (tag, value) = probe.next()?.ok_or(CodecError::NonCanonical)?;
            if tag == 1 {
                break u32::try_from(value.integer()?).map_err(|_| CodecError::NonCanonical)?;
            }
        };
        match format {
            0 => Err(CodecError::NonCanonical),
            1 => Self::decode_canonical(bytes).map(DecodeOutcome::Supported),
            _ => Ok(DecodeOutcome::UnsupportedFormat(format)),
        }
    }
}
impl MemberList {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut count = Writer::counting();
        write_members(&mut count, 1, &self.members)?;
        if count.len as u64 > MAX_MEMBER_LIST_BYTES {
            return Err(CodecError::Oversize);
        }
        let mut writer = Writer::with_capacity(count.len);
        write_members(&mut writer, 1, &self.members)?;
        Ok(writer.finish())
    }
    /// Supplied asset facts only. Integrity failures retain K20's retryable
    /// classification; a verified but noncanonical MemberList is refused.
    /// Validates the entire asset before returning a view borrowing its bytes.
    pub fn decode_asset<'a>(
        asset: AssetInput<'a>,
        expected_sha256: &[u8; 32],
        expected_size: u64,
    ) -> std::result::Result<MemberListView<'a>, ReceiveError> {
        let refuse = |e| ReceiveError::Refused(Refusal::Encoding(e));
        if expected_size > MAX_MEMBER_LIST_BYTES {
            return Err(refuse(CodecError::Oversize));
        }
        let bytes = match asset {
            AssetInput::Absent => return Err(ReceiveError::RetryableAsset(AssetProblem::Absent)),
            AssetInput::Unreadable => return Err(ReceiveError::RetryableAsset(AssetProblem::Unreadable)),
            AssetInput::Bytes(bytes) => bytes,
        };
        if bytes.len() as u64 != expected_size || sha256(bytes) != *expected_sha256 {
            return Err(ReceiveError::RetryableAsset(AssetProblem::Integrity));
        }
        validate_member_list(bytes).map_err(refuse)
    }
}

fn validate_member_list(bytes: &[u8]) -> Result<MemberListView<'_>> {
    if bytes.len() as u64 > MAX_MEMBER_LIST_BYTES {
        return Err(CodecError::Oversize);
    }
    let mut reader = Reader::new(bytes);
    let mut canonical = Writer::counting();
    let mut previous = None;
    let mut len = 0;
    while let Some((tag, value)) = reader.next()? {
        if tag != 1 {
            return Err(CodecError::NonCanonical);
        }
        let member_bytes = value.bytes()?;
        let member = read_member_ref(member_bytes)?;
        if previous.is_some_and(|last| last >= member.record_name) {
            return Err(CodecError::NonCanonical);
        }
        previous = Some(member.record_name);
        canonical.bytes(1, member_bytes, true)?;
        len += 1;
    }
    // Reader accepts overlong varints, but they can only increase wire length.
    // All tags, types, fields and ordering are checked above and below, and each
    // member is canonical. Equal length therefore proves minimal outer headers.
    if canonical.len != bytes.len() {
        return Err(CodecError::NonCanonical);
    }
    Ok(MemberListView { bytes, len })
}

fn read_member_ref(bytes: &[u8]) -> Result<MemberRef<'_>> {
    if bytes.len() > MAX_GENERATION_BYTES {
        return Err(CodecError::Oversize);
    }
    let mut reader = Reader::new(bytes);
    let mut last = 0;
    let mut record_name = "";
    let mut erev = 0;
    while let Some((tag, value)) = reader.next()? {
        order(&mut last, tag, &[])?;
        match tag {
            1 => {
                record_name = std::str::from_utf8(value.bytes()?).map_err(|_| CodecError::NonCanonical)?;
            }
            2 => erev = value.integer()?,
            _ => return Err(CodecError::NonCanonical),
        }
    }
    entity_record_name(record_name)?;
    let mut canonical = Writer::counting();
    canonical.bytes(1, record_name.as_bytes(), false)?;
    canonical.scalar(2, erev, false)?;
    // With exact fields in order, only overlong varints or an explicit default
    // could differ from the canonical writer; both make the input longer.
    if canonical.len != bytes.len() {
        return Err(CodecError::NonCanonical);
    }
    Ok(MemberRef { record_name, erev })
}

fn order(last: &mut u32, tag: u32, repeated: &[u32]) -> Result<()> {
    if tag < *last || (tag == *last && !repeated.contains(&tag)) {
        return Err(CodecError::NonCanonical);
    }
    *last = tag;
    Ok(())
}
fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N]> {
    bytes.try_into().map_err(|_| CodecError::InvalidLength)
}
fn text(value: Value<'_>) -> Result<String> {
    let bytes = value.bytes()?;
    let name = std::str::from_utf8(bytes).map_err(|_| CodecError::NonCanonical)?;
    entity_record_name(name)?;
    Ok(name.to_owned())
}
fn entity_record_name(name: &str) -> Result<()> {
    // K4.2 entity names: e plus a canonical lowercase, unpadded 128-bit base32.
    let bytes = name.as_bytes();
    if bytes.len() != 27
        || bytes[0] != b'e'
        || !bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(b))
        || !b"aeimquy4".contains(&bytes[26])
    {
        return Err(CodecError::NonCanonical);
    }
    Ok(())
}
fn ascending<T: Ord>(values: impl IntoIterator<Item = T>) -> Result<()> {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|last| last >= &value) {
            return Err(CodecError::NonCanonical);
        }
        previous = Some(value);
    }
    Ok(())
}
fn write_members(w: &mut Writer, tag: u32, members: &[Member]) -> Result<()> {
    ascending(members.iter().map(|m| &m.record_name))?;
    for member in members {
        w.bytes(tag, &member.encode_canonical()?, true)?;
    }
    Ok(())
}
fn read_member(members: &mut Vec<Member>, value: Value<'_>) -> Result<()> {
    let member = Member::decode_canonical(value.bytes()?)?;
    if members
        .last()
        .is_some_and(|last| last.record_name >= member.record_name)
    {
        return Err(CodecError::NonCanonical);
    }
    members.push(member);
    Ok(())
}
fn write_packed(w: &mut Writer, tag: u32, values: impl Iterator<Item = u64> + Clone) -> Result<()> {
    let mut count = Writer::counting();
    for value in values.clone() {
        count.varint(value)?;
    }
    if count.len != 0 {
        w.header(tag, count.len)?;
        for value in values {
            w.varint(value)?;
        }
    }
    Ok(())
}
fn write_map(w: &mut Writer, tag: u32, values: &[i32]) -> Result<()> {
    if values.iter().any(|&value| value < -1) {
        return Err(CodecError::NonCanonical);
    }
    write_packed(
        w,
        tag,
        values
            .iter()
            .map(|&v| u64::from(((v as u32) << 1) ^ ((v >> 31) as u32))),
    )
}
fn read_map(value: Value<'_>) -> Result<Vec<i32>> {
    let mut r = Reader::new(value.bytes()?);
    let mut values = Vec::new();
    while let Some(raw) = r.packed_varint()? {
        let raw = u32::try_from(raw).map_err(|_| CodecError::NonCanonical)?;
        let value = ((raw >> 1) as i32) ^ -((raw & 1) as i32);
        if value < -1 {
            return Err(CodecError::NonCanonical);
        }
        values.push(value);
    }
    Ok(values)
}

impl Canonical for Member {
    fn write(&self, w: &mut Writer) -> Result<()> {
        entity_record_name(&self.record_name)?;
        w.bytes(1, self.record_name.as_bytes(), false)?;
        w.scalar(2, self.erev, false)
    }
    fn read(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let mut last = 0;
        let mut member = Self {
            record_name: String::new(),
            erev: 0,
        };
        while let Some((tag, value)) = r.next()? {
            order(&mut last, tag, &[])?;
            match tag {
                1 => member.record_name = text(value)?,
                2 => member.erev = value.integer()?,
                _ => return Err(CodecError::NonCanonical),
            }
        }
        Ok(member)
    }
}
impl Canonical for LayoutMap {
    fn write(&self, w: &mut Writer) -> Result<()> {
        w.scalar(1, self.notetype_id as u64, false)?;
        w.scalar(2, self.from_struct_gen, false)?;
        w.scalar(3, self.to_struct_gen, false)?;
        write_map(w, 4, &self.field_map)?;
        write_map(w, 5, &self.template_map)
    }
    fn read(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let mut last = 0;
        let mut map = Self {
            notetype_id: 0,
            from_struct_gen: 0,
            to_struct_gen: 0,
            field_map: Vec::new(),
            template_map: Vec::new(),
        };
        while let Some((tag, value)) = r.next()? {
            order(&mut last, tag, &[])?;
            match tag {
                1 => map.notetype_id = value.integer()? as i64,
                2 => map.from_struct_gen = value.integer()?,
                3 => map.to_struct_gen = value.integer()?,
                4 => map.field_map = read_map(value)?,
                5 => map.template_map = read_map(value)?,
                _ => return Err(CodecError::NonCanonical),
            }
        }
        Ok(map)
    }
}
impl Canonical for TypeChange {
    fn write(&self, w: &mut Writer) -> Result<()> {
        w.scalar(1, self.old_notetype_id as u64, false)?;
        w.scalar(2, self.old_struct_gen, false)?;
        w.scalar(3, self.new_notetype_id as u64, false)?;
        w.scalar(4, self.new_struct_gen, false)?;
        write_map(w, 5, &self.field_map)?;
        write_map(w, 6, &self.template_map)
    }
    fn read(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let mut last = 0;
        let mut change = Self {
            old_notetype_id: 0,
            old_struct_gen: 0,
            new_notetype_id: 0,
            new_struct_gen: 0,
            field_map: Vec::new(),
            template_map: Vec::new(),
        };
        while let Some((tag, value)) = r.next()? {
            order(&mut last, tag, &[])?;
            match tag {
                1 => change.old_notetype_id = value.integer()? as i64,
                2 => change.old_struct_gen = value.integer()?,
                3 => change.new_notetype_id = value.integer()? as i64,
                4 => change.new_struct_gen = value.integer()?,
                5 => change.field_map = read_map(value)?,
                6 => change.template_map = read_map(value)?,
                _ => return Err(CodecError::NonCanonical),
            }
        }
        Ok(change)
    }
}
impl Canonical for Generation {
    fn write(&self, w: &mut Writer) -> Result<()> {
        ascending(self.reasons.iter())?;
        if self.reasons.iter().any(|r| !(1..=10).contains(r)) {
            return Err(CodecError::NonCanonical);
        }
        ascending(self.anchors.iter())?;
        ascending(self.maps.iter().map(|m| m.notetype_id))?;
        ascending(self.type_changes.iter().map(|c| (c.old_notetype_id, c.new_notetype_id)))?;
        match (self.members_sha256, self.members_size) {
            (None, None) => (),
            (Some(_), Some(size)) if self.members.is_empty() => {
                if size > MAX_MEMBER_LIST_BYTES {
                    return Err(CodecError::Oversize);
                }
            }
            _ => return Err(CodecError::NonCanonical),
        }
        w.scalar(1, 1, false)?;
        w.bytes(2, &self.generation_id, false)?;
        write_packed(w, 3, self.reasons.iter().map(|&r| u64::from(r)))?;
        w.scalar(4, self.state as u64, false)?;
        for anchor in &self.anchors {
            entity_record_name(anchor)?;
            w.bytes(5, anchor.as_bytes(), true)?;
        }
        write_members(w, 6, &self.members)?;
        for map in &self.maps {
            w.bytes(7, &map.encode_canonical()?, true)?;
        }
        for change in &self.type_changes {
            w.bytes(8, &change.encode_canonical()?, true)?;
        }
        if let Some(digest) = &self.members_sha256 {
            w.bytes(9, digest, true)?;
        }
        if let Some(size) = self.members_size {
            w.scalar(10, size, true)?;
        }
        Ok(())
    }
    fn read(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let mut last = 0;
        let mut generation_id = None;
        let mut state = None;
        let mut result = Self {
            generation_id: [0; 16],
            reasons: Vec::new(),
            state: GenerationState::Claimed,
            anchors: Vec::new(),
            members: Vec::new(),
            maps: Vec::new(),
            type_changes: Vec::new(),
            members_sha256: None,
            members_size: None,
        };
        while let Some((tag, value)) = r.next()? {
            order(&mut last, tag, &[5, 6, 7, 8])?;
            match tag {
                1 => {
                    if value.integer()? != 1 {
                        return Err(CodecError::NonCanonical);
                    }
                }
                2 => generation_id = Some(fixed(value.bytes()?)?),
                3 => {
                    let mut packed = Reader::new(value.bytes()?);
                    while let Some(reason) = packed.packed_varint()? {
                        let reason = u32::try_from(reason).map_err(|_| CodecError::NonCanonical)?;
                        if !(1..=10).contains(&reason) || result.reasons.last().is_some_and(|&last| last >= reason) {
                            return Err(CodecError::NonCanonical);
                        }
                        result.reasons.push(reason);
                    }
                }
                4 => state = Some(GenerationState::try_from(value.integer()?)?),
                5 => {
                    let anchor = text(value)?;
                    if result.anchors.last().is_some_and(|last| last >= &anchor) {
                        return Err(CodecError::NonCanonical);
                    }
                    result.anchors.push(anchor);
                }
                6 => read_member(&mut result.members, value)?,
                7 => {
                    let map = LayoutMap::decode_canonical(value.bytes()?)?;
                    if result
                        .maps
                        .last()
                        .is_some_and(|last| last.notetype_id >= map.notetype_id)
                    {
                        return Err(CodecError::NonCanonical);
                    }
                    result.maps.push(map);
                }
                8 => {
                    let change = TypeChange::decode_canonical(value.bytes()?)?;
                    if result.type_changes.last().is_some_and(|last| {
                        (last.old_notetype_id, last.new_notetype_id) >= (change.old_notetype_id, change.new_notetype_id)
                    }) {
                        return Err(CodecError::NonCanonical);
                    }
                    result.type_changes.push(change);
                }
                9 => result.members_sha256 = Some(fixed(value.bytes()?)?),
                10 => result.members_size = Some(value.integer()?),
                _ => return Err(CodecError::NonCanonical),
            }
        }
        result.generation_id = generation_id.ok_or(CodecError::InvalidLength)?;
        result.state = state.ok_or(CodecError::NonCanonical)?;
        Ok(result)
    }
}
