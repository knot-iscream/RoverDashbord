//! Operator lease: who is allowed to write motor commands right now.
//!
//! `auto` requires a fresh heartbeat from the laptop backend. A missing
//! heartbeat is a hazard signal, not a pause: the mission returns home.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Auto,
    Manual,
    Hold,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Idle => "idle",
            Mode::Auto => "auto",
            Mode::Manual => "manual",
            Mode::Hold => "hold",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "idle" | "stop" => Some(Mode::Idle),
            "auto" => Some(Mode::Auto),
            "manual" => Some(Mode::Manual),
            "hold" => Some(Mode::Hold),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Lease {
    pub mode: Mode,
    pub last_heartbeat_s: Option<f64>,
    pub changed_at_s: f64,
}

impl Lease {
    pub fn new(now_s: f64) -> Self {
        Self {
            mode: Mode::Idle,
            last_heartbeat_s: None,
            changed_at_s: now_s,
        }
    }

    pub fn set_mode(&mut self, mode: Mode, now_s: f64) {
        if self.mode != mode {
            self.mode = mode;
            self.changed_at_s = now_s;
        }
    }

    pub fn heartbeat(&mut self, now_s: f64) {
        self.last_heartbeat_s = Some(now_s);
    }

    /// True while the operator station is alive.
    pub fn operator_present(&self, now_s: f64, timeout_s: f64) -> bool {
        self.last_heartbeat_s
            .is_some_and(|t| now_s - t <= timeout_s)
    }

    /// Auto may write motors only with a fresh heartbeat and the auto mode.
    pub fn auto_allowed(&self, now_s: f64, timeout_s: f64) -> bool {
        self.mode == Mode::Auto && self.operator_present(now_s, timeout_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_has_a_short_lease() {
        let mut l = Lease::new(0.0);
        l.set_mode(Mode::Auto, 0.0);
        l.heartbeat(10.0);
        assert!(l.auto_allowed(10.5, 3.0));
        assert!(!l.auto_allowed(14.0, 3.0));
    }

    #[test]
    fn no_auto_without_a_heartbeat() {
        let mut l = Lease::new(0.0);
        l.set_mode(Mode::Auto, 0.0);
        assert!(!l.auto_allowed(1.0, 3.0));
    }

    #[test]
    fn manual_overrides_auto() {
        let mut l = Lease::new(0.0);
        l.set_mode(Mode::Auto, 0.0);
        l.heartbeat(1.0);
        l.set_mode(Mode::Manual, 2.0);
        assert!(!l.auto_allowed(2.1, 3.0));
        assert_eq!(l.mode, Mode::Manual);
    }

    #[test]
    fn mode_strings_round_trip() {
        for m in [Mode::Idle, Mode::Auto, Mode::Manual, Mode::Hold] {
            assert_eq!(Mode::parse(m.as_str()), Some(m));
        }
        assert_eq!(Mode::parse("nonsense"), None);
        assert_eq!(Mode::parse("stop"), Some(Mode::Idle));
    }
}
