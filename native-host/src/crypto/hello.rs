use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use windows::Win32::Networking::WindowsWebServices::{
    WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE, WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM,
    WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS,
    WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_CURRENT_VERSION,
    WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS,
    WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_CURRENT_VERSION, WEBAUTHN_CLIENT_DATA,
    WEBAUTHN_CLIENT_DATA_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETER,
    WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETERS,
    WEBAUTHN_CREDENTIAL, WEBAUTHN_CREDENTIAL_CURRENT_VERSION, WEBAUTHN_CREDENTIAL_EX,
    WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION, WEBAUTHN_CREDENTIAL_LIST, WEBAUTHN_CREDENTIALS,
    WEBAUTHN_CTAP_TRANSPORT_INTERNAL, WEBAUTHN_RP_ENTITY_INFORMATION,
    WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION, WEBAUTHN_USER_ENTITY_INFORMATION,
    WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
    WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED, WebAuthNAuthenticatorGetAssertion,
    WebAuthNAuthenticatorMakeCredential, WebAuthNFreeAssertion, WebAuthNFreeCredentialAttestation,
    WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable,
};
use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM,
    BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS, BCRYPT_KEY_HANDLE, BCryptCloseAlgorithmProvider,
    BCryptDestroyKey, BCryptImportKeyPair, BCryptOpenAlgorithmProvider, BCryptVerifySignature,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::core::PCWSTR;

use crate::atomic_file::write_verified;
use crate::crypto::capability::{CapabilitySigner, CapabilityVerifier, PlatformAuthorizer};
use crate::crypto::fill_random;
use crate::crypto::webauthn_codec::{
    der_ecdsa_signature_to_raw, hex_decode, hex_encode, parse_attested_credential,
};
use crate::protocol::messages::{CapabilityPayload, SignedCapability};
use crate::{FcpError, FcpResult};

/// Not a real, resolvable domain: this credential is never used by a browser or checked against
/// an external Relying Party, so any syntactically valid RP id works. Windows shows this string
/// verbatim in the Hello consent dialog on every sign (not just at enrollment), so it was renamed
/// alongside the product (was "fursoy-cookie-protector.local") — this invalidates any existing
/// stored credential, which the app transparently recreates on next use (see ADR-023).
const RP_ID: &str = "fursoy-vault.local";
const RP_NAME: &str = "FURSOY Vault";
const ORIGIN: &str = "https://fursoy-vault.local";
const USER_NAME: &str = "fursoy-vault";
const COSE_ALGORITHM_ES256: i32 = -7;

pub struct HelloAuthorizer {
    credential_id: Vec<u8>,
    public_key_x: [u8; 32],
    public_key_y: [u8; 32],
    // Kept alive for the authorizer's whole lifetime. Without an initialized COM apartment on
    // this thread, WebAuthNAuthenticatorGetAssertion measurably delays showing its UI (several
    // seconds) — presumably falling back to bootstrapping COM itself on every call instead of
    // reusing an apartment that is already there.
    _apartment: ComApartment,
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> FcpResult<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(FcpError::from)?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialRegistry {
    version: u16,
    profile_user_id_hex: String,
    credential_id_hex: String,
    public_key_x_hex: String,
    public_key_y_hex: String,
}

impl PlatformAuthorizer for HelloAuthorizer {
    /// Opens an already-enrolled credential without mutating or replacing it.
    fn open_existing(credential_path: &Path) -> FcpResult<Self> {
        let apartment = ComApartment::initialize()?;
        let (credential_id, public_key_x, public_key_y) = load_registry(credential_path)?;
        Ok(Self {
            credential_id,
            public_key_x,
            public_key_y,
            _apartment: apartment,
        })
    }

    fn open_or_create(credential_path: &Path) -> FcpResult<Self> {
        let apartment = ComApartment::initialize()?;
        if credential_path.exists() {
            match load_registry(credential_path) {
                Ok((credential_id, public_key_x, public_key_y)) => {
                    return Ok(Self {
                        credential_id,
                        public_key_x,
                        public_key_y,
                        _apartment: apartment,
                    });
                }
                Err(_) => {
                    // The registry is only a pointer/public-key cache; vault encryption does not
                    // depend on it. Preserve corrupt evidence and enroll a fresh platform key.
                    quarantine_registry(credential_path)?;
                }
            }
        }
        create_authorizer(apartment, credential_path)
    }

