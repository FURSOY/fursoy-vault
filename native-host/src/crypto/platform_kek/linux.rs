//! Linux KEK backend: an RSA-2048 OAEP key created inside the TPM, whose private half never
//! leaves it.
//!
//! Linux has no equivalent of NCrypt's named key store, so the TPM-wrapped key blobs are this
//! code's responsibility. They are not secret — the private key is sealed to the TPM and useless
//! anywhere else — but losing them loses the vault, so they are written with the same verified
//! atomic write the rest of the state uses.
//!
//! Two habits from the `poc/linux-authorizer` bring-up are load-bearing here rather than tidiness:
//! no persistent TPM slot is consumed (primaries are deterministic for a given seed and template,
//! so the parent is recreated per operation), and every handle is flushed on the way out including
//! the error paths. Transient object slots are scarce enough that a handful of leaked handles
//! exhausts the TPM and every later operation fails with an unrelated-looking out-of-memory error.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tss_esapi::attributes::ObjectAttributesBuilder;
use tss_esapi::handles::KeyHandle;
use tss_esapi::interface_types::algorithm::{
    HashingAlgorithm, PublicAlgorithm, RsaSchemeAlgorithm,
};
use tss_esapi::interface_types::ecc::EccCurve;
use tss_esapi::interface_types::key_bits::RsaKeyBits;
use tss_esapi::interface_types::resource_handles::Hierarchy;
use tss_esapi::structures::{
    Data, EccPoint, EccScheme, HashScheme, KeyDerivationFunctionScheme, Private, Public,
    PublicBuilder, PublicEccParametersBuilder, PublicKeyRsa, PublicRsaParametersBuilder,
    RsaDecryptionScheme, RsaExponent, RsaScheme, SymmetricDefinitionObject,
};
use tss_esapi::traits::{Marshall, UnMarshall};
use tss_esapi::{Context, TctiNameConf};
use zeroize::Zeroize;

use super::{RSA_BITS, WRAPPED_DEK_BYTES};
use crate::atomic_file::write_verified;
use crate::crypto::aead::{DEK_BYTES, SecretDek};
use crate::{FcpError, FcpResult};

/// The TPM-wrapped key, as stored next to the vault. Not a secret: the private area is encrypted
/// to the TPM's storage hierarchy and cannot be used on another machine.
#[derive(Serialize, Deserialize)]
struct KeyBlobs {
    version: u16,
    private_hex: String,
    public_hex: String,
}

pub(super) fn ensure_exists(key_path: &Path) -> FcpResult<()> {
    if key_path.exists() {
        // Prove the stored blobs still load into this TPM rather than trusting the file's
        // existence: a machine change or a cleared TPM leaves the file readable but useless, and
        // finding that out now is better than at eviction time with cookies already removed.
        with_key(key_path, |_, _| Ok(()))
    } else {
        create(key_path)
    }
}

pub(super) fn wrap_dek(key_path: &Path, dek: &SecretDek) -> FcpResult<Vec<u8>> {
    let wrapped = with_key(key_path, |context, key| {
        let message = PublicKeyRsa::try_from(dek.expose().to_vec())
            .map_err(|_| FcpError::Crypto("DEK is not a valid RSA input"))?;
        let wrapped = context
            .execute_with_nullauth_session(|ctx| {
                ctx.rsa_encrypt(key, message, oaep_scheme(), Data::default())
            })
            .map_err(FcpError::from)?;
        Ok(wrapped.value().to_vec())
    })?;
    if wrapped.len() != WRAPPED_DEK_BYTES {
        return Err(FcpError::Crypto("RSA-2048 wrapped DEK length is not 256"));
    }
    Ok(wrapped)
}

pub(super) fn unwrap_dek(key_path: &Path, wrapped: &[u8]) -> FcpResult<SecretDek> {
    if wrapped.len() != WRAPPED_DEK_BYTES {
        return Err(FcpError::Format(format!(
            "wrapped DEK must be {WRAPPED_DEK_BYTES} bytes"
        )));
    }
    let mut recovered = with_key(key_path, |context, key| {
        let cipher_text = PublicKeyRsa::try_from(wrapped.to_vec())
            .map_err(|_| FcpError::Format("wrapped DEK is not a valid RSA ciphertext".into()))?;
        let plain = context
            .execute_with_nullauth_session(|ctx| {
                ctx.rsa_decrypt(key, cipher_text, oaep_scheme(), Data::default())
            })
            .map_err(FcpError::from)?;
        Ok(plain.value().to_vec())
    })?;

    let result = if recovered.len() == DEK_BYTES {
        let mut fixed = [0u8; DEK_BYTES];
        fixed.copy_from_slice(&recovered);
        let secret = SecretDek::from_bytes(fixed);
        fixed.zeroize();
        Ok(secret)
    } else {
        Err(FcpError::Crypto("unwrapped DEK length is not 32"))
    };
    // The plaintext DEK passed through this buffer whichever way the length check went.
    recovered.zeroize();
    result
}

