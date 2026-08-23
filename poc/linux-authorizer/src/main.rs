//! Faz L0 prototype: a TPM 2.0 stand-in for the Windows Hello authorizer.
//!
//! The question this exists to answer is not "can a TPM sign" — obviously it can — but whether a
//! TPM-backed authorizer can satisfy the same contract `HelloAuthorizer` satisfies, and what its
//! proof has to carry. Two findings drive the design here:
//!
//! * **User verification is structural, not a flag.** WebAuthn puts a UV bit inside
//!   `authenticatorData` because an authenticator *may* or *may not* have verified the user. A TPM
//!   key created with an `authValue` cannot produce a signature at all unless the correct PIN is
//!   supplied, so the existence of the signature *is* the proof. There is nothing to fake and no
//!   flag to check — which also means this backend must never accept a key whose attributes do not
//!   actually require auth. `assert_requires_auth` enforces that at load time.
//!
//! * **No persistent TPM state.** TPM primary keys are deterministic from the seed and the
//!   template, so the parent is recreated on every operation and flushed afterwards rather than
//!   occupying one of the very few persistent slots. Transient object slots are scarce (the
//!   simulator ran out after three leaked contexts during bring-up), so every handle opened here is
//!   flushed on the way out, including on the error paths.

use std::convert::TryFrom;
use std::path::{Path, PathBuf};

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature as P256Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use tss_esapi::attributes::ObjectAttributesBuilder;
use tss_esapi::handles::KeyHandle;
use tss_esapi::interface_types::algorithm::{HashingAlgorithm, PublicAlgorithm};
use tss_esapi::interface_types::ecc::EccCurve;
use tss_esapi::interface_types::resource_handles::Hierarchy;
use tss_esapi::constants::tss::{TPM2_RH_NULL, TPM2_ST_HASHCHECK};
use tss_esapi::structures::{
    Auth, Digest, EccPoint, EccScheme, EccSignature, HashScheme, HashcheckTicket,
    KeyDerivationFunctionScheme, Private, Public, PublicBuilder, PublicEccParametersBuilder,
    Signature, SignatureScheme, SymmetricDefinitionObject,
};
use tss_esapi::traits::{Marshall, UnMarshall};
use tss_esapi::tss2_esys::TPMT_TK_HASHCHECK;
use tss_esapi::{Context, TctiNameConf};

type Failure = Box<dyn std::error::Error>;

