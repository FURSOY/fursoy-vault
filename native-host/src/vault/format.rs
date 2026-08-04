use uuid::Uuid;
use zeroize::Zeroize;

use crate::crypto::aead::{
    GCM_NONCE_BYTES, GCM_TAG_BYTES, SecretDek, decrypt, encrypt, generate_nonce,
};
use crate::crypto::platform_kek::WRAPPED_DEK_BYTES;
use crate::vault::payload::{PAYLOAD_SCHEMA_VERSION, VaultPayload};
use crate::{FcpError, FcpResult};

pub const VAULT_MAGIC: [u8; 4] = *b"FCPV";
pub const VAULT_FORMAT_VERSION: u16 = 1;
pub const AEAD_ALG_AES_256_GCM: u16 = 1;
pub const WRAP_ALG_RSA_2048_OAEP_SHA256: u16 = 1;
pub const MAX_CIPHERTEXT_BYTES: usize = 4 * 1024 * 1024;
pub const FIXED_HEADER_BYTES: usize =
    4 + 2 + 2 + 16 + 2 + 2 + 16 + GCM_NONCE_BYTES + 2 + WRAPPED_DEK_BYTES + 4;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultHeader {
    pub group_id: Uuid,
    pub kek_key_id: [u8; 16],
    pub nonce: [u8; GCM_NONCE_BYTES],
    pub wrapped_dek: Vec<u8>,
    pub ciphertext_len: u32,
}

impl VaultHeader {
    pub fn new(
        group_id: Uuid,
        kek_key_id: [u8; 16],
        nonce: [u8; GCM_NONCE_BYTES],
        wrapped_dek: Vec<u8>,
        ciphertext_len: usize,
    ) -> FcpResult<Self> {
        if group_id.is_nil() {
            return Err(FcpError::Format("group_id must not be nil".into()));
        }
        if wrapped_dek.len() != WRAPPED_DEK_BYTES {
            return Err(FcpError::Format(format!(
                "wrapped DEK must be {WRAPPED_DEK_BYTES} bytes"
            )));
        }
        if ciphertext_len > MAX_CIPHERTEXT_BYTES {
            return Err(FcpError::Format(format!(
                "ciphertext exceeds {MAX_CIPHERTEXT_BYTES} byte limit"
            )));
        }
        Ok(Self {
            group_id,
            kek_key_id,
            nonce,
            wrapped_dek,
            ciphertext_len: u32::try_from(ciphertext_len)
                .map_err(|_| FcpError::Format("ciphertext length exceeds u32".into()))?,
        })
    }

