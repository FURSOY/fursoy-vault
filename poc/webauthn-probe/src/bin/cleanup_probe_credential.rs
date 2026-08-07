// One-off cleanup: lists the platform credentials registered under the probe's RP id and deletes
// only the one whose user name is "probe-user", leaving the real app's "fursoy-cookie-protector"
// credential (same RP id, different user) untouched.

use std::error::Error;

use windows::Win32::Networking::WindowsWebServices::{
    WEBAUTHN_CREDENTIAL_DETAILS, WEBAUTHN_GET_CREDENTIALS_OPTIONS,
    WEBAUTHN_GET_CREDENTIALS_OPTIONS_CURRENT_VERSION, WebAuthNDeletePlatformCredential,
    WebAuthNFreePlatformCredentialList, WebAuthNGetPlatformCredentialList,
};
use windows::core::w;

const RP_ID: windows::core::PCWSTR = w!("fursoy-cookie-protector.local");
const TARGET_USER_NAME: &str = "probe-user";

fn main() -> Result<(), Box<dyn Error>> {
    let options = WEBAUTHN_GET_CREDENTIALS_OPTIONS {
        dwVersion: WEBAUTHN_GET_CREDENTIALS_OPTIONS_CURRENT_VERSION,
        pwszRpId: RP_ID,
        bBrowserInPrivateMode: false.into(),
    };

    let list = unsafe { WebAuthNGetPlatformCredentialList(&options) }?;
    let count = unsafe { (*list).cCredentialDetails } as usize;
    println!("found {count} credential(s) for RP id fursoy-cookie-protector.local");

    let mut deleted = 0;
    for index in 0..count {
        let entry_ptr = unsafe { *(*list).ppCredentialDetails.add(index) };
        let details: &WEBAUTHN_CREDENTIAL_DETAILS = unsafe { &*entry_ptr };
        let user_name = unsafe {
            if details.pUserInformation.is_null() {
                String::new()
            } else {
                (*details.pUserInformation)
                    .pwszName
                    .to_string()
                    .unwrap_or_default()
            }
        };
        let credential_id =
            unsafe { std::slice::from_raw_parts(details.pbCredentialID, details.cbCredentialID as usize) };
        println!("  credential user={user_name:?} id_len={}", credential_id.len());

        if user_name == TARGET_USER_NAME {
            match unsafe { WebAuthNDeletePlatformCredential(credential_id) } {
                Ok(()) => {
                    println!("  -> deleted (user={user_name:?})");
                    deleted += 1;
                }
                Err(error) => println!("  -> delete FAILED: {error}"),
            }
        }
    }

    unsafe { WebAuthNFreePlatformCredentialList(list) };
    println!("done, deleted {deleted} credential(s)");
    Ok(())
}
