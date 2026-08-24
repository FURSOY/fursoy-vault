//! Linux backend: the secret is sealed to the TPM as a keyed-hash object, so recovering it needs
//! this machine's TPM and not merely a copy of the file.
//!
//! Nothing is stored on the side. TPM primaries are deterministic for a given seed and template,
//! so the parent is recreated for each call and flushed afterwards — the blob returned to the
//! caller is the whole of the state, exactly as DPAPI's ciphertext is on Windows. As in the KEK
//! backend, every handle is flushed on the error paths too: transient object slots are scarce
//! enough that a few leaks make later operations fail for unrelated-looking reasons.

use tss_esapi::attributes::ObjectAttributesBuilder;
use tss_esapi::handles::KeyHandle;
use tss_esapi::interface_types::algorithm::{HashingAlgorithm, PublicAlgorithm};
use tss_esapi::interface_types::ecc::EccCurve;
use tss_esapi::interface_types::resource_handles::Hierarchy;
use tss_esapi::structures::{
    EccPoint, EccScheme, KeyDerivationFunctionScheme, KeyedHashScheme, Private, Public,
    PublicBuilder, PublicEccParametersBuilder, PublicKeyedHashParameters, SensitiveData,
    SymmetricDefinitionObject,
};
use tss_esapi::traits::{Marshall, UnMarshall};
use tss_esapi::{Context, TctiNameConf};

use crate::{FcpError, FcpResult};

/// Identifies this blob format so a stored secret from another scheme is refused outright rather
/// than misparsed into something that looks like a valid unseal.
const MAGIC: &[u8; 4] = b"FVS1";

pub(super) fn protect(plaintext: &[u8]) -> FcpResult<Vec<u8>> {
    let data = SensitiveData::try_from(plaintext.to_vec())
        .map_err(|_| FcpError::Crypto("secret is too large to seal"))?;

    let mut context = open_context()?;
    let parent = create_parent(&mut context)?;
    let result = (|| -> FcpResult<Vec<u8>> {
        let template = sealed_template()?;
        let created = context
            .execute_with_nullauth_session(|ctx| {
                ctx.create(parent, template, None, Some(data), None, None)
            })
            .map_err(FcpError::from)?;
        encode(
            &created.out_public.marshall().map_err(FcpError::from)?,
            created.out_private.value(),
        )
    })();
    let _ = context.flush_context(parent.into());
    result
}

pub(super) fn unprotect(ciphertext: &[u8]) -> FcpResult<Vec<u8>> {
    let (public_bytes, private_bytes) = decode(ciphertext)?;
    let public = Public::unmarshall(&public_bytes)
        .map_err(|_| FcpError::Crypto("sealed secret has a malformed public area"))?;
    let private = Private::try_from(private_bytes)
        .map_err(|_| FcpError::Crypto("sealed secret has a malformed private area"))?;

    let mut context = open_context()?;
    let parent = create_parent(&mut context)?;
    let result = (|| -> FcpResult<Vec<u8>> {
        let handle: KeyHandle = context
            .execute_with_nullauth_session(|ctx| ctx.load(parent, private, public))
            .map_err(FcpError::from)?;
        let unsealed = context
            .execute_with_nullauth_session(|ctx| ctx.unseal(handle.into()))
            .map_err(FcpError::from);
        let _ = context.flush_context(handle.into());
        Ok(unsealed?.value().to_vec())
    })();
    let _ = context.flush_context(parent.into());
    result
}

// ---------------------------------------------------------------------------------------------
// Blob framing
// ---------------------------------------------------------------------------------------------

fn encode(public: &[u8], private: &[u8]) -> FcpResult<Vec<u8>> {
    let public_len = u16::try_from(public.len())
        .map_err(|_| FcpError::Crypto("sealed public area is too large to frame"))?;
    let private_len = u16::try_from(private.len())
        .map_err(|_| FcpError::Crypto("sealed private area is too large to frame"))?;
    let mut out = Vec::with_capacity(8 + public.len() + private.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&public_len.to_le_bytes());
    out.extend_from_slice(public);
    out.extend_from_slice(&private_len.to_le_bytes());
    out.extend_from_slice(private);
    Ok(out)
}

