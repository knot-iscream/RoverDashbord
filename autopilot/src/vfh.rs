//! Sector-gap steering, a small VFH-style reflex. No training, no map.

use crate::config::AutoConfig;
use crate::geo::angle_diff;
use crate::scan::Bins;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gap {
    pub start_deg: f64,
    pub end_deg: f64,
    pub min_range_m: f64,
}

impl Gap {
    pub fn center_deg(&self) -> f64 {
        (self.start_deg + self.end_deg) / 2.0
    }

    pub fn width_deg(&self) -> f64 {
        self.end_deg - self.start_deg
    }
}

/// Contiguous free arcs across the front field of view.
///
/// A bin is blocked when its nearest return is closer than the stop distance
/// plus the rover half width. Empty bins count as open.
pub fn find_gaps(bins: &Bins, cfg: &AutoConfig) -> Vec<Gap> {
    let blocked_below = cfg.stop_distance_m + cfg.robot_half_width_m;
    let start = (-cfg.front_half_fov_deg).max(-180.0);
    let end = cfg.front_half_fov_deg.min(180.0);
    let step = cfg.bin_deg;

    let mut gaps = Vec::new();
    let mut open_start: Option<f64> = None;
    let mut open_min = f64::INFINITY;
    let mut a = start;

    while a < end {
        let range = bins.range_at(a);
        let free = range.is_none_or(|r| r > blocked_below);
        if free {
            if open_start.is_none() {
                open_start = Some(a);
                open_min = f64::INFINITY;
            }
            if let Some(r) = range {
                open_min = open_min.min(r);
            }
        } else if let Some(s) = open_start.take() {
            push_gap(&mut gaps, s, a, open_min, cfg);
            open_min = f64::INFINITY;
        }
        a += step;
    }
    if let Some(s) = open_start {
        push_gap(&mut gaps, s, end, open_min, cfg);
    }
    gaps
}

fn push_gap(gaps: &mut Vec<Gap>, start: f64, end: f64, min_range: f64, cfg: &AutoConfig) {
    if end - start < cfg.min_gap_width_deg {
        return;
    }
    gaps.push(Gap {
        start_deg: start,
        end_deg: end,
        min_range_m: if min_range.is_finite() {
            min_range
        } else {
            12.0
        },
    });
}

