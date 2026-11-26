//! Flame localisation: 5 digital sectors, one analog strength, optional
//! lidar range at the same bearing. The camera is not used here.

use crate::config::AutoConfig;
use crate::geo::{normalize_deg, Pose};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlameReading {
    /// Bit 0 = D1, bit 4 = D5.
    pub mask: u8,
    pub level_v: Option<f64>,
    pub heat_l: Option<f64>,
    pub heat_r: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FireMark {
    pub bearing_deg: f64,
    pub range_m: Option<f64>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub confidence: f64,
    pub mask: u8,
    pub level_v: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thermal {
    Ok,
    Warn,
    Abort,
}

/// Weighted centre of the active flame channels.
pub fn bearing_from_mask(mask: u8, table: &[f64; 5]) -> Option<f64> {
    let mut sum = 0.0;
    let mut n = 0;
    for (i, bearing) in table.iter().enumerate() {
        if mask & (1 << i) != 0 {
            sum += bearing;
            n += 1;
        }
    }
    (n > 0).then(|| normalize_deg(sum / n as f64))
}

/// Consecutive-frame confirmation so a single comparator tick does not turn
/// into a fire report.
#[derive(Debug, Clone, Default)]
pub struct FireTracker {
    streak: u32,
    pub last: Option<FireMark>,
}

impl FireTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(
        &mut self,
        reading: &FlameReading,
        pose: Option<&Pose>,
        range_m: Option<f64>,
        cfg: &AutoConfig,
    ) -> Option<FireMark> {
        let strong = reading.level_v.is_none_or(|v| v >= cfg.flame_level_floor_v);
        if reading.mask == 0 || !strong {
            self.streak = 0;
            self.last = None;
            return None;
        }
        self.streak = self.streak.saturating_add(1);
        if self.streak < cfg.flame_confirm_frames {
            return None;
        }
        let bearing = bearing_from_mask(reading.mask, &cfg.flame_bearing_deg)?;
        let (x, y) = match (pose, range_m) {
            (Some(p), Some(r)) => {
                let b = (p.yaw_deg + bearing).to_radians();
                (Some(p.x + r * b.cos()), Some(p.y + r * b.sin()))
            }
            _ => (None, None),
        };
        let level_factor = reading
            .level_v
            .map(|v| (v / 1.0).clamp(0.0, 1.0))
            .unwrap_or(0.4);
        let mark = FireMark {
            bearing_deg: bearing,
            range_m,
            x,
            y,
            confidence: ((self.streak as f64) / (cfg.flame_confirm_frames as f64)).min(1.0)
                * level_factor,
            mask: reading.mask,
            level_v: reading.level_v,
        };
        self.last = Some(mark.clone());
        Some(mark)
    }
}

pub fn thermal_state(heat_c: Option<f64>, cfg: &AutoConfig) -> Thermal {
    match heat_c {
        Some(t) if t >= cfg.temp_abort_c => Thermal::Abort,
        Some(t) if t >= cfg.temp_warn_c => Thermal::Warn,
        _ => Thermal::Ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AutoConfig {
        AutoConfig::default()
    }

    #[test]
    fn single_channel_maps_to_its_bearing() {
        let table = cfg().flame_bearing_deg;
        assert_eq!(bearing_from_mask(0b00100, &table), Some(0.0));
        assert_eq!(bearing_from_mask(0b00001, &table), Some(-60.0));
        assert_eq!(bearing_from_mask(0b10000, &table), Some(60.0));
    }

    #[test]
    fn multiple_channels_average() {
        let table = cfg().flame_bearing_deg;
        assert_eq!(bearing_from_mask(0b00011, &table), Some(-45.0));
    }

    #[test]
    fn weak_analog_resets_the_streak() {
        let mut t = FireTracker::new();
        let r = FlameReading {
            mask: 0b00100,
            level_v: Some(0.05),
            ..Default::default()
        };
        assert!(t.observe(&r, None, None, &cfg()).is_none());
        assert_eq!(t.streak, 0);
    }

    #[test]
    fn needs_consecutive_confirmation() {
        let mut t = FireTracker::new();
        let r = FlameReading {
            mask: 0b00100,
            level_v: Some(0.8),
            ..Default::default()
        };
        assert!(t.observe(&r, None, None, &cfg()).is_none());
        assert!(t.observe(&r, None, None, &cfg()).is_none());
        assert!(t.observe(&r, None, None, &cfg()).is_some());
    }

    #[test]
    fn localizes_in_the_map_frame() {
        let mut t = FireTracker::new();
        let r = FlameReading {
            mask: 0b00100,
            level_v: Some(1.0),
            ..Default::default()
        };
        let p = Pose::new(1.0, 2.0, 90.0);
        for _ in 0..3 {
            t.observe(&r, Some(&p), Some(2.0), &cfg());
        }
        let m = t.last.unwrap();
        assert!((m.x.unwrap() - 1.0).abs() < 1e-9);
        assert!((m.y.unwrap() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn thermal_bands() {
        let c = cfg();
        assert_eq!(thermal_state(Some(30.0), &c), Thermal::Ok);
        assert_eq!(thermal_state(Some(48.0), &c), Thermal::Warn);
        assert_eq!(thermal_state(Some(60.0), &c), Thermal::Abort);
        assert_eq!(thermal_state(None, &c), Thermal::Ok);
    }
}
