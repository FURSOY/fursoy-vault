use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use windows::Win32::Networking::WindowsWebServices::{
    WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE, WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM,
    WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS, WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_CURRENT_VERSION,
    WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS, WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_CURRENT_VERSION,
    WEBAUTHN_CLIENT_DATA, WEBAUTHN_CLIENT_DATA_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETER,
    WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETERS,
    WEBAUTHN_CREDENTIAL, WEBAUTHN_CREDENTIAL_CURRENT_VERSION, WEBAUTHN_CREDENTIAL_EX,
    WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION, WEBAUTHN_CREDENTIAL_LIST, WEBAUTHN_CREDENTIALS,
    WEBAUTHN_CTAP_TRANSPORT_INTERNAL,
    WEBAUTHN_RP_ENTITY_INFORMATION, WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
    WEBAUTHN_USER_ENTITY_INFORMATION, WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
    WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED, WebAuthNAuthenticatorGetAssertion,
    WebAuthNAuthenticatorMakeCredential, WebAuthNFreeAssertion, WebAuthNFreeCredentialAttestation,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM,
    BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS, BCRYPT_KEY_HANDLE, BCryptCloseAlgorithmProvider,
    BCryptDestroyKey, BCryptImportKeyPair, BCryptOpenAlgorithmProvider, BCryptVerifySignature,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::core::PCWSTR;

use crate::atomic_file::write_verified;
use crate::crypto::capability::{CapabilitySigner, CapabilityVerifier};
use crate::crypto::fill_random;
use crate::crypto::webauthn_codec::{der_ecdsa_signature_to_raw, hex_decode, hex_encode, parse_attested_credential};
use crate::paths::DataPaths;
use crate::protocol::messages::{CapabilityPayload, SignedCapability};
use crate::{FcpError, FcpResult};

/// Not a real, resolvable domain: this credential is never used by a browser or checked against
/// an external Relying Party, so any syntactically valid RP id works. Fixed so the credential
/// created on first run can always be found again.
const RP_ID: &str = "fursoy-cookie-protector.local";
const RP_NAME: &str = "FURSOY Vault";
const ORIGIN: &str = "https://fursoy-cookie-protector.local";
/// There is exactly one local "user" for this credential; the id only needs to be stable, not
/// secret or unique across installs.
const USER_ID: &[u8; 16] = b"FCP.Hello.User.1";
const USER_NAME: &str = "fursoy-cookie-protector";
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
    credential_id_hex: String,
    public_key_x_hex: String,
    public_key_y_hex: String,
}

impl HelloAuthorizer {
    pub fn open_or_create() -> FcpResult<Self> {
        let apartment = ComApartment::initialize()?;
        let paths = DataPaths::discover()?;
        if paths.hello_credential.exists() {
            let bytes = std::fs::read(&paths.hello_credential)?;
            let registry: CredentialRegistry = serde_json::from_slice(&bytes)?;
            if registry.version != 1 {
                return Err(FcpError::Format(
                    "unsupported Hello credential registry version".into(),
                ));
            }
            return Ok(Self {
                credential_id: hex_decode(&registry.credential_id_hex)?,
                public_key_x: to_array_32(&hex_decode(&registry.public_key_x_hex)?)?,
                public_key_y: to_array_32(&hex_decode(&registry.public_key_y_hex)?)?,
                _apartment: apartment,
            });
        }
        let (credential_id, public_key_x, public_key_y) = create_credential()?;
        let registry = CredentialRegistry {
            version: 1,
            credential_id_hex: hex_encode(&credential_id),
            public_key_x_hex: hex_encode(&public_key_x),
            public_key_y_hex: hex_encode(&public_key_y),
        };
        let bytes = serde_json::to_vec_pretty(&registry)?;
        write_verified(&paths.hello_credential, &bytes, |persisted| {
            let parsed: CredentialRegistry = serde_json::from_slice(persisted)?;
            if parsed.credential_id_hex != registry.credential_id_hex {
                return Err(FcpError::Format(
                    "Hello credential registry failed to round-trip".into(),
                ));
            }
            Ok(())
        })?;
        Ok(Self {
            credential_id,
            public_key_x,
            public_key_y,
            _apartment: apartment,
        })
    }

    /// Signs `payload` with a fresh Windows Hello gesture. `WebAuthNAuthenticatorGetAssertion` is
    /// stateless — unlike the WinRT `KeyCredentialManager` handle this replaced, there is no
    /// short window in which a repeat call skips the prompt, so every call here shows UI. Kept as
    /// two names (`sign_fresh`/`sign_cached`) purely so call sites that predate this — and that
    /// may one day gain a real app-level caching layer — do not need to change.
    pub fn sign_fresh(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability> {
        self.sign(payload)
    }

    pub fn sign_cached(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability> {
        self.sign(payload)
    }

    pub fn has_cached_handle(&self, _group_id: uuid::Uuid) -> bool {
        false
    }

    pub fn clear_cached_handle(&self, _group_id: uuid::Uuid) {}

    fn sign(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability> {
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

        let mut options = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS::default();
        // A V1 dwVersion tells Windows to only honour the fields that existed in that struct
        // layout — pAllowCredentialList was added later, so leaving this at V1 silently dropped
        // it regardless of what was set below, which is a second, independent reason the earlier
        // AllowCredentialCount: 0 happened.
        options.dwVersion = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_CURRENT_VERSION;
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
        .map_err(|error| FcpError::Capability(format!("Windows Hello signing failed: {error}")))?;

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
            authenticator_data,
        };
        // Immediate self-check: catches an encoding mistake here rather than at consume time.
        self.verify_signature(&signed)?;
        Ok(signed)
    }
}

fn create_credential() -> FcpResult<(Vec<u8>, [u8; 32], [u8; 32])> {
    let hwnd = unsafe { GetForegroundWindow() };

    let rp_id = widen(RP_ID);
    let rp_name = widen(RP_NAME);
    let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
        pwszId: PCWSTR(rp_id.as_ptr()),
        pwszName: PCWSTR(rp_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
    };

    let mut user_id = *USER_ID;
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

    let mut options = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS::default();
    options.dwVersion = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_CURRENT_VERSION;
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

impl CapabilitySigner for HelloAuthorizer {
    fn sign(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability> {
        self.sign_fresh(payload)
    }
}

impl CapabilityVerifier for HelloAuthorizer {
    fn verify_signature(&self, capability: &SignedCapability) -> FcpResult<()> {
        capability.payload.validate_shape()?;
        let signature: &[u8; 64] = capability
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| FcpError::Capability("signature is not 64 bytes".into()))?;
        let client_data_json =
            client_data_json(&capability.payload.canonical_bytes(), "webauthn.get");
        let client_data_hash = Sha256::digest(&client_data_json);
        let mut message = capability.authenticator_data.clone();
        message.extend_from_slice(&client_data_hash);
        let digest = Sha256::digest(&message);
        verify_ecdsa_p256(&self.public_key_x, &self.public_key_y, &digest, signature)
    }
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

fn verify_ecdsa_p256(x: &[u8; 32], y: &[u8; 32], digest: &[u8], signature: &[u8; 64]) -> FcpResult<()> {
    let mut blob = Vec::with_capacity(8 + 32 + 32);
    blob.extend_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend_from_slice(&32u32.to_le_bytes());
    blob.extend_from_slice(x);
    blob.extend_from_slice(y);

    let mut alg_handle = BCRYPT_ALG_HANDLE::default();
    let status = unsafe {
        BCryptOpenAlgorithmProvider(&mut alg_handle, BCRYPT_ECDSA_P256_ALGORITHM, PCWSTR::null(), Default::default())
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
