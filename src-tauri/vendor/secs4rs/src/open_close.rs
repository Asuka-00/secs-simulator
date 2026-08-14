//! Open/Close lifecycle (`OpenAndCloseable`).
//!
//! Source: `Secs4Net.OpenAndCloseable` / `AbstractBaseCommunicator`.

/// Open/close lifecycle parity surface.
///
/// Implementors may use internal flags; `is_open` ≡ opened ∧ ¬closed.
pub trait OpenAndCloseable {
    /// Open the communicator (throws/returns err if already opened or closed).
    fn open(&self) -> Result<(), OpenCloseError>;

    /// True if opened and not yet closed.
    fn is_open(&self) -> bool;

    /// True if closed (`Dispose` / close completed).
    fn is_closed(&self) -> bool;

    /// Close / dispose (idempotent).
    fn close(&self);
}

/// Distinguishable open/close failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCloseError {
    /// `AlreadyOpenedException`.
    AlreadyOpened,
    /// `AlreadyClosedException`.
    AlreadyClosed,
    /// Protocol/transport failure during open.
    Failed,
}

impl std::fmt::Display for OpenCloseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyOpened => write!(f, "already opened"),
            Self::AlreadyClosed => write!(f, "already closed"),
            Self::Failed => write!(f, "open failed"),
        }
    }
}

impl std::error::Error for OpenCloseError {}

/// Shared opened/closed flags (AbstractBaseCommunicator `_opened` / `_closed`).
#[derive(Debug, Default)]
pub struct OpenCloseState {
    opened: std::sync::atomic::AtomicBool,
    closed: std::sync::atomic::AtomicBool,
}

impl OpenCloseState {
    pub fn new() -> Self {
        Self {
            opened: std::sync::atomic::AtomicBool::new(false),
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Mark opened; err if already opened or closed.
    pub fn mark_open(&self) -> Result<(), OpenCloseError> {
        use std::sync::atomic::Ordering::SeqCst;
        if self.closed.load(SeqCst) {
            return Err(OpenCloseError::AlreadyClosed);
        }
        if self
            .opened
            .compare_exchange(false, true, SeqCst, SeqCst)
            .is_err()
        {
            return Err(OpenCloseError::AlreadyOpened);
        }
        Ok(())
    }

    /// Mark closed (idempotent).
    pub fn mark_closed(&self) {
        use std::sync::atomic::Ordering::SeqCst;
        self.closed.store(true, SeqCst);
    }

    pub fn is_open(&self) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        self.opened.load(SeqCst) && !self.closed.load(SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        self.closed.load(SeqCst)
    }

    pub fn is_opened_flag(&self) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        self.opened.load(SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_state_parity() {
        let s = OpenCloseState::new();
        assert!(!s.is_open());
        assert!(!s.is_closed());
        s.mark_open().unwrap();
        assert!(s.is_open());
        assert_eq!(s.mark_open(), Err(OpenCloseError::AlreadyOpened));
        s.mark_closed();
        assert!(s.is_closed());
        assert!(!s.is_open());
        assert_eq!(s.mark_open(), Err(OpenCloseError::AlreadyClosed));
    }
}
