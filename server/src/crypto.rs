// SPDX-License-Identifier: MIT
use crate::{Error, Result};
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub fn digest(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}
pub fn random_token() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::new("internal_error"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
pub fn bearer(value: &str) -> Result<[u8; 32]> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| Error::new("capability_invalid"))?;
    if bytes.len() != 32 || URL_SAFE_NO_PAD.encode(&bytes) != value {
        return Err(Error::new("capability_invalid"));
    }
    Ok(digest(value.as_bytes()))
}
pub fn same_digest(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}
pub fn username(value: &str) -> Result<String> {
    let v = value
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_ascii_lowercase();
    if !(3..=64).contains(&v.len())
        || !v.as_bytes()[0].is_ascii_alphanumeric()
        || !v
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
    {
        return Err(Error::field("username"));
    }
    Ok(v)
}
pub fn new_password(value: &str) -> Result<()> {
    if !(12..=1024).contains(&value.len()) || value.contains('\0') {
        Err(Error::field("password"))
    } else {
        Ok(())
    }
}
fn argon() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19456, 2, 1, Some(32)).expect("fixed Argon2 parameters"),
    )
}
pub fn hash(value: &str) -> Result<String> {
    let mut salt = [0; 16];
    getrandom::fill(&mut salt).map_err(|_| Error::new("internal_error"))?;
    let phc: PasswordHash = argon()
        .hash_password_with_salt(value.as_bytes(), &salt)
        .map_err(|_| Error::new("internal_error"))?;
    Ok(phc.to_string())
}
/// Parse the supported stored credential shape without doing password work.
/// Retains the existing hash policy; startup must never run Argon2.
fn supported_phc(h: &PasswordHash) -> bool {
    h.algorithm.as_str() == "argon2id"
        && h.version == Some(19)
        && h.params.get_decimal("m") == Some(19456)
        && h.params.get_decimal("t") == Some(2)
        && h.params.get_decimal("p") == Some(1)
        && h.salt.as_ref().is_some_and(|s| s.len() == 16)
        && h.hash.as_ref().is_some_and(|d| d.len() == 32)
}
pub(crate) fn valid_phc_shape(phc: &str) -> bool {
    PasswordHash::new(phc).is_ok_and(|h| supported_phc(&h))
}
pub fn verify(value: &str, phc: &str) -> bool {
    PasswordHash::new(phc)
        .is_ok_and(|h| supported_phc(&h) && argon().verify_password(value.as_bytes(), &h).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Public synthetic fixture captured by the executed 0.5.3 characterization.
    // Password and salt are test data, never an account credential.
    const LEGACY_PASSWORD: &str = "compatibility-password-2026";
    const LEGACY_SALT: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    const LEGACY_PHC: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAECAwQFBgcICQoLDA0ODw$krbozlfmG/YbBoj1y9KHSlgK7HHNhVNfzFmZ8bEcWH4";

    fn assert_contract(phc: &str) -> [u8; 16] {
        let stored = PasswordHash::new(phc).expect("valid PHC");
        assert_eq!(stored.algorithm.as_str(), "argon2id");
        assert_eq!(stored.version, Some(19));
        assert_eq!(stored.params.get_decimal("m"), Some(19456));
        assert_eq!(stored.params.get_decimal("t"), Some(2));
        assert_eq!(stored.params.get_decimal("p"), Some(1));
        let decoded_salt: &[u8] = stored.salt.as_ref().expect("stored salt").as_ref();
        assert_eq!(decoded_salt.len(), 16);
        assert_eq!(stored.hash.as_ref().expect("stored output").len(), 32);
        decoded_salt.try_into().expect("16-byte salt")
    }

    #[test]
    fn argon2_legacy_phc_compatibility() {
        assert!(verify(LEGACY_PASSWORD, LEGACY_PHC));
        assert!(!verify("another-synthetic-password", LEGACY_PHC));
        assert_eq!(assert_contract(LEGACY_PHC), LEGACY_SALT);

        let current: PasswordHash = argon()
            .hash_password_with_salt(LEGACY_PASSWORD.as_bytes(), &LEGACY_SALT)
            .expect("hash synthetic legacy inputs");
        assert_eq!(current.to_string(), LEGACY_PHC);
    }

    #[test]
    fn generated_phc_preserves_contract() {
        let phc = hash(LEGACY_PASSWORD).expect("hash with OS-random salt");
        assert_contract(&phc);
        assert!(verify(LEGACY_PASSWORD, &phc));
        assert!(!verify("another-synthetic-password", &phc));
    }
}
