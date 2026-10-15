//! The `tags` row (`specs/model/row-records.md` R8).

use crate::usn::Usn;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tag {
    /// Hierarchy levels joined by `::`.
    pub name: String,
    pub usn: Usn,
    /// Collapsed in the browser sidebar.
    pub collapsed: bool,
    /// Unused by Anki, which writes NULL; kept unchanged.
    pub config: Option<Vec<u8>>,
}
