//! orders O1. No collection SQL; register through the connection hook.
use rusqlite::{Connection, functions::FunctionFlags, types::ValueRef};
pub fn hash(values: &[i64]) -> u64 {
    values
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(0xcbf29ce484222325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        })
}
pub fn register(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        "fnvhash",
        -1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS,
        |ctx| {
            let mut args = Vec::with_capacity(ctx.len());
            for i in 0..ctx.len() {
                match ctx.get_raw(i) {
                    ValueRef::Integer(v) => args.push(v),
                    _ => {
                        return Err(rusqlite::Error::UserFunctionError(Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "fnvhash requires integers",
                        ))));
                    }
                }
            }
            Ok(hash(&args) as i64)
        },
    )
}
