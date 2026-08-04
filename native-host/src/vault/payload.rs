use serde::{Deserialize, Serialize};

use crate::protocol::messages::CookieRecord;

pub const PAYLOAD_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultPayload {
    pub schema_version: u16,
    pub vault_sequence: u64,
    pub cookies: Vec<CookieRecord>,
}

impl VaultPayload {
    pub fn empty() -> Self {
        Self {
            schema_version: PAYLOAD_SCHEMA_VERSION,
            vault_sequence: 0,
            cookies: Vec::new(),
        }
    }
}