/// Pick the gap that best balances aim and clearance. A gap already
/// containing the current heading gets a small bonus, which stops the rover
/// from ping-ponging between two equally wide openings.
pub fn choose_gap<'a>(
    gaps: &'a [Gap],
    current_deg: f64,
    goal_deg: Option<f64>,
    cfg: &AutoConfig,
) -> Option<&'a Gap> {
    let target = goal_deg.unwrap_or(current_deg);
    gaps.iter().max_by(|a, b| {
        score(a, current_deg, target, cfg)
            .partial_cmp(&score(b, current_deg, target, cfg))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn score(g: &Gap, current_deg: f64, target_deg: f64, cfg: &AutoConfig) -> f64 {
    let aim = 1.0 - (angle_diff(g.center_deg(), target_deg).abs() / 180.0);
    let width = (g.width_deg() / 60.0).clamp(0.0, 1.0);
    let clearance = ((g.min_range_m - cfg.stop_distance_m) / 2.0).clamp(0.0, 1.0);
    let contains_current = if current_deg >= g.start_deg && current_deg <= g.end_deg {
        0.25
    } else {
        0.0
    };
    aim * 2.0 + width + clearance + contains_current
}

/// Duty for the left and right wheel banks.
///
/// Positive scan angles are to the left. A gap to the left slows the left
/// bank and speeds up the right one, which turns the rover toward it.
pub fn banks_for_heading(heading_deg: f64, base_speed: i32, cfg: &AutoConfig) -> (i32, i32) {
    let turn = (heading_deg / 45.0).clamp(-1.0, 1.0) * cfg.turn_speed as f64;
    let left = (base_speed as f64 - turn).round() as i32;
    let right = (base_speed as f64 + turn).round() as i32;
    (
        left.clamp(-cfg.max_speed, cfg.max_speed),
        right.clamp(-cfg.max_speed, cfg.max_speed),
    )
}

/// Scale duty with front clearance. 0 at the stop distance, full at the
/// slow distance, linear in between.
pub fn speed_for_clearance(range_m: f64, cfg: &AutoConfig) -> i32 {
    if range_m <= cfg.stop_distance_m {
        return 0;
    }
    if range_m >= cfg.slow_distance_m {
        return cfg.max_speed;
    }
    let span = cfg.slow_distance_m - cfg.stop_distance_m;
    let f = ((range_m - cfg.stop_distance_m) / span).clamp(0.0, 1.0);
    (cfg.max_speed as f64 * f).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::Bins;

    fn bins_with_wall(center_deg: f64, half_width_deg: f64, range_m: f64) -> Bins {
        let helper = Bins::from_ranges(vec![None; 180], 2.0);
        let mut ranges = vec![None; 180];
        let mut a = center_deg - half_width_deg;
        while a <= center_deg + half_width_deg {
            let idx = helper.index_of(a);
            ranges[idx] = Some(range_m);
            a += 2.0;
        }
        Bins::from_ranges(ranges, 2.0)
    }

    #[test]
    fn open_field_is_one_wide_gap() {
        let bins = Bins::from_ranges(vec![None; 180], 2.0);
        let gaps = find_gaps(&bins, &AutoConfig::default());
        assert_eq!(gaps.len(), 1);
        assert!((gaps[0].start_deg + 100.0).abs() < 1e-9);
        assert!((gaps[0].end_deg - 100.0).abs() < 1e-9);
    }

    #[test]
    fn wall_ahead_splits_the_field_into_two_gaps() {
        let bins = bins_with_wall(0.0, 30.0, 0.5);
        let gaps = find_gaps(&bins, &AutoConfig::default());
        assert_eq!(gaps.len(), 2);
        assert!(gaps[0].end_deg <= -30.0);
        assert!(gaps[1].start_deg >= 30.0);
    }

    #[test]
    fn narrow_slot_is_discarded() {
        let mut ranges = vec![Some(0.5); 180];
        for cell in ranges.iter_mut().take(92).skip(88) {
            *cell = None;
        }
        let bins = Bins::from_ranges(ranges, 2.0);
        let gaps = find_gaps(&bins, &AutoConfig::default());
        assert!(gaps.is_empty(), "an 8 degree slot is not a path");
    }

    #[test]
    fn chooser_prefers_the_gap_near_the_goal() {
        let cfg = AutoConfig::default();
        let gaps = vec![
            Gap {
                start_deg: -90.0,
                end_deg: -30.0,
                min_range_m: 2.0,
            },
            Gap {
                start_deg: 30.0,
                end_deg: 90.0,
                min_range_m: 1.2,
            },
        ];
        let g = choose_gap(&gaps, 0.0, Some(60.0), &cfg).unwrap();
        assert_eq!(g.center_deg(), 60.0);
    }

    #[test]
    fn heading_to_the_left_slows_the_left_bank() {
        let cfg = AutoConfig::default();
        let (l, r) = banks_for_heading(30.0, cfg.max_speed, &cfg);
        assert!(l < r);
    }

    #[test]
    fn heading_to_the_right_slows_the_right_bank() {
        let cfg = AutoConfig::default();
        let (l, r) = banks_for_heading(-30.0, cfg.max_speed, &cfg);
        assert!(l > r);
    }

    #[test]
    fn speed_drops_to_zero_at_the_stop_distance() {
        let cfg = AutoConfig::default();
        assert_eq!(speed_for_clearance(0.1, &cfg), 0);
        assert_eq!(speed_for_clearance(1.5, &cfg), cfg.max_speed);
        assert!(speed_for_clearance(0.5, &cfg) > 0);
    }
}
