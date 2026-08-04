use windows::Security::Credentials::{
    KeyCredential, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use windows::Security::Cryptography::Core::{
    AsymmetricAlgorithmNames, AsymmetricKeyAlgorithmProvider, CryptographicEngine,
    CryptographicPublicKeyBlobType,
};
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::core::{Array, HSTRING};

use crate::crypto::capability::{CapabilitySigner, CapabilityVerifier};
use crate::protocol::messages::{CapabilityPayload, SignedCapability};
use crate::{FcpError, FcpResult};

const HELLO_CREDENTIAL_NAME: &str = "FURSOY.CookieProtector.Hello.v1";

pub struct HelloAuthorizer {
    // The apartment lives for the native connection, preventing repeated WinRT teardown crashes.
    // KeyCredential handles do NOT live here: every capability opens a fresh handle so Windows
    // Hello's same-handle gesture cache cannot silently authorize a later operation.
    _apartment: WinRtApartment,
}

impl HelloAuthorizer {
    pub fn open_or_create() -> FcpResult<Self> {
        let apartment = WinRtApartment::initialize()?;
        if !KeyCredentialManager::IsSupportedAsync()?.join()? {
            return Err(FcpError::Capability(
                "Windows Hello KeyCredentialManager is unsupported".into(),
            ));
        }
        let name = HSTRING::from(HELLO_CREDENTIAL_NAME);
        let credential = open_or_create_credential(&name)?;
        // Release the bootstrap handle while the retained apartment is alive. Sensitive signing
        // always obtains a separate fresh handle in sign().
        drop(credential);
        Ok(Self {
            _apartment: apartment,
        })
    }
}

fn open_or_create_credential(name: &HSTRING) -> FcpResult<KeyCredential> {
    let retrieval =
        KeyCredentialManager::RequestCreateAsync(name, KeyCredentialCreationOption::FailIfExists)?
            .join()?;
    let status = retrieval.Status()?;
    let credential = match status {
        KeyCredentialStatus::Success => retrieval.Credential()?,
        KeyCredentialStatus::CredentialAlreadyExists => open_credential(name)?,
        _ => {
            return Err(FcpError::Capability(format!(
                "Windows Hello credential creation returned status {}",
                status.0
            )));
        }
    };
    Ok(credential)
}

impl CapabilitySigner for HelloAuthorizer {
    fn sign(&self, payload: CapabilityPayload) -> FcpResult<SignedCapability> {
        payload.validate_shape()?;
        let name = HSTRING::from(HELLO_CREDENTIAL_NAME);
        let credential = open_credential(&name)?;
        let canonical = payload.canonical_bytes();
        let challenge = CryptographicBuffer::CreateFromByteArray(&canonical)?;
        let result = credential.RequestSignAsync(&challenge)?.join()?;
        let status = result.Status()?;
        if status != KeyCredentialStatus::Success {
            return Err(FcpError::Capability(format!(
                "Windows Hello signing returned status {}",
                status.0
            )));
        }
        let signature_buffer = result.Result()?;
        let mut signature_array = Array::<u8>::new();
        CryptographicBuffer::CopyToByteArray(&signature_buffer, &mut signature_array)?;
        let signature = signature_array.to_vec();
        let signed = SignedCapability { payload, signature };
        verify_with_credential(&credential, &signed)?;
        Ok(signed)
    }
}

impl CapabilityVerifier for HelloAuthorizer {
    fn verify_signature(&self, capability: &SignedCapability) -> FcpResult<()> {
        let name = HSTRING::from(HELLO_CREDENTIAL_NAME);
        let credential = open_credential(&name)?;
        verify_with_credential(&credential, capability)
    }
}

fn verify_with_credential(
    credential: &KeyCredential,
    capability: &SignedCapability,
) -> FcpResult<()> {
    capability.payload.validate_shape()?;
    if capability.signature.is_empty() {
        return Err(FcpError::Capability("signature is empty".into()));
    }
    let challenge =
        CryptographicBuffer::CreateFromByteArray(&capability.payload.canonical_bytes())?;
    let signature = CryptographicBuffer::CreateFromByteArray(&capability.signature)?;
    let public_key = credential
        .RetrievePublicKeyWithBlobType(CryptographicPublicKeyBlobType::Pkcs1RsaPublicKey)?;
    let algorithm = AsymmetricKeyAlgorithmProvider::OpenAlgorithm(
        &AsymmetricAlgorithmNames::RsaSignPkcs1Sha256()?,
    )?;
    let verification_key = algorithm.ImportPublicKeyWithBlobType(
        &public_key,
        CryptographicPublicKeyBlobType::Pkcs1RsaPublicKey,
    )?;
    if !CryptographicEngine::VerifySignature(&verification_key, &challenge, &signature)? {
        return Err(FcpError::Capability(
            "Windows Hello capability signature verification failed".into(),
        ));
    }
    Ok(())
}

fn open_credential(name: &HSTRING) -> FcpResult<KeyCredential> {
    let retrieval = KeyCredentialManager::OpenAsync(name)?.join()?;
    let status = retrieval.Status()?;
    if status != KeyCredentialStatus::Success {
        return Err(FcpError::Capability(format!(
            "Windows Hello credential open returned status {}",
            status.0
        )));
    }
    Ok(retrieval.Credential()?)
}

struct WinRtApartment;

impl WinRtApartment {
    fn initialize() -> FcpResult<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self)
    }
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}