    /// Windows can remove a platform credential independently of this app (Hello reset, account
    /// recovery, TPM maintenance). `hello_credential_missing` is the only signal that means that;
    /// a user cancelling the prompt or failing verification produces a different error and must
    /// stay a hard failure, or a refused check could be retried into a fresh credential.
    fn recover_if_credential_vanished(
        &mut self,
        error: &FcpError,
        credential_path: &Path,
    ) -> FcpResult<bool> {
        if !matches!(error, FcpError::Capability(message) if message == "hello_credential_missing")
        {
            return Ok(false);
        }
        *self = Self::recreate(credential_path)?;
        Ok(true)
    }
}

impl HelloAuthorizer {
    fn recreate(credential_path: &Path) -> FcpResult<Self> {
        let apartment = ComApartment::initialize()?;
        if credential_path.exists() {
            quarantine_registry(credential_path)?;
        }
        create_authorizer(apartment, credential_path)
    }

    /// Signs `payload` with a fresh Windows Hello gesture. `WebAuthNAuthenticatorGetAssertion` is
    /// stateless — unlike the WinRT `KeyCredentialManager` backend this replaced, there is no
    /// short window in which a repeat call skips the prompt, so every call here shows UI
    /// (see ADR-021).
    fn sign_assertion(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability> {
        payload.validate_shape()?;
        let canonical = payload.canonical_bytes();
        let client_data_json = client_data_json(&canonical, "webauthn.get");
        let hwnd = unsafe { GetForegroundWindow() };

        let mut credential_id_bytes = self.credential_id.clone();
        let mut credential_id_bytes_legacy = self.credential_id.clone();
        let credential_type = windows::core::w!("public-key");
        let mut credential_ex = WEBAUTHN_CREDENTIAL_EX {
            dwVersion: WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION,
            cbId: credential_id_bytes.len() as u32,
            pbId: credential_id_bytes.as_mut_ptr(),
            pwszCredentialType: credential_type,
            // Telling Windows this credential is reachable only via the internal/platform
            // transport (never USB/NFC/BLE/hybrid) lets it skip probing those transports —
            // including hybrid's Bluetooth/QR discovery step — before showing the Hello prompt.
            // Leaving this at 0 (unspecified) was the cause of a multi-second delay before the
            // dialog appeared.
            dwTransports: WEBAUTHN_CTAP_TRANSPORT_INTERNAL,
        };
        let mut credential_ex_ptr = &mut credential_ex as *mut WEBAUTHN_CREDENTIAL_EX;
        let allow_list = WEBAUTHN_CREDENTIAL_LIST {
            cCredentials: 1,
            ppCredentials: &mut credential_ex_ptr,
        };
        // The options struct carries two separate allow-list fields: the modern, transport-aware
        // `pAllowCredentialList` (above) and this older embedded `CredentialList`. Measured on
        // Windows 11 (2026-08-08): only the embedded one was actually honoured — the CTAP request
        // logged `AllowCredentialCount: 0` with only `pAllowCredentialList` set, meaning every
        // call silently fell back to discoverable-credential mode (enumerate everything for the
        // RP) instead of verifying this one credential directly, which is measurably slower. Both
        // are set so this holds regardless of which field a given Windows build reads.
        let mut credential_legacy = WEBAUTHN_CREDENTIAL {
            dwVersion: WEBAUTHN_CREDENTIAL_CURRENT_VERSION,
            cbId: credential_id_bytes_legacy.len() as u32,
            pbId: credential_id_bytes_legacy.as_mut_ptr(),
            pwszCredentialType: credential_type,
        };

        let mut client_data_bytes = client_data_json;
        let client_data = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: client_data_bytes.len() as u32,
            pbClientDataJSON: client_data_bytes.as_mut_ptr(),
            pwszHashAlgId: windows::core::w!("SHA-256"),
        };

        // A V1 dwVersion tells Windows to only honour the fields that existed in that struct
        // layout — pAllowCredentialList was added later, so leaving this at V1 silently dropped
        // it regardless of what was set below, which is a second, independent reason the earlier
        // AllowCredentialCount: 0 happened.
        let mut options = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS {
            dwVersion: WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_CURRENT_VERSION,
            ..Default::default()
        };
        options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM;
        options.dwUserVerificationRequirement = WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
        options.CredentialList = WEBAUTHN_CREDENTIALS {
            cCredentials: 1,
            pCredentials: &mut credential_legacy,
        };
        options.pAllowCredentialList = &allow_list as *const _ as *mut _;

        let rp_id = widen(RP_ID);
        let assertion = unsafe {
            WebAuthNAuthenticatorGetAssertion(
                hwnd,
                PCWSTR(rp_id.as_ptr()),
                &client_data,
                Some(&options),
            )
        }
        .map_err(|error| {
            let code = error.code().0 as u32;
            if code == 0x8009_0011 || code == 0x8007_0490 {
                FcpError::Capability("hello_credential_missing".into())
            } else {
                FcpError::Capability(format!("Windows Hello signing failed: {error}"))
            }
        })?;

        let (authenticator_data, signature_der) = unsafe {
            let authenticator_data = std::slice::from_raw_parts(
                (*assertion).pbAuthenticatorData,
                (*assertion).cbAuthenticatorData as usize,
            )
            .to_vec();
            let signature_der = std::slice::from_raw_parts(
                (*assertion).pbSignature,
                (*assertion).cbSignature as usize,
            )
            .to_vec();
            WebAuthNFreeAssertion(assertion);
            (authenticator_data, signature_der)
        };

        let signature = der_ecdsa_signature_to_raw(&signature_der)?;
        let signed = SignedCapability {
            payload,
            signature: signature.to_vec(),
            proof_context: authenticator_data,
        };
        // Immediate self-check: catches an encoding mistake here rather than at consume time.
        self.verify_signature(&signed)?;
        Ok(signed)
    }
}

fn load_registry(path: &Path) -> FcpResult<(Vec<u8>, [u8; 32], [u8; 32])> {
    let bytes = std::fs::read(path)?;
    let registry: CredentialRegistry = serde_json::from_slice(&bytes)?;
    if registry.version != 2 {
        return Err(FcpError::Format(
            "unsupported Hello credential registry version".into(),
        ));
    }
    if registry.profile_user_id_hex != hex_encode(&hello_user_id(path)?) {
        return Err(FcpError::Format(
            "Hello credential belongs to another profile identity".into(),
        ));
    }
    Ok((
        hex_decode(&registry.credential_id_hex)?,
        to_array_32(&hex_decode(&registry.public_key_x_hex)?)?,
        to_array_32(&hex_decode(&registry.public_key_y_hex)?)?,
    ))
}

fn quarantine_registry(path: &Path) -> FcpResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| FcpError::Format("Hello registry has no parent".into()))?;
    let quarantine = parent.join(format!(
        "hello-credential.retired-{}.json",
        uuid::Uuid::new_v4()
    ));
    std::fs::rename(path, quarantine)?;
    Ok(())
}

