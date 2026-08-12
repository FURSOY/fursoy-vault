use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Write};

use crate::{FcpError, FcpResult};

/// Deterministic failure boundaries compiled only into the Rust unit-test build.
///
/// The counters are thread-local so parallel tests cannot consume each other's failures. A value
/// of `n` fails the nth visit to that boundary and removes the arm after it fires.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FailurePoint {
    AtomicAfterTempSync,
    AtomicAfterReplace,
    LeaseBeforePersist,
    AuditBeforeAppend,
    AuditAfterAppend,
    ProtocolBeforeResponseWrite,
}

thread_local! {
    static ARMED: RefCell<HashMap<FailurePoint, usize>> = RefCell::new(HashMap::new());
}

pub(crate) struct FailureGuard {
    point: FailurePoint,
}

impl Drop for FailureGuard {
    fn drop(&mut self) {
        ARMED.with(|armed| {
            armed.borrow_mut().remove(&self.point);
        });
    }
}

pub(crate) fn fail_on_nth(point: FailurePoint, visit: usize) -> FailureGuard {
    assert!(visit > 0, "failure visit must be positive");
    ARMED.with(|armed| {
        assert!(armed.borrow_mut().insert(point, visit).is_none());
    });
    FailureGuard { point }
}

pub(crate) fn fail_next(point: FailurePoint) -> FailureGuard {
    fail_on_nth(point, 1)
}

pub(crate) fn check(point: FailurePoint) -> FcpResult<()> {
    let should_fail = ARMED.with(|armed| {
        let mut armed = armed.borrow_mut();
        let Some(remaining) = armed.get_mut(&point) else {
            return false;
        };
        *remaining -= 1;
        if *remaining == 0 {
            armed.remove(&point);
            true
        } else {
            false
        }
    });
    if should_fail {
        return Err(FcpError::Io(io::Error::other(format!(
            "test-only injected failure at {point:?}"
        ))));
    }
    Ok(())
}

/// Writer test double for Native Messaging response-boundary tests.
///
/// It retains only protocol bytes generated from synthetic test payloads. Production code never
/// constructs this type.
pub(crate) struct FailingWriter {
    bytes: Vec<u8>,
    remaining_before_failure: usize,
}

impl FailingWriter {
    pub(crate) fn after_bytes(byte_count: usize) -> Self {
        Self {
            bytes: Vec::new(),
            remaining_before_failure: byte_count,
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Write for FailingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.remaining_before_failure == 0 {
            return Err(io::Error::other("test-only protocol writer failure"));
        }
        let accepted = buffer.len().min(self.remaining_before_failure);
        self.bytes.extend_from_slice(&buffer[..accepted]);
        self.remaining_before_failure -= accepted;
        Ok(accepted)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.remaining_before_failure == 0 {
            return Err(io::Error::other("test-only protocol flush failure"));
        }
        Ok(())
    }
}
