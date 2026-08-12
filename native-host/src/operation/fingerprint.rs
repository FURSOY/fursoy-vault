use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::model::Digest32;
use crate::atomic_file;
use crate::crypto::fill_random;
use crate::{FcpError, FcpResult, dpapi};

const SNAPSHOT_KEY_BYTES: usize = 32;

pub(crate) struct SnapshotTagContext {
    pub(crate) profile_id: Uuid,
    pub(crate) account_group_id: Uuid,
    pub(crate) operation_id: Uuid,
    pub(crate) operation_sequence: u64,
    pub(crate) base_vault_sequence: u64,
}

pub(crate) struct SnapshotTagger {
    key: Zeroizing<[u8; SNAPSHOT_KEY_BYTES]>,
}

impl SnapshotTagger {
    pub(crate) fn load_or_create(path: &Path, journal_exists: bool) -> FcpResult<Self> {
        if path.exists() {
            let plaintext = Zeroizing::new(dpapi::unprotect(&fs::read(path)?)?);
            let key = plaintext
                .as_slice()
                .try_into()
                .map_err(|_| FcpError::Format("snapshot tag key has invalid length".into()))?;
            return Ok(Self {
                key: Zeroizing::new(key),
            });
        }
        if journal_exists {
            return Err(FcpError::Format(
                "snapshot tag key is missing while operation journals exist".into(),
            ));
        }
        let mut key = Zeroizing::new([0u8; SNAPSHOT_KEY_BYTES]);
        fill_random(key.as_mut())?;
        let protected = dpapi::protect(key.as_ref())?;
        atomic_file::write_verified(path, &protected, |candidate| {
            let recovered = Zeroizing::new(dpapi::unprotect(candidate)?);
            if recovered.as_slice() != key.as_ref() {
                return Err(FcpError::Crypto(
                    "snapshot tag key write verification failed",
                ));
            }
            Ok(())
        })?;
        Ok(Self { key })
    }

    pub(crate) fn tag(&self, context: &SnapshotTagContext, canonical_snapshot: &[u8]) -> Digest32 {
        let mut input = Vec::with_capacity(80 + canonical_snapshot.len());
        input.extend_from_slice(b"FCPSNAP1");
        input.extend_from_slice(context.profile_id.as_bytes());
        input.extend_from_slice(context.account_group_id.as_bytes());
        input.extend_from_slice(context.operation_id.as_bytes());
        input.extend_from_slice(&context.operation_sequence.to_le_bytes());
        input.extend_from_slice(&context.base_vault_sequence.to_le_bytes());
        input.extend_from_slice(canonical_snapshot);
        Digest32(hmac_sha256(&self.key, &input))
    }

    #[cfg(test)]
    pub(crate) fn for_test(key: [u8; SNAPSHOT_KEY_BYTES]) -> Self {
        Self {
            key: Zeroizing::new(key),
        }
    }
}

fn hmac_sha256(key: &[u8; SNAPSHOT_KEY_BYTES], bytes: &[u8]) -> [u8; 32] {
    const BLOCK_BYTES: usize = 64;
    let mut inner_pad = Zeroizing::new([0x36u8; BLOCK_BYTES]);
    let mut outer_pad = Zeroizing::new([0x5cu8; BLOCK_BYTES]);
    for index in 0..SNAPSHOT_KEY_BYTES {
        inner_pad[index] ^= key[index];
        outer_pad[index] ^= key[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad.as_slice());
    inner.update(bytes);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad.as_slice());
    outer.update(inner_hash);
    outer.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> SnapshotTagContext {
        SnapshotTagContext {
            profile_id: Uuid::new_v4(),
            account_group_id: Uuid::new_v4(),
            operation_id: Uuid::new_v4(),
            operation_sequence: 4,
            base_vault_sequence: 3,
        }
    }

    #[test]
    fn snapshot_tag_binds_key_context_and_canonical_payload() {
        let first = SnapshotTagger::for_test([1; 32]);
        let second = SnapshotTagger::for_test([2; 32]);
        let context = context();
        let tag = first.tag(&context, b"synthetic-cookie-records");
        assert_eq!(tag, first.tag(&context, b"synthetic-cookie-records"));
        assert_ne!(tag, first.tag(&context, b"changed-records"));
        assert_ne!(tag, second.tag(&context, b"synthetic-cookie-records"));
        let changed_context = SnapshotTagContext {
            operation_sequence: context.operation_sequence + 1,
            ..context
        };
        assert_ne!(
            tag,
            first.tag(&changed_context, b"synthetic-cookie-records")
        );
    }

    #[test]
    fn snapshot_key_is_dpapi_protected_and_missing_key_with_journals_fails_closed() {
        let root = std::env::temp_dir().join(format!("fcp-snapshot-key-{}", Uuid::new_v4()));
        let path = root.join("snapshot-key.dpapi");
        let tagger = SnapshotTagger::load_or_create(&path, false).unwrap();
        let protected = fs::read(&path).unwrap();
        assert_ne!(protected, [0u8; SNAPSHOT_KEY_BYTES]);
        let context = context();
        let tag = tagger.tag(&context, b"synthetic-cookie-records");
        drop(tagger);
        let reopened = SnapshotTagger::load_or_create(&path, true).unwrap();
        assert_eq!(tag, reopened.tag(&context, b"synthetic-cookie-records"));
        fs::remove_file(&path).unwrap();
        assert!(SnapshotTagger::load_or_create(&path, true).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
