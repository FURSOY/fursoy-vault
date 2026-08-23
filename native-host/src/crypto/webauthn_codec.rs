//! Narrow, self-contained decoders for the two binary shapes webauthn.dll hands back that this
//! crate needs to read: the COSE EC2 public key embedded in `authenticatorData`, and the DER
//! ECDSA signature in a `WEBAUTHN_ASSERTION`. Neither is a general-purpose CBOR/DER parser — each
//! only understands the exact, fixed shape Windows' own platform authenticator produces for an
//! ES256 credential, and rejects anything else rather than guessing.

use crate::{FcpError, FcpResult};

const FLAG_ATTESTED_CREDENTIAL_DATA: u8 = 0x40;
const COSE_KTY_EC2: i64 = 2;
const COSE_ALG_ES256: i64 = -7;
const COSE_CRV_P256: i64 = 1;

/// Extracts the credential ID and P-256 public key coordinates from the `authenticatorData` bytes
/// returned by `WebAuthNAuthenticatorMakeCredential`. Layout: rpIdHash(32) + flags(1) +
/// signCount(4) + [attestedCredentialData: aaguid(16) + credIdLen(2) + credId + COSE_Key].
pub fn parse_attested_credential(auth_data: &[u8]) -> FcpResult<(Vec<u8>, [u8; 32], [u8; 32])> {
    if auth_data.len() < 37 {
        return Err(FcpError::Capability(
            "authenticatorData is shorter than the fixed header".into(),
        ));
    }
    let flags = auth_data[32];
    if flags & FLAG_ATTESTED_CREDENTIAL_DATA == 0 {
        return Err(FcpError::Capability(
            "authenticatorData has no attested credential data".into(),
        ));
    }
    let mut pos = 37usize;
    pos = pos
        .checked_add(16) // aaguid, unused
        .ok_or_else(|| FcpError::Capability("authenticatorData offset overflow".into()))?;
    let cred_id_len_bytes = auth_data.get(pos..pos + 2).ok_or_else(|| {
        FcpError::Capability("authenticatorData truncated before credIdLen".into())
    })?;
    let cred_id_len = u16::from_be_bytes([cred_id_len_bytes[0], cred_id_len_bytes[1]]) as usize;
    pos += 2;
    let credential_id = auth_data
        .get(pos..pos + cred_id_len)
        .ok_or_else(|| {
            FcpError::Capability("authenticatorData truncated before credentialId".into())
        })?
        .to_vec();
    pos += cred_id_len;
    let cose_key = auth_data.get(pos..).ok_or_else(|| {
        FcpError::Capability("authenticatorData truncated before credentialPublicKey".into())
    })?;
    let (x, y) = parse_cose_ec2_public_key(cose_key)?;
    Ok((credential_id, x, y))
}

/// Parses a COSE_Key CBOR map for an ES256 EC2 key and returns its (x, y) coordinates. Only the
/// five well-known integer keys Windows emits for this algorithm are accepted; anything else
/// (a different algorithm, a nested/indefinite-length encoding) fails closed.
fn parse_cose_ec2_public_key(data: &[u8]) -> FcpResult<([u8; 32], [u8; 32])> {
    let mut reader = CborReader::new(data);
    let count = reader.read_map_header()?;
    let (mut kty, mut alg, mut crv, mut x, mut y) = (None, None, None, None, None);
    for _ in 0..count {
        let key = reader.read_int()?;
        match key {
            1 => kty = Some(reader.read_int()?),
            3 => alg = Some(reader.read_int()?),
            -1 => crv = Some(reader.read_int()?),
            -2 => x = Some(reader.read_byte_string()?),
            -3 => y = Some(reader.read_byte_string()?),
            other => {
                return Err(FcpError::Capability(format!(
                    "unexpected COSE_Key field {other}"
                )));
            }
        }
    }
    if kty != Some(COSE_KTY_EC2) || alg != Some(COSE_ALG_ES256) || crv != Some(COSE_CRV_P256) {
        return Err(FcpError::Capability(
            "COSE_Key is not an ES256 EC2 key".into(),
        ));
    }
    let x = to_array_32(x.ok_or_else(|| FcpError::Capability("COSE_Key missing x".into()))?)?;
    let y = to_array_32(y.ok_or_else(|| FcpError::Capability("COSE_Key missing y".into()))?)?;
    Ok((x, y))
}

fn to_array_32(bytes: &[u8]) -> FcpResult<[u8; 32]> {
    bytes
        .try_into()
        .map_err(|_| FcpError::Capability("COSE_Key coordinate is not 32 bytes".into()))
}

