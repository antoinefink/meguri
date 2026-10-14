//! Connection initialization (`storage/open-and-connections` C5): the
//! collation first, then the registered initializers, through which other
//! crates add the SQL functions their queries call (search U80/U81, queue
//! building U41) without meguri-storage depending on them.

use std::fmt;
use std::sync::Arc;

use rusqlite::Connection;

use crate::collation;

type InitFn = dyn Fn(&Connection) -> rusqlite::Result<()> + Send + Sync;

/// The initializers of one open collection, run in registration order on
/// the writer and on every reader (never on the open gate's probe). The set
/// is fixed when the collection opens.
///
/// An initializer registers functions deterministic and innocuous
/// (`FunctionFlags::SQLITE_DETERMINISTIC | SQLITE_INNOCUOUS`), and they are
/// only ever called from statements: no schema object may use them
/// (`storage/schema-18` S4).
#[derive(Clone, Default)]
pub struct Initializers {
    list: Vec<Arc<InitFn>>,
}

impl Initializers {
    pub fn new() -> Initializers {
        Initializers::default()
    }

    /// Adds an initializer after the ones already registered.
    pub fn register(&mut self, init: impl Fn(&Connection) -> rusqlite::Result<()> + Send + Sync + 'static) {
        self.list.push(Arc::new(init));
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

impl fmt::Debug for Initializers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Initializers({})", self.list.len())
    }
}

/// The probe's initialization: the collation and nothing else (C2.1).
pub(crate) fn probe(conn: &Connection) -> Result<(), String> {
    collation::register(conn)
}

/// The writer's and every reader's initialization: the collation, then each
/// initializer. A failure fails that connection's open (`ConnectionInit`).
pub(crate) fn full(conn: &Connection, initializers: &Initializers) -> Result<(), String> {
    collation::register(conn)?;
    for (index, init) in initializers.list.iter().enumerate() {
        init(conn).map_err(|e| format!("initializer {index}: {e}"))?;
    }
    Ok(())
}