// ---------------------------------------------------------------------------------------------
// TPM plumbing
// ---------------------------------------------------------------------------------------------

fn create(key_path: &Path) -> FcpResult<()> {
    let mut context = open_context()?;
    let parent = create_parent(&mut context)?;

    let result = (|| -> FcpResult<()> {
        let template = kek_template()?;
        let created = context
            .execute_with_nullauth_session(|ctx| {
                ctx.create(parent, template, None, None, None, None)
            })
            .map_err(FcpError::from)?;
        let blobs = KeyBlobs {
            version: 1,
            private_hex: hex(created.out_private.value()),
            public_hex: hex(&created.out_public.marshall().map_err(FcpError::from)?),
        };
        let bytes = serde_json::to_vec_pretty(&blobs)?;
        write_verified(key_path, &bytes, |persisted| {
            let parsed: KeyBlobs = serde_json::from_slice(persisted)?;
            if parsed.private_hex != blobs.private_hex || parsed.public_hex != blobs.public_hex {
                return Err(FcpError::Format("KEK blobs failed to round-trip".into()));
            }
            Ok(())
        })
    })();

    let _ = context.flush_context(parent.into());
    result
}

/// Recreates the parent, loads the stored key, runs `operation`, then flushes both handles
/// whatever the outcome. Every entry point goes through here so no path can leak a slot.
fn with_key<T>(
    key_path: &Path,
    operation: impl FnOnce(&mut Context, KeyHandle) -> FcpResult<T>,
) -> FcpResult<T> {
    let blobs: KeyBlobs = serde_json::from_slice(&std::fs::read(key_path)?)?;
    if blobs.version != 1 {
        return Err(FcpError::Format("unsupported KEK blob version".into()));
    }
    let private =
        Private::try_from(unhex(&blobs.private_hex)?).map_err(|_| invalid("private area"))?;
    let public =
        Public::unmarshall(&unhex(&blobs.public_hex)?).map_err(|_| invalid("public area"))?;

    let mut context = open_context()?;
    let parent = create_parent(&mut context)?;
    let result = (|| -> FcpResult<T> {
        let key = context
            .execute_with_nullauth_session(|ctx| ctx.load(parent, private, public))
            .map_err(FcpError::from)?;
        let outcome = operation(&mut context, key);
        let _ = context.flush_context(key.into());
        outcome
    })();
    let _ = context.flush_context(parent.into());
    result
}

/// Refuses anything but a real TPM character device unless this was built for development.
///
/// The Windows backend enforces the same invariant through the provider's hardware-only property.
/// Here the TCTI string is what selects a software simulator, so that is what is checked. It is a
/// compile-time switch on purpose: a runtime setting is one someone can leave enabled, and a build
/// that accepts `swtpm:` in a user's hands offers hardware protection it does not have.
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

/// A decryption-only RSA-2048 key with OAEP baked into the template, so the TPM itself refuses to
/// use it with any other padding — the wrapping scheme cannot be downgraded by a caller.
fn kek_template() -> FcpResult<Public> {
    let attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_decrypt(true)
        .with_sign_encrypt(false)
        .with_restricted(false)
        .build()
        .map_err(FcpError::from)?;

    PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::Rsa)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(attributes)
        .with_rsa_parameters(
            PublicRsaParametersBuilder::new()
                .with_scheme(
                    RsaScheme::create(RsaSchemeAlgorithm::Oaep, Some(HashingAlgorithm::Sha256))
                        .map_err(FcpError::from)?,
                )
                .with_key_bits(rsa_key_bits()?)
                .with_exponent(RsaExponent::default())
                .with_is_signing_key(false)
                .with_is_decryption_key(true)
                .with_restricted(false)
                .build()
                .map_err(FcpError::from)?,
        )
        .with_rsa_unique_identifier(PublicKeyRsa::default())
        .build()
        .map_err(FcpError::from)
}

fn rsa_key_bits() -> FcpResult<RsaKeyBits> {
    match RSA_BITS {
        2048 => Ok(RsaKeyBits::Rsa2048),
        _ => Err(FcpError::Crypto("unsupported KEK modulus size")),
    }
}

fn oaep_scheme() -> RsaDecryptionScheme {
    RsaDecryptionScheme::Oaep(HashScheme::new(HashingAlgorithm::Sha256))
}

fn invalid(part: &'static str) -> FcpError {
    match part {
        "private area" => FcpError::Format("stored KEK private area is malformed".into()),
        _ => FcpError::Format("stored KEK public area is malformed".into()),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> FcpResult<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return Err(FcpError::Format("KEK blob hex has odd length".into()));
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let text = std::str::from_utf8(pair)
                .map_err(|_| FcpError::Format("KEK blob hex is not ASCII".into()))?;
            u8::from_str_radix(text, 16)
                .map_err(|_| FcpError::Format("KEK blob hex is malformed".into()))
        })
        .collect()
}
