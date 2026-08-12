use std::io::{Read, Write};

use crate::protocol::envelope::Envelope;
use crate::{FcpError, FcpResult};

/// A conservative Phase 5 ceiling, below Chrome's larger extension-to-host allowance and equal to
/// the documented native-host-to-extension ceiling. Chunking is deliberately outside this slice.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub fn read_envelope(reader: &mut impl Read) -> FcpResult<Option<Envelope>> {
    let mut length_bytes = [0u8; 4];
    match reader.read(&mut length_bytes[..1]) {
        Ok(0) => return Ok(None),
        Ok(1) => reader.read_exact(&mut length_bytes[1..])?,
        Ok(_) => unreachable!("one-byte read returned more than one byte"),
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(FcpError::Protocol(format!(
            "native message length {length} is outside 1..={MAX_FRAME_BYTES}"
        )));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

pub fn write_envelope(writer: &mut impl Write, envelope: &Envelope) -> FcpResult<()> {
    let body = serde_json::to_vec(envelope)?;
    if body.is_empty() || body.len() > MAX_FRAME_BYTES {
        return Err(FcpError::Protocol(format!(
            "native message length {} is outside 1..={MAX_FRAME_BYTES}",
            body.len()
        )));
    }
    let length = u32::try_from(body.len())
        .map_err(|_| FcpError::Protocol("native message length exceeds u32".into()))?;

    #[cfg(test)]
    crate::test_support::check(crate::test_support::FailurePoint::ProtocolBeforeResponseWrite)?;

    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::envelope::PROTOCOL_VERSION;
    use crate::protocol::messages::{Handshake, Message, Nonce32};
    use crate::test_support::{FailingWriter, FailurePoint, fail_next};
    use std::io::Cursor;
    use uuid::Uuid;

    #[test]
    fn native_framing_round_trips() {
        let envelope = Envelope {
            v: PROTOCOL_VERSION,
            conn_nonce: Nonce32([3; 32]),
            seq: 1,
            id: Uuid::new_v4(),
            message: Message::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION,
                extension_id: "test".into(),
                profile_id: Uuid::new_v4(),
                extension_version: "0.4.1".into(),
                min_host_version: "0.4.1".into(),
                capabilities: vec![
                    "chunked_cookies".into(),
                    "request_correlation".into(),
                    "config_v3".into(),
                    "audit_recovery".into(),
                    "profile_namespace".into(),
                ],
                cached_config_digest: None,
            }),
        };
        let mut wire = Vec::new();
        write_envelope(&mut wire, &envelope).unwrap();
        let decoded = read_envelope(&mut Cursor::new(wire)).unwrap().unwrap();
        assert_eq!(decoded, envelope);
    }

    #[test]
    fn truncated_length_prefix_is_an_error_not_clean_eof() {
        assert!(read_envelope(&mut Cursor::new(vec![1, 2])).is_err());
    }

    #[test]
    fn injected_response_boundary_failure_writes_no_frame_bytes() {
        let envelope = test_envelope();
        let _failure = fail_next(FailurePoint::ProtocolBeforeResponseWrite);
        let mut wire = Vec::new();

        assert!(write_envelope(&mut wire, &envelope).is_err());
        assert!(wire.is_empty());
    }

    #[test]
    fn writer_failure_after_length_prefix_leaves_a_detectably_truncated_frame() {
        let envelope = test_envelope();
        let mut writer = FailingWriter::after_bytes(4);

        assert!(write_envelope(&mut writer, &envelope).is_err());
        assert_eq!(writer.bytes().len(), 4);
        assert!(read_envelope(&mut Cursor::new(writer.bytes())).is_err());
    }

    fn test_envelope() -> Envelope {
        Envelope {
            v: PROTOCOL_VERSION,
            conn_nonce: Nonce32([3; 32]),
            seq: 1,
            id: Uuid::new_v4(),
            message: Message::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION,
                extension_id: "test".into(),
                profile_id: Uuid::new_v4(),
                extension_version: "0.4.1".into(),
                min_host_version: "0.4.1".into(),
                capabilities: vec![
                    "chunked_cookies".into(),
                    "request_correlation".into(),
                    "config_v3".into(),
                    "audit_recovery".into(),
                    "profile_namespace".into(),
                ],
                cached_config_digest: None,
            }),
        }
    }
}
