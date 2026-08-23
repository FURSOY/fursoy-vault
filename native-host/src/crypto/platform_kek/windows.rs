//! Windows KEK backend: the Platform Crypto Provider owns an RSA-2048 key under a name, so there
//! is no key material for this code to persist and `key_path` is unused here.
//!
//! Every operation opens a fresh provider and key handle and drops both immediately. That is
//! deliberate: a long-lived handle would be a decrypt oracle for the lifetime of the process, and
//! the cost of reopening is irrelevant next to the user-verification step that gates the callers
//! which matter.

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Security::Cryptography::{
    BCRYPT_OAEP_PADDING_INFO, BCRYPT_SHA256_ALGORITHM, CERT_KEY_SPEC, MS_PLATFORM_CRYPTO_PROVIDER,
    NCRYPT_ALGORITHM_PROPERTY, NCRYPT_EXPORT_POLICY_PROPERTY, NCRYPT_FLAGS, NCRYPT_HANDLE,
    NCRYPT_IMPL_HARDWARE_FLAG, NCRYPT_IMPL_SOFTWARE_FLAG, NCRYPT_IMPL_TYPE_PROPERTY,
    NCRYPT_KEY_HANDLE, NCRYPT_LENGTH_PROPERTY, NCRYPT_PAD_OAEP_FLAG,
    NCRYPT_PCP_TPM_VERSION_PROPERTY, NCRYPT_PROV_HANDLE, NCRYPT_RSA_ALGORITHM,
    NCryptCreatePersistedKey, NCryptDecrypt, NCryptEncrypt, NCryptFinalizeKey, NCryptFreeObject,
    NCryptGetProperty, NCryptOpenKey, NCryptOpenStorageProvider, NCryptSetProperty,
};
use windows::Win32::Security::OBJECT_SECURITY_INFORMATION;
use windows::core::{PCWSTR, w};
use zeroize::Zeroize;

use super::{RSA_BITS, WRAPPED_DEK_BYTES};
use crate::crypto::aead::{DEK_BYTES, SecretDek};
use crate::{FcpError, FcpResult};

/// The provider-side name Windows locates the persisted key by. See the note on `KEK_KEY_ID`.
pub(super) const KEK_NAME: PCWSTR = w!("FURSOY.Vault.KEK.v1");
const NTE_BAD_KEYSET: u32 = 0x8009_0016;

pub(super) fn ensure_exists(_key_path: &Path) -> FcpResult<()> {
    let provider = Provider::open()?;
    provider.require_hardware_only()?;
    match Key::open(&provider) {
        Ok(key) => key.validate(),
        Err(FcpError::Windows(error)) if error.code().0 as u32 == NTE_BAD_KEYSET => {
            let key = Key::create(&provider)?;
            key.validate()
        }
        Err(error) => Err(error),
    }
}

pub(super) fn wrap_dek(_key_path: &Path, dek: &SecretDek) -> FcpResult<Vec<u8>> {
    let provider = Provider::open()?;
    provider.require_hardware_only()?;
    let key = Key::open(&provider)?;
    key.validate()?;
    let padding = oaep_padding();
    let padding_ptr = (&padding as *const BCRYPT_OAEP_PADDING_INFO).cast::<c_void>();
    let mut wrapped_length = 0u32;
    unsafe {
        NCryptEncrypt(
            key.0,
            Some(dek.expose()),
            Some(padding_ptr),
            None,
            &mut wrapped_length,
            NCRYPT_PAD_OAEP_FLAG,
        )?;
    }
    if wrapped_length as usize != WRAPPED_DEK_BYTES {
        return Err(FcpError::Crypto("RSA-2048 wrapped DEK length is not 256"));
    }
    let mut wrapped = vec![0u8; wrapped_length as usize];
    unsafe {
        NCryptEncrypt(
            key.0,
            Some(dek.expose()),
            Some(padding_ptr),
            Some(&mut wrapped),
            &mut wrapped_length,
            NCRYPT_PAD_OAEP_FLAG,
        )?;
    }
    wrapped.truncate(wrapped_length as usize);
    Ok(wrapped)
}

pub(super) fn unwrap_dek(_key_path: &Path, wrapped: &[u8]) -> FcpResult<SecretDek> {
    if wrapped.len() != WRAPPED_DEK_BYTES {
        return Err(FcpError::Format(format!(
            "wrapped DEK must be {WRAPPED_DEK_BYTES} bytes"
        )));
    }
    let provider = Provider::open()?;
    provider.require_hardware_only()?;
    let key = Key::open(&provider)?;
    key.validate()?;
    let padding = oaep_padding();
    let padding_ptr = (&padding as *const BCRYPT_OAEP_PADDING_INFO).cast::<c_void>();
    let mut recovered = [0u8; DEK_BYTES];
    let result = (|| -> FcpResult<SecretDek> {
        let mut recovered_length = 0u32;
        unsafe {
            NCryptDecrypt(
                key.0,
                Some(wrapped),
                Some(padding_ptr),
                Some(&mut recovered),
                &mut recovered_length,
                NCRYPT_PAD_OAEP_FLAG,
            )?;
        }
        if recovered_length as usize != DEK_BYTES {
            return Err(FcpError::Crypto("unwrapped DEK length is not 32"));
        }
        let secret = SecretDek::from_bytes(recovered);
        recovered.zeroize();
        Ok(secret)
    })();
    if result.is_err() {
        recovered.zeroize();
    }
    result
}

struct Provider(NCRYPT_PROV_HANDLE);

