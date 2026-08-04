use std::error::Error;
use std::fmt::{Display, Formatter};

pub type FcpResult<T> = Result<T, FcpError>;

#[derive(Debug)]
pub enum FcpError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Windows(windows::core::Error),
    Crypto(&'static str),
    Format(String),
    Protocol(String),
    Capability(String),
}

impl Display for FcpError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::Windows(error) => write!(formatter, "Windows API error: {error}"),
            Self::Crypto(message) => write!(formatter, "cryptographic operation failed: {message}"),
            Self::Format(message) => write!(formatter, "vault format error: {message}"),
            Self::Protocol(message) => write!(formatter, "protocol error: {message}"),
            Self::Capability(message) => write!(formatter, "capability rejected: {message}"),
        }
    }
}

impl Error for FcpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Windows(error) => Some(error),
            Self::Crypto(_) | Self::Format(_) | Self::Protocol(_) | Self::Capability(_) => None,
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

impl From<windows::core::Error> for FcpError {
    fn from(value: windows::core::Error) -> Self {
        Self::Windows(value)
    }
}
