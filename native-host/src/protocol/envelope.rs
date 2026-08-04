use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::protocol::messages::{Message, Nonce32};
use crate::{FcpError, FcpResult};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub v: u16,
    pub conn_nonce: Nonce32,
    pub seq: u64,
    pub id: Uuid,
    #[serde(flatten)]
    pub message: Message,
}

impl Envelope {
    pub fn validate(&self, expected_nonce: &Nonce32, previous_sequence: u64) -> FcpResult<()> {
        if self.v != PROTOCOL_VERSION {
            return Err(FcpError::Protocol(format!(
                "unsupported protocol version {}",
                self.v
            )));
        }
        if &self.conn_nonce != expected_nonce {
            return Err(FcpError::Protocol("connection nonce mismatch".into()));
        }
        let expected_sequence = previous_sequence
            .checked_add(1)
            .ok_or_else(|| FcpError::Protocol("sequence overflow".into()))?;
        if self.seq != expected_sequence {
            return Err(FcpError::Protocol(format!(
                "expected sequence {expected_sequence}, got {}",
                self.seq
            )));
        }
        if self.id.is_nil() {
            return Err(FcpError::Protocol("message id must not be nil".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::messages::{Handshake, Message};

    #[test]
    fn rejects_replayed_sequence() {
        let nonce = Nonce32([1; 32]);
        let envelope = Envelope {
            v: PROTOCOL_VERSION,
            conn_nonce: nonce,
            seq: 7,
            id: Uuid::new_v4(),
            message: Message::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION,
                extension_id: "fixed-extension-id".into(),
            }),
        };
        assert!(envelope.validate(&nonce, 6).is_ok());
        assert!(envelope.validate(&nonce, 7).is_err());
    }

    #[test]
    fn strict_deserialization_rejects_unknown_payload_fields() {
        let json = r#"{
            "v":1,
            "conn_nonce":"0101010101010101010101010101010101010101010101010101010101010101",
            "seq":1,
            "id":"74b0c995-85c6-4db2-9ff4-c148068461a3",
            "type":"handshake",
            "payload":{"protocol_version":1,"extension_id":"fixed","unexpected":true}
        }"#;
        assert!(serde_json::from_str::<Envelope>(json).is_err());
    }
}
