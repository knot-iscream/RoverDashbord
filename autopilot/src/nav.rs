//! Waypoint controller: A* route, pure pursuit, latched segments, arrival.
//!
//! Pure logic. `main.rs` supplies the pose, the grid, the front clearance and
//! the safety gates (board freshness, person, fire, alarm). The controller is
//! testable with a simulated unicycle, which is how the drive is proven
//! before the rover exists.

use crate::astar;
use crate::config::AutoConfig;
use crate::geo::{distance, Pose};
use crate::grid::Grid;
use crate::pursuit;

/// Replan cadence while driving. A growing map can open a better route.
pub const REPLAN_S: f64 = 2.0;
/// The firmware restarts its 12 s ramp whenever the target changes, so the
/// pursuit duty is latched for this long instead of being sent every tick.
pub const RELATCH_S: f64 = 1.5;

#[derive(Debug, Clone, PartialEq)]
pub enum NavCommand {
    Drive(i32, i32),
    Stop(&'static str),
    Done,
}

pub struct GoalNav {
    goal: Option<(f64, f64)>,
    path: Vec<(f64, f64)>,
    plan_s: f64,
    cmd_s: f64,
    last_cmd: Option<(i32, i32)>,
    reason: String,
}

impl Default for GoalNav {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalNav {
    pub fn new() -> Self {
        Self {
            goal: None,
            path: Vec::new(),
            plan_s: 0.0,
            cmd_s: 0.0,
            last_cmd: None,
            reason: String::new(),
        }
    }

    pub fn goal(&self) -> Option<(f64, f64)> {
        self.goal
    }