fn create_authorizer(apartment: ComApartment, registry_path: &Path) -> FcpResult<HelloAuthorizer> {
    let profile_user_id = hello_user_id(registry_path)?;
    let (credential_id, public_key_x, public_key_y) = create_credential(profile_user_id)?;
    let registry = CredentialRegistry {
        version: 2,
        profile_user_id_hex: hex_encode(&profile_user_id),
        credential_id_hex: hex_encode(&credential_id),
        public_key_x_hex: hex_encode(&public_key_x),
        public_key_y_hex: hex_encode(&public_key_y),
    };
    let bytes = serde_json::to_vec_pretty(&registry)?;
    write_verified(registry_path, &bytes, |persisted| {
        let parsed: CredentialRegistry = serde_json::from_slice(persisted)?;
        if parsed.profile_user_id_hex != registry.profile_user_id_hex
            || parsed.credential_id_hex != registry.credential_id_hex
        {
            return Err(FcpError::Format(
                "Hello credential registry failed to round-trip".into(),
            ));
        }
        Ok(())
    })?;
    Ok(HelloAuthorizer {
        credential_id,
        public_key_x,
        public_key_y,
        _apartment: apartment,
    })
}

fn create_credential(mut user_id: [u8; 16]) -> FcpResult<(Vec<u8>, [u8; 32], [u8; 32])> {
    // Without an enrolled Hello gesture (PIN/biometric) on this Windows account,
    // WebAuthNAuthenticatorMakeCredential still opens a native dialog but fails inside it with an
    // opaque OS error ("there was a problem saving your shared secret"), because there is no
    // platform authenticator to complete the ceremony with. Checking first skips that confusing
    // dialog entirely and reports a specific, actionable error instead.
    if !unsafe { WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable() }
        .map_err(FcpError::from)?
        .as_bool()
    {
        return Err(FcpError::HelloNotConfigured);
    }

    let hwnd = unsafe { GetForegroundWindow() };

    let rp_id = widen(RP_ID);
    let rp_name = widen(RP_NAME);
    let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
        pwszId: PCWSTR(rp_id.as_ptr()),
        pwszName: PCWSTR(rp_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
    };

    // A platform authenticator identifies an account by (RP ID, user ID). Reusing one constant
    // user ID across Chrome profiles lets enrollment in profile B replace profile A's credential
    // on Windows Hello. The profile namespace UUID is stable, non-secret and already unique, so
    // it is the correct account identifier here. Re-enrollment replaces only this profile's key.
    let user_name = widen(USER_NAME);
    let user = WEBAUTHN_USER_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
        cbId: user_id.len() as u32,
        pbId: user_id.as_mut_ptr(),
        pwszName: PCWSTR(user_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
        pwszDisplayName: PCWSTR(user_name.as_ptr()),
    };

    let credential_type = windows::core::w!("public-key");
    let mut cose_params = [WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
        dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
        pwszCredentialType: credential_type,
        lAlg: COSE_ALGORITHM_ES256,
    }];
    let cose_credential_params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
        cCredentialParameters: cose_params.len() as u32,
        pCredentialParameters: cose_params.as_mut_ptr(),
    };

    let mut creation_nonce = [0u8; 16];
    fill_random(&mut creation_nonce)?;
    let mut client_data_bytes = client_data_json(&creation_nonce, "webauthn.create");
    let client_data = WEBAUTHN_CLIENT_DATA {
        dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
        cbClientDataJSON: client_data_bytes.len() as u32,
        pbClientDataJSON: client_data_bytes.as_mut_ptr(),
        pwszHashAlgId: windows::core::w!("SHA-256"),
    };

    let mut options = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS {
        dwVersion: WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_CURRENT_VERSION,
        ..Default::default()
    };
    options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM;
    options.dwUserVerificationRequirement = WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
    options.dwAttestationConveyancePreference = WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE;

    let attestation = unsafe {
        WebAuthNAuthenticatorMakeCredential(
            hwnd,
            &rp,
            &user,
            &cose_credential_params,
            &client_data,
            Some(&options),
        )
    }
    .map_err(|error| {
        FcpError::Capability(format!("Windows Hello credential creation failed: {error}"))
    })?;

    let result = unsafe {
        let authenticator_data = std::slice::from_raw_parts(
            (*attestation).pbAuthenticatorData,
            (*attestation).cbAuthenticatorData as usize,
        );
        parse_attested_credential(authenticator_data)
    };
    unsafe { WebAuthNFreeCredentialAttestation(Some(attestation)) };
    result
}

