//! Pure-pursuit style heading control for the skid-steer banks.

use crate::config::AutoConfig;
use crate::geo::{angle_diff, Pose};

/// First path point at least `lookahead` away. Falls back to the last point
/// when the path is shorter than the lookahead.
pub fn lookahead_point(pose: &Pose, path: &[(f64, f64)], lookahead: f64) -> Option<(f64, f64)> {
    if path.is_empty() {
        return None;
    }
    for p in path {
        let dx = p.0 - pose.x;
        let dy = p.1 - pose.y;
        if (dx * dx + dy * dy).sqrt() >= lookahead {
            return Some(*p);
        }
    }
    path.last().copied()
}

/// Left/right duty for a path point.
///
/// Positive local Y is to the rover's left. The sign convention matches the
/// drive banks: a left turn slows the left bank and speeds up the right one.
pub fn command(
    pose: &Pose,
    path: &[(f64, f64)],
    speed_mps: f64,
    cfg: &AutoConfig,
) -> Option<(i32, i32)> {
    let target = lookahead_point(pose, path, cfg.lookahead_m)?;
    let yaw = pose.yaw_deg.to_radians();
    let dx = target.0 - pose.x;
    let dy = target.1 - pose.y;
    let local_x = dx * yaw.cos() + dy * yaw.sin();
    let local_y = -dx * yaw.sin() + dy * yaw.cos();

    if local_x <= 0.05 {
        let turn = cfg.turn_speed;
        return Some(if local_y > 0.0 {
            (-turn, turn)
        } else {
            (turn, -turn)
        });
    }

    let l2 = local_x * local_x + local_y * local_y;
    let curvature = 2.0 * local_y / l2;
    let omega = speed_mps * curvature;
    let v_left = speed_mps - omega * cfg.wheel_base_m / 2.0;
    let v_right = speed_mps + omega * cfg.wheel_base_m / 2.0;
    let duty_per_mps = cfg.max_speed as f64 / cfg.max_mps;
    Some((
        (v_left * duty_per_mps).round() as i32,
        (v_right * duty_per_mps).round() as i32,
    ))
}

/// Heading error toward a single point, for the fixed-hop mode that has no
/// map. Positive means the target is to the left.
pub fn heading_error(pose: &Pose, target: (f64, f64)) -> f64 {
    let bearing = (target.1 - pose.y).atan2(target.0 - pose.x).to_degrees();
    angle_diff(bearing, pose.yaw_deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose() -> Pose {
        Pose::new(0.0, 0.0, 0.0)
    }

    #[test]
    fn straight_ahead_keeps_the_banks_equal() {
        let cfg = AutoConfig::default();
        let path = vec![(0.5, 0.0), (1.0, 0.0)];
        let (l, r) = command(&pose(), &path, 0.2, &cfg).unwrap();
        assert_eq!(l, r);
    }

    #[test]
    fn target_to_the_left_slows_the_left_bank() {
        let cfg = AutoConfig::default();
        let path = vec![(0.4, 0.3), (0.8, 0.6)];
        let (l, r) = command(&pose(), &path, 0.2, &cfg).unwrap();
        assert!(l < r, "left {l} right {r}");
    }

    #[test]
    fn target_to_the_right_slows_the_right_bank() {
        let cfg = AutoConfig::default();
        let path = vec![(0.4, -0.3), (0.8, -0.6)];
        let (l, r) = command(&pose(), &path, 0.2, &cfg).unwrap();
        assert!(l > r, "left {l} right {r}");
    }

    #[test]
    fn target_behind_pivots_in_place() {
        let cfg = AutoConfig::default();
        let path = vec![(-0.5, 0.2)];
        let (l, r) = command(&pose(), &path, 0.2, &cfg).unwrap();
        assert_eq!(l, -cfg.turn_speed);
        assert_eq!(r, cfg.turn_speed);
    }

    #[test]
    fn empty_path_has_no_command() {
        assert!(command(&pose(), &[], 0.2, &AutoConfig::default()).is_none());
    }

    #[test]
    fn heading_error_crosses_the_yaw_seam() {
        // Target bearing -170, rover yaw 170: the short way is +20 degrees.
        let p = Pose::new(0.0, 0.0, 170.0);
        let target = (-0.985, -0.174);
        let e = heading_error(&p, target);
        assert!((e - 20.0).abs() < 1.0, "{e}");
    }
}
