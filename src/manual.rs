//! Last manual duty per motor, kept alive by the backend.
//!
//! The dashboard is one-shot: keydown sends one command, keyup sends zero.
//! A 200 ms task republishes the active targets with the firmware deadman
//! arm, so a dead backend, broker or laptop stops the rover instead of
//! leaving the last duty running.

/// Deadline sent with every manual hold tick. Must be shorter than the
/// firmware's maximum (5 s) and longer than the 200 ms republish interval.
pub const MANUAL_ARM_MS: u64 = 600;

#[derive(Debug, Clone, Default)]
pub struct ManualHold {
    speeds: [i32; 6],
    active: bool,
}

impl ManualHold {
    /// `motor` follows the wire ids: 0 = all six, 1..4 corners, 5/6 mid.
    pub fn set(&mut self, motor: i64, speed: i32) {
        self.active = true;
        match motor {
            0 => self.speeds = [speed; 6],
            1..=4 => self.speeds[(motor as usize) - 1] = speed,
            5 => self.speeds[4] = speed,
            6 => self.speeds[5] = speed,
            _ => {}
        }
    }

    pub fn clear(&mut self) {
        self.speeds = [0; 6];
        self.active = false;
    }

    /// `None` when nothing should be republished.
    pub fn snapshot(&self) -> Option<[i32; 6]> {
        (self.active && self.speeds.iter().any(|s| *s != 0)).then_some(self.speeds)
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_motors_set_one_target() {
        let mut h = ManualHold::default();
        h.set(0, 120);
        assert_eq!(h.snapshot(), Some([120; 6]));
    }

    #[test]
    fn a_single_motor_leaves_the_rest_alone() {
        let mut h = ManualHold::default();
        h.set(1, 80);
        h.set(3, -40);
        assert_eq!(h.snapshot(), Some([80, 0, -40, 0, 0, 0]));
    }

    #[test]
    fn mid_ids_map_to_the_last_two_slots() {
        let mut h = ManualHold::default();
        h.set(5, 30);
        h.set(6, 60);
        assert_eq!(h.snapshot(), Some([0, 0, 0, 0, 30, 60]));
    }

    #[test]
    fn zero_clears_the_snapshot() {
        let mut h = ManualHold::default();
        h.set(0, 100);
        h.set(0, 0);
        assert_eq!(h.snapshot(), None);
        assert!(h.is_active(), "still active until an explicit clear");
    }

    #[test]
    fn clear_drops_everything() {
        let mut h = ManualHold::default();
        h.set(2, 90);
        h.clear();
        assert_eq!(h.snapshot(), None);
        assert!(!h.is_active());
    }
}
