//! Pi-side autonomy loop. Wires the pure logic in `lib.rs` to the rover's
//! MQTT contract. Nothing here is required for manual driving: if this
//! process dies, the firmware deadman stops the motors and the dashboard
//! keeps working.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use autopilot::astar;
use autopilot::config::AutoConfig;
use autopilot::fire::{thermal_state, FireTracker, FlameReading, Thermal};
use autopilot::geo::{distance, Pose};
use autopilot::grid::Grid;
use autopilot::lease::{Lease, Mode};
use autopilot::mission::{Action, Event as MissionEvent, Mission, State};
use autopilot::pursuit;
use autopilot::scan::{self, Bins};
use autopilot::segment::Segment;
use autopilot::vfh;

const SUB_TOPICS: [&str; 12] = [
    "rover/lidar/scan",
    "rover/pose",
    "rover/map/grid",
    "rover/scan/state",
    "rover/autonomy/mode",
    "rover/control/heartbeat",
    "rover/env/telemetry",
    "rover/alarm/state",
    "rover/perception/detections",
    "rover/nav/goal",
    "rover/motor/status",
    "rover/mid/status",
];

const SCAN_STALE_S: f64 = 0.6;
const PERSON_HOLD_S: f64 = 1.5;

struct Runtime {
    cfg: AutoConfig,
    client: AsyncClient,
    connected: Arc<AtomicBool>,
    started: Instant,
    lease: Lease,
    mission: Mission,
    fire: FireTracker,

    bins: Option<Bins>,
    last_scan_s: Option<f64>,
    pose: Option<Pose>,
    last_pose_check: Option<Pose>,
    grid: Option<Grid>,
    scan_state: Option<Value>,
    scan_baseline_sweeps: u64,
    goal: Option<(f64, f64)>,
    /// Smoothed A* route to `goal`, empty when there is no plan.
    nav_path: Vec<(f64, f64)>,
    nav_plan_s: f64,
    nav_cmd_s: f64,
    nav_reason: String,
    /// Last `rover/alarm/state` level, for the state topic and the UI.
    alarm_level: String,

    hop_start_s: f64,
    hop_start_pose: Option<(f64, f64)>,
    return_path: Vec<(f64, f64)>,
    return_target: Option<(f64, f64)>,
    segment: Segment,
    person_until_s: f64,
    /// Closest in-path person range from the perception/lidar fusion, if the
    /// frame carried one. Only reported while the person hold is active.
    person_m: Option<f64>,
    thermal: Thermal,
    heartbeat_seen: bool,
    last_corner_s: Option<f64>,
    last_mid_s: Option<f64>,

    last_state_pub_s: f64,
    published_mission: Option<(State, String)>,
}