struct CborReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> CborReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, len: usize) -> FcpResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| FcpError::Capability("CBOR offset overflow".into()))?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| FcpError::Capability("CBOR input truncated".into()))?;
        self.pos = end;
        Ok(slice)
    }

    /// Reads one item header: (major type 0-7, argument). Only the definite-length encodings
    /// Windows actually emits (argument inline or in 1/2/4/8 follow-up bytes) are supported.
    fn read_header(&mut self) -> FcpResult<(u8, u64)> {
        let first = self.take(1)?[0];
        let major = first >> 5;
        let info = first & 0x1f;
        let argument = match info {
            0..=23 => info as u64,
            24 => self.take(1)?[0] as u64,
            25 => u16::from_be_bytes(self.take(2)?.try_into().unwrap()) as u64,
            26 => u32::from_be_bytes(self.take(4)?.try_into().unwrap()) as u64,
            27 => u64::from_be_bytes(self.take(8)?.try_into().unwrap()),
            _ => {
                return Err(FcpError::Capability(
                    "unsupported CBOR length encoding".into(),
                ));
            }
        };
        Ok((major, argument))
    }

    /// Reads a map header (major type 5) and returns its entry count.
    fn read_map_header(&mut self) -> FcpResult<u64> {
        let (major, count) = self.read_header()?;
        if major != 5 {
            return Err(FcpError::Capability("expected a CBOR map".into()));
        }
        Ok(count)
    }

    /// Reads an unsigned (major type 0) or negative (major type 1) integer as i64.
    fn read_int(&mut self) -> FcpResult<i64> {
        let (major, argument) = self.read_header()?;
        match major {
            0 => i64::try_from(argument)
                .map_err(|_| FcpError::Capability("CBOR uint too large".into())),
            1 => {
                let value = i64::try_from(argument)
                    .map_err(|_| FcpError::Capability("CBOR negint too large".into()))?;
                Ok(-1 - value)
            }
            _ => Err(FcpError::Capability("expected a CBOR integer".into())),
        }
    }

    /// Reads a byte string (major type 2) and returns a borrowed slice.
    fn read_byte_string(&mut self) -> FcpResult<&'a [u8]> {
        let (major, length) = self.read_header()?;
        if major != 2 {
            return Err(FcpError::Capability("expected a CBOR byte string".into()));
        }
        self.take(length as usize)
    }
}

/// Converts a DER-encoded `SEQUENCE { r INTEGER, s INTEGER }` ECDSA signature (what
/// `WEBAUTHN_ASSERTION.pbSignature` carries) into the raw, fixed-width r||s format CNG's
/// `BCryptVerifySignature` expects for an ECDSA key.
pub fn der_ecdsa_signature_to_raw(der: &[u8]) -> FcpResult<[u8; 64]> {
    let mut reader = DerReader::new(der);
    reader.expect_tag(0x30)?;
    let r = reader.read_integer()?;
    let s = reader.read_integer()?;
    let mut raw = [0u8; 64];
    raw[..32].copy_from_slice(&left_pad_32(r)?);
    raw[32..].copy_from_slice(&left_pad_32(s)?);
    Ok(raw)
}

fn left_pad_32(value: &[u8]) -> FcpResult<[u8; 32]> {
    // DER INTEGERs strip leading zero bytes (except one to keep the sign bit clear), so a P-256
    // coordinate can legally be shorter than 32 bytes; it is never longer once that guard byte is
    // dropped.
    if value.len() > 32 {
        return Err(FcpError::Capability(
            "DER integer is wider than a P-256 coordinate".into(),
        ));
    }
    let mut padded = [0u8; 32];
    padded[32 - value.len()..].copy_from_slice(value);
    Ok(padded)
}

struct DerReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> DerReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, len: usize) -> FcpResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| FcpError::Capability("DER offset overflow".into()))?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| FcpError::Capability("DER input truncated".into()))?;
        self.pos = end;
        Ok(slice)
    }

    fn expect_tag(&mut self, tag: u8) -> FcpResult<()> {
        if self.take(1)?[0] != tag {
            return Err(FcpError::Capability("unexpected DER tag".into()));
        }
        // Only short-form lengths are needed: ECDSA P-256 signatures never exceed 127 bytes.
        let _len = self.read_length()?;
        Ok(())
    }

    fn read_length(&mut self) -> FcpResult<usize> {
        let first = self.take(1)?[0];
        if first & 0x80 == 0 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        let bytes = self.take(count)?;
        let mut length = 0usize;
        for byte in bytes {
            length = length
                .checked_shl(8)
                .and_then(|value| value.checked_add(*byte as usize))
                .ok_or_else(|| FcpError::Capability("DER length overflow".into()))?;
        }
        Ok(length)
    }

    fn read_integer(&mut self) -> FcpResult<&'a [u8]> {
        if self.take(1)?[0] != 0x02 {
            return Err(FcpError::Capability("expected a DER INTEGER".into()));
        }
        let length = self.read_length()?;
        let bytes = self.take(length)?;
        // Strip a single leading 0x00 guard byte (present when the high bit would otherwise make
        // the value look negative); anything past that is a real, non-P-256-sized integer.
        Ok(match bytes {
            [0x00, rest @ ..] if rest.first().is_some_and(|b| b & 0x80 != 0) => rest,
            other => other,
        })
    }
}