    /// The complete v1 header is also the AES-GCM AAD. This binds format/group/algorithm/KEK,
    /// nonce, the single authoritative wrapped DEK, and ciphertext length to the authentication tag.
    pub fn encode(&self) -> FcpResult<Vec<u8>> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(FIXED_HEADER_BYTES);
        bytes.extend_from_slice(&VAULT_MAGIC);
        bytes.extend_from_slice(&VAULT_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&(FIXED_HEADER_BYTES as u16).to_le_bytes());
        bytes.extend_from_slice(self.group_id.as_bytes());
        bytes.extend_from_slice(&AEAD_ALG_AES_256_GCM.to_le_bytes());
        bytes.extend_from_slice(&WRAP_ALG_RSA_2048_OAEP_SHA256.to_le_bytes());
        bytes.extend_from_slice(&self.kek_key_id);
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&(WRAPPED_DEK_BYTES as u16).to_le_bytes());
        bytes.extend_from_slice(&self.wrapped_dek);
        bytes.extend_from_slice(&self.ciphertext_len.to_le_bytes());
        debug_assert_eq!(bytes.len(), FIXED_HEADER_BYTES);
        Ok(bytes)
    }

    fn validate(&self) -> FcpResult<()> {
        if self.group_id.is_nil() {
            return Err(FcpError::Format("group_id must not be nil".into()));
        }
        if self.wrapped_dek.len() != WRAPPED_DEK_BYTES {
            return Err(FcpError::Format(format!(
                "wrapped DEK must be {WRAPPED_DEK_BYTES} bytes"
            )));
        }
        if self.ciphertext_len as usize > MAX_CIPHERTEXT_BYTES {
            return Err(FcpError::Format("ciphertext length exceeds limit".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultRecord {
    pub header: VaultHeader,
    pub ciphertext: Vec<u8>,
    pub tag: [u8; GCM_TAG_BYTES],
}

impl VaultRecord {
    pub fn seal(
        group_id: Uuid,
        kek_key_id: [u8; 16],
        wrapped_dek: Vec<u8>,
        dek: &SecretDek,
        payload: &VaultPayload,
    ) -> FcpResult<Self> {
        if payload.schema_version != PAYLOAD_SCHEMA_VERSION {
            return Err(FcpError::Format(format!(
                "unsupported payload schema version {}",
                payload.schema_version
            )));
        }
        let mut plaintext = serde_json::to_vec(payload)?;
        let result = (|| {
            let nonce = generate_nonce()?;
            let header =
                VaultHeader::new(group_id, kek_key_id, nonce, wrapped_dek, plaintext.len())?;
            let aad = header.encode()?;
            let (ciphertext, tag) = encrypt(dek, &nonce, &aad, &plaintext)?;
            Ok(Self {
                header,
                ciphertext,
                tag,
            })
        })();
        plaintext.zeroize();
        result
    }

    pub fn open(&self, dek: &SecretDek) -> FcpResult<VaultPayload> {
        self.validate()?;
        let aad = self.header.encode()?;
        let mut plaintext = decrypt(dek, &self.header.nonce, &aad, &self.ciphertext, &self.tag)?;
        let result = serde_json::from_slice::<VaultPayload>(&plaintext).map_err(FcpError::from);
        plaintext.zeroize();
        let payload = result?;
        if payload.schema_version != PAYLOAD_SCHEMA_VERSION {
            return Err(FcpError::Format(format!(
                "unsupported payload schema version {}",
                payload.schema_version
            )));
        }
        Ok(payload)
    }

    pub fn encode(&self) -> FcpResult<Vec<u8>> {
        self.validate()?;
        let mut bytes = self.header.encode()?;
        bytes.extend_from_slice(&self.ciphertext);
        bytes.extend_from_slice(&self.tag);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> FcpResult<Self> {
        if bytes.len() < FIXED_HEADER_BYTES + GCM_TAG_BYTES {
            return Err(FcpError::Format("vault record is truncated".into()));
        }
        let mut cursor = Cursor::new(bytes);
        if cursor.take_array::<4>()? != VAULT_MAGIC {
            return Err(FcpError::Format("vault magic mismatch".into()));
        }
        require_u16(&mut cursor, VAULT_FORMAT_VERSION, "format_version")?;
        require_u16(&mut cursor, FIXED_HEADER_BYTES as u16, "header_len")?;
        let group_id = Uuid::from_bytes(cursor.take_array::<16>()?);
        require_u16(&mut cursor, AEAD_ALG_AES_256_GCM, "aead_alg_id")?;
        require_u16(&mut cursor, WRAP_ALG_RSA_2048_OAEP_SHA256, "wrap_alg_id")?;
        let kek_key_id = cursor.take_array::<16>()?;
        let nonce = cursor.take_array::<GCM_NONCE_BYTES>()?;
        require_u16(&mut cursor, WRAPPED_DEK_BYTES as u16, "wrapped_dek_len")?;
        let wrapped_dek = cursor.take(WRAPPED_DEK_BYTES)?.to_vec();
        let ciphertext_len = u32::from_le_bytes(cursor.take_array::<4>()?);
        if ciphertext_len as usize > MAX_CIPHERTEXT_BYTES {
            return Err(FcpError::Format("ciphertext length exceeds limit".into()));
        }
        let expected_len = FIXED_HEADER_BYTES
            .checked_add(ciphertext_len as usize)
            .and_then(|length| length.checked_add(GCM_TAG_BYTES))
            .ok_or_else(|| FcpError::Format("vault length overflow".into()))?;
        if bytes.len() != expected_len {
            return Err(FcpError::Format(format!(
                "vault length mismatch: expected {expected_len}, got {}",
                bytes.len()
            )));
        }
        let ciphertext = cursor.take(ciphertext_len as usize)?.to_vec();
        let tag = cursor.take_array::<GCM_TAG_BYTES>()?;
        let header = VaultHeader::new(
            group_id,
            kek_key_id,
            nonce,
            wrapped_dek,
            ciphertext_len as usize,
        )?;
        let record = Self {
            header,
            ciphertext,
            tag,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> FcpResult<()> {
        self.header.validate()?;
        if self.ciphertext.len() != self.header.ciphertext_len as usize {
            return Err(FcpError::Format(
                "ciphertext length does not match authenticated header".into(),
            ));
        }
        Ok(())
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> FcpResult<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| FcpError::Format("vault cursor overflow".into()))?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| FcpError::Format("vault record is truncated".into()))?;
        self.position = end;
        Ok(value)
    }

    fn take_array<const N: usize>(&mut self) -> FcpResult<[u8; N]> {
        self.take(N)?
            .try_into()
            .map_err(|_| FcpError::Format("vault field has invalid length".into()))
    }
}

fn require_u16(cursor: &mut Cursor<'_>, expected: u16, field: &str) -> FcpResult<()> {
    let actual = u16::from_le_bytes(cursor.take_array::<2>()?);
    if actual != expected {
        return Err(FcpError::Format(format!(
            "unsupported {field}: expected {expected}, got {actual}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::aead::DEK_BYTES;
    use crate::vault::payload::VaultPayload;

    fn record() -> (VaultRecord, SecretDek) {
        let dek = SecretDek::from_bytes([3; DEK_BYTES]);
        let record = VaultRecord::seal(
            Uuid::from_u128(9),
            [4; 16],
            vec![5; WRAPPED_DEK_BYTES],
            &dek,
            &VaultPayload::empty(),
        )
        .unwrap();
        (record, dek)
    }

    #[test]
    fn v1_record_round_trips() {
        let (record, dek) = record();
        let bytes = record.encode().unwrap();
        let decoded = VaultRecord::decode(&bytes).unwrap();
        assert_eq!(decoded.open(&dek).unwrap(), VaultPayload::empty());
    }

    #[test]
    fn authenticated_header_tampering_is_rejected() {
        let (record, dek) = record();
        let mut bytes = record.encode().unwrap();
        let kek_id_offset = 4 + 2 + 2 + 16 + 2 + 2;
        bytes[kek_id_offset] ^= 1;
        let decoded = VaultRecord::decode(&bytes).unwrap();
        assert!(decoded.open(&dek).is_err());
    }

    #[test]
    fn trailing_data_is_rejected() {
        let (record, _) = record();
        let mut bytes = record.encode().unwrap();
        bytes.push(0);
        assert!(VaultRecord::decode(&bytes).is_err());
    }
}