impl Runtime {
    fn new(cfg: AutoConfig, client: AsyncClient, connected: Arc<AtomicBool>) -> Self {
        Self {
            lease: Lease::new(0.0),
            mission: Mission::new(cfg.clone()),
            fire: FireTracker::new(),
            cfg,
            client,
            connected,
            started: Instant::now(),
            bins: None,
            last_scan_s: None,
            pose: None,
            last_pose_check: None,
            grid: None,
            scan_state: None,
            scan_baseline_sweeps: 0,
            goal: None,
            nav_path: Vec::new(),
            nav_plan_s: 0.0,
            nav_cmd_s: 0.0,
            nav_reason: String::new(),
            alarm_level: String::new(),
            hop_start_s: 0.0,
            hop_start_pose: None,
            return_path: Vec::new(),
            return_target: None,
            segment: Segment::new(),
            person_until_s: 0.0,
            person_m: None,
            thermal: Thermal::Ok,
            heartbeat_seen: false,
            last_corner_s: None,
            last_mid_s: None,
            last_state_pub_s: -1.0,
            published_mission: None,
        }
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    async fn publish(&self, topic: &str, payload: Value, retain: bool) {
        if !self.connected.load(Ordering::SeqCst) {
            return;
        }
        let _ = self
            .client
            .publish(topic, QoS::AtMostOnce, retain, payload.to_string())
            .await;
    }

    async fn publish_motor_pair(&self, left: i32, right: i32) {
        let arm = self.cfg.auto_arm_ms;
        for (motor, speed) in [(1, left), (3, left), (2, right), (4, right)] {
            self.publish(
                "rover/motor/command",
                json!({"motor": motor, "speed": speed, "arm_ms": arm}),
                false,
            )
            .await;
        }
        for (motor, speed) in [(1, left), (2, right)] {
            self.publish(
                "rover/mid/command",
                json!({"motor": motor, "speed": speed, "arm_ms": arm}),
                false,
            )
            .await;
        }
    }

    async fn publish_stop(&self) {
        // A manual zero disarms the firmware deadline. Zero is zero.
        self.publish(
            "rover/motor/command",
            json!({"motor": 0, "speed": 0}),
            false,
        )
        .await;
        self.publish("rover/mid/command", json!({"motor": 0, "speed": 0}), false)
            .await;
    }

    async fn on_message(&mut self, topic: &str, payload: &[u8]) {
        let Ok(data) = serde_json::from_slice::<Value>(payload) else {
            tracing::warn!("bad JSON on {topic}");
            return;
        };
        let now = self.now();

        match topic {
            "rover/lidar/scan" => {
                if let Ok(frame) = scan::parse(&data) {
                    self.bins = Some(Bins::from_frame(
                        &frame,
                        self.cfg.bin_deg,
                        self.cfg.lidar_yaw_offset_deg,
                    ));
                    self.last_scan_s = Some(now);
                }
            }
            "rover/pose" => {
                if let Some(p) = parse_pose(&data) {
                    if let Some(prev) = self.last_pose_check {
                        if distance((prev.x, prev.y), (p.x, p.y)) > self.cfg.pose_jump_m {
                            let actions = self.mission.step(MissionEvent::PoseJump, now);
                            self.run_actions(actions, now).await;
                        }
                    }
                    self.last_pose_check = Some(p);
                    self.pose = Some(p);
                }
            }
            "rover/map/grid" => match Grid::decode(&data) {
                Ok(g) => self.grid = Some(g),
                Err(e) => tracing::warn!("map grid rejected: {e}"),
            },
            "rover/scan/state" => {
                self.scan_state = Some(data);
            }
            "rover/control/heartbeat" => {
                self.heartbeat_seen = true;
                self.lease.heartbeat(now);
            }
            "rover/autonomy/mode" => {
                self.handle_mode(&data, now).await;
            }
            "rover/env/telemetry" => {
                let reading = parse_flame(&data);
                let range = self.bins.as_ref().and_then(|b| b.min_in_arc(0.0, 12.0));
                self.fire
                    .observe(&reading, self.pose.as_ref(), range, &self.cfg);
                let heat = reading
                    .heat_l
                    .into_iter()
                    .chain(reading.heat_r)
                    .fold(None, |m: Option<f64>, v| {
                        Some(m.map_or(v, |x: f64| x.max(v)))
                    });
                self.thermal = thermal_state(heat, &self.cfg);
                if self.thermal == Thermal::Abort && self.mission.active() {
                    let actions = self.mission.step(MissionEvent::ThermalAbort, now);
                    self.run_actions(actions, now).await;
                }
            }
            "rover/alarm/state" => {
                let level = data.get("level").and_then(|v| v.as_str()).unwrap_or("");
                self.alarm_level = level.to_string();
                if level == "alarm" {
                    tracing::warn!("gas alarm level {level}, stopping");
                    // A high gas reading clears any waypoint and stops. In a
                    // survey it returns home through the FSM; in goal mode it
                    // holds in place because there is no survey to return to.
                    self.goal = None;
                    self.nav_path.clear();
                    if self.lease.mode == Mode::Auto {
                        if self.mission.active() {
                            let actions = self.mission.step(MissionEvent::GasAlarm, now);
                            self.run_actions(actions, now).await;
                        } else {
                            self.lease.set_mode(Mode::Hold, now);
                            self.publish_stop().await;
                        }
                    }
                }
            }
            "rover/perception/detections" => {
                if let Some(range) = person_range_in_path(&data) {
                    self.person_until_s = now + PERSON_HOLD_S;
                    self.person_m = range;
                }
            }
            "rover/nav/goal" => {
                if data.get("cancel").and_then(|v| v.as_bool()) == Some(true) {
                    self.goal = None;
                    self.nav_path.clear();
                    self.segment.clear();
                    self.nav_reason = "cancelled".into();
                    self.publish_stop().await;
                } else if let (Some(x), Some(y)) = (
                    data.get("x").and_then(|v| v.as_f64()),
                    data.get("y").and_then(|v| v.as_f64()),
                ) {
                    // Retained goals survive a restart. A goal older than 5 s
                    // must not re-arm navigation by itself, same rule as the
                    // auto mode.
                    let stale = data.get("ts").and_then(|v| v.as_i64()).is_some_and(|ms| {
                        let age = chrono::Utc::now().timestamp_millis() - ms;
                        !(-60_000..=5_000).contains(&age)
                    });
                    if stale {
                        tracing::warn!("stale goal ignored");
                    } else {
                        // A goal replaces the fixed-hop survey: stop it first
                        // so neither controller publishes motor commands.
                        if self.mission.active() {
                            let actions = self.mission.step(MissionEvent::Stop, now);
                            self.run_actions(actions, now).await;
                        }
                        self.goal = Some((x, y));
                        self.nav_path.clear();
                        self.nav_cmd_s = 0.0;
                        self.replan_goal(now);
                    }
                }
            }
            "rover/motor/status" => self.last_corner_s = Some(now),
            "rover/mid/status" => self.last_mid_s = Some(now),
            _ => {}
        }
    }

    async fn handle_mode(&mut self, data: &Value, now: f64) {
        let Some(mode) = data
            .get("mode")
            .and_then(|v| v.as_str())
            .and_then(Mode::parse)
        else {
            return;
        };
        // A retained "auto" from a previous run must not start the motors.
        // The backend stamps every mode command; older than 5 s is ignored.
        let stale = data
            .get("ts")
            .and_then(|v| v.as_i64())
            .map(|ms| {
                let age = chrono::Utc::now().timestamp_millis() - ms;
                !(-60_000..=5_000).contains(&age)
            })
            .unwrap_or(false);

        match mode {
            Mode::Auto => {
                // A restart during an auto mission must not re-arm itself
                // from the retained mode message. The operator presses Auto.
                if now < 2.0 {
                    tracing::warn!("auto ignored during the 2 s boot grace");
                    return;
                }
                if stale {
                    tracing::warn!("stale auto command ignored");
                    return;
                }
                self.lease.heartbeat(now);
                self.lease.set_mode(Mode::Auto, now);
                let actions = if self.goal.is_some() {
                    // Waypoint mode: drive_goal owns the motors, do not
                    // start the fixed-hop survey on top of it.
                    vec![]
                } else if self.mission.state == State::Hold {
                    self.mission.step(MissionEvent::Resume, now)
                } else if !self.mission.active() {
                    self.mission.step(MissionEvent::Start, now)
                } else {
                    vec![]
                };
                if let Some(p) = self.pose {
                    self.mission.record_pose(p.x, p.y);
                }
                self.run_actions(actions, now).await;
            }
            Mode::Manual => {
                if self.lease.mode == Mode::Manual && self.mission.state == State::Manual {
                    return;
                }
                self.goal = None;
                self.nav_path.clear();
                self.lease.set_mode(Mode::Manual, now);
                let actions = self.mission.step(MissionEvent::Takeover, now);
                self.publish_stop().await;
                self.run_actions(actions, now).await;
            }
            Mode::Idle => {
                self.goal = None;
                self.nav_path.clear();
                self.lease.set_mode(Mode::Idle, now);
                let actions = self.mission.step(MissionEvent::Stop, now);
                self.run_actions(actions, now).await;
            }
            Mode::Hold => {
                self.lease.set_mode(Mode::Hold, now);
                self.publish_stop().await;
            }
        }
    }

    async fn run_actions(&mut self, actions: Vec<Action>, now: f64) {
        let mut queue = actions;
        while let Some(action) = queue.pop() {
            match action {
                Action::Stop | Action::Hold | Action::Idle => {
                    self.segment.clear();
                    self.return_target = None;
                    self.publish_stop().await;
                    if action == Action::Idle {
                        self.lease.set_mode(Mode::Idle, now);
                    }
                }
                Action::DriveHop => {
                    self.segment.clear();
                    self.return_target = None;
                    self.hop_start_s = now;
                    self.hop_start_pose = self.pose.map(|p| (p.x, p.y));
                    if let Some(p) = self.pose {
                        self.mission.record_pose(p.x, p.y);
                    }
                }
                Action::WaitScan => {
                    self.segment.clear();
                    self.return_target = None;
                    self.scan_baseline_sweeps = self.current_sweeps();
                    self.publish_stop().await;
                }
                Action::PlanNext => {
                    let next = self.mission.decide(now);
                    queue.extend(next);
                }
                Action::ReturnHome => {
                    self.segment.clear();
                    self.return_target = None;
                    self.return_path = self.mission.return_path();
                    if self.return_path.is_empty() {
                        let next = self.mission.step(MissionEvent::ReturnBlocked, now);
                        queue.extend(next);
                    }
                }
            }
        }
    }

    fn current_sweeps(&self) -> u64 {
        self.scan_state
            .as_ref()
            .and_then(|s| s.get("sweeps"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }

    fn scan_phase(&self) -> &str {
        self.scan_state
            .as_ref()
            .and_then(|s| s.get("phase"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }

    async fn on_tick(&mut self) {
        let now = self.now();

        // Heartbeat is a hazard signal: lose it in auto and we go home.
        if self.lease.mode == Mode::Auto
            && !self
                .lease
                .operator_present(now, self.cfg.heartbeat_timeout_s)
        {
            self.lease.set_mode(Mode::Hold, now);
            let actions = self.mission.step(MissionEvent::HeartbeatLost, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
        }

        // Manual link loss stops the rover. There is no safe way to keep
        // driving a mode nobody is supervising.
        if self.lease.mode == Mode::Manual
            && !self
                .lease
                .operator_present(now, self.cfg.heartbeat_timeout_s)
        {
            self.lease.set_mode(Mode::Hold, now);
            self.publish_stop().await;
        }

        // Waypoint mode takes precedence over the fixed-hop survey. The
        // survey is stopped when the goal arrives, so only one controller
        // publishes motor commands.
        if self.goal.is_some() && self.lease.mode == Mode::Auto {
            self.drive_goal(now).await;
            if now - self.last_state_pub_s >= 0.5 {
                self.last_state_pub_s = now;
                self.publish_state(now).await;
            }
            return;
        }

        // Both motor boards must be alive before any autonomous motion.
        if self.mission.active() && !self.boards_ready(now) {
            let actions = self.mission.step(MissionEvent::MotorLinkLost, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }

        let tick_actions = self.mission.tick(now);
        self.run_actions(tick_actions, now).await;

        match self.mission.state {
            State::Hop => self.drive_hop(now).await,
            State::Scan => self.drive_scan(now).await,
            State::Decide => {
                let actions = self.mission.decide(now);
                self.run_actions(actions, now).await;
            }
            State::Return => self.drive_return(now).await,
            State::Idle | State::Hold | State::Manual | State::Fault => {
                if self.lease.mode != Mode::Manual {
                    self.publish_stop().await;
                }
            }
        }

        if now - self.last_state_pub_s >= 0.5 {
            self.last_state_pub_s = now;
            self.publish_state(now).await;
        }
    }

    fn boards_ready(&self, now: f64) -> bool {
        self.last_corner_s.is_some_and(|t| now - t < 3.0)
            && self.last_mid_s.is_some_and(|t| now - t < 3.0)
    }

    /// Plan from the current pose to the stored goal on the latest nav grid.
    /// Called when a goal arrives and every 2 s while driving, so a growing
    /// map can open a better route.
    fn replan_goal(&mut self, now: f64) {
        let Some(goal) = self.goal else { return };
        let Some(pose) = self.pose else { return };
        let Some(grid) = self.grid.as_ref() else {
            return;
        };
        match astar::plan(grid, (pose.x, pose.y), goal) {
            Ok(path) => {
                self.nav_path = astar::smooth(grid, &path);
                self.nav_plan_s = now;
                self.nav_reason = format!("path {} pts", self.nav_path.len());
                tracing::info!(
                    "goal ({:.2},{:.2}) planned, {} points",
                    goal.0,
                    goal.1,
                    self.nav_path.len()
                );
            }
            Err(e) => {
                self.nav_path.clear();
                self.nav_reason = format!("no path: {e}");
                tracing::warn!("goal ({:.2},{:.2}) plan failed: {e}", goal.0, goal.1);
            }
        }
    }

    /// Waypoint controller: A* route plus pure pursuit, latched in segments
    /// so the firmware's 12 s torque ramp can actually progress.
    async fn drive_goal(&mut self, now: f64) {
        if !self.boards_ready(now) {
            self.segment.clear();
            self.publish_stop().await;
            self.nav_reason = "boards not ready".into();
            return;
        }
        if self.person_until_s > now {
            self.segment.clear();
            self.publish_stop().await;
            self.nav_reason = "person".into();
            return;
        }
        if self.fire.last.is_some() {
            self.segment.clear();
            self.publish_stop().await;
            self.nav_reason = "fire".into();
            return;
        }
        let Some(goal) = self.goal else {
            self.publish_stop().await;
            return;
        };
        let Some(pose) = self.pose else {
            self.publish_stop().await;
            return;
        };
        if distance((pose.x, pose.y), goal) <= self.cfg.goal_tolerance_m {
            self.goal = None;
            self.nav_path.clear();
            self.segment.clear();
            self.publish_stop().await;
            self.nav_reason = "reached".into();
            tracing::info!("goal reached");
            return;
        }
        if self.last_scan_s.is_none_or(|t| now - t > SCAN_STALE_S) {
            self.segment.clear();
            self.publish_stop().await;
            self.nav_reason = "scan stale".into();
            return;
        }
        if self.nav_path.is_empty() || now - self.nav_plan_s > 2.0 {
            self.replan_goal(now);
        }
        if self.nav_path.is_empty() {
            self.segment.clear();
            self.publish_stop().await;
            return;
        }
        let front = self.bins.as_ref().and_then(|b| b.min_in_arc(0.0, 20.0));
        if front.is_some_and(|r| r <= self.cfg.stop_distance_m) {
            self.segment.clear();
            self.nav_path.clear();
            self.publish_stop().await;
            self.nav_reason = "obstacle".into();
            return;
        }
        let speed = match front {
            Some(r) if r < self.cfg.slow_distance_m => self.cfg.max_mps * 0.5,
            _ => self.cfg.max_mps,
        };
        let Some((l, r)) = pursuit::command(&pose, &self.nav_path, speed, &self.cfg) else {
            self.segment.clear();
            self.publish_stop().await;
            return;
        };
        // Re-latch at most every 1.5 s. Changing the duty restarts the ramp,
        // so a per-tick update would never build speed.
        if !self.segment.is_set() || now - self.nav_cmd_s >= 1.5 {
            self.segment.set(l, r, false, now);
            self.nav_cmd_s = now;
            self.nav_reason = format!("driving {} pts", self.nav_path.len());
            tracing::info!("goal segment left={l} right={r}");
        }
        if let Some((l, r)) = self.segment.target() {
            self.publish_motor_pair(l, r).await;
        } else {
            self.publish_stop().await;
        }
    }

    async fn drive_hop(&mut self, now: f64) {
        if self.person_until_s > now {
            self.segment.clear();
            let actions = self.mission.step(MissionEvent::PersonSeen, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }
        if self.fire.last.is_some() {
            self.segment.clear();
            let actions = self.mission.step(MissionEvent::FireSeen, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }
        let Some(bins) = self.bins.as_ref() else {
            self.publish_stop().await;
            return;
        };
        let stale = self.last_scan_s.is_none_or(|t| now - t > SCAN_STALE_S);
        if stale {
            self.segment.clear();
            self.publish_stop().await;
            if self.last_scan_s.is_some_and(|t| now - t > 1.0) {
                let actions = self.mission.step(MissionEvent::ScanLost, now);
                self.run_actions(actions, now).await;
            }
            return;
        }
        if let Some(p) = self.pose {
            self.mission.record_pose(p.x, p.y);
        }

        let arrived = match (self.hop_start_pose, self.pose) {
            (Some(start), Some(p)) => distance(start, (p.x, p.y)) >= self.cfg.max_hop_m,
            _ => now - self.hop_start_s >= self.cfg.hop_seconds,
        };
        if arrived {
            self.segment.clear();
            let actions = self.mission.step(MissionEvent::Arrived, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }

        let front = bins.min_in_arc(0.0, 20.0);
        let front_blocked = front.is_some_and(|r| r <= self.cfg.stop_distance_m);
        let front_clear = front.is_none_or(|r| r > self.cfg.stop_distance_m + 0.10);

        // One target per segment. Changing it while moving would restart
        // the 12 s ramp, so a new target only happens from a stop.
        if !self.segment.is_set() {
            let gaps = vfh::find_gaps(bins, &self.cfg);
            let Some(gap) = vfh::choose_gap(&gaps, 0.0, None, &self.cfg) else {
                let actions = self.mission.step(MissionEvent::ObstacleBlocked, now);
                self.publish_stop().await;
                self.run_actions(actions, now).await;
                return;
            };
            let base = if front_blocked {
                0
            } else {
                vfh::speed_for_clearance(gap.min_range_m, &self.cfg)
            };
            let (l, r) = vfh::banks_for_heading(gap.center_deg(), base, &self.cfg);
            self.segment.set(l, r, front_blocked, now);
            tracing::info!(
                "segment left={l} right={r} pivot={front_blocked} gap={:.0}deg",
                gap.center_deg()
            );
        }

        // A pivot segment ends when the front is clear. The mission then
        // scans from a stop; the next hop drives into the opening.
        if self.segment.pivot() && front_clear {
            self.segment.clear();
            let actions = self.mission.step(MissionEvent::Arrived, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }

        // A driving segment that sees a close front stops first. The next
        // tick chooses a pivot target from zero.
        if !self.segment.pivot() && front_blocked {
            self.segment.clear();
            self.publish_stop().await;
            return;
        }

        if let Some((l, r)) = self.segment.target() {
            self.publish_motor_pair(l, r).await;
        } else {
            self.publish_stop().await;
        }
    }

    async fn drive_scan(&mut self, now: f64) {
        self.publish_stop().await;
        if self.last_scan_s.is_none_or(|t| now - t > 1.0) {
            let actions = self.mission.step(MissionEvent::ScanLost, now);
            self.run_actions(actions, now).await;
            return;
        }
        if self.fire.last.is_some() {
            let actions = self.mission.step(MissionEvent::FireSeen, now);
            self.run_actions(actions, now).await;
            return;
        }
        if self.person_until_s > now {
            let actions = self.mission.step(MissionEvent::PersonSeen, now);
            self.run_actions(actions, now).await;
            return;
        }
        if self.current_sweeps() > self.scan_baseline_sweeps {
            let actions = self.mission.step(MissionEvent::ScanComplete, now);
            self.run_actions(actions, now).await;
            return;
        }
        if self.scan_phase() == "scanning" {
            let actions = self.mission.step(MissionEvent::ScanStarted, now);
            self.run_actions(actions, now).await;
        }
    }

    async fn drive_return(&mut self, now: f64) {
        if self.last_scan_s.is_none_or(|t| now - t > 1.0) {
            self.segment.clear();
            let actions = self.mission.step(MissionEvent::ScanLost, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }
        if self.person_until_s > now || self.fire.last.is_some() {
            self.segment.clear();
            let event = if self.fire.last.is_some() {
                MissionEvent::FireSeen
            } else {
                MissionEvent::PersonSeen
            };
            let actions = self.mission.step(event, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }
        let Some(pose) = self.pose else {
            self.publish_stop().await;
            return;
        };
        if self.return_path.is_empty() {
            self.segment.clear();
            let actions = self.mission.step(MissionEvent::ReturnBlocked, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }
        // Drop points we have already reached.
        self.return_path
            .retain(|p| distance((pose.x, pose.y), *p) > 0.25);
        let Some(last) = self.return_path.last().copied() else {
            self.segment.clear();
            let actions = self.mission.step(MissionEvent::GoalReached, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        };
        if distance((pose.x, pose.y), last) <= self.cfg.goal_tolerance_m {
            self.segment.clear();
            let actions = self.mission.step(MissionEvent::GoalReached, now);
            self.publish_stop().await;
            self.run_actions(actions, now).await;
            return;
        }
        if let Some(bins) = self.bins.as_ref() {
            if bins
                .min_in_arc(0.0, 15.0)
                .is_some_and(|r| r <= self.cfg.stop_distance_m)
            {
                self.segment.clear();
                let actions = self.mission.step(MissionEvent::ReturnBlocked, now);
                self.publish_stop().await;
                self.run_actions(actions, now).await;
                return;
            }
        }

        // One target per segment: a breadcrumb about 1 m ahead, then re-aim
        // from a stop. The 12 s ramp makes continuous pursuit impossible.
        if !self.segment.is_set() {
            let target = self
                .return_path
                .iter()
                .copied()
                .find(|p| distance((pose.x, pose.y), *p) >= 1.0)
                .unwrap_or(last);
            self.return_target = Some(target);
            let speed_mps = self.cfg.max_mps * 0.5;
            match pursuit::command(&pose, &[target], speed_mps, &self.cfg) {
                Some((l, r)) => {
                    let pivot = (l < 0 && r > 0) || (l > 0 && r < 0);
                    self.segment.set(l, r, pivot, now)
                }
                None => {
                    self.publish_stop().await;
                    return;
                }
            }
        }

        if let Some(target) = self.return_target {
            if distance((pose.x, pose.y), target) <= 0.30
                || self.segment.age(now) >= self.cfg.hop_seconds
            {
                self.segment.clear();
                self.return_target = None;
                self.publish_stop().await;
                return;
            }
        }

        if let Some((l, r)) = self.segment.target() {
            self.publish_motor_pair(l, r).await;
        } else {
            self.publish_stop().await;
        }
    }

    async fn publish_state(&mut self, now: f64) {
        let front = self.bins.as_ref().and_then(|b| b.min_in_arc(0.0, 20.0));
        let scan_age = self.last_scan_s.map(|t| now - t);
        let state = json!({
            "state": self.mission.state.as_str(),
            "mode": self.lease.mode.as_str(),
            "reason": self.mission.reason,
            "stops": self.mission.stops,
            "max_stops": self.cfg.max_stops,
            "elapsed_s": self.mission.elapsed(now).round(),
            "front_m": front.map(|v| (v * 100.0).round() / 100.0),
            "scan_age_s": scan_age.map(|v| (v * 100.0).round() / 100.0),
            "pose": self.pose.map(|p| json!({"x": p.x, "y": p.y, "yaw": p.yaw_deg})),
            "fire": self.fire.last,
            "person_m": if self.person_until_s > now {
                self.person_m
            } else {
                None
            },
            "thermal": match self.thermal {
                Thermal::Ok => "ok",
                Thermal::Warn => "warn",
                Thermal::Abort => "abort",
            },
            "operator_present": self.lease.operator_present(now, self.cfg.heartbeat_timeout_s),
            "heartbeat_seen": self.heartbeat_seen,
            "boards_ready": self.boards_ready(now),
            "corner_age_s": self.last_corner_s.map(|t| (now - t).round()),
            "mid_age_s": self.last_mid_s.map(|t| (now - t).round()),
            "goal": self.goal.map(|g| json!({"x": g.0, "y": g.1})),
            "path_len": self.nav_path.len(),
            "nav": self.nav_reason.clone(),
            "alarm": self.alarm_level.clone(),
            "segment": self.segment.target().map(|(l, r)| json!({
                "left": l,
                "right": r,
                "pivot": self.segment.pivot(),
            })),
        });
        self.publish("rover/autonomy/state", state, false).await;

        let key = (self.mission.state, self.mission.reason.clone());
        if self.published_mission.as_ref() != Some(&key) {
            self.published_mission = Some(key);
            let mission = json!({
                "state": self.mission.state.as_str(),
                "reason": self.mission.reason,
                "stops": self.mission.stops,
                "elapsed_s": self.mission.elapsed(now).round(),
                "return_points": self.return_path.len(),
            });
            self.publish("rover/mission/state", mission, true).await;
        }
    }
}

fn parse_pose(data: &Value) -> Option<Pose> {
    let x = data.get("x").and_then(|v| v.as_f64())?;
    let y = data.get("y").and_then(|v| v.as_f64())?;
    let yaw = data
        .get("yaw")
        .or_else(|| data.get("yaw_deg"))
        .or_else(|| data.get("heading"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    Some(Pose::new(x, y, yaw))
}

fn parse_flame(data: &Value) -> FlameReading {
    let mask = data
        .get("flame_mask")
        .and_then(|v| v.as_u64())
        .map(|v| v as u8)
        .or_else(|| {
            data.get("flame_dir")
                .and_then(|v| v.as_u64())
                .filter(|d| *d >= 1 && *d <= 5)
                .map(|d| 1u8 << (d - 1))
        })
        .unwrap_or(0);
    FlameReading {
        mask,
        level_v: data.get("flame_lvl").and_then(|v| v.as_f64()),
        heat_l: data.get("heat_l").and_then(|v| v.as_f64()),
        heat_r: data.get("heat_r").and_then(|v| v.as_f64()),
    }
}

/// Person box in the centre third of the frame. The detector publishes
/// normalised boxes; missing fields are ignored rather than guessed.
/// Person inside the middle third of the frame. The return carries the
/// fusion range when the perception service found a lidar point on that
/// bearing: `Some(Some(m))`, or `Some(None)` when the lidar was off.
fn person_range_in_path(data: &Value) -> Option<Option<f64>> {
    let items = data.get("items").and_then(|v| v.as_array())?;
    let mut found = false;
    let mut closest: Option<f64> = None;
    for it in items {
        let label = it.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let class = it.get("c").and_then(|v| v.as_i64()).unwrap_or(-1);
        let p = it.get("p").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let x = it.get("x").and_then(|v| v.as_f64()).unwrap_or(0.5);
        let w = it.get("w").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let is_person = label == "person" || class == 0;
        let centre = x + w / 2.0;
        if !(is_person && p >= 0.5 && (0.33..=0.67).contains(&centre)) {
            continue;
        }
        found = true;
        if let Some(r) = it.get("range_m").and_then(|v| v.as_f64()) {
            if r.is_finite() && closest.is_none_or(|c| r < c) {
                closest = Some(r);
            }
        }
    }
    if found {
        Some(closest)
    } else {
        None
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "rover_autopilot=info".into()),
        ))
        .init();

    let broker = std::env::var("MQTT_BROKER").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("MQTT_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(1883);
    let cfg = AutoConfig::load(std::env::var("AUTOPILOT_CONFIG").ok().as_deref());

    let mut opts = MqttOptions::new(
        format!("rover-autopilot-{}", std::process::id()),
        broker.clone(),
        port,
    );
    opts.set_keep_alive(Duration::from_secs(15));
    opts.set_max_packet_size(16 * 1024 * 1024, 16 * 1024 * 1024);
    let (client, mut eventloop) = AsyncClient::new(opts, 256);
    let connected = Arc::new(AtomicBool::new(false));
    let (tx, mut rx) = mpsc::channel::<(String, Vec<u8>)>(256);

    let ev_client = client.clone();
    let ev_connected = connected.clone();
    tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    ev_connected.store(true, Ordering::SeqCst);
                    tracing::info!("mqtt connected to {broker}:{port}");
                    for topic in SUB_TOPICS {
                        let _ = ev_client.subscribe(topic, QoS::AtMostOnce).await;
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    if tx.send((p.topic, p.payload.to_vec())).await.is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    ev_connected.store(false, Ordering::SeqCst);
                    tracing::warn!("mqtt: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });

    let mut rt = Runtime::new(cfg, client, connected);
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            msg = rx.recv() => {
                if let Some((topic, payload)) = msg {
                    rt.on_message(&topic, &payload).await;
                }
            }
            _ = tick.tick() => {
                rt.on_tick().await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::person_range_in_path;
    use serde_json::json;

    #[test]
    fn person_in_path_uses_the_closest_fused_range() {
        let data = json!({"items": [
            {"c": 0, "label": "person", "p": 0.8, "x": 0.40, "w": 0.20, "range_m": 1.4},
            {"c": 0, "label": "person", "p": 0.9, "x": 0.45, "w": 0.10, "range_m": 2.1},
        ]});
        assert_eq!(person_range_in_path(&data), Some(Some(1.4)));
    }

    #[test]
    fn person_in_path_without_a_range_still_stops() {
        let data = json!({"items": [
            {"c": 0, "label": "person", "p": 0.8, "x": 0.40, "w": 0.20},
        ]});
        assert_eq!(person_range_in_path(&data), Some(None));
    }

    #[test]
    fn person_outside_the_middle_third_is_ignored() {
        let data = json!({"items": [
            {"c": 0, "label": "person", "p": 0.9, "x": 0.02, "w": 0.10, "range_m": 1.0},
        ]});
        assert_eq!(person_range_in_path(&data), None);
    }
}
