use std::env;
use std::error::Error;
use std::ffi::c_void;
use std::io::{self, Write};
use std::mem::size_of;
use std::time::{Duration, Instant};

use windows::Security::Credentials::{
    KeyCredential, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use windows::Security::Cryptography::Core::{
    AsymmetricAlgorithmNames, AsymmetricKeyAlgorithmProvider, CryptographicEngine,
    CryptographicPublicKeyBlobType,
};
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::Security::Cryptography::{
    BCRYPT_OAEP_PADDING_INFO, BCRYPT_SHA256_ALGORITHM, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    BCryptGenRandom, CERT_KEY_SPEC, MS_NGC_KEY_STORAGE_PROVIDER, MS_PLATFORM_CRYPTO_PROVIDER,
    NCRYPT_ALGORITHM_PROPERTY, NCRYPT_EXPORT_POLICY_PROPERTY, NCRYPT_FLAGS, NCRYPT_HANDLE,
    NCRYPT_IMPL_HARDWARE_FLAG, NCRYPT_IMPL_SOFTWARE_FLAG, NCRYPT_IMPL_TYPE_PROPERTY,
    NCRYPT_KEY_HANDLE, NCRYPT_LENGTH_PROPERTY, NCRYPT_PAD_OAEP_FLAG,
    NCRYPT_PCP_PLATFORM_TYPE_PROPERTY, NCRYPT_PCP_TPM_VERSION_PROPERTY, NCRYPT_PROV_HANDLE,
    NCRYPT_RSA_ALGORITHM, NCRYPT_UI_FORCE_HIGH_PROTECTION_FLAG, NCRYPT_UI_POLICY,
    NCRYPT_UI_POLICY_PROPERTY, NCRYPT_UI_PROTECT_KEY_FLAG, NCRYPT_UNIQUE_NAME_PROPERTY,
    NCryptCreatePersistedKey, NCryptDecrypt, NCryptDeleteKey, NCryptEncrypt, NCryptFinalizeKey,
    NCryptFreeObject, NCryptGetProperty, NCryptOpenKey, NCryptOpenStorageProvider,
    NCryptSetProperty,
};
use windows::Win32::Security::OBJECT_SECURITY_INFORMATION;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::core::{HSTRING, PCWSTR, w};
use zeroize::Zeroize;

const KEY_NAME: PCWSTR = w!("FURSOY.CookieProtector.Exp01");
const PASSPORT_KEY_NAME: PCWSTR = w!("FURSOY.CookieProtector.Exp01.Passport");
const HELLO_CREDENTIAL_NAME: &str = "FURSOY.CookieProtector.Exp01.Hello";
const RSA_BITS: u32 = 2048;
const DEFAULT_ITERATIONS: usize = 30;
const DEFAULT_HELLO_ITERATIONS: usize = 10;
const PROMPT_HEURISTIC: Duration = Duration::from_millis(500);

type ProbeResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderKind {
    Platform,
    Passport,
}

impl ProviderKind {
    fn provider_name(self) -> PCWSTR {
        match self {
            Self::Platform => MS_PLATFORM_CRYPTO_PROVIDER,
            Self::Passport => MS_NGC_KEY_STORAGE_PROVIDER,
        }
    }

    fn provider_label(self) -> &'static str {
        match self {
            Self::Platform => "Microsoft Platform Crypto Provider",
            Self::Passport => "Microsoft Passport Key Storage Provider",
        }
    }

    fn key_name(self) -> PCWSTR {
        match self {
            Self::Platform => KEY_NAME,
            Self::Passport => PASSPORT_KEY_NAME,
        }
    }
}

struct Provider {
    handle: NCRYPT_PROV_HANDLE,
    kind: ProviderKind,
}

impl Provider {
    fn open(kind: ProviderKind) -> ProbeResult<Self> {
        let mut handle = NCRYPT_PROV_HANDLE::default();
        unsafe { NCryptOpenStorageProvider(&mut handle, kind.provider_name(), 0)? };
        Ok(Self { handle, kind })
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { NCryptFreeObject(NCRYPT_HANDLE(self.handle.0)) };
        }
    }
}

struct Key(NCRYPT_KEY_HANDLE);

