use std::io::{Read, Write};

use crate::dispatcher::NativeHostApp;
use crate::dispatcher::PRODUCT_EXTENSION_ID;
use crate::instance_lock::InstanceLock;
use crate::paths::DataPaths;
use crate::protocol::envelope::{Envelope, PROTOCOL_VERSION};
use crate::protocol::framing::{read_envelope, write_envelope};
use crate::protocol::messages::{Message, Nonce32, OperationError};
use crate::{FcpError, FcpResult};

pub fn validate_caller_origin(args: &[String]) -> FcpResult<()> {
    let expected = format!("chrome-extension://{PRODUCT_EXTENSION_ID}/");
    match args.first() {
        Some(origin) if origin == &expected => Ok(()),
        _ => Err(FcpError::Protocol(
            "native host caller origin is not authorized".into(),
        )),
    }
}

/// Runs one Chrome Native Messaging connection. Chrome owns the process lifetime; all diagnostic
/// text goes to stderr because stdout is exclusively the length-prefixed protocol stream.
pub fn run_connection(reader: &mut impl Read, writer: &mut impl Write) -> FcpResult<()> {
    let paths = DataPaths::discover()?;
    // Taken before anything is opened: a concurrent instance must not reach the audit chain.
    let _lock = InstanceLock::acquire(&paths.root)?;
    let mut app = NativeHostApp::open(&paths)?;
    run_connection_with_app(reader, writer, &mut app)
}

fn run_connection_with_app(
    reader: &mut impl Read,
    writer: &mut impl Write,
    app: &mut NativeHostApp,
) -> FcpResult<()> {
    let mut connection_nonce: Option<Nonce32> = None;
    let mut incoming_sequence = 0u64;
    let mut outgoing_sequence = 0u64;

    while let Some(envelope) = read_envelope(reader)? {
        match connection_nonce {
            Some(expected_nonce) => envelope.validate(&expected_nonce, incoming_sequence)?,
            None => validate_first_envelope(&envelope)?,
        }
        let nonce = *connection_nonce.get_or_insert(envelope.conn_nonce);
        incoming_sequence = envelope.seq;

        let request_id = envelope.id;
        let output = match app.handle(envelope.message) {
            Ok(messages) => messages,
            Err(error) => {
                // Never serialize the underlying error: it may contain OS/provider detail. A
                // stable fail-closed denial is sufficient for the extension and safe to audit.
                eprintln!("native host rejected message: {}", error_category(&error));
                vec![Message::OperationError(OperationError {
                    request_id,
                    account_group_id: app.error_group_id(),
                    code: error_category(&error).into(),
                })]
            }
        };

        for message in output {
            outgoing_sequence = outgoing_sequence
                .checked_add(1)
                .ok_or_else(|| FcpError::Protocol("outgoing sequence overflow".into()))?;
            write_envelope(
                writer,
                &Envelope {
                    v: PROTOCOL_VERSION,
                    conn_nonce: nonce,
                    seq: outgoing_sequence,
                    id: request_id,
                    message,
                },
            )?;
        }
    }
    Ok(())
}

fn validate_first_envelope(envelope: &Envelope) -> FcpResult<()> {
    if !matches!(envelope.message, Message::Handshake(_)) {
        return Err(FcpError::Protocol(
            "handshake must be the first framed message".into(),
        ));
    }
    envelope.validate(&envelope.conn_nonce, 0)
}

fn error_category(error: &FcpError) -> &'static str {
    match error {
        FcpError::Io(_) => "io",
        FcpError::Json(_) => "json",
        FcpError::Windows(_) => "windows_provider",
        FcpError::Crypto(_) => "crypto",
        FcpError::Format(_) => "vault_format",
        FcpError::Protocol(_) => "protocol",
        FcpError::Capability(_) => "capability",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::messages::Handshake;
    use uuid::Uuid;

    #[test]
    fn first_frame_requires_sequence_one_handshake() {
        let envelope = Envelope {
            v: PROTOCOL_VERSION,
            conn_nonce: Nonce32([1; 32]),
            seq: 1,
            id: Uuid::new_v4(),
            message: Message::Handshake(Handshake {
                protocol_version: PROTOCOL_VERSION,
                extension_id: "test".into(),
                extension_version: "0.3.1".into(),
                min_host_version: "0.3.1".into(),
                capabilities: vec![
                    "chunked_cookies".into(),
                    "request_correlation".into(),
                    "config_v3".into(),
                    "audit_recovery".into(),
                ],
                cached_config_digest: None,
            }),
        };
        assert!(validate_first_envelope(&envelope).is_ok());
        let mut replay = envelope;
        replay.seq = 2;
        assert!(validate_first_envelope(&replay).is_err());
    }

    #[test]
    fn caller_origin_is_exactly_the_registered_extension() {
        let expected = format!("chrome-extension://{PRODUCT_EXTENSION_ID}/");
        assert!(validate_caller_origin(&[expected]).is_ok());
        assert!(validate_caller_origin(&[]).is_err());
        assert!(validate_caller_origin(&["chrome-extension://evil/".into()]).is_err());
        assert!(
            validate_caller_origin(&[format!("chrome-extension://{PRODUCT_EXTENSION_ID}")])
                .is_err()
        );
    }
}