impl Provider {
    fn open() -> FcpResult<Self> {
        let mut handle = NCRYPT_PROV_HANDLE::default();
        unsafe { NCryptOpenStorageProvider(&mut handle, MS_PLATFORM_CRYPTO_PROVIDER, 0)? };
        Ok(Self(handle))
    }

    fn require_hardware_only(&self) -> FcpResult<()> {
        let implementation = get_u32(NCRYPT_HANDLE(self.0.0), NCRYPT_IMPL_TYPE_PROPERTY)?;
        if implementation & NCRYPT_IMPL_HARDWARE_FLAG == 0
            || implementation & NCRYPT_IMPL_SOFTWARE_FLAG != 0
        {
            return Err(FcpError::Crypto(
                "Microsoft Platform Crypto Provider is not hardware-only",
            ));
        }
        let tpm_version = get_u32(NCRYPT_HANDLE(self.0.0), NCRYPT_PCP_TPM_VERSION_PROPERTY)?;
        if tpm_version != 0x0002_0000 {
            return Err(FcpError::Crypto(
                "Microsoft Platform Crypto Provider is not TPM 2.0",
            ));
        }
        Ok(())
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { NCryptFreeObject(NCRYPT_HANDLE(self.0.0)) };
        }
    }
}

struct Key(NCRYPT_KEY_HANDLE);

impl Key {
    fn open(provider: &Provider) -> FcpResult<Self> {
        let mut handle = NCRYPT_KEY_HANDLE::default();
        unsafe {
            NCryptOpenKey(
                provider.0,
                &mut handle,
                KEK_NAME,
                CERT_KEY_SPEC(0),
                NCRYPT_FLAGS(0),
            )?;
        }
        Ok(Self(handle))
    }

    fn create(provider: &Provider) -> FcpResult<Self> {
        let mut handle = NCRYPT_KEY_HANDLE::default();
        unsafe {
            NCryptCreatePersistedKey(
                provider.0,
                &mut handle,
                NCRYPT_RSA_ALGORITHM,
                KEK_NAME,
                CERT_KEY_SPEC(0),
                NCRYPT_FLAGS(0),
            )?;
        }
        let key = Self(handle);
        set_u32(NCRYPT_HANDLE(key.0.0), NCRYPT_LENGTH_PROPERTY, RSA_BITS)?;
        set_u32(NCRYPT_HANDLE(key.0.0), NCRYPT_EXPORT_POLICY_PROPERTY, 0)?;
        // Deliberately no NCRYPT_UI_POLICY: Windows Hello authorizes only inject/unlock. This TPM
        // key must also support silent enrollment/eviction/reconciliation transactions.
        unsafe { NCryptFinalizeKey(key.0, NCRYPT_FLAGS(0))? };
        Ok(key)
    }

    fn validate(&self) -> FcpResult<()> {
        let algorithm = get_utf16(NCRYPT_HANDLE(self.0.0), NCRYPT_ALGORITHM_PROPERTY)?;
        let bits = get_u32(NCRYPT_HANDLE(self.0.0), NCRYPT_LENGTH_PROPERTY)?;
        let export_policy = get_u32(NCRYPT_HANDLE(self.0.0), NCRYPT_EXPORT_POLICY_PROPERTY)?;
        if algorithm != "RSA" || bits != RSA_BITS || export_policy != 0 {
            return Err(FcpError::Crypto(
                "persisted KEK is not non-exportable RSA-2048",
            ));
        }
        Ok(())
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { NCryptFreeObject(NCRYPT_HANDLE(self.0.0)) };
        }
    }
}

fn oaep_padding() -> BCRYPT_OAEP_PADDING_INFO {
    BCRYPT_OAEP_PADDING_INFO {
        pszAlgId: BCRYPT_SHA256_ALGORITHM,
        pbLabel: std::ptr::null_mut(),
        cbLabel: 0,
    }
}

fn set_u32(handle: NCRYPT_HANDLE, property: PCWSTR, value: u32) -> FcpResult<()> {
    unsafe {
        NCryptSetProperty(handle, property, &value.to_ne_bytes(), NCRYPT_FLAGS(0))?;
    }
    Ok(())
}

fn get_u32(handle: NCRYPT_HANDLE, property: PCWSTR) -> FcpResult<u32> {
    let bytes = get_property(handle, property)?;
    let fixed: [u8; 4] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| FcpError::Crypto("CNG u32 property has invalid length"))?;
    Ok(u32::from_ne_bytes(fixed))
}

fn get_utf16(handle: NCRYPT_HANDLE, property: PCWSTR) -> FcpResult<String> {
    let bytes = get_property(handle, property)?;
    // `as_chunks` yields fixed-size arrays, so the pair goes straight into `from_ne_bytes` with no
    // indexing. A trailing odd byte would be in `.1` and is dropped, which is correct here: a
    // half code unit cannot be part of valid UTF-16.
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_ne_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|_| FcpError::Crypto("CNG UTF-16 property is invalid"))
}

fn get_property(handle: NCRYPT_HANDLE, property: PCWSTR) -> FcpResult<Vec<u8>> {
    let mut length = 0u32;
    unsafe {
        NCryptGetProperty(
            handle,
            property,
            None,
            &mut length,
            OBJECT_SECURITY_INFORMATION(0),
        )?;
    }
    let mut bytes = vec![0u8; length as usize];
    unsafe {
        NCryptGetProperty(
            handle,
            property,
            Some(&mut bytes),
            &mut length,
            OBJECT_SECURITY_INFORMATION(0),
        )?;
    }
    bytes.truncate(length as usize);
    Ok(bytes)
}
