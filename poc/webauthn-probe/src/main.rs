// Spike: does WebAuthNAuthenticatorMakeCredential accept a synthetic (non-web) RP id and origin
// from a native caller, and does the consent dialog come up properly owned by our HWND? Then:
// does a second WebAuthNAuthenticatorGetAssertion call shortly after the first skip the prompt
// (mirroring the WinRT KeyCredential handle-reuse behaviour hello.rs's hello_cache_ms relies on),
// or does every call show fresh UI regardless of timing?
//
// This does not touch the real hello.rs credential. It uses its own RP id / credential name so it
// can be run repeatedly without disturbing the FURSOY.CookieProtector.Hello.v1 credential.

use std::error::Error;
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::Networking::WindowsWebServices::{
    WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM, WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS,
    WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_1,
    WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS,
    WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_1, WEBAUTHN_CLIENT_DATA,
    WEBAUTHN_CLIENT_DATA_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETER,
    WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETERS,
    WEBAUTHN_CREDENTIAL_EX, WEBAUTHN_CREDENTIAL_LIST, WEBAUTHN_RP_ENTITY_INFORMATION,
    WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION, WEBAUTHN_USER_ENTITY_INFORMATION,
    WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION, WebAuthNAuthenticatorGetAssertion,
    WebAuthNAuthenticatorMakeCredential, WebAuthNFreeAssertion, WebAuthNFreeCredentialAttestation,
    WebAuthNGetErrorName,
};
use windows::Win32::System::Console::GetConsoleWindow;
use windows::core::{HRESULT, PCWSTR, w};

const RP_ID: PCWSTR = w!("fursoy-cookie-protector.local");
const CRED_TYPE_PUBLIC_KEY: PCWSTR = w!("public-key");

const COSE_ALGORITHM_ES256: i32 = -7;

fn main() -> Result<(), Box<dyn Error>> {
    // A real top-level window we own, standing in for what fcp-host would pass from its own
    // console/message-only window. This is the ownership test: if WebAuthN respects hWnd the way
    // its docs claim, the dialog should come up in front of and owned by this console window.
    let hwnd: HWND = unsafe { GetConsoleWindow() };
    println!("probe console hwnd = {:?}", hwnd.0);

    let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
        pwszId: RP_ID,
        pwszName: w!("FURSOY Cookie Protector (probe)"),
        pwszIcon: PCWSTR::null(),
    };

    let user_id: [u8; 16] = *b"webauthn-probe01";
    let user = WEBAUTHN_USER_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
        cbId: user_id.len() as u32,
        pbId: user_id.as_ptr() as *mut u8,
        pwszName: w!("probe-user"),
        pwszIcon: PCWSTR::null(),
        pwszDisplayName: w!("Probe User"),
    };

    let mut cose_params = [WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
        dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
        pwszCredentialType: w!("public-key"),
        lAlg: COSE_ALGORITHM_ES256,
    }];
    let cose_credential_params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
        cCredentialParameters: cose_params.len() as u32,
        pCredentialParameters: cose_params.as_mut_ptr(),
    };

    // Synthetic origin: this is the thing we actually don't know the answer for. We are not a
    // browser and this is not a resolvable HTTPS origin. If WebAuthN validates origin-vs-RP-id
    // the way a browser does, this call fails; if it just hashes whatever clientDataJSON bytes we
    // hand it (as the struct docs suggest), it should not care.
    let client_data_json =
        br#"{"type":"webauthn.create","challenge":"cHJvYmUtY2hhbGxlbmdl","origin":"https://fursoy-cookie-protector.local","crossOrigin":false}"#;
    let mut client_data_bytes = client_data_json.to_vec();
    let client_data = WEBAUTHN_CLIENT_DATA {
        dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
        cbClientDataJSON: client_data_bytes.len() as u32,
        pbClientDataJSON: client_data_bytes.as_mut_ptr(),
        pwszHashAlgId: w!("SHA-256"),
    };

    let mut options = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS::default();
    options.dwVersion = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_1;
    options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM;
    options.dwUserVerificationRequirement =
        windows::Win32::Networking::WindowsWebServices::WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
    options.dwAttestationConveyancePreference =
        windows::Win32::Networking::WindowsWebServices::WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE;

    println!("calling WebAuthNAuthenticatorMakeCredential — watch whether the Hello prompt is owned/foregrounded...");
    let result = unsafe {
        WebAuthNAuthenticatorMakeCredential(
            hwnd,
            &rp,
            &user,
            &cose_credential_params,
            &client_data,
            Some(&options),
        )
    };

    let credential_id: Vec<u8> = match result {
        Ok(attestation) => {
            println!("SUCCESS: credential created, synthetic RP id/origin were accepted.");
            let id = unsafe {
                std::slice::from_raw_parts(
                    (*attestation).pbCredentialId,
                    (*attestation).cbCredentialId as usize,
                )
                .to_vec()
            };
            unsafe { WebAuthNFreeCredentialAttestation(Some(attestation)) };
            id
        }
        Err(err) => {
            let hresult: HRESULT = err.code();
            let name = readable_error_name(hresult);
            println!("FAILED: hresult={hresult:?} name={name} message={err}");
            return Ok(());
        }
    };

    println!("\n--- now testing whether a second GetAssertion right after the first skips UI ---");
    get_assertion(&credential_id, hwnd, 1)?;
    get_assertion(&credential_id, hwnd, 2)?;

    Ok(())
}