fn decode(bytes: &[u8]) -> FcpResult<(Vec<u8>, Vec<u8>)> {
    let malformed = || FcpError::Crypto("sealed secret is malformed");
    let rest = bytes.strip_prefix(MAGIC).ok_or_else(malformed)?;
    let (public, rest) = take_field(rest).ok_or_else(malformed)?;
    let (private, rest) = take_field(rest).ok_or_else(malformed)?;
    if !rest.is_empty() {
        return Err(malformed());
    }
    Ok((public.to_vec(), private.to_vec()))
}

fn take_field(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (length, rest) = bytes.split_at_checked(2)?;
    let length = usize::from(u16::from_le_bytes([length[0], length[1]]));
    rest.split_at_checked(length)
}

// ---------------------------------------------------------------------------------------------
// TPM plumbing
// ---------------------------------------------------------------------------------------------

/// Mirrors the KEK backend: only a real TPM character device is accepted unless this was built for
/// development. A simulator would still round-trip, which is exactly why it must not be reachable
/// by accident — the protection this offers is the hardware binding, not the encryption.
fn open_context() -> FcpResult<Context> {
    let tcti = std::env::var("FCP_TPM_TCTI").unwrap_or_else(|_| "device:/dev/tpmrm0".into());
    if !cfg!(feature = "tpm-simulator") && !tcti.starts_with("device:") {
        return Err(FcpError::Crypto(
            "refusing a non-hardware TPM: this build requires a TPM character device",
        ));
    }
    let tcti = tcti
        .parse::<TctiNameConf>()
        .map_err(|_| FcpError::Crypto("TPM TCTI configuration is not valid"))?;
    Context::new(tcti).map_err(FcpError::from)
}

fn create_parent(context: &mut Context) -> FcpResult<KeyHandle> {
    let attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_restricted(true)
        .with_decrypt(true)
        // This parent has no authValue, so dictionary-attack protection guards nothing here while
        // making it fail whenever the TPM is locked out over some other object's PIN.
        .with_no_da(true)
        .build()
        .map_err(FcpError::from)?;

    let public = PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::Ecc)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(attributes)
        .with_ecc_parameters(
            PublicEccParametersBuilder::new()
                .with_ecc_scheme(EccScheme::Null)
                .with_curve(EccCurve::NistP256)
                .with_is_signing_key(false)
                .with_is_decryption_key(true)
                .with_restricted(true)
                .with_symmetric(SymmetricDefinitionObject::AES_128_CFB)
                .with_key_derivation_function_scheme(KeyDerivationFunctionScheme::Null)
                .build()
                .map_err(FcpError::from)?,
        )
        .with_ecc_unique_identifier(EccPoint::default())
        .build()
        .map_err(FcpError::from)?;

    let created = context
        .execute_with_nullauth_session(|ctx| {
            ctx.create_primary(Hierarchy::Owner, public, None, None, None, None)
        })
        .map_err(FcpError::from)?;
    Ok(created.key_handle)
}

/// A sealed data object: it holds bytes and does nothing else. `sensitive_data_origin` is false
/// because the caller supplies the secret rather than the TPM generating it, and neither sign nor
/// decrypt is set, so the only thing this object can ever do is be unsealed.
fn sealed_template() -> FcpResult<Public> {
    let attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(false)
        .with_user_with_auth(true)
        .with_decrypt(false)
        .with_sign_encrypt(false)
        .with_restricted(false)
        // No authValue guards this object either; unsealing needs the TPM, not a secret the user
        // types. Leaving it under DA protection meant a locked-out TPM took the audit chain with
        // it, so the host could not even start.
        .with_no_da(true)
        .build()
        .map_err(FcpError::from)?;

    PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::KeyedHash)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(attributes)
        .with_keyed_hash_parameters(PublicKeyedHashParameters::new(KeyedHashScheme::Null))
        .with_keyed_hash_unique_identifier(Default::default())
        .build()
        .map_err(FcpError::from)
}
