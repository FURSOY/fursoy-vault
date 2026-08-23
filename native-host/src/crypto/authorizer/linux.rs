//! Linux backend: a TPM-held ECDSA P-256 key gated by a PIN.
//!
//! The security argument here is different in kind from the Windows one, and worth stating plainly
//! because the rest of the system depends on it. WebAuthn reports whether the user was verified as
//! a *flag*, because an authenticator may or may not have checked. A TPM key created with an
//! `authValue` cannot produce a signature at all unless the correct PIN is supplied: the existence
//! of the signature is the proof, and there is no flag to forge. The catch is that nothing stops a
//! key from being created *without* an auth requirement, so [`assert_requires_auth`] refuses to
//! sign with such a key. That check is what makes the argument hold.
//!
//! A short PIN is defensible only because the TPM applies its own dictionary-attack lockout to
//! wrong attempts. Without that, six digits would be trivially brute-forced by anyone holding the
//! key blobs.
//!
//! The PIN is collected by this process, never by the extension. Sending it through the browser
//! would put it in the address space the whole design exists to distrust.

use std::path::Path;
use std::process::Command;

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature as P256Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tss_esapi::attributes::ObjectAttributesBuilder;
use tss_esapi::constants::tss::{TPM2_RH_NULL, TPM2_ST_HASHCHECK};
use tss_esapi::handles::KeyHandle;
use tss_esapi::interface_types::algorithm::{HashingAlgorithm, PublicAlgorithm};
use tss_esapi::interface_types::ecc::EccCurve;
use tss_esapi::interface_types::resource_handles::Hierarchy;
use tss_esapi::structures::{
    Auth, Digest as TpmDigest, EccPoint, EccScheme, EccSignature, HashScheme, HashcheckTicket,
    KeyDerivationFunctionScheme, Private, Public, PublicBuilder, PublicEccParametersBuilder,
    Signature, SignatureScheme, SymmetricDefinitionObject,
};
use tss_esapi::traits::{Marshall, UnMarshall};
use tss_esapi::tss2_esys::TPMT_TK_HASHCHECK;
use tss_esapi::{Context, TctiNameConf};
use zeroize::Zeroizing;

use crate::atomic_file::write_verified;
use crate::crypto::capability::{CapabilitySigner, CapabilityVerifier, PlatformAuthorizer};
use crate::crypto::webauthn_codec::{hex_decode, hex_encode};
use crate::protocol::messages::{CapabilityPayload, SignedCapability};
use crate::{FcpError, FcpResult};

/// Short enough to be memorable, long enough that the TPM's lockout has something to defend.
const MINIMUM_PIN_LENGTH: usize = 6;
/// Long enough for someone to walk to their machine, short enough that a forgotten prompt does not
/// hold a vault operation open indefinitely.
const PROMPT_TIMEOUT_SECONDS: u32 = 120;

/// A pointer and a public-key cache — not a secret. The private key exists only inside the TPM and
/// the stored private area is encrypted to it, so this file is useless on another machine.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialRegistry {
    version: u16,
    private_hex: String,
    public_hex: String,
    public_key_x_hex: String,
    public_key_y_hex: String,
}

pub struct Authorizer {
    registry: CredentialRegistry,
}

impl PlatformAuthorizer for Authorizer {
    fn open_existing(credential_path: &Path) -> FcpResult<Self> {
        Ok(Self {
            registry: load_registry(credential_path)?,
        })
    }

    fn open_or_create(credential_path: &Path) -> FcpResult<Self> {
        if credential_path.exists() {
            match load_registry(credential_path) {
                Ok(registry) => return Ok(Self { registry }),
                Err(_) => {
                    // The registry is only a pointer and a public-key cache; vault encryption does
                    // not depend on it. Preserve the corrupt file as evidence and enroll again.
                    quarantine_registry(credential_path)?;
                }
            }
        }
        enroll(credential_path)
    }

