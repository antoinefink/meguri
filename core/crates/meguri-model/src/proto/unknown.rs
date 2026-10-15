//! Fields a message does not declare (`specs/model/protobuf-records.md` P4):
//! kept as their exact bytes, from the first byte of the key to the last byte
//! of the payload, in the order they were read.

/// One unknown field.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UnknownField {
    number: u32,
    bytes: Vec<u8>,
}

impl UnknownField {
    /// The field number of its key.
    pub fn number(&self) -> u32 {
        self.number
    }

    /// Key and payload, exactly as read.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The unknown fields of one message, in read order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct UnknownFields {
    fields: Vec<UnknownField>,
}

impl UnknownFields {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &UnknownField> {
        self.fields.iter()
    }

    pub(crate) fn push(&mut self, number: u32, bytes: &[u8]) {
        self.fields.push(UnknownField {
            number,
            bytes: bytes.to_vec(),
        });
    }

    /// Sorted by field number; fields that share a number keep their read
    /// order (P4).
    pub(crate) fn by_number(&self) -> Vec<&UnknownField> {
        let mut sorted: Vec<&UnknownField> = self.fields.iter().collect();
        sorted.sort_by_key(|f| f.number);
        sorted
    }
}
