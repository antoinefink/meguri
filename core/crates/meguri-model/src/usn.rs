//! Update sequence numbers (`specs/model/row-records.md`): every synced row
//! carries one. How they move is the storage area's rule (U23).

use std::fmt;

/// A row's `usn`, or `col.usn` on a client: the server's update sequence
/// number as of the last sync.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Usn(pub i32);

impl Usn {
    /// -1: changed locally and not yet sent to the server.
    pub const PENDING: Usn = Usn(-1);

    pub fn is_pending(self) -> bool {
        self == Usn::PENDING
    }
}

impl fmt::Display for Usn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<i32> for Usn {
    fn from(usn: i32) -> Usn {
        Usn(usn)
    }
}
