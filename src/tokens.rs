//! Opaque bearer and invite tokens: the raw value goes to the client, only its hash is stored.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Whether `token` has the shape [`new_token`] gives: 32 bytes in unpadded URL safe base64.
pub fn well_formed(token: &str) -> bool {
    URL_SAFE_NO_PAD.decode(token).is_ok_and(|bytes| bytes.len() == 32)
}

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_tokens_shaped_like_ours_are_well_formed() {
        assert!(well_formed(&new_token()));
        assert!(!well_formed("") && !well_formed("abc") && !well_formed(&format!("{}=", new_token())) && !well_formed(&"a".repeat(44)));
    }
}