impl Key {
    fn open(provider: &Provider) -> ProbeResult<Self> {
        let mut handle = NCRYPT_KEY_HANDLE::default();
        unsafe {
            NCryptOpenKey(
                provider.handle,
                &mut handle,
                provider.kind.key_name(),
                CERT_KEY_SPEC(0),
                NCRYPT_FLAGS(0),
            )?
        };
        Ok(Self(handle))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { NCryptFreeObject(NCRYPT_HANDLE(self.0.0)) };
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("probe_error={error}");
        std::process::exit(1);
    }
}

fn run() -> ProbeResult<()> {
    let command = env::args().nth(1).unwrap_or_else(|| "status".to_owned());
    match command.as_str() {
        "status" => with_provider(ProviderKind::Platform, print_provider_status),
        "create" => with_provider(ProviderKind::Platform, create_key),
        "inspect" => with_open_key(ProviderKind::Platform),
        "roundtrip" => with_roundtrip(ProviderKind::Platform),
        "handle-cycle" => with_handle_cycle(ProviderKind::Platform),
        "lock-probe" => with_lock_probe(ProviderKind::Platform),
        "lock-handle-probe" => with_lock_handle_probe(ProviderKind::Platform),
        "delete" => with_provider(ProviderKind::Platform, delete_key),
        "passport-status" => with_provider(ProviderKind::Passport, print_provider_status),
        "passport-create" => with_provider(ProviderKind::Passport, create_key),
        "passport-inspect" => with_open_key(ProviderKind::Passport),
        "passport-roundtrip" => with_roundtrip(ProviderKind::Passport),
        "passport-handle-cycle" => with_handle_cycle(ProviderKind::Passport),
        "passport-lock-probe" => with_lock_probe(ProviderKind::Passport),
        "passport-delete" => with_provider(ProviderKind::Passport, delete_key),
        "hello-status" => hello_status(),
        "hello-challenge" => hello_challenge(false),
        "hello-open-challenge" => hello_challenge(true),
        "hello-sign-cycle" => hello_sign_cycle(),
        _ => {
            print_usage();
            Err(format!("unknown command: {command}").into())
        }
    }
}

fn print_usage() {
    eprintln!(
        "usage: tpm-probe [status|create|inspect|roundtrip [N]|handle-cycle [N]|lock-probe|\
         lock-handle-probe|delete|\
         passport-status|passport-create|passport-inspect|passport-roundtrip [N]|\
         passport-handle-cycle [N]|passport-lock-probe|passport-delete|\
         hello-status|hello-challenge|hello-open-challenge|hello-sign-cycle [N]]"
    );
}

fn parse_iterations() -> ProbeResult<usize> {
    match env::args().nth(2) {
        Some(value) => {
            let iterations = value.parse::<usize>()?;
            if iterations == 0 {
                return Err("iterations must be greater than zero".into());
            }
            Ok(iterations)
        }
        None => Ok(DEFAULT_ITERATIONS),
    }
}

fn parse_hello_iterations() -> ProbeResult<usize> {
    match env::args().nth(2) {
        Some(value) => {
            let iterations = value.parse::<usize>()?;
            if iterations == 0 {
                return Err("iterations must be greater than zero".into());
            }
            Ok(iterations)
        }
        None => Ok(DEFAULT_HELLO_ITERATIONS),
    }
}

fn with_provider(
    kind: ProviderKind,
    operation: fn(&Provider) -> ProbeResult<()>,
) -> ProbeResult<()> {
    let provider = Provider::open(kind)?;
    println!("provider={}", kind.provider_label());
    operation(&provider)
}

fn with_open_key(kind: ProviderKind) -> ProbeResult<()> {
    let provider = Provider::open(kind)?;
    println!("provider={}", kind.provider_label());
    let Some(key) = open_key_for_experiment(&provider, "inspect")? else {
        return Ok(());
    };
    inspect_key(&key)
}

fn with_roundtrip(kind: ProviderKind) -> ProbeResult<()> {
    let iterations = parse_iterations()?;
    let provider = Provider::open(kind)?;
    println!("provider={}", kind.provider_label());
    print_provider_status(&provider)?;
    let Some(key) = open_key_for_experiment(&provider, "roundtrip")? else {
        return Ok(());
    };
    inspect_key(&key)?;
    roundtrip(&key, iterations)
}

fn with_handle_cycle(kind: ProviderKind) -> ProbeResult<()> {
    let iterations = parse_iterations()?;
    let provider = Provider::open(kind)?;
    println!("provider={}", kind.provider_label());
    print_provider_status(&provider)?;
    handle_cycle(&provider, iterations)
}