/// Mirrors `hello-credential.json` on the Windows side: a pointer plus a public-key cache. It is
/// not a secret and the vault's confidentiality never depends on it — the private key exists only
/// inside the TPM and cannot be exported.
#[derive(Serialize, Deserialize)]
struct Registry {
    version: u16,
    /// TPM-wrapped private area. Useless without the TPM that produced it.
    private_hex: String,
    public_hex: String,
    public_key_x_hex: String,
    public_key_y_hex: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Failure> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "selftest".into());
    let registry_path = registry_path();

    match command.as_str() {
        "enroll" => {
            let pin = require(args.next(), "enroll <pin>")?;
            enroll(&registry_path, &pin)?;
            println!("enrolled -> {}", registry_path.display());
        }
        "sign" => {
            let pin = require(args.next(), "sign <pin> <payload>")?;
            let payload = require(args.next(), "sign <pin> <payload>")?;
            let signature = sign(&registry_path, &pin, payload.as_bytes())?;
            println!("{}", hex(&signature));
        }
        "verify" => {
            let payload = require(args.next(), "verify <payload> <signature-hex>")?;
            let signature = require(args.next(), "verify <payload> <signature-hex>")?;
            verify(&registry_path, payload.as_bytes(), &unhex(&signature)?)?;
            println!("signature is valid");
        }
        "selftest" => selftest(&registry_path)?,
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The three operations the authorizer contract needs
// ---------------------------------------------------------------------------------------------

fn enroll(registry_path: &Path, pin: &str) -> Result<(), Failure> {
    let mut context = open_context()?;
    let parent = create_parent(&mut context)?;

    let result = (|| -> Result<(), Failure> {
        let auth = Auth::try_from(pin.as_bytes().to_vec())?;
        // Built outside the closure: `execute_with_nullauth_session` requires a closure that fails
        // only with `tss_esapi::Error`, so anything that can fail differently is resolved first.
        let template = signing_key_template()?;
        let created = context.execute_with_nullauth_session(|ctx| {
            ctx.create(parent, template, Some(auth), None, None, None)
        })?;

        let (x, y) = public_key_point(&created.out_public)?;
        let registry = Registry {
            version: 1,
            private_hex: hex(created.out_private.as_slice()),
            public_hex: hex(&marshal_public(&created.out_public)?),
            public_key_x_hex: hex(&x),
            public_key_y_hex: hex(&y),
        };
        std::fs::write(registry_path, serde_json::to_vec_pretty(&registry)?)?;
        Ok(())
    })();

    let _ = context.flush_context(parent.into());
    result
}

fn sign(registry_path: &Path, pin: &str, payload: &[u8]) -> Result<Vec<u8>, Failure> {
    let registry = load_registry(registry_path)?;
    let mut context = open_context()?;
    let parent = create_parent(&mut context)?;

    let result = (|| -> Result<Vec<u8>, Failure> {
        let private = Private::try_from(unhex(&registry.private_hex)?)?;
        let public = unmarshal_public(&unhex(&registry.public_hex)?)?;

        // Refuse a key that would sign without a PIN. Without this check the whole security
        // argument of this backend evaporates: a key with no auth requirement produces a
        // signature that proves nothing about who asked for it.
        assert_requires_auth(&public)?;

        let key = context
            .execute_with_nullauth_session(|ctx| ctx.load(parent, private, public))?;

        let signed = (|| -> Result<Vec<u8>, Failure> {
            context.tr_set_auth(key.into(), Auth::try_from(pin.as_bytes().to_vec())?)?;
            let digest = Digest::try_from(sha256(payload).to_vec())?;
            let validation = null_hashcheck()?;
            let signature = context.execute_with_nullauth_session(|ctx| {
                ctx.sign(
                    key,
                    digest,
                    SignatureScheme::EcDsa {
                        hash_scheme: HashScheme::new(HashingAlgorithm::Sha256),
                    },
                    validation,
                )
            })?;
            raw_signature(&signature)
        })();

        let _ = context.flush_context(key.into());
        signed
    })();

    let _ = context.flush_context(parent.into());
    result
}

/// Verification needs no TPM at all: the public key is in the registry and the maths is ordinary
/// ECDSA. This matches how the Windows side verifies with BCrypt rather than going back to the
/// authenticator, and it is what lets the host self-check a signature the moment it is produced.
fn verify(registry_path: &Path, payload: &[u8], signature: &[u8]) -> Result<(), Failure> {
    let registry = load_registry(registry_path)?;
    let mut sec1 = vec![0x04u8];
    sec1.extend_from_slice(&unhex(&registry.public_key_x_hex)?);
    sec1.extend_from_slice(&unhex(&registry.public_key_y_hex)?);

    let key = VerifyingKey::from_sec1_bytes(&sec1)?;
    let signature = P256Signature::from_slice(signature)?;
    key.verify(payload, &signature)
        .map_err(|_| "signature did not verify".into())
}

// ---------------------------------------------------------------------------------------------
// TPM plumbing
// ---------------------------------------------------------------------------------------------

fn open_context() -> Result<Context, Failure> {
    // swtpm during development, /dev/tpmrm0 on a real machine. TPM2TOOLS_TCTI is reused so the
    // shell that drives tpm2-tools and this binary agree on which TPM they mean.
    let tcti = std::env::var("TPM2TOOLS_TCTI")
        .or_else(|_| std::env::var("TCTI"))
        .unwrap_or_else(|_| "device:/dev/tpmrm0".into());
    Ok(Context::new(tcti.parse::<TctiNameConf>()?)?)
}

/// Recreated on every operation instead of being persisted: TPM primaries are deterministic for a
/// given seed and template, so this always yields the same parent without consuming a persistent
/// slot. The caller must flush it.
fn create_parent(context: &mut Context) -> Result<KeyHandle, Failure> {
    let attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_restricted(true)
        .with_decrypt(true)
        .build()?;

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
                .build()?,
        )
        .with_ecc_unique_identifier(EccPoint::default())
        .build()?;

    let created = context.execute_with_nullauth_session(|ctx| {
        ctx.create_primary(Hierarchy::Owner, public, None, None, None, None)
    })?;
    Ok(created.key_handle)
}

/// An unrestricted P-256 signing key. `user_with_auth` is what makes the PIN the gate: the TPM
/// will only sign when the caller supplies the matching authValue, and it applies its own
/// dictionary-attack lockout to wrong attempts. That lockout is the reason a six-digit PIN is
/// defensible here at all.
fn signing_key_template() -> Result<Public, Failure> {
    let attributes = ObjectAttributesBuilder::new()
        .with_fixed_tpm(true)
        .with_fixed_parent(true)
        .with_sensitive_data_origin(true)
        .with_user_with_auth(true)
        .with_sign_encrypt(true)
        .with_restricted(false)
        .build()?;

    Ok(PublicBuilder::new()
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
                .build()?,
        )
        .with_ecc_unique_identifier(EccPoint::default())
        .build()?)
}