    pub fn path_len(&self) -> usize {
        self.path.len()
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn set_goal(&mut self, goal: (f64, f64)) {
        self.goal = Some(goal);
        self.path.clear();
        self.last_cmd = None;
        self.reason = "goal set".into();
    }

    pub fn cancel(&mut self) {
        self.goal = None;
        self.path.clear();
        self.last_cmd = None;
        self.reason = "cancelled".into();
    }

    /// Plan once from the current pose, so the state topic shows the route
    /// immediately after a goal even before the autopilot is armed.
    pub fn plan_now(&mut self, pose: Option<Pose>, grid: Option<&Grid>, now: f64) {
        let (Some(goal), Some(pose), Some(grid)) = (self.goal, pose, grid) else {
            return;
        };
        self.plan(pose, grid, goal, now);
    }

    fn plan(&mut self, pose: Pose, grid: &Grid, goal: (f64, f64), now: f64) {
        match astar::plan(grid, (pose.x, pose.y), goal) {
            Ok(path) => {
                self.path = astar::smooth(grid, &path);
                self.plan_s = now;
                self.reason = format!("path {} pts", self.path.len());
            }
            Err(e) => {
                self.path.clear();
                self.reason = format!("no path: {e}");
            }
        }
    }

    /// One control step. Safety gates are the caller's job; this only knows
    /// the goal, the map, the pose and the measured front clearance.
    pub fn step(
        &mut self,
        pose: &Pose,
        grid: &Grid,
        front_m: Option<f64>,
        cfg: &AutoConfig,
        now: f64,
    ) -> NavCommand {
        let Some(goal) = self.goal else {
            return NavCommand::Done;
        };
        if distance((pose.x, pose.y), goal) <= cfg.goal_tolerance_m {
            self.goal = None;
            self.path.clear();
            self.last_cmd = None;
            self.reason = "reached".into();
            return NavCommand::Done;
        }
        if self.path.is_empty() || now - self.plan_s > REPLAN_S {
            self.plan(*pose, grid, goal, now);
        }
        if self.path.is_empty() {
            self.last_cmd = None;
            return NavCommand::Stop("no path");
        }
        if front_m.is_some_and(|r| r <= cfg.stop_distance_m) {
            self.path.clear();
            self.last_cmd = None;
            self.reason = "obstacle".into();
            return NavCommand::Stop("obstacle");
        }
        let speed = match front_m {
            Some(r) if r < cfg.slow_distance_m => cfg.max_mps * 0.5,
            _ => cfg.max_mps,
        };
        let Some((l, r)) = pursuit::command(pose, &self.path, speed, cfg) else {
            self.last_cmd = None;
            return NavCommand::Stop("no pursuit");
        };
        if self.last_cmd.is_none() || now - self.cmd_s >= RELATCH_S {
            self.last_cmd = Some((l, r));
            self.cmd_s = now;
            self.reason = "driving".into();
        }
        let (l, r) = self.last_cmd.unwrap_or((l, r));
        NavCommand::Drive(l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free_grid() -> Grid {
        let rows = vec!["...................."; 20];
        Grid::from_rows(&rows, 0.1, (0.0, 0.0))
    }

    /// Apply one command to a simple unicycle, the same way the firmware
    /// would move the rover. No ramp, no slip: this only proves the steering.
    fn drive(pose: &mut Pose, l: i32, r: i32, cfg: &AutoConfig, dt: f64) {
        let duty = cfg.max_mps / cfg.max_speed as f64;
        let v = (l + r) as f64 / 2.0 * duty;
        let omega = (r - l) as f64 * duty / cfg.wheel_base_m;
        let yaw = pose.yaw_deg.to_radians();
        pose.x += v * yaw.cos() * dt;
        pose.y += v * yaw.sin() * dt;
        pose.yaw_deg += omega.to_degrees() * dt;
    }

    #[test]
    fn drives_to_a_goal_ahead_and_stops() {
        let cfg = AutoConfig::default();
        let grid = free_grid();
        let mut nav = GoalNav::new();
        nav.set_goal((1.0, 0.0));
        let mut pose = Pose::new(0.0, 0.0, 0.0);
        let mut done = false;
        for step in 0..200 {
            let now = step as f64 * 0.2;
            match nav.step(&pose, &grid, Some(3.0), &cfg, now) {
                NavCommand::Drive(l, r) => drive(&mut pose, l, r, &cfg, 0.2),
                NavCommand::Stop(reason) => panic!("unexpected stop: {reason}"),
                NavCommand::Done => {
                    done = true;
                    break;
                }
            }
        }
        assert!(done, "goal not reached, pose {:?}", pose);
        assert!(distance((pose.x, pose.y), (1.0, 0.0)) <= cfg.goal_tolerance_m);
        assert!(nav.goal().is_none());
    }

    #[test]
    fn close_front_stops_and_drops_the_path() {
        let cfg = AutoConfig::default();
        let grid = free_grid();
        let mut nav = GoalNav::new();
        nav.set_goal((1.0, 0.0));
        let pose = Pose::new(0.0, 0.0, 0.0);
        let cmd = nav.step(&pose, &grid, Some(cfg.stop_distance_m * 0.5), &cfg, 1.0);
        assert_eq!(cmd, NavCommand::Stop("obstacle"));
        assert_eq!(nav.path_len(), 0);
    }

    #[test]
    fn goal_inside_the_tolerance_is_done() {
        let cfg = AutoConfig::default();
        let grid = free_grid();
        let mut nav = GoalNav::new();
        nav.set_goal((0.1, 0.0));
        let pose = Pose::new(0.0, 0.0, 0.0);
        assert_eq!(
            nav.step(&pose, &grid, Some(3.0), &cfg, 1.0),
            NavCommand::Done
        );
    }

    #[test]
    fn goal_outside_the_map_has_no_path() {
        let cfg = AutoConfig::default();
        let grid = free_grid();
        let mut nav = GoalNav::new();
        nav.set_goal((20.0, 20.0));
        let pose = Pose::new(0.0, 0.0, 0.0);
        assert_eq!(
            nav.step(&pose, &grid, Some(3.0), &cfg, 1.0),
            NavCommand::Stop("no path")
        );
    }
}
