//! Survey mission FSM. Pure state transitions: the binary decides how to
//! drive, this module decides what phase comes next and when to go home.

use crate::config::AutoConfig;
use crate::geo::distance;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Hop,
    Scan,
    Decide,
    Return,
    Hold,
    Manual,
    Fault,
}

impl State {
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Hop => "hop",
            State::Scan => "scan",
            State::Decide => "decide",
            State::Return => "return",
            State::Hold => "hold",
            State::Manual => "manual",
            State::Fault => "fault",
        }
    }

    pub fn active(&self) -> bool {
        matches!(
            self,
            State::Hop | State::Scan | State::Decide | State::Return
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Start,
    Stop,
    Takeover,
    Release,
    HeartbeatLost,
    Arrived,
    ScanStarted,
    ScanComplete,
    ScanTimeout,
    GoalReached,
    ReturnBlocked,
    ObstacleBlocked,
    PersonSeen,
    FireSeen,
    ThermalAbort,
    GasAlarm,
    PoseJump,
    MotorLinkLost,
    ScanLost,
    Resume,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Stop,
    DriveHop,
    WaitScan,
    PlanNext,
    ReturnHome,
    Hold,
    Idle,
}

#[derive(Debug, Clone)]
pub struct Mission {
    pub state: State,
    pub reason: String,
    pub stops: u32,
    pub started_s: Option<f64>,
    pub phase_started_s: f64,
    breadcrumbs: Vec<(f64, f64)>,
    last_crumb: Option<(f64, f64)>,
    cfg: AutoConfig,
}

const MAX_CRUMBS: usize = 5000;
const HOP_TIMEOUT_SLACK_S: f64 = 5.0;
const RETURN_TIMEOUT_S: f64 = 180.0;

impl Mission {
    pub fn new(cfg: AutoConfig) -> Self {
        Self {
            state: State::Idle,
            reason: String::new(),
            stops: 0,
            started_s: None,
            phase_started_s: 0.0,
            breadcrumbs: Vec::new(),
            last_crumb: None,
            cfg,
        }
    }

    pub fn active(&self) -> bool {
        self.state.active()
    }

    pub fn elapsed(&self, now_s: f64) -> f64 {
        self.started_s.map_or(0.0, |t| now_s - t)
    }

    pub fn record_pose(&mut self, x: f64, y: f64) {
        if self.state != State::Hop {
            return;
        }
        let p = (x, y);
        if self.breadcrumbs.len() >= MAX_CRUMBS {
            return;
        }
        match self.last_crumb {
            None => {
                self.breadcrumbs.push(p);
                self.last_crumb = Some(p);
            }
            Some(prev) if distance(prev, p) >= self.cfg.breadcrumb_m => {
                self.breadcrumbs.push(p);
                self.last_crumb = Some(p);
            }
            _ => {}
        }
    }

    /// Route home, newest crumb first.
    pub fn return_path(&self) -> Vec<(f64, f64)> {
        self.breadcrumbs.iter().rev().copied().collect()
    }

    pub fn has_return_path(&self) -> bool {
        !self.breadcrumbs.is_empty()
    }

    fn enter(&mut self, state: State, now_s: f64, reason: impl Into<String>) {
        self.state = state;
        self.phase_started_s = now_s;
        self.reason = reason.into();
    }

    fn return_or_hold(&mut self, now_s: f64, reason: impl Into<String>) -> Vec<Action> {
        let reason = reason.into();
        if self.has_return_path() {
            self.enter(State::Return, now_s, reason);
            vec![Action::Stop, Action::ReturnHome]
        } else {
            self.enter(State::Hold, now_s, format!("{reason} (no return path)"));
            vec![Action::Stop, Action::Hold]
        }
    }

    pub fn step(&mut self, event: Event, now_s: f64) -> Vec<Action> {
        match event {
            Event::Start => {
                self.stops = 0;
                self.started_s = Some(now_s);
                self.breadcrumbs.clear();
                self.last_crumb = None;
                self.enter(State::Hop, now_s, "mission started");
                vec![Action::DriveHop]
            }
            Event::Stop => {
                self.enter(State::Idle, now_s, "stopped by operator");
                self.started_s = None;
                vec![Action::Stop, Action::Idle]
            }
            Event::Takeover => {
                self.enter(State::Manual, now_s, "operator took control");
                vec![Action::Stop]
            }
            Event::Release => {
                if self.state == State::Manual {
                    self.enter(State::Idle, now_s, "manual released");
                    self.started_s = None;
                    vec![Action::Stop, Action::Idle]
                } else {
                    vec![]
                }
            }
            Event::HeartbeatLost => match self.state {
                State::Hop | State::Scan | State::Decide => {
                    self.return_or_hold(now_s, "link lost, returning")
                }
                State::Return => {
                    self.reason = "link lost, returning".into();
                    vec![Action::ReturnHome]
                }
                State::Manual => {
                    self.reason = "link lost, manual hold".into();
                    vec![Action::Stop]
                }
                State::Hold => vec![Action::Hold],
                State::Idle | State::Fault => vec![Action::Idle],
            },
            Event::Arrived => {
                if self.state == State::Hop {
                    self.enter(State::Scan, now_s, "arrived, scanning");
                    vec![Action::Stop, Action::WaitScan]
                } else {
                    vec![]
                }
            }
            Event::ScanStarted => {
                if self.state == State::Scan {
                    self.reason = "scanning".into();
                    vec![Action::WaitScan]
                } else {
                    vec![]
                }
            }
            Event::ScanComplete => {
                if self.state == State::Scan {
                    self.stops = self.stops.saturating_add(1);
                    self.enter(State::Decide, now_s, "scan complete");
                    vec![Action::PlanNext]
                } else {
                    vec![]
                }
            }
            Event::ScanTimeout => {
                if self.state == State::Scan {
                    self.enter(State::Hold, now_s, "scan did not complete");
                    vec![Action::Stop, Action::Hold]
                } else {
                    vec![]
                }
            }
            Event::GoalReached => {
                if self.state == State::Return {
                    self.enter(State::Idle, now_s, "mission complete");
                    self.started_s = None;
                    vec![Action::Stop, Action::Idle]
                } else {
                    vec![]
                }
            }
            Event::ReturnBlocked => {
                if self.state == State::Return {
                    self.enter(State::Hold, now_s, "return blocked");
                    vec![Action::Stop, Action::Hold]
                } else {
                    vec![]
                }
            }
            Event::ObstacleBlocked => {
                if self.state.active() {
                    self.enter(State::Hold, now_s, "no passable gap");
                    vec![Action::Stop, Action::Hold]
                } else {
                    vec![]
                }
            }
            Event::PersonSeen => {
                if self.state.active() {
                    self.enter(State::Hold, now_s, "person in the path");
                    vec![Action::Stop, Action::Hold]
                } else {
                    vec![]
                }
            }
            Event::FireSeen => {
                if self.state.active() {
                    self.enter(State::Hold, now_s, "fire ahead");
                    vec![Action::Stop, Action::Hold]
                } else {
                    vec![]
                }
            }
            Event::ThermalAbort => self.return_or_hold(now_s, "thermal limit, returning"),
            Event::GasAlarm => self.return_or_hold(now_s, "gas alarm, returning"),
            Event::PoseJump => {
                if self.state.active() {
                    self.enter(State::Hold, now_s, "pose jump");
                    vec![Action::Stop, Action::Hold]
                } else {
                    vec![]
                }
            }
            Event::MotorLinkLost => {
                if self.state.active() {
                    self.enter(State::Hold, now_s, "motor board offline");
                    vec![Action::Stop, Action::Hold]
                } else {
                    vec![]
                }
            }
            Event::ScanLost => {
                if self.state.active() {
                    self.enter(State::Hold, now_s, "scan stale");
                    vec![Action::Stop, Action::Hold]
                } else {
                    vec![]
                }
            }
            Event::Resume => {
                if self.state == State::Hold {
                    self.enter(State::Decide, now_s, "operator resumed");
                    vec![Action::PlanNext]
                } else {
                    vec![]
                }
            }
        }
    }

    /// Decide what the next phase is. Called after `PlanNext`.
    pub fn decide(&mut self, now_s: f64) -> Vec<Action> {
        if self.state != State::Decide {
            return vec![];
        }
        let over_stops = self.stops >= self.cfg.max_stops;
        let over_time = self.elapsed(now_s) >= self.cfg.max_mission_s;
        if over_stops || over_time {
            let why = if over_stops {
                "stop limit reached, returning"
            } else {
                "time limit reached, returning"
            };
            self.return_or_hold(now_s, why)
        } else {
            self.enter(State::Hop, now_s, "next hop");
            vec![Action::DriveHop]
        }
    }

    /// Phase and mission timeouts. Returns actions when a timeout fires.
    pub fn tick(&mut self, now_s: f64) -> Vec<Action> {
        if !self.state.active() {
            return vec![];
        }
        if let Some(started) = self.started_s {
            if now_s - started >= self.cfg.max_mission_s {
                return self.return_or_hold(now_s, "mission time limit, returning");
            }
        }
        let phase = now_s - self.phase_started_s;
        match self.state {
            State::Scan if phase >= self.cfg.scan_timeout_s => {
                self.enter(State::Hold, now_s, "scan did not complete");
                vec![Action::Stop, Action::Hold]
            }
            State::Hop if phase >= self.cfg.hop_seconds + HOP_TIMEOUT_SLACK_S => {
                self.enter(State::Hold, now_s, "hop timeout");
                vec![Action::Stop, Action::Hold]
            }
            State::Return if phase >= RETURN_TIMEOUT_S => {
                self.enter(State::Hold, now_s, "return timeout");
                vec![Action::Stop, Action::Hold]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission() -> Mission {
        Mission::new(AutoConfig::default())
    }

    #[test]
    fn survey_cycle_then_return() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        assert_eq!(m.state, State::Hop);
        for i in 0..5 {
            m.record_pose(i as f64 * 0.5, 0.0);
            m.step(Event::Arrived, 10.0);
            m.step(Event::ScanComplete, 60.0);
            assert_eq!(m.state, State::Decide);
            let a = m.decide(60.0);
            if i < 4 {
                assert_eq!(m.state, State::Hop);
                assert!(a.contains(&Action::DriveHop));
            } else {
                assert_eq!(m.state, State::Return);
                assert!(a.contains(&Action::ReturnHome));
            }
        }
        m.step(Event::GoalReached, 120.0);
        assert_eq!(m.state, State::Idle);
    }

    #[test]
    fn breadcrumbs_come_back_reversed() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.record_pose(0.0, 0.0);
        m.record_pose(0.5, 0.0);
        m.record_pose(0.0, 0.5);
        let path = m.return_path();
        assert_eq!(path, vec![(0.0, 0.5), (0.5, 0.0), (0.0, 0.0)]);
    }

    #[test]
    fn link_loss_returns_home() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.record_pose(0.0, 0.0);
        m.record_pose(1.0, 0.0);
        let a = m.step(Event::HeartbeatLost, 20.0);
        assert_eq!(m.state, State::Return);
        assert!(a.contains(&Action::ReturnHome));
    }

    #[test]
    fn link_loss_without_crumbs_holds() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.step(Event::HeartbeatLost, 2.0);
        assert_eq!(m.state, State::Hold);
    }

    #[test]
    fn blocked_return_holds() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.record_pose(0.0, 0.0);
        m.step(Event::HeartbeatLost, 5.0);
        assert_eq!(m.state, State::Return);
        m.step(Event::ReturnBlocked, 15.0);
        assert_eq!(m.state, State::Hold);
    }

    #[test]
    fn takeover_requires_an_explicit_resume() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.step(Event::Takeover, 5.0);
        assert_eq!(m.state, State::Manual);
        m.step(Event::Release, 6.0);
        assert_eq!(m.state, State::Idle);
        assert!(!m.active());
    }

    #[test]
    fn pose_jump_holds() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.record_pose(0.0, 0.0);
        m.step(Event::PoseJump, 5.0);
        assert_eq!(m.state, State::Hold);
    }

    #[test]
    fn motor_or_scan_loss_holds() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.record_pose(0.0, 0.0);
        m.step(Event::MotorLinkLost, 2.0);
        assert_eq!(m.state, State::Hold);
        assert!(m.reason.contains("motor"));

        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.record_pose(0.0, 0.0);
        m.step(Event::ScanLost, 2.0);
        assert_eq!(m.state, State::Hold);
        assert!(m.reason.contains("scan"));
    }

    #[test]
    fn scan_timeout_holds() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.step(Event::Arrived, 1.0);
        let a = m.tick(1.0 + m.cfg.scan_timeout_s + 0.1);
        assert_eq!(m.state, State::Hold);
        assert!(a.contains(&Action::Hold));
    }

    #[test]
    fn mission_time_limit_returns() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.record_pose(0.0, 0.0);
        let a = m.tick(m.cfg.max_mission_s + 1.0);
        assert_eq!(m.state, State::Return);
        assert!(a.contains(&Action::ReturnHome));
    }

    #[test]
    fn resume_goes_back_to_decide() {
        let mut m = mission();
        m.step(Event::Start, 0.0);
        m.record_pose(0.0, 0.0);
        m.step(Event::PersonSeen, 3.0);
        assert_eq!(m.state, State::Hold);
        let a = m.step(Event::Resume, 4.0);
        assert_eq!(m.state, State::Decide);
        assert!(a.contains(&Action::PlanNext));
    }
}
