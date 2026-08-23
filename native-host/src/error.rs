use std::error::Error;
use std::fmt::{Display, Formatter};

pub type FcpResult<T> = Result<T, FcpError>;

#[derive(Debug)]
pub enum FcpError {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// A failure reported by a Win32 API. Target-gated because the type comes from the `windows`
    /// crate, which a Linux build does not pull in at all; a platform failure there arrives as
    /// `Io` or as one of the message-carrying variants instead.
    #[cfg(windows)]
    Windows(windows::core::Error),
    Crypto(&'static str),
    Format(String),
    Protocol(String),
    Capability(String),
    /// The platform has no user-verification method enrolled for this account — no Windows Hello
    /// PIN, fingerprint or face. Distinct from a verification that was attempted and refused: this
    /// one is fixed in the OS settings, not by trying again, and the extension turns it into a
    /// message that says where to go.
    UserVerificationNotConfigured,
}

impl Display for FcpError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            #[cfg(windows)]
            Self::Windows(error) => write!(formatter, "Windows API error: {error}"),
            Self::Crypto(message) => write!(formatter, "cryptographic operation failed: {message}"),
            Self::Format(message) => write!(formatter, "vault format error: {message}"),
            Self::Protocol(message) => write!(formatter, "protocol error: {message}"),
            Self::Capability(message) => write!(formatter, "capability rejected: {message}"),
            Self::UserVerificationNotConfigured => write!(
                formatter,
                "no user verification method is set up for this account"
            ),
        }
    }
}

impl Error for FcpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            #[cfg(windows)]
            Self::Windows(error) => Some(error),
            Self::Crypto(_)
            | Self::Format(_)
            | Self::Protocol(_)
            | Self::Capability(_)
            | Self::UserVerificationNotConfigured => None,
        }
    }
}

impl From<std::io::Error> for FcpError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for FcpError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[cfg(windows)]
impl From<windows::core::Error> for FcpError {
    fn from(value: windows::core::Error) -> Self {
        Self::Windows(value)
    }
}
