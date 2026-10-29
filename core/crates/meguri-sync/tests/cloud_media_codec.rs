use meguri_sync::cloud::media::blob::Blob;

#[test]
fn blob_metadata_has_exact_canonical_wire_and_rejects_ambiguous_lengths() {
    let metadata = Blob {
        sha256: [0x21; 32],
        sha1: [0x43; 20],
        size: 129,
    };
    let mut wire = vec![8, 1, 18, 32];
    wire.extend([0x21; 32]);
    wire.extend([26, 20]);
    wire.extend([0x43; 20]);
    wire.extend([32, 129, 1]);
    assert_eq!(metadata.encode().unwrap(), wire);
    assert_eq!(Blob::decode(&wire).unwrap(), metadata);
    let mut duplicate = wire.clone();
    duplicate.extend([32, 129, 1]);
    assert!(Blob::decode(&duplicate).is_err());
    let mut nonminimal = wire.clone();
    nonminimal.splice(1..2, [129, 0]);
    assert!(Blob::decode(&nonminimal).is_err());
    assert!(
        Blob {
            size: 104_857_601,
            ..metadata.clone()
        }
        .encode()
        .is_err()
    );
    assert!(Blob { size: 0, ..metadata }.encode().is_err());
}