fn hello_user_id(registry_path: &Path) -> FcpResult<[u8; 16]> {
    let profile_id = registry_path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .ok_or_else(|| FcpError::Format("Hello registry has no profile identity".into()))?
        .parse::<uuid::Uuid>()
        .map_err(|_| FcpError::Format("Hello registry profile identity is invalid".into()))?;
    Ok(*profile_id.as_bytes())
}

impl CapabilitySigner for HelloAuthorizer {
    fn sign(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability> {
        self.sign_assertion(payload)
    }
}

impl CapabilityVerifier for HelloAuthorizer {
    fn verify_signature(&self, capability: &SignedCapability) -> FcpResult<()> {
        capability.payload.validate_shape()?;
        validate_authenticator_context(&capability.proof_context)?;
        let signature: &[u8; 64] = capability
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| FcpError::Capability("signature is not 64 bytes".into()))?;
        let client_data_json =
            client_data_json(&capability.payload.canonical_bytes(), "webauthn.get");
        let client_data_hash = Sha256::digest(&client_data_json);
        let mut message = capability.proof_context.clone();
        message.extend_from_slice(&client_data_hash);
        let digest = Sha256::digest(&message);
        verify_ecdsa_p256(&self.public_key_x, &self.public_key_y, &digest, signature)
    }
}

fn validate_authenticator_context(authenticator_data: &[u8]) -> FcpResult<()> {
    if authenticator_data.len() < 37 {
        return Err(FcpError::Capability(
            "assertion authenticatorData is truncated".into(),
        ));
    }
    let expected_rp_id_hash = Sha256::digest(RP_ID.as_bytes());
    if authenticator_data[..32] != expected_rp_id_hash[..] {
        return Err(FcpError::Capability("assertion RP ID hash mismatch".into()));
    }
    let flags = authenticator_data[32];
    if flags & 0x01 == 0 {
        return Err(FcpError::Capability("assertion lacks user presence".into()));
    }
    if flags & 0x04 == 0 {
        return Err(FcpError::Capability(
            "assertion lacks user verification".into(),
        ));
    }
    Ok(())
}