fn with_lock_probe(kind: ProviderKind) -> ProbeResult<()> {
    let provider = Provider::open(kind)?;
    println!("provider={}", kind.provider_label());
    print_provider_status(&provider)?;
    let Some(key) = open_key_for_experiment(&provider, "lock_probe")? else {
        return Ok(());
    };
    inspect_key(&key)?;
    lock_probe(&key)
}

fn with_lock_handle_probe(kind: ProviderKind) -> ProbeResult<()> {
    let provider = Provider::open(kind)?;
    println!("provider={}", kind.provider_label());
    print_provider_status(&provider)?;
    lock_handle_probe(&provider)
}

fn print_provider_status(provider: &Provider) -> ProbeResult<()> {
    let implementation = get_u32(NCRYPT_HANDLE(provider.handle.0), NCRYPT_IMPL_TYPE_PROPERTY)?;
    println!("implementation_flags=0x{implementation:08x}");
    println!(
        "hardware_provider={}",
        implementation & NCRYPT_IMPL_HARDWARE_FLAG != 0
    );
    println!(
        "software_provider={}",
        implementation & NCRYPT_IMPL_SOFTWARE_FLAG != 0
    );

    print_optional_u32(
        NCRYPT_HANDLE(provider.handle.0),
        NCRYPT_PCP_PLATFORM_TYPE_PROPERTY,
        "pcp_platform_type",
    );
    print_optional_u32(
        NCRYPT_HANDLE(provider.handle.0),
        NCRYPT_PCP_TPM_VERSION_PROPERTY,
        "pcp_tpm_version",
    );

    if provider.kind == ProviderKind::Platform
        && (implementation & NCRYPT_IMPL_HARDWARE_FLAG == 0
            || implementation & NCRYPT_IMPL_SOFTWARE_FLAG != 0)
    {
        return Err("platform provider did not report hardware-only implementation".into());
    }
    if provider.kind == ProviderKind::Passport {
        println!("key_level_hardware_verification_required=true");
        if implementation & NCRYPT_IMPL_HARDWARE_FLAG == 0 {
            return Err("Passport provider did not report hardware capability".into());
        }
    }

    Ok(())
}

fn create_key(provider: &Provider) -> ProbeResult<()> {
    print_provider_status(provider)?;
    let mut handle = NCRYPT_KEY_HANDLE::default();
    let create_result = unsafe {
        NCryptCreatePersistedKey(
            provider.handle,
            &mut handle,
            NCRYPT_RSA_ALGORITHM,
            provider.kind.key_name(),
            CERT_KEY_SPEC(0),
            NCRYPT_FLAGS(0),
        )
    };
    if let Err(error) = create_result {
        if provider.kind == ProviderKind::Passport && is_nte_invalid_parameter(&error) {
            report_passport_direct_cng_unsupported("create", error.code().0);
            return Ok(());
        }
        return Err(error.into());
    }
    let key = Key(handle);

    set_u32(NCRYPT_HANDLE(key.0.0), NCRYPT_LENGTH_PROPERTY, RSA_BITS)?;
    set_u32(NCRYPT_HANDLE(key.0.0), NCRYPT_EXPORT_POLICY_PROPERTY, 0)?;

    if provider.kind == ProviderKind::Platform {
        let policy = NCRYPT_UI_POLICY {
            dwVersion: 1,
            dwFlags: NCRYPT_UI_PROTECT_KEY_FLAG | NCRYPT_UI_FORCE_HIGH_PROTECTION_FLAG,
            pszCreationTitle: w!("FURSOY Cookie Protector experiment"),
            pszFriendlyName: w!("FURSOY TPM/Hello probe key"),
            pszDescription: w!("Authorizes one TPM probe private-key operation"),
        };
        let policy_bytes = unsafe {
            std::slice::from_raw_parts(
                (&policy as *const NCRYPT_UI_POLICY).cast::<u8>(),
                size_of::<NCRYPT_UI_POLICY>(),
            )
        };
        unsafe {
            NCryptSetProperty(
                NCRYPT_HANDLE(key.0.0),
                NCRYPT_UI_POLICY_PROPERTY,
                policy_bytes,
                NCRYPT_FLAGS(0),
            )?;
        }
    }
    unsafe { NCryptFinalizeKey(key.0, NCRYPT_FLAGS(0))? };

    println!("key_created=true");
    inspect_key(&key)
}