fn get_assertion(credential_id: &[u8], hwnd: HWND, attempt: u32) -> Result<(), Box<dyn Error>> {
    let mut cred_id_bytes = credential_id.to_vec();
    let mut credential_ex = WEBAUTHN_CREDENTIAL_EX {
        dwVersion: windows::Win32::Networking::WindowsWebServices::WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION,
        cbId: cred_id_bytes.len() as u32,
        pbId: cred_id_bytes.as_mut_ptr(),
        pwszCredentialType: CRED_TYPE_PUBLIC_KEY,
        dwTransports: 0,
    };
    let mut credential_ex_ptr = &mut credential_ex as *mut WEBAUTHN_CREDENTIAL_EX;
    let allow_list = WEBAUTHN_CREDENTIAL_LIST {
        cCredentials: 1,
        ppCredentials: &mut credential_ex_ptr,
    };

    let client_data_json = br#"{"type":"webauthn.get","challenge":"cHJvYmUtY2hhbGxlbmdl","origin":"https://fursoy-cookie-protector.local","crossOrigin":false}"#;
    let mut client_data_bytes = client_data_json.to_vec();
    let client_data = WEBAUTHN_CLIENT_DATA {
        dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
        cbClientDataJSON: client_data_bytes.len() as u32,
        pbClientDataJSON: client_data_bytes.as_mut_ptr(),
        pwszHashAlgId: w!("SHA-256"),
    };

    let mut options = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS::default();
    options.dwVersion = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_1;
    options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM;
    options.dwUserVerificationRequirement =
        windows::Win32::Networking::WindowsWebServices::WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED;
    options.pAllowCredentialList = &allow_list as *const _ as *mut _;

    println!("[attempt {attempt}] calling WebAuthNAuthenticatorGetAssertion...");
    let start = Instant::now();
    let result = unsafe { WebAuthNAuthenticatorGetAssertion(hwnd, RP_ID, &client_data, Some(&options)) };
    let elapsed = start.elapsed();

    match result {
        Ok(assertion) => {
            println!("[attempt {attempt}] SUCCESS in {elapsed:?}");
            unsafe { WebAuthNFreeAssertion(assertion) };
        }
        Err(err) => {
            let name = readable_error_name(err.code());
            println!("[attempt {attempt}] FAILED in {elapsed:?}: {name} — {err}");
        }
    }
    Ok(())
}

fn readable_error_name(hresult: HRESULT) -> String {
    unsafe {
        let ptr: PCWSTR = WebAuthNGetErrorName(hresult);
        if ptr.is_null() {
            return "?".into();
        }
        ptr.to_string().unwrap_or_else(|_| "?".into())
    }
}