/// Deterministic clientDataJSON: the "challenge" is the hex of the exact bytes being
/// authorized, so `sign` and `verify_signature` always reconstruct identical bytes without
/// needing to persist the clientDataJSON itself.
fn client_data_json(challenge_bytes: &[u8], ceremony_type: &str) -> Vec<u8> {
    format!(
        r#"{{"type":"{ceremony_type}","challenge":"{}","origin":"{ORIGIN}","crossOrigin":false}}"#,
        hex_encode(challenge_bytes)
    )
    .into_bytes()
}

fn verify_ecdsa_p256(
    x: &[u8; 32],
    y: &[u8; 32],
    digest: &[u8],
    signature: &[u8; 64],
) -> FcpResult<()> {
    let mut blob = Vec::with_capacity(8 + 32 + 32);
    blob.extend_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend_from_slice(&32u32.to_le_bytes());
    blob.extend_from_slice(x);
    blob.extend_from_slice(y);

    let mut alg_handle = BCRYPT_ALG_HANDLE::default();
    let status = unsafe {
        BCryptOpenAlgorithmProvider(
            &mut alg_handle,
            BCRYPT_ECDSA_P256_ALGORITHM,
            PCWSTR::null(),
            Default::default(),
        )
    };
    check_ntstatus(status, "BCryptOpenAlgorithmProvider")?;
    let alg_guard = AlgHandle(alg_handle);

    let mut key_handle = BCRYPT_KEY_HANDLE::default();
    let status = unsafe {
        BCryptImportKeyPair(
            alg_guard.0,
            None,
            BCRYPT_ECCPUBLIC_BLOB,
            &mut key_handle,
            &blob,
            0,
        )
    };
    check_ntstatus(status, "BCryptImportKeyPair")?;
    let key_guard = KeyHandle(key_handle);

    let status =
        unsafe { BCryptVerifySignature(key_guard.0, None, digest, signature, BCRYPT_FLAGS(0)) };
    check_ntstatus(status, "BCryptVerifySignature")
}

struct AlgHandle(BCRYPT_ALG_HANDLE);
impl Drop for AlgHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptCloseAlgorithmProvider(self.0, 0);
        }
    }
}

struct KeyHandle(BCRYPT_KEY_HANDLE);
impl Drop for KeyHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptDestroyKey(self.0);
        }
    }
}

fn check_ntstatus(status: windows::Win32::Foundation::NTSTATUS, context: &str) -> FcpResult<()> {
    if status.0 >= 0 {
        Ok(())
    } else {
        Err(FcpError::Capability(format!(
            "{context} failed with NTSTATUS {:#x}",
            status.0
        )))
    }
}

fn to_array_32(bytes: &[u8]) -> FcpResult<[u8; 32]> {
    bytes
        .try_into()
        .map_err(|_| FcpError::Capability("Hello credential public key is not 32 bytes".into()))
}

fn widen(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assertion_data(flags: u8) -> Vec<u8> {
        let mut data = vec![0u8; 37];
        data[..32].copy_from_slice(&Sha256::digest(RP_ID.as_bytes()));
        data[32] = flags;
        data
    }

    #[test]
    fn assertion_binds_rp_presence_and_user_verification() {
        assert!(validate_authenticator_context(&assertion_data(0x01 | 0x04)).is_ok());
        assert!(validate_authenticator_context(&assertion_data(0x01)).is_err());
        assert!(validate_authenticator_context(&assertion_data(0x04)).is_err());
        let mut wrong_rp = assertion_data(0x05);
        wrong_rp[0] ^= 1;
        assert!(validate_authenticator_context(&wrong_rp).is_err());
        assert!(validate_authenticator_context(&[0u8; 36]).is_err());
    }

    #[test]
    fn hello_user_identity_is_stable_and_isolated_per_profile() {
        let first =
            Path::new("profiles/5760e42f-5c62-42e3-8f4b-b94d52e144b0/hello-credential.json");
        let second =
            Path::new("profiles/ab9ca091-b1dd-44e7-aba0-804af7511c73/hello-credential.json");
        assert_eq!(hello_user_id(first).unwrap(), hello_user_id(first).unwrap());
        assert_ne!(
            hello_user_id(first).unwrap(),
            hello_user_id(second).unwrap()
        );
    }

    #[test]
    fn legacy_shared_identity_registry_is_rejected_for_automatic_reenrollment() {
        let root = std::env::temp_dir().join(format!("fcp-hello-{}", uuid::Uuid::new_v4()));
        let profile = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&profile).unwrap();
        let registry = profile.join("hello-credential.json");
        std::fs::write(
            &registry,
            br#"{"version":1,"credential_id_hex":"00","public_key_x_hex":"00","public_key_y_hex":"00"}"#,
        )
        .unwrap();
        assert!(load_registry(&registry).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