fn inspect_key(key: &Key) -> ProbeResult<()> {
    let algorithm = get_utf16(NCRYPT_HANDLE(key.0.0), NCRYPT_ALGORITHM_PROPERTY)?;
    let length = get_u32(NCRYPT_HANDLE(key.0.0), NCRYPT_LENGTH_PROPERTY)?;
    let export_policy = get_u32(NCRYPT_HANDLE(key.0.0), NCRYPT_EXPORT_POLICY_PROPERTY)?;
    let unique_name = get_utf16(NCRYPT_HANDLE(key.0.0), NCRYPT_UNIQUE_NAME_PROPERTY)?;

    println!("key_algorithm={algorithm}");
    println!("key_length_bits={length}");
    println!("key_export_policy=0x{export_policy:08x}");
    println!(
        "key_unique_name_redacted={}",
        redact_unique_name(&unique_name)
    );
    println!("key_non_exportable={}", export_policy == 0);
    if algorithm != "RSA" || length != RSA_BITS || export_policy != 0 {
        return Err("key properties do not match the experiment's security requirements".into());
    }
    Ok(())
}

fn roundtrip(key: &Key, iterations: usize) -> ProbeResult<()> {
    let (mut dek, wrapped) = prepare_wrapped_dek(key)?;
    let result = (|| -> ProbeResult<()> {
        let mut samples = Vec::with_capacity(iterations);

        for index in 0..iterations {
            let elapsed = unwrap_once(key, &wrapped, &dek)?;
            samples.push(elapsed);
            println!(
                "unwrap_sample={} duration_ms={:.3}",
                index + 1,
                elapsed.as_secs_f64() * 1000.0
            );
        }

        print_timing_summary(&samples);
        println!("roundtrip_verified=true");
        Ok(())
    })();
    dek.zeroize();
    result
}

fn prepare_wrapped_dek(key: &Key) -> ProbeResult<([u8; 32], Vec<u8>)> {
    let mut dek = [0u8; 32];
    unsafe {
        BCryptGenRandom(None, &mut dek, BCRYPT_USE_SYSTEM_PREFERRED_RNG).ok()?;
    }

    let result = (|| -> ProbeResult<Vec<u8>> {
        let padding = oaep_padding();
        let padding_ptr = (&padding as *const BCRYPT_OAEP_PADDING_INFO).cast::<c_void>();
        let mut wrapped_length = 0u32;
        unsafe {
            NCryptEncrypt(
                key.0,
                Some(&dek),
                Some(padding_ptr),
                None,
                &mut wrapped_length,
                NCRYPT_PAD_OAEP_FLAG,
            )?;
        }
        let mut wrapped = vec![0u8; wrapped_length as usize];
        unsafe {
            NCryptEncrypt(
                key.0,
                Some(&dek),
                Some(padding_ptr),
                Some(&mut wrapped),
                &mut wrapped_length,
                NCRYPT_PAD_OAEP_FLAG,
            )?;
        }
        wrapped.truncate(wrapped_length as usize);
        Ok(wrapped)
    })();

    match result {
        Ok(wrapped) => Ok((dek, wrapped)),
        Err(error) => {
            dek.zeroize();
            Err(error)
        }
    }
}

fn unwrap_once(key: &Key, wrapped: &[u8], expected: &[u8; 32]) -> ProbeResult<Duration> {
    let padding = oaep_padding();
    let padding_ptr = (&padding as *const BCRYPT_OAEP_PADDING_INFO).cast::<c_void>();
    let mut recovered = [0u8; 32];
    let result = (|| -> ProbeResult<Duration> {
        let started = Instant::now();
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
        let elapsed = started.elapsed();

        if recovered_length != expected.len() as u32 || recovered != *expected {
            return Err("unwrapped DEK did not match the original".into());
        }
        Ok(elapsed)
    })();
    recovered.zeroize();
    result
}

fn oaep_padding() -> BCRYPT_OAEP_PADDING_INFO {
    BCRYPT_OAEP_PADDING_INFO {
        pszAlgId: BCRYPT_SHA256_ALGORITHM,
        pbLabel: std::ptr::null_mut(),
        cbLabel: 0,
    }
}

