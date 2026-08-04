use uuid::Uuid;

use crate::audit::unix_ms;
use crate::crypto::aead::SecretDek;
use crate::crypto::capability::CapabilitySigner;
use crate::crypto::hello::HelloAuthorizer;
use crate::crypto::platform_kek::{KEK_KEY_ID, PlatformKek};
use crate::lease::state_machine::{CapabilityLedger, ConsumedCapability};
use crate::lease::store::FileCapabilityLedgerStore;
use crate::protocol::messages::{CapabilityOperation, CookieRecord};
use crate::vault::format::VaultRecord;
use crate::vault::payload::{PAYLOAD_SCHEMA_VERSION, VaultPayload};
use crate::vault::store::VaultStore;
use crate::{FcpError, FcpResult};

const CAPABILITY_LIFETIME_MS: u64 = 60_000;

pub struct VaultTransactions {
    group_id: Uuid,
    vault_store: VaultStore,
    capability_ledger: CapabilityLedger,
    capability_store: FileCapabilityLedgerStore,
    // WinRT/KeyCredential teardown is process-lifetime sensitive. Keep one lazily-created
    // authorizer (and therefore one initialized apartment) alive for the whole native connection
    // instead of repeatedly RoInitialize/RoUninitialize around each Hello capability.
    hello_authorizer: Option<HelloAuthorizer>,
}

impl VaultTransactions {
    pub fn open(
        group_id: Uuid,
        vault_store: VaultStore,
        mut capability_store: FileCapabilityLedgerStore,
    ) -> FcpResult<Self> {
        let mut capability_ledger = capability_store.load_or_initialize(group_id)?;
        if capability_ledger.pending.is_some() {
            // A reservation is not an authorization. If the prior native-host process died while
            // Hello was open, discard only that stale reservation while preserving the issued
            // sequence, so it can never be reused. Signed capabilities never leave the host.
            capability_ledger.cancel_pending(&mut capability_store)?;
        }
        Ok(Self {
            group_id,
            vault_store,
            capability_ledger,
            capability_store,
            hello_authorizer: None,
        })
    }

    /// Displays/obtains Windows Hello authorization, verifies all five bound fields, and durably
    /// consumes sequence+nonce. Only the returned linear token can enter an inject vault read.
    pub fn authorize_inject(&mut self) -> FcpResult<ConsumedCapability> {
        let now = unix_ms()?;
        let payload = self.capability_ledger.reserve(
            CapabilityOperation::Inject,
            now,
            CAPABILITY_LIFETIME_MS,
            &mut self.capability_store,
        )?;
        if self.hello_authorizer.is_none() {
            match HelloAuthorizer::open_or_create() {
                Ok(authorizer) => self.hello_authorizer = Some(authorizer),
                Err(error) => {
                    self.capability_ledger
                        .cancel_pending(&mut self.capability_store)?;
                    return Err(error);
                }
            }
        }
        let authorizer = match self.hello_authorizer.as_ref() {
            Some(authorizer) => authorizer,
            None => {
                self.capability_ledger
                    .cancel_pending(&mut self.capability_store)?;
                return Err(FcpError::Capability(
                    "Windows Hello authorizer was not initialized".into(),
                ));
            }
        };
        let signed = match authorizer.sign(payload) {
            Ok(signed) => signed,
            Err(error) => {
                self.capability_ledger
                    .cancel_pending(&mut self.capability_store)?;
                return Err(error);
            }
        };
        self.capability_ledger.verify_and_consume(
            &signed,
            CapabilityOperation::Inject,
            unix_ms()?,
            authorizer,
            &mut self.capability_store,
        )
    }

    /// Capability consumption has already completed when this function starts. The DEK is
    /// unwrapped into a ZeroizeOnDrop value, used for one decrypt, and destroyed before return.
    pub fn read_for_inject(&self, authorization: ConsumedCapability) -> FcpResult<VaultPayload> {
        self.require_authorization(&authorization, CapabilityOperation::Inject)?;
        let record = self.vault_store.read(self.group_id)?;
        let dek = PlatformKek::unwrap_dek(&record.header.wrapped_dek)?;
        let payload = record.open(&dek)?;
        drop(dek);
        Ok(payload)
    }

    /// Creates the first group vault from a controlled login snapshot. Sealing reduces exposure,
    /// so it is deliberately non-interactive; the generated DEK never leaves this transaction.
    pub fn enroll(&self, cookies: Vec<CookieRecord>) -> FcpResult<u64> {
        if self.vault_store.path_for(self.group_id).exists() {
            return Err(FcpError::Format(
                "enrollment refused because the group vault already exists".into(),
            ));
        }
        PlatformKek::ensure_exists()?;
        let dek = SecretDek::generate()?;
        let wrapped_dek = PlatformKek::wrap_dek(&dek)?;
        let payload = VaultPayload {
            schema_version: PAYLOAD_SCHEMA_VERSION,
            vault_sequence: 1,
            cookies,
        };
        let record = VaultRecord::seal(self.group_id, KEK_KEY_ID, wrapped_dek, &dek, &payload)?;
        self.vault_store.write_verified(&record, &dek)?;
        drop(dek);
        Ok(payload.vault_sequence)
    }

    /// Replaces the encrypted cookie payload after snapshot. Eviction is a fail-closed operation:
    /// TPM-backed unwrap is silent, transaction-scoped, and never waits for user presence.
    pub fn update_after_snapshot(&self, cookies: Vec<CookieRecord>) -> FcpResult<u64> {
        let existing = self.vault_store.read(self.group_id)?;
        let dek = PlatformKek::unwrap_dek(&existing.header.wrapped_dek)?;
        let previous = existing.open(&dek)?;
        let vault_sequence = previous
            .vault_sequence
            .checked_add(1)
            .ok_or_else(|| FcpError::Format("vault sequence exhausted".into()))?;
        let payload = VaultPayload {
            schema_version: PAYLOAD_SCHEMA_VERSION,
            vault_sequence,
            cookies,
        };
        let replacement = VaultRecord::seal(
            self.group_id,
            existing.header.kek_key_id,
            existing.header.wrapped_dek,
            &dek,
            &payload,
        )?;
        self.vault_store.write_verified(&replacement, &dek)?;
        drop(dek);
        Ok(vault_sequence)
    }

    pub fn vault_exists(&self) -> bool {
        self.vault_store.path_for(self.group_id).exists()
    }

    /// Revokes a server-invalidated session by removing its encrypted payload. The vault is already
    /// ciphertext, so deletion needs no plaintext unwrap and must never display Windows Hello.
    pub fn invalidate(&self) -> FcpResult<()> {
        self.vault_store.delete(self.group_id)
    }

    fn require_authorization(
        &self,
        authorization: &ConsumedCapability,
        expected_operation: CapabilityOperation,
    ) -> FcpResult<()> {
        if authorization.account_group_id() != self.group_id {
            return Err(FcpError::Capability(
                "consumed capability belongs to another account group".into(),
            ));
        }
        if authorization.operation() != expected_operation {
            return Err(FcpError::Capability(
                "consumed capability operation does not authorize this transaction".into(),
            ));
        }
        Ok(())
    }
}
