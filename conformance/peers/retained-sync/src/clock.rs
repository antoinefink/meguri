use meguri_model::clock::{Clock, UtcOffset};

#[derive(Debug)]
pub struct WallClock;
impl Clock for WallClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock precedes epoch")
            .as_millis().try_into().expect("clock exceeds model range")
    }
    fn utc_offset_at(&self, _: i64) -> UtcOffset { UtcOffset::UTC }
    fn zone_id(&self) -> String { "UTC".into() }
}
