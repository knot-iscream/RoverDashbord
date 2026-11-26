//! Autonomy core for the survey rover.
//!
//! Pure logic only: scan parsing, sector gaps, occupancy grid, A*, pursuit,
//! mission FSM, operator lease, and fire localisation. The binary in
//! `main.rs` wires these to MQTT. Keeping the logic free of async and
//! hardware means the whole thing is testable on the PC before the rover
//! is built.

pub mod astar;
pub mod config;
pub mod fire;
pub mod geo;
pub mod grid;
pub mod lease;
pub mod mission;
pub mod pursuit;
pub mod scan;
pub mod segment;
pub mod vfh;

pub use config::AutoConfig;
pub use fire::{FireMark, FireTracker, FlameReading};
pub use geo::Pose;
pub use grid::Grid;
pub use lease::{Lease, Mode};
pub use mission::{Action, Event, Mission, State};
pub use segment::Segment;
