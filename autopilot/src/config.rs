//! Autonomy tuning. Defaults are deliberately slow and conservative; the
//! bench pass raises them only after the stop distances are measured.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoConfig {
    /// Duty cap in autonomous mode (0..255).
    pub max_speed: i32,
    /// Extra duty used for steering, added to one bank and removed from the other.
    pub turn_speed: i32,
    /// Cap while retracing the breadcrumb path home.
    pub return_speed: i32,
    /// Hard stop below this front clearance.
    pub stop_distance_m: f64,
    /// Full speed at or above this clearance.
    pub slow_distance_m: f64,
    /// Half the rover width, used to inflate blocked sectors.
    pub robot_half_width_m: f64,
    /// Front field of view used by the gap finder.
    pub front_half_fov_deg: f64,
    /// Scan bin size.
    pub bin_deg: f64,
    /// A free arc narrower than this is not a gap, it is a slot.
    pub min_gap_width_deg: f64,
    /// Waypoint arrival tolerance.
    pub goal_tolerance_m: f64,
    /// Pursuit lookahead distance.
    pub lookahead_m: f64,
    /// Distance between the left and right wheel lines.
    pub wheel_base_m: f64,
    /// Duty-to-speed scale. Measure it on the floor, do not trust the default.
    pub max_mps: f64,
    /// One survey hop, measured along the path. Short on purpose: the 12 s
    /// ramp means a long hop barely reaches cruising speed.
    pub max_hop_m: f64,
    /// Fallback hop length when no pose is available.
    pub hop_seconds: f64,
    /// Survey stops before the mission returns.
    pub max_stops: u32,
    /// Whole-mission cap.
    pub max_mission_s: f64,
    /// Operator heartbeat timeout.
    pub heartbeat_timeout_s: f64,
    /// One sweep is about 52 s; allow margin.
    pub scan_timeout_s: f64,
    /// Hector pose jump that invalidates the breadcrumbs.
    pub pose_jump_m: f64,
    /// Breadcrumb spacing.
    pub breadcrumb_m: f64,
    /// Mounting offset: scan angle plus this equals rover angle.
    pub lidar_yaw_offset_deg: f64,
    /// Occupancy grid cell size.
    pub map_cell_m: f64,
    /// Global path inflation radius.
    pub inflation_m: f64,
    /// Firmware deadman window sent with every auto command.
    pub auto_arm_ms: u64,
    /// Calibrated bearing of flame channels D1..D5.
    pub flame_bearing_deg: [f64; 5],
    /// Analog flame floor. Below this a channel hit is treated as noise.
    pub flame_level_floor_v: f64,
    /// Consecutive confirmed frames before a fire is reported.
    pub flame_confirm_frames: u32,
    /// Warm warning.
    pub temp_warn_c: f64,
    /// Abort and return.
    pub temp_abort_c: f64,
}

impl Default for AutoConfig {
    fn default() -> Self {
        Self {
            max_speed: 70,
            turn_speed: 55,
            return_speed: 45,
            stop_distance_m: 0.35,
            slow_distance_m: 0.90,
            robot_half_width_m: 0.28,
            front_half_fov_deg: 100.0,
            bin_deg: 2.0,
            min_gap_width_deg: 18.0,
            goal_tolerance_m: 0.25,
            lookahead_m: 0.45,
            wheel_base_m: 0.45,
            max_mps: 0.45,
            max_hop_m: 1.5,
            hop_seconds: 15.0,
            max_stops: 5,
            max_mission_s: 600.0,
            heartbeat_timeout_s: 3.0,
            scan_timeout_s: 75.0,
            pose_jump_m: 0.30,
            breadcrumb_m: 0.20,
            lidar_yaw_offset_deg: 0.0,
            map_cell_m: 0.10,
            inflation_m: 0.25,
            auto_arm_ms: 600,
            flame_bearing_deg: [-60.0, -30.0, 0.0, 30.0, 60.0],
            flame_level_floor_v: 0.20,
            flame_confirm_frames: 3,
            temp_warn_c: 45.0,
            temp_abort_c: 55.0,
        }
    }
}

impl AutoConfig {
    /// Load a JSON override file, falling back to defaults.
    pub fn load(path: Option<&str>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("config {path} invalid ({e}); using defaults");
                Self::default()
            }),
            Err(e) => {
                tracing::warn!("config {path} unreadable ({e}); using defaults");
                Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_slow_enough_for_a_first_floor_test() {
        let c = AutoConfig::default();
        assert!(c.max_speed <= 90);
        assert!(c.stop_distance_m >= 0.30);
        assert!(c.max_stops >= 1);
        assert!(c.flame_bearing_deg.iter().all(|v| v.abs() <= 90.0));
    }

    #[test]
    fn partial_json_keeps_the_other_defaults() {
        let c: AutoConfig = serde_json::from_str(r#"{"max_speed": 40}"#).unwrap();
        assert_eq!(c.max_speed, 40);
        assert_eq!(c.turn_speed, 55);
    }
}