fn handle_cycle(provider: &Provider, iterations: usize) -> ProbeResult<()> {
    let Some(initial_key) = open_key_for_experiment(provider, "handle_cycle")? else {
        return Ok(());
    };
    inspect_key(&initial_key)?;
    let (mut dek, wrapped) = prepare_wrapped_dek(&initial_key)?;
    drop(initial_key);

    let result = (|| -> ProbeResult<()> {
        let mut samples = Vec::with_capacity(iterations);
        for index in 0..iterations {
            let key = Key::open(provider)?;
            let elapsed = unwrap_once(&key, &wrapped, &dek)?;
            drop(key);
            samples.push(elapsed);
            println!(
                "handle_cycle_sample={} duration_ms={:.3}",
                index + 1,
                elapsed.as_secs_f64() * 1000.0
            );
        }
        print_timing_summary(&samples);
        println!("handle_cycle_verified=true");
        Ok(())
    })();
    dek.zeroize();
    result
}

fn lock_probe(key: &Key) -> ProbeResult<()> {
    let (mut dek, wrapped) = prepare_wrapped_dek(key)?;
    let result = (|| -> ProbeResult<()> {
        let before = unwrap_once(key, &wrapped, &dek)?;
        println!("before_lock_ms={:.3}", before.as_secs_f64() * 1000.0);
        println!(
            "before_lock_prompt_likely_observed={}",
            prompt_likely(before)
        );
        print!("lock_instruction=Press Win+L, sign back in, then press Enter here: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let after = unwrap_once(key, &wrapped, &dek)?;
        println!("after_lock_ms={:.3}", after.as_secs_f64() * 1000.0);
        println!("after_lock_prompt_likely_observed={}", prompt_likely(after));
        println!("prompt_detection=latency_heuristic_only");
        println!(
            "note=this test reuses the same key handle across the lock boundary; see lock-handle-probe for a fresh-handle comparison"
        );
        println!("lock_probe_verified=true");
        Ok(())
    })();
    dek.zeroize();
    result
}

fn lock_handle_probe(provider: &Provider) -> ProbeResult<()> {
    let key_a = Key::open(provider)?;
    inspect_key(&key_a)?;
    let (mut dek, wrapped) = prepare_wrapped_dek(&key_a)?;
    let result = (|| -> ProbeResult<()> {
        let before = unwrap_once(&key_a, &wrapped, &dek)?;
        println!("before_lock_handle=A");
        println!("before_lock_ms={:.3}", before.as_secs_f64() * 1000.0);
        println!(
            "before_lock_prompt_likely_observed={}",
            prompt_likely(before)
        );
        drop(key_a);

        print!("lock_instruction=Press Win+L, sign back in, then press Enter here: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let key_b = Key::open(provider)?;
        let after = unwrap_once(&key_b, &wrapped, &dek)?;
        println!("after_lock_handle=B");
        println!("after_lock_ms={:.3}", after.as_secs_f64() * 1000.0);
        println!("after_lock_prompt_likely_observed={}", prompt_likely(after));
        println!("prompt_detection=latency_heuristic_only");
        println!("lock_handle_probe_verified=true");
        Ok(())
    })();
    dek.zeroize();
    result
}

fn print_timing_summary(samples: &[Duration]) {
    let first = samples[0];
    let mut steady = samples[1..].to_vec();
    steady.sort_unstable();

    println!("iterations={}", samples.len());
    println!("first_op_ms={:.3}", first.as_secs_f64() * 1000.0);
    if steady.is_empty() {
        println!("steady_state_samples=0");
        println!("steady_state_p50_ms=unavailable");
        println!("steady_state_p95_ms=unavailable");
        println!("steady_state_max_ms=unavailable");
    } else {
        println!("steady_state_samples={}", steady.len());
        println!(
            "steady_state_p50_ms={:.3}",
            percentile(&steady, 50).as_secs_f64() * 1000.0
        );
        println!(
            "steady_state_p95_ms={:.3}",
            percentile(&steady, 95).as_secs_f64() * 1000.0
        );
        println!(
            "steady_state_max_ms={:.3}",
            steady.last().unwrap().as_secs_f64() * 1000.0
        );
    }
    println!("sample_size_low={}", samples.len() < 20);
    println!("prompt_likely_observed={}", prompt_likely(first));
    println!("prompt_heuristic_threshold_ms=500");
    println!("prompt_detection=latency_heuristic_only");
}

fn prompt_likely(duration: Duration) -> bool {
    duration > PROMPT_HEURISTIC
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = (samples.len() * percentile).div_ceil(100) - 1;
    samples[index]
}

struct WinRtApartment;

impl WinRtApartment {
    fn initialize() -> ProbeResult<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self)
    }
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

fn hello_status() -> ProbeResult<()> {
    let _apartment = WinRtApartment::initialize()?;
    let supported = KeyCredentialManager::IsSupportedAsync()?.join()?;
    println!("hello_supported={supported}");
    Ok(())
}

fn hello_challenge(open_only: bool) -> ProbeResult<()> {
    let _apartment = WinRtApartment::initialize()?;
    let supported = KeyCredentialManager::IsSupportedAsync()?.join()?;
    println!("hello_supported={supported}");
    if !supported {
        return Err("KeyCredentialManager is not supported in this process context".into());
    }

    let name = HSTRING::from(HELLO_CREDENTIAL_NAME);
    let acquire_started = Instant::now();
    let credential = if open_only {
        println!("credential_access_mode=open_existing");
        open_hello_credential(&name)?
    } else {
        println!("credential_access_mode=create_or_open");
        create_or_open_hello_credential(&name)?
    };
    println!(
        "hello_acquire_ms={:.3}",
        acquire_started.elapsed().as_secs_f64() * 1000.0
    );
    sign_and_verify_hello_challenge(&credential)
}

fn hello_sign_cycle() -> ProbeResult<()> {
    let iterations = parse_hello_iterations()?;
    let _apartment = WinRtApartment::initialize()?;
    let supported = KeyCredentialManager::IsSupportedAsync()?.join()?;
    println!("hello_supported={supported}");
    if !supported {
        return Err("KeyCredentialManager is not supported in this process context".into());
    }

    let name = HSTRING::from(HELLO_CREDENTIAL_NAME);
    let acquire_started = Instant::now();
    let credential = create_or_open_hello_credential(&name)?;
    println!(
        "hello_acquire_ms={:.3}",
        acquire_started.elapsed().as_secs_f64() * 1000.0
    );

    let mut samples = Vec::with_capacity(iterations);
    for index in 0..iterations {
        let mut challenge = [0u8; 32];
        unsafe {
            BCryptGenRandom(None, &mut challenge, BCRYPT_USE_SYSTEM_PREFERRED_RNG).ok()?;
        }
        let result = request_sign_and_verify(&credential, &challenge);
        challenge.zeroize();
        let elapsed = result?;
        samples.push(elapsed);
        println!(
            "hello_sign_sample={} duration_ms={:.3}",
            index + 1,
            elapsed.as_secs_f64() * 1000.0
        );
    }

    print_timing_summary(&samples);
    println!("hello_sign_cycle_verified=true");
    Ok(())
}

fn create_or_open_hello_credential(name: &HSTRING) -> ProbeResult<KeyCredential> {
    let retrieval =
        KeyCredentialManager::RequestCreateAsync(name, KeyCredentialCreationOption::FailIfExists)?
            .join()?;
    let status = retrieval.Status()?;
    println!("hello_create_status={}", key_credential_status_name(status));

    if status == KeyCredentialStatus::Success {
        println!("hello_credential_created=true");
        return Ok(retrieval.Credential()?);
    }
    if status == KeyCredentialStatus::CredentialAlreadyExists {
        println!("hello_credential_created=false");
        return open_hello_credential(name);
    }
    Err(format!(
        "Hello credential creation failed with {} ({})",
        key_credential_status_name(status),
        status.0
    )
    .into())
}

fn open_hello_credential(name: &HSTRING) -> ProbeResult<KeyCredential> {
    let retrieval = KeyCredentialManager::OpenAsync(name)?.join()?;
    let status = retrieval.Status()?;
    println!("hello_open_status={}", key_credential_status_name(status));
    if status != KeyCredentialStatus::Success {
        return Err(format!(
            "Hello credential open failed with {} ({})",
            key_credential_status_name(status),
            status.0
        )
        .into());
    }
    Ok(retrieval.Credential()?)
}

fn sign_and_verify_hello_challenge(credential: &KeyCredential) -> ProbeResult<()> {
    let mut challenge = [0u8; 32];
    unsafe {
        BCryptGenRandom(None, &mut challenge, BCRYPT_USE_SYSTEM_PREFERRED_RNG).ok()?;
    }

    let result = request_sign_and_verify(credential, &challenge);
    challenge.zeroize();
    let elapsed = result?;
    println!("hello_sign_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
    println!("prompt_type_observed={}", read_prompt_observation()?);
    Ok(())
}

fn request_sign_and_verify(
    credential: &KeyCredential,
    challenge: &[u8; 32],
) -> ProbeResult<Duration> {
    let challenge_buffer = CryptographicBuffer::CreateFromByteArray(challenge)?;
    let started = Instant::now();
    let operation = credential.RequestSignAsync(&challenge_buffer)?.join()?;
    let elapsed = started.elapsed();
    let status = operation.Status()?;
    println!("hello_sign_status={}", key_credential_status_name(status));

    if status != KeyCredentialStatus::Success {
        return Err(format!(
            "Hello signing failed with {} ({})",
            key_credential_status_name(status),
            status.0
        )
        .into());
    }

    let public_key = credential
        .RetrievePublicKeyWithBlobType(CryptographicPublicKeyBlobType::Pkcs1RsaPublicKey)?;
    let algorithm = AsymmetricKeyAlgorithmProvider::OpenAlgorithm(
        &AsymmetricAlgorithmNames::RsaSignPkcs1Sha256()?,
    )?;
    let verification_key = algorithm.ImportPublicKeyWithBlobType(
        &public_key,
        CryptographicPublicKeyBlobType::Pkcs1RsaPublicKey,
    )?;
    let signature = operation.Result()?;
    let verified =
        CryptographicEngine::VerifySignature(&verification_key, &challenge_buffer, &signature)?;
    println!("signature_verified={verified}");
    if !verified {
        return Err("Hello challenge signature verification failed".into());
    }
    Ok(elapsed)
}

fn read_prompt_observation() -> ProbeResult<&'static str> {
    print!("prompt_observation=Enter face|fingerprint|pin|password|none|unknown: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    match input.trim().to_ascii_lowercase().as_str() {
        "face" | "yuz" | "yüz" => Ok("face"),
        "fingerprint" | "parmak izi" => Ok("fingerprint"),
        "pin" => Ok("pin"),
        "password" | "parola" | "sifre" | "şifre" => Ok("password"),
        "none" | "yok" => Ok("none"),
        "unknown" | "bilinmiyor" | "" => Ok("unknown"),
        _ => Err(
            "invalid prompt observation; use face, fingerprint, pin, password, none, or unknown"
                .into(),
        ),
    }
}

fn key_credential_status_name(status: KeyCredentialStatus) -> &'static str {
    match status {
        KeyCredentialStatus::Success => "success",
        KeyCredentialStatus::UnknownError => "unknown_error",
        KeyCredentialStatus::NotFound => "not_found",
        KeyCredentialStatus::UserCanceled => "user_canceled",
        KeyCredentialStatus::UserPrefersPassword => "user_prefers_password",
        KeyCredentialStatus::CredentialAlreadyExists => "credential_already_exists",
        KeyCredentialStatus::SecurityDeviceLocked => "security_device_locked",
        _ => "unrecognized_status",
    }
}