/// Hex encode/decode for the small, non-secret byte blobs (credential id, public key
/// coordinates) persisted in the Hello credential registry file.
pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn hex_decode(text: &str) -> FcpResult<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) {
        return Err(FcpError::Capability("hex string has odd length".into()));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    // The odd-length case was already rejected above, so `as_chunks` leaves no remainder.
    for pair in text.as_chunks::<2>().0 {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> FcpResult<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(FcpError::Capability("invalid hex digit".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built authenticatorData for an ES256 credential: fixed rpIdHash/flags/signCount, a
    /// 2-byte credential id, and a 5-entry COSE_Key map with 32-byte x/y coordinates. Mirrors
    /// exactly what Windows' platform authenticator returns for a MakeCredential call.
    fn sample_authenticator_data(x: [u8; 32], y: [u8; 32]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend([0xAAu8; 32]); // rpIdHash (not checked here)
        data.push(0x40); // flags: AT set, no extensions
        data.extend([0u8; 4]); // signCount
        data.extend([0xBBu8; 16]); // aaguid
        data.extend(2u16.to_be_bytes()); // credIdLen
        data.extend([0x01, 0x02]); // credentialId
        data.push(0xA5); // map(5)
        data.extend([0x01, 0x02]); // kty: 2 (EC2)
        data.extend([0x03, 0x26]); // alg: -7 (ES256) -> negint arg 6 -> 0x20+6=0x26
        data.extend([0x20, 0x01]); // crv: 1 (P-256) -> key -1 -> 0x20, value 1 -> 0x01
        data.push(0x21); // key -2 (x)
        data.push(0x58); // byte string, 1-byte length follows
        data.push(32);
        data.extend(x);
        data.push(0x22); // key -3 (y)
        data.push(0x58);
        data.push(32);
        data.extend(y);
        data
    }

    #[test]
    fn parses_credential_id_and_p256_coordinates() {
        let x = [0x11u8; 32];
        let y = [0x22u8; 32];
        let auth_data = sample_authenticator_data(x, y);
        let (credential_id, parsed_x, parsed_y) = parse_attested_credential(&auth_data).unwrap();
        assert_eq!(credential_id, vec![0x01, 0x02]);
        assert_eq!(parsed_x, x);
        assert_eq!(parsed_y, y);
    }

    #[test]
    fn rejects_data_without_attested_credential_flag() {
        let mut data = vec![0u8; 37];
        data[32] = 0x00; // AT flag not set
        assert!(parse_attested_credential(&data).is_err());
    }

    #[test]
    fn rejects_truncated_authenticator_data() {
        assert!(parse_attested_credential(&[0u8; 10]).is_err());
    }

    #[test]
    fn der_signature_round_trips_full_width_coordinates() {
        // r and s both have their high bit set, so DER must prepend a 0x00 guard byte to each.
        let mut r = [0xFFu8; 32];
        r[0] = 0x80;
        let mut s = [0xEEu8; 32];
        s[0] = 0x90;
        let der = encode_der_signature(&r, &s);
        let raw = der_ecdsa_signature_to_raw(&der).unwrap();
        assert_eq!(&raw[..32], &r[..]);
        assert_eq!(&raw[32..], &s[..]);
    }

    #[test]
    fn der_signature_round_trips_short_coordinates() {
        // A coordinate with leading zero bytes and a clear high bit is legally shorter in DER.
        let mut r = [0u8; 32];
        r[30] = 0x01;
        r[31] = 0x02;
        let mut s = [0u8; 32];
        s[31] = 0x7f;
        let der = encode_der_signature(&r, &s);
        let raw = der_ecdsa_signature_to_raw(&der).unwrap();
        assert_eq!(&raw[..32], &r[..]);
        assert_eq!(&raw[32..], &s[..]);
    }

    /// Minimal DER SEQUENCE{INTEGER,INTEGER} encoder used only to build test fixtures.
    fn encode_der_signature(r: &[u8; 32], s: &[u8; 32]) -> Vec<u8> {
        let r_enc = encode_der_integer(r);
        let s_enc = encode_der_integer(s);
        let mut body = Vec::new();
        body.extend_from_slice(&r_enc);
        body.extend_from_slice(&s_enc);
        let mut out = vec![0x30, body.len() as u8];
        out.extend(body);
        out
    }

    fn encode_der_integer(value: &[u8; 32]) -> Vec<u8> {
        let mut trimmed: &[u8] = value;
        while trimmed.len() > 1 && trimmed[0] == 0 && trimmed[1] & 0x80 == 0 {
            trimmed = &trimmed[1..];
        }
        let needs_guard = trimmed[0] & 0x80 != 0;
        let mut content = Vec::new();
        if needs_guard {
            content.push(0x00);
        }
        content.extend(trimmed);
        let mut out = vec![0x02, content.len() as u8];
        out.extend(content);
        out
    }

    #[test]
    fn hex_round_trips() {
        let bytes = [0x00, 0x01, 0xAB, 0xFF];
        let text = hex_encode(&bytes);
        assert_eq!(text, "0001abff");
        assert_eq!(hex_decode(&text).unwrap(), bytes);
    }

    #[test]
    fn hex_decode_rejects_bad_input() {
        assert!(hex_decode("abc").is_err()); // odd length
        assert!(hex_decode("zz").is_err()); // invalid digit
    }
}
