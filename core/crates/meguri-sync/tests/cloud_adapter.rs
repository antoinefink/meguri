//! Source regressions for the adapter's K2/K4.1 domain naming boundary.
use meguri_sync::cloud::{EpochId, adapter::collection_zone};

#[test]
fn collection_zone_uses_rfc4648_lowercase_unpadded_epoch_bytes() {
    assert_eq!(collection_zone(EpochId([0; 16])), "c-aaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(
        collection_zone(EpochId([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])),
        "c-aaaqeayeaudaocajbifqydiob4",
    );
    assert_eq!(collection_zone(EpochId([255; 16])), "c-77777777777777777777777774");
}
