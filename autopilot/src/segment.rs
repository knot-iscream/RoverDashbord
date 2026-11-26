//! A latched drive target.
//!
//! The 12 s ramp is a hard frame limit, so the controller must not change a
//! target while moving. It sets one target at the start of a segment and
//! only republishes it. The next target is chosen from a standstill.

#[derive(Debug, Clone, Default)]
pub struct Segment {
    left: i32,
    right: i32,
    pivot: bool,
    started_s: f64,
    set: bool,
}

impl Segment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, left: i32, right: i32, pivot: bool, now_s: f64) {
        self.left = left;
        self.right = right;
        self.pivot = pivot;
        self.started_s = now_s;
        self.set = true;
    }

    pub fn clear(&mut self) {
        self.left = 0;
        self.right = 0;
        self.pivot = false;
        self.started_s = 0.0;
        self.set = false;
    }

    pub fn is_set(&self) -> bool {
        self.set
    }

    pub fn pivot(&self) -> bool {
        self.pivot
    }

    pub fn started_s(&self) -> f64 {
        self.started_s
    }

    pub fn age(&self, now_s: f64) -> f64 {
        if self.set {
            now_s - self.started_s
        } else {
            0.0
        }
    }

    pub fn target(&self) -> Option<(i32, i32)> {
        self.set.then_some((self.left, self.right))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_segment_has_no_target() {
        let s = Segment::new();
        assert!(!s.is_set());
        assert_eq!(s.target(), None);
    }

    #[test]
    fn set_holds_the_target() {
        let mut s = Segment::new();
        s.set(70, 40, false, 10.0);
        assert!(s.is_set());
        assert_eq!(s.target(), Some((70, 40)));
        assert!((s.age(12.5) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn pivot_flag_survives_until_clear() {
        let mut s = Segment::new();
        s.set(-55, 55, true, 1.0);
        assert!(s.pivot());
        s.clear();
        assert!(!s.is_set());
        assert!(!s.pivot());
        assert!((s.age(2.0) - 0.0).abs() < 1e-9);
    }
}