    /// The TPM does not lose keys the way Windows can discard a Hello credential — the blobs are
    /// this code's to keep. The one equivalent is a TPM that no longer accepts them at all: a
    /// cleared TPM, or the file having been moved from another machine. Re-enrolling then is safe
    /// and is the only way forward; a wrong PIN is a different error and must stay a hard failure,
    /// or a refused verification could be retried into a fresh credential.
    fn recover_if_credential_vanished(
        &mut self,
        error: &FcpError,
        credential_path: &Path,
    ) -> FcpResult<bool> {
        if !matches!(error, FcpError::Capability(message) if message == "tpm_credential_unusable") {
            return Ok(false);
        }
        quarantine_registry(credential_path)?;
        *self = enroll(credential_path)?;
        Ok(true)
    }
}

impl CapabilitySigner for Authorizer {
    fn sign(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability> {
        payload.validate_shape()?;
        let message = payload.canonical_bytes();
        let pin = prompt_pin("Enter your FURSOY Vault PIN to unlock this session")?;

        let signature = with_key(&self.registry, |context, key| {
            context
                .tr_set_auth(key.into(), auth_from(&pin)?)
                .map_err(FcpError::from)?;
            let digest = TpmDigest::try_from(Sha256::digest(&message).to_vec())
                .map_err(|_| FcpError::Crypto("capability digest is not a valid TPM digest"))?;
            let validation = null_hashcheck()?;
            let signature = context
                .execute_with_nullauth_session(|ctx| {
                    ctx.sign(
                        key,
                        digest,
                        SignatureScheme::EcDsa {
                            hash_scheme: HashScheme::new(HashingAlgorithm::Sha256),
                        },
                        validation,
                    )
                })
                .map_err(|_| FcpError::Capability("user verification failed".into()))?;
            raw_signature(&signature)
        })?;

        let signed = SignedCapability {
            payload,
            signature,
            // Structural verification: there is no assertion to carry. See the module comment.
            proof_context: Vec::new(),
        };
        // Immediate self-check, as on Windows: catches an encoding mistake here rather than at
        // consume time, when the failure would look like tampering.
        self.verify_signature(&signed)?;
        Ok(signed)
    }
}

impl CapabilityVerifier for Authorizer {
    fn verify_signature(&self, capability: &SignedCapability) -> FcpResult<()> {
        capability.payload.validate_shape()?;
        if !capability.proof_context.is_empty() {
            return Err(FcpError::Capability(
                "capability carries proof context this backend never produces".into(),
            ));
        }
        let mut sec1 = vec![0x04u8];
        sec1.extend_from_slice(&hex_decode(&self.registry.public_key_x_hex)?);
        sec1.extend_from_slice(&hex_decode(&self.registry.public_key_y_hex)?);

        let key = VerifyingKey::from_sec1_bytes(&sec1)
            .map_err(|_| FcpError::Capability("stored public key is malformed".into()))?;
        let signature = P256Signature::from_slice(&capability.signature)
            .map_err(|_| FcpError::Capability("signature is not a P-256 signature".into()))?;
        key.verify(&capability.payload.canonical_bytes(), &signature)
            .map_err(|_| FcpError::Capability("capability signature did not verify".into()))
    }
}

// ---------------------------------------------------------------------------------------------
// Enrollment
// ---------------------------------------------------------------------------------------------

fn enroll(credential_path: &Path) -> FcpResult<Authorizer> {
    let pin = prompt_pin("Choose a PIN for FURSOY Vault")?;
    if pin.chars().count() < MINIMUM_PIN_LENGTH {
        return Err(FcpError::Capability(format!(
            "the PIN must be at least {MINIMUM_PIN_LENGTH} characters"
        )));
    }
    let confirmation = prompt_pin("Enter the same PIN again to confirm")?;
    if *pin != *confirmation {
        return Err(FcpError::Capability("the two PINs did not match".into()));
    }

    let mut context = open_context()?;
    let parent = create_parent(&mut context)?;
    let result = (|| -> FcpResult<Authorizer> {
        let auth = auth_from(&pin)?;
        let template = signing_key_template()?;
        let created = context
            .execute_with_nullauth_session(|ctx| {
                ctx.create(parent, template, Some(auth), None, None, None)
            })
            .map_err(FcpError::from)?;

        let (x, y) = public_key_point(&created.out_public)?;
        let registry = CredentialRegistry {
            version: 1,
            private_hex: hex_encode(created.out_private.value()),
            public_hex: hex_encode(&created.out_public.marshall().map_err(FcpError::from)?),
            public_key_x_hex: hex_encode(&x),
            public_key_y_hex: hex_encode(&y),
        };
        let bytes = serde_json::to_vec_pretty(&registry)?;
        write_verified(credential_path, &bytes, |persisted| {
            let parsed: CredentialRegistry = serde_json::from_slice(persisted)?;
            if parsed.private_hex != registry.private_hex {
                return Err(FcpError::Format(
                    "credential registry failed to round-trip".into(),
                ));
            }
            Ok(())
        })?;
        Ok(Authorizer { registry })
    })();
    let _ = context.flush_context(parent.into());
    result
}

// ---------------------------------------------------------------------------------------------
// PIN collection
// ---------------------------------------------------------------------------------------------

/// Asks through `systemd-ask-password`, which routes to a graphical agent on a desktop and to the
/// console otherwise, so this works on a headless machine too.
///
/// Two details matter. The child's stdout is captured rather than inherited: this process's stdout
/// *is* the Native Messaging stream, and a prompt written there would corrupt the protocol. And
/// `--no-tty` keeps it from trying to claim a terminal this process does not own.
fn prompt_pin(prompt: &str) -> FcpResult<Zeroizing<String>> {
    let output = Command::new("systemd-ask-password")
        .arg("--no-tty")
        .arg(format!("--timeout={PROMPT_TIMEOUT_SECONDS}"))
        .arg("--icon=security-high")
        .arg("--id=fursoy-vault")
        .arg(prompt)
        .output()
        .map_err(|_| {
            FcpError::Capability(
                "could not ask for the PIN: systemd-ask-password is unavailable".into(),
            )
        })?;

    if !output.status.success() {
        return Err(FcpError::Capability(
            "the PIN prompt was cancelled or timed out".into(),
        ));
    }
    let pin = Zeroizing::new(
        String::from_utf8(output.stdout)
            .map_err(|_| FcpError::Capability("the PIN was not valid text".into()))?,
    );
    let trimmed = Zeroizing::new(pin.trim_end_matches(['\n', '\r']).to_owned());
    if trimmed.is_empty() {
        return Err(FcpError::Capability("no PIN was entered".into()));
    }
    Ok(trimmed)
}

fn auth_from(pin: &str) -> FcpResult<Auth> {
    Auth::try_from(pin.as_bytes().to_vec())
        .map_err(|_| FcpError::Capability("the PIN is too long for this TPM".into()))
}

// ---------------------------------------------------------------------------------------------
// TPM plumbing
// ---------------------------------------------------------------------------------------------

/// Recreates the parent, loads the credential, runs `operation`, then flushes both handles
/// whatever the outcome. Transient object slots are scarce; a leak here makes later operations
/// fail with an unrelated-looking out-of-memory error.
fn with_key<T>(
    registry: &CredentialRegistry,
    operation: impl FnOnce(&mut Context, KeyHandle) -> FcpResult<T>,
) -> FcpResult<T> {
    let private = Private::try_from(hex_decode(&registry.private_hex)?)
        .map_err(|_| unusable("private area is malformed"))?;
    let public = Public::unmarshall(&hex_decode(&registry.public_hex)?)
        .map_err(|_| unusable("public area is malformed"))?;
    assert_requires_auth(&public)?;

    let mut context = open_context()?;
    let parent = create_parent(&mut context)?;
    let result = (|| -> FcpResult<T> {
        let key = context
            .execute_with_nullauth_session(|ctx| ctx.load(parent, private, public))
            .map_err(|_| unusable("the TPM will not load this credential"))?;
        let outcome = operation(&mut context, key);
        let _ = context.flush_context(key.into());
        outcome
    })();
    let _ = context.flush_context(parent.into());
    result
}

/// Distinguished from an ordinary failure so `recover_if_credential_vanished` can act on it: this
/// means the credential itself is unusable on this machine, not that verification was refused.
fn unusable(_detail: &str) -> FcpError {
    FcpError::Capability("tpm_credential_unusable".into())
}

/// Without this, a key created with no auth requirement would sign for anyone, and the signature
/// would prove nothing about who asked for it.
fn assert_requires_auth(public: &Public) -> FcpResult<()> {
    let attributes = match public {
        Public::Ecc {
            object_attributes, ..
        } => object_attributes,
        _ => return Err(unusable("stored credential is not an ECC key")),
    };
    if !attributes.user_with_auth() {
        return Err(FcpError::Capability(
            "stored credential does not require a PIN; refusing to sign".into(),
        ));
    }
    if !attributes.sign_encrypt() {
        return Err(unusable("stored credential is not a signing key"));
    }
    Ok(())
}

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

/// `user_with_auth` is what makes the PIN the gate; the ECDSA scheme is fixed in the template so
/// the TPM itself refuses to use this key any other way.
fn signing_key_template() -> FcpResult<Public> {
    let attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_sign_encrypt(true)
        .with_restricted(false)
        .build()
        .map_err(FcpError::from)?;

    PublicBuilder::new()
        .with_public_algorithm(PublicAlgorithm::Ecc)
        .with_name_hashing_algorithm(HashingAlgorithm::Sha256)
        .with_object_attributes(attributes)
        .with_ecc_parameters(
            PublicEccParametersBuilder::new()
                .with_ecc_scheme(EccScheme::EcDsa(HashScheme::new(HashingAlgorithm::Sha256)))
                .with_curve(EccCurve::NistP256)
                .with_is_signing_key(true)
                .with_is_decryption_key(false)
                .with_restricted(false)
                .with_key_derivation_function_scheme(KeyDerivationFunctionScheme::Null)
                .build()
                .map_err(FcpError::from)?,
        )
        .with_ecc_unique_identifier(EccPoint::default())
        .build()
        .map_err(FcpError::from)
}

/// The TPM returns r and s as separate buffers, so unlike the WebAuthn path there is no DER to
/// parse — each only needs left-padding to the curve size.
fn raw_signature(signature: &Signature) -> FcpResult<Vec<u8>> {
    let ecc: &EccSignature = match signature {
        Signature::EcDsa(value) => value,
        _ => return Err(FcpError::Crypto("TPM returned a non-ECDSA signature")),
    };
    let mut raw = pad32(ecc.signature_r().value());
    raw.extend_from_slice(&pad32(ecc.signature_s().value()));
    Ok(raw)
}

fn public_key_point(public: &Public) -> FcpResult<(Vec<u8>, Vec<u8>)> {
    match public {
        Public::Ecc { unique, .. } => Ok((pad32(unique.x().value()), pad32(unique.y().value()))),
        _ => Err(FcpError::Crypto("created credential is not an ECC key")),
    }
}

/// This signs a digest the caller produced rather than one the TPM hashed, so there is no
/// hash-validation ticket to present.
fn null_hashcheck() -> FcpResult<HashcheckTicket> {
    HashcheckTicket::try_from(TPMT_TK_HASHCHECK {
        tag: TPM2_ST_HASHCHECK,
        hierarchy: TPM2_RH_NULL,
        digest: Default::default(),
    })
    .map_err(FcpError::from)
}

/// TPM buffers drop leading zero bytes; a P-256 verifier expects the fixed-width form.
fn pad32(bytes: &[u8]) -> Vec<u8> {
    let mut padded = vec![0u8; 32_usize.saturating_sub(bytes.len())];
    padded.extend_from_slice(bytes);
    padded
}

fn load_registry(path: &Path) -> FcpResult<CredentialRegistry> {
    let registry: CredentialRegistry = serde_json::from_slice(&std::fs::read(path)?)?;
    if registry.version != 1 {
        return Err(FcpError::Format(
            "unsupported credential registry version".into(),
        ));
    }
    Ok(registry)
}

fn quarantine_registry(path: &Path) -> FcpResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| FcpError::Format("credential registry has no parent".into()))?;
    let quarantine = parent.join(format!(
        "authorizer-credential.retired-{}.json",
        uuid::Uuid::new_v4()
    ));
    std::fs::rename(path, quarantine)?;
    Ok(())
}
