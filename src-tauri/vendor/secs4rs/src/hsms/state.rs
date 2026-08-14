//! HSMS communicate state.
//!
//! Source: `HsmsCommunicateState`.

/// HSMS link/select state machine values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HsmsCommunicateState {
    /// TCP not connected.
    #[default]
    NotConnected,
    /// Connected but not yet selected.
    NotSelected,
    /// Selected (communicatable).
    Selected,
}

impl HsmsCommunicateState {
    /// Whether SECS data exchange is allowed (`SELECTED` only).
    pub const fn communicatable(self) -> bool {
        matches!(self, Self::Selected)
    }

    /// C# / Java constant name (`ToString` parity).
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotConnected => "NOT_CONNECTED",
            Self::NotSelected => "NOT_SELECTED",
            Self::Selected => "SELECTED",
        }
    }
}

impl std::fmt::Display for HsmsCommunicateState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsms_communicate_state_names() {
        assert_eq!(HsmsCommunicateState::NotConnected.name(), "NOT_CONNECTED");
        assert_eq!(HsmsCommunicateState::NotSelected.name(), "NOT_SELECTED");
        assert_eq!(HsmsCommunicateState::Selected.name(), "SELECTED");
        assert!(!HsmsCommunicateState::NotConnected.communicatable());
        assert!(!HsmsCommunicateState::NotSelected.communicatable());
        assert!(HsmsCommunicateState::Selected.communicatable());
    }
}
