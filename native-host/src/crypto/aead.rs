use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::crypto::fill_random;
use crate::{FcpError, FcpResult};

pub const DEK_BYTES: usize = 32;
pub const GCM_NONCE_BYTES: usize = 12;
pub const GCM_TAG_BYTES: usize = 16;

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretDek([u8; DEK_BYTES]);

impl SecretDek {
    pub fn generate() -> FcpResult<Self> {
        let mut bytes = [0u8; DEK_BYTES];
        fill_random(&mut bytes)?;
        Ok(Self(bytes))
    }

    pub fn from_bytes(bytes: [u8; DEK_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn expose(&self) -> &[u8; DEK_BYTES] {
        &self.0
    }
}

pub fn generate_nonce() -> FcpResult<[u8; GCM_NONCE_BYTES]> {
    let mut nonce = [0u8; GCM_NONCE_BYTES];
    fill_random(&mut nonce)?;
    Ok(nonce)
}

pub fn encrypt(
    dek: &SecretDek,
    nonce: &[u8; GCM_NONCE_BYTES],
    aad: &[u8],
    plaintext: &[u8],
) -> FcpResult<(Vec<u8>, [u8; GCM_TAG_BYTES])> {
    let cipher = Aes256Gcm::new_from_slice(dek.expose())
        .map_err(|_| FcpError::Crypto("invalid AES-256 key length"))?;
    let mut ciphertext = plaintext.to_vec();
    let tag = cipher
        .encrypt_in_place_detached(Nonce::from_slice(nonce), aad, &mut ciphertext)
        .map_err(|_| FcpError::Crypto("AES-256-GCM encryption failed"))?;
    let mut tag_bytes = [0u8; GCM_TAG_BYTES];
    tag_bytes.copy_from_slice(tag.as_slice());
    Ok((ciphertext, tag_bytes))
}

pub fn decrypt(
    dek: &SecretDek,
    nonce: &[u8; GCM_NONCE_BYTES],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8; GCM_TAG_BYTES],
) -> FcpResult<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(dek.expose())
        .map_err(|_| FcpError::Crypto("invalid AES-256 key length"))?;
    let mut plaintext = ciphertext.to_vec();
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(nonce),
            aad,
            &mut plaintext,
            Tag::from_slice(tag),
        )
        .map_err(|_| FcpError::Crypto("AES-256-GCM authentication failed"))?;
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aead_round_trip_and_tamper_detection() {
        let dek = SecretDek::from_bytes([9; DEK_BYTES]);
        let nonce = [4; GCM_NONCE_BYTES];
        let aad = b"authenticated header";
        let (ciphertext, tag) = encrypt(&dek, &nonce, aad, b"cookie payload").unwrap();
        assert_eq!(
            decrypt(&dek, &nonce, aad, &ciphertext, &tag).unwrap(),
            b"cookie payload"
        );

        let mut tampered = ciphertext;
        tampered[0] ^= 1;
        assert!(decrypt(&dek, &nonce, aad, &tampered, &tag).is_err());
    }
}
