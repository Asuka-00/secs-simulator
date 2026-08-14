//! Minimal GEM configuration (MDLN / SOFTREV / equip / clock type).
//!
//! Source: `GemConfig` / `AbstractGemConfig` (protocol-path fields only).

use std::sync::Mutex;

use super::clock::ClockType;

/// GEM config used by S1F2 / S1F13 / S1F14 MDLN·SOFTREV and S2F18/31 clock.
#[derive(Debug)]
pub struct GemConfig {
    mdln: Mutex<String>,
    softrev: Mutex<String>,
    /// Equipment role → non-empty MDLN/SOFTREV list; host → empty L.
    is_equip: Mutex<bool>,
    clock_type: Mutex<ClockType>,
}

impl Default for GemConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl GemConfig {
    /// Defaults: mdln/softrev empty, is_equip=false (host), clock A16.
    pub fn new() -> Self {
        Self {
            mdln: Mutex::new(String::new()),
            softrev: Mutex::new(String::new()),
            is_equip: Mutex::new(false),
            clock_type: Mutex::new(ClockType::A16),
        }
    }

    pub fn set_mdln(&self, s: impl Into<String>) {
        *self.mdln.lock().expect("mdln") = s.into();
    }

    pub fn mdln(&self) -> String {
        self.mdln.lock().expect("mdln").clone()
    }

    pub fn set_softrev(&self, s: impl Into<String>) {
        *self.softrev.lock().expect("softrev") = s.into();
    }

    pub fn softrev(&self) -> String {
        self.softrev.lock().expect("softrev").clone()
    }

    pub fn set_is_equip(&self, equip: bool) {
        *self.is_equip.lock().expect("is_equip") = equip;
    }

    pub fn is_equip(&self) -> bool {
        *self.is_equip.lock().expect("is_equip")
    }

    pub fn set_clock_type(&self, t: ClockType) {
        *self.clock_type.lock().expect("clock_type") = t;
    }

    pub fn clock_type(&self) -> ClockType {
        *self.clock_type.lock().expect("clock_type")
    }

    /// `Mdlnsoftrev()` — equip: `L <A mdln> <A softrev>`; host: empty `L`.
    pub fn mdln_softrev(&self) -> crate::secs2::Result<crate::secs2::Secs2> {
        if self.is_equip() {
            let mdln = crate::secs2::Secs2::ascii(self.mdln())?;
            let soft = crate::secs2::Secs2::ascii(self.softrev())?;
            crate::secs2::Secs2::list([mdln, soft])
        } else {
            Ok(crate::secs2::Secs2::list_empty())
        }
    }

    /// Encode clock per configured type (`GetClockSecs2`).
    pub fn clock_secs2(&self, clock: &super::clock::Clock) -> crate::secs2::Result<crate::secs2::Secs2> {
        clock.to_secs2(self.clock_type())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gem_config_mdln_softrev_host_empty() {
        let c = GemConfig::new();
        c.set_mdln("EQ");
        c.set_softrev("1.0");
        // host default
        assert_eq!(c.mdln_softrev().unwrap().size(), 0);
    }

    #[test]
    fn gem_config_mdln_softrev_equip_list() {
        let c = GemConfig::new();
        c.set_is_equip(true);
        c.set_mdln("MDL");
        c.set_softrev("REV");
        let s = c.mdln_softrev().unwrap();
        assert_eq!(s.size(), 2);
        assert_eq!(s.get_ascii_at(&[0]).unwrap(), "MDL");
        assert_eq!(s.get_ascii_at(&[1]).unwrap(), "REV");
    }
}