fn delete_key(provider: &Provider) -> ProbeResult<()> {
    let Some(mut key) = open_key_for_experiment(provider, "delete")? else {
        println!("key_deleted=false");
        return Ok(());
    };
    unsafe { NCryptDeleteKey(key.0, 0)? };
    key.0 = NCRYPT_KEY_HANDLE::default();
    println!("key_deleted=true");
    Ok(())
}

fn open_key_for_experiment(provider: &Provider, operation: &str) -> ProbeResult<Option<Key>> {
    match Key::open(provider) {
        Ok(key) => Ok(Some(key)),
        Err(error)
            if provider.kind == ProviderKind::Passport
                && is_nte_invalid_parameter(error.as_ref()) =>
        {
            let status = windows_error_code(error.as_ref())
                .expect("NTE_INVALID_PARAMETER guard requires a Windows error");
            report_passport_direct_cng_unsupported(operation, status);
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn report_passport_direct_cng_unsupported(operation: &str, status: i32) {
    println!("operation={operation}");
    println!("passport_direct_cng_supported=false");
    println!("path_b_result=unsupported");
    println!("nte_status=0x{:08x}", status as u32);
}

fn is_nte_invalid_parameter(error: &(dyn Error + 'static)) -> bool {
    const NTE_INVALID_PARAMETER: i32 = 0x8009_0027u32 as i32;

    windows_error_code(error).is_some_and(|status| status == NTE_INVALID_PARAMETER)
}

fn windows_error_code(error: &(dyn Error + 'static)) -> Option<i32> {
    error
        .downcast_ref::<windows::core::Error>()
        .map(|error| error.code().0)
}

fn set_u32(handle: NCRYPT_HANDLE, property: PCWSTR, value: u32) -> ProbeResult<()> {
    unsafe { NCryptSetProperty(handle, property, &value.to_ne_bytes(), NCRYPT_FLAGS(0))? };
    Ok(())
}

fn get_property(handle: NCRYPT_HANDLE, property: PCWSTR) -> ProbeResult<Vec<u8>> {
    let mut length = 0u32;
    unsafe {
        NCryptGetProperty(
            handle,
            property,
            None,
            &mut length,
            OBJECT_SECURITY_INFORMATION(0),
        )?
    };
    let mut bytes = vec![0u8; length as usize];
    unsafe {
        NCryptGetProperty(
            handle,
            property,
            Some(&mut bytes),
            &mut length,
            OBJECT_SECURITY_INFORMATION(0),
        )?
    };
    bytes.truncate(length as usize);
    Ok(bytes)
}

fn get_u32(handle: NCRYPT_HANDLE, property: PCWSTR) -> ProbeResult<u32> {
    let bytes = get_property(handle, property)?;
    parse_u32(&bytes)
}

fn parse_u32(bytes: &[u8]) -> ProbeResult<u32> {
    let bytes: [u8; 4] = bytes
        .get(..4)
        .ok_or("property value was shorter than four bytes")?
        .try_into()?;
    Ok(u32::from_ne_bytes(bytes))
}

fn get_utf16(handle: NCRYPT_HANDLE, property: PCWSTR) -> ProbeResult<String> {
    let bytes = get_property(handle, property)?;
    parse_utf16(&bytes)
}

fn parse_utf16(bytes: &[u8]) -> ProbeResult<String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("UTF-16 property had an odd byte length".into());
    }
    let mut units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    while units.last() == Some(&0) {
        units.pop();
    }
    Ok(String::from_utf16(&units)?)
}

fn redact_unique_name(unique_name: &str) -> &str {
    unique_name
        .rsplit(['\\', '/'])
        .find(|segment| !segment.is_empty())
        .unwrap_or("<empty>")
}

fn print_optional_u32(handle: NCRYPT_HANDLE, property: PCWSTR, label: &str) {
    match get_u32(handle, property) {
        Ok(value) => println!("{label}=0x{value:08x}"),
        Err(error) => println!("{label}=unavailable ({error})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_nearest_rank() {
        let samples = (1..=10).map(Duration::from_millis).collect::<Vec<_>>();

        assert_eq!(percentile(&samples, 50), Duration::from_millis(5));
        assert_eq!(percentile(&samples, 95), Duration::from_millis(10));
    }

    #[test]
    fn percentile_handles_one_sample() {
        let samples = [Duration::from_millis(42)];

        assert_eq!(percentile(&samples, 50), samples[0]);
        assert_eq!(percentile(&samples, 95), samples[0]);
    }

    #[test]
    fn parse_utf16_trims_trailing_nuls() {
        let bytes = [b'A', 0, b'B', 0, 0, 0, 0, 0];

        assert_eq!(parse_utf16(&bytes).unwrap(), "AB");
    }

    #[test]
    fn parse_utf16_accepts_empty_value() {
        assert_eq!(parse_utf16(&[]).unwrap(), "");
    }

    #[test]
    fn parse_utf16_rejects_odd_byte_length() {
        assert!(parse_utf16(&[b'A']).is_err());
    }

    #[test]
    fn parse_utf16_rejects_invalid_surrogate() {
        assert!(parse_utf16(&[0x00, 0xD8]).is_err());
    }

    #[test]
    fn parse_u32_reads_native_endian_value() {
        assert_eq!(parse_u32(&42u32.to_ne_bytes()).unwrap(), 42);
    }

    #[test]
    fn parse_u32_rejects_short_and_empty_values() {
        assert!(parse_u32(&[1, 2, 3]).is_err());
        assert!(parse_u32(&[]).is_err());
    }

    #[test]
    fn redact_unique_name_removes_parent_path() {
        assert_eq!(
            redact_unique_name(r"C:\Users\example\AppData\key-file"),
            "key-file"
        );
    }
}