/// The load-time half of "user verification is structural": a key that does not require its
/// authValue would sign for anyone, so this backend refuses to use one.
fn assert_requires_auth(public: &Public) -> Result<(), Failure> {
    let attributes = match public {
        Public::Ecc {
            object_attributes, ..
        } => object_attributes,
        _ => return Err("stored key is not an ECC key".into()),
    };
    if !attributes.user_with_auth() {
        return Err("stored key does not require its authValue; refusing to sign".into());
    }
    if !attributes.sign_encrypt() {
        return Err("stored key is not a signing key".into());
    }
    Ok(())
}

fn public_key_point(public: &Public) -> Result<(Vec<u8>, Vec<u8>), Failure> {
    match public {
        Public::Ecc { unique, .. } => Ok((
            pad32(unique.x().value()),
            pad32(unique.y().value()),
        )),
        _ => Err("created key is not an ECC key".into()),
    }
}

/// The TPM hands back r and s as separate buffers, so unlike the WebAuthn path there is no DER to
/// parse — they only need left-padding to the curve size before concatenation.
fn raw_signature(signature: &Signature) -> Result<Vec<u8>, Failure> {
    let ecc: &EccSignature = match signature {
        Signature::EcDsa(value) => value,
        _ => return Err("TPM returned a non-ECDSA signature".into()),
    };
    let mut raw = pad32(ecc.signature_r().value());
    raw.extend_from_slice(&pad32(ecc.signature_s().value()));
    Ok(raw)
}

/// `Public` carries TPM-defined wire structure, so it round-trips through the TPM's own
/// marshalling rather than a Rust-side encoding that could drift from it.
fn marshal_public(public: &Public) -> Result<Vec<u8>, Failure> {
    Ok(public.marshall()?)
}

fn unmarshal_public(bytes: &[u8]) -> Result<Public, Failure> {
    Ok(Public::unmarshall(bytes)?)
}

/// A null ticket: this signs a digest the caller produced rather than one the TPM hashed itself,
/// so there is no hash-validation ticket to present.
fn null_hashcheck() -> Result<HashcheckTicket, Failure> {
    Ok(HashcheckTicket::try_from(TPMT_TK_HASHCHECK {
        tag: TPM2_ST_HASHCHECK,
        hierarchy: TPM2_RH_NULL,
        digest: Default::default(),
    })?)
}

// ---------------------------------------------------------------------------------------------
// Self-test: the whole contract, including the case that must fail
// ---------------------------------------------------------------------------------------------

fn selftest(registry_path: &Path) -> Result<(), Failure> {
    const PIN: &str = "482913";
    const WRONG: &str = "000000";
    let payload = b"capability-payload-bytes";
    let mut failed = false;

    let mut check = |name: &str, ok: bool| {
        println!("    {}  {name}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failed = true;
        }
    };

    let _ = std::fs::remove_file(registry_path);
    check("enrolls a PIN-gated key", enroll(registry_path, PIN).is_ok());

    let signature = sign(registry_path, PIN, payload);
    check("signs with the correct PIN", signature.is_ok());

    match &signature {
        Ok(bytes) => {
            check("signature is 64 raw bytes", bytes.len() == 64);
            check(
                "signature verifies",
                verify(registry_path, payload, bytes).is_ok(),
            );
            check(
                "tampered payload is rejected",
                verify(registry_path, b"tampered-payload-bytes!!", bytes).is_err(),
            );
        }
        Err(error) => println!("      signing failed: {error}"),
    }

    check(
        "wrong PIN is refused",
        sign(registry_path, WRONG, payload).is_err(),
    );

    println!();
    if failed {
        Err("selftest failed".into())
    } else {
        println!("TPM+PIN satisfies the authorizer contract");
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------------------------

fn registry_path() -> PathBuf {
    std::env::var_os("LINUX_AUTHORIZER_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("linux-authorizer-credential.json"))
}

fn load_registry(path: &Path) -> Result<Registry, Failure> {
    let registry: Registry = serde_json::from_slice(&std::fs::read(path)?)?;
    if registry.version != 1 {
        return Err("unsupported registry version".into());
    }
    Ok(registry)
}

fn require(value: Option<String>, usage: &str) -> Result<String, Failure> {
    value.ok_or_else(|| format!("usage: linux-authorizer {usage}").into())
}

/// TPM buffers drop leading zero bytes; the fixed-width form is what a P-256 verifier expects.
fn pad32(bytes: &[u8]) -> Vec<u8> {
    let mut padded = vec![0u8; 32_usize.saturating_sub(bytes.len())];
    padded.extend_from_slice(bytes);
    padded
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, Failure> {
    if text.len() % 2 != 0 {
        return Err("hex string has an odd length".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|e| e.to_string().into()))
        .collect()
}

