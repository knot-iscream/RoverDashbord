//! The one-press bench capture.
//!
//! On hardware day the rover goes up on a stand, one button is pressed and
//! everything that can be captured is captured. The order is not cosmetic:
//!
//! 1. `hold_still` - the IMU zero averages a few seconds of rest pose. If the
//!    wheels spin during that window the "rest" offsets capture motor noise,
//!    so the motors are not started until it is done. The command is a no-op
//!    on boards that predate it, which costs 3 seconds and nothing else.
//! 2. `capture` - the ENV board takes its clean-air baseline. No arm sweep:
//!    no angle encoder is fitted, so there is no servo calibration to run.
//!    The app writes down what every sensor is reading over the last
//!    [`CAPTURE_WINDOW_S`].
//! 3. `motors` - the existing motor calibration runs on its own, as before.
//!
//! Every step is fire-and-forget over MQTT with the same honesty rule as
//! `motor_control`: a broker that is down means the board was not told, and
//! the response says so instead of implying the rover did something.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::mqtt::{broadcast, Ctx, MqttSlot};

/// The MQTT slot, the same `Arc<RwLock<MqttSlot>>` `AppState` holds. The
/// ingest `Ctx` has no client handle (it only broadcasts), so a spawned
/// sequence needs this to publish.
pub type Publisher = Arc<RwLock<MqttSlot>>;

/// How long the IMU zero averages, plus a little for the board to answer.
/// Matches `IMU_ZERO_WINDOW_MS` in the MID sketch's config.h.
pub const HOLD_STILL_MS: u64 = 3500;
/// Gap between the ENV/servo commands and the snapshot: the board needs a
/// moment to answer, the arm needs to reach its first limit.
pub const CAPTURE_SETTLE_MS: u64 = 1500;
/// Small gap before the motors, so the arm has finished moving.
pub const ARM_SETTLE_MS: u64 = 1500;

/// Live state of a bench run, broadcast as `bench_capture` so the page can
/// show what is happening without polling.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BenchState {
    pub running: bool,
    /// `idle` | `hold_still` | `capture` | `motors` | `done`
    pub stage: String,
    pub detail: String,
    pub started_at: Option<f64>,
    pub finished_at: Option<f64>,
    /// Sensor keys this run captured.
    pub captured: Vec<String>,
    /// Sensor keys it could not: nothing arrived in the window.
    pub missing: Vec<String>,
    /// Board commands that were not delivered (broker down, usually).
    pub unsent: Vec<String>,
    /// Bumped by start and by cancel. A sleeping step compares against the
    /// number it captured when it began and bows out if it no longer matches,
    /// which is how Stop actually stops a sequence that is mid-wait.
    #[serde(skip)]
    generation: u64,
}

impl BenchState {
    fn start() -> Self {
        Self {
            running: true,
            stage: "hold_still".into(),
            detail: "Hold still. The IMU is averaging its rest pose.".into(),
            started_at: Some(crate::data::now_secs()),
            generation: 1,
            ..Default::default()
        }
    }

    pub fn payload(&self) -> Value {
        let mut out = serde_json::to_value(self).unwrap_or(json!({}));
        if let Value::Object(map) = &mut out {
            map.insert("type".into(), json!("bench_capture"));
        }
        out
    }
}

pub type BenchSlot = Arc<RwLock<BenchState>>;

pub fn new_bench() -> BenchSlot {
    Arc::new(RwLock::new(BenchState {
        stage: "idle".into(),
        detail: "Not started.".into(),
        ..Default::default()
    }))
}

/// True when a run is already going. The page refuses a second press rather
/// than stacking two sequences on one rover.
pub async fn is_running(bench: &BenchSlot) -> bool {
    bench.read().await.running
}

async fn publish(
    mqtt: &Publisher,
    bench: &BenchSlot,
    topic: &str,
    action: &str,
    generation: u64,
) -> bool {
    let sent = mqtt
        .read()
        .await
        .handle
        .publish(topic, &json!({"action": action}))
        .await;
    if !sent {
        let label = format!("{topic} {action}");
        let mut state = bench.write().await;
        if state.generation == generation {
            state.unsent.push(label);
        }
    }
    sent
}

/// Start the sequence and return its first state. The rest runs in a spawned
/// task: a capture that needs the IMU to sit still for three seconds must not
/// hold an HTTP request open.
pub async fn start(ctx: Ctx, mqtt: Publisher, bench: BenchSlot) -> BenchState {
    let start_state = {
        let mut state = bench.write().await;
        // Bumping here retires any run still sleeping through its steps.
        state.generation += 1;
        *state = BenchState {
            generation: state.generation,
            ..BenchState::start()
        };
        state.clone()
    };
    broadcast(&ctx, start_state.payload());
    let generation = start_state.generation;

    tokio::spawn(async move {
        // No-op on pre-bench firmware: nobody subscribes to the IMU command
        // topic, so this costs the wait and nothing else.
        let _ = publish(&mqtt, &bench, "rover/imu/command", "imu_zero", generation).await;
        tokio::time::sleep(std::time::Duration::from_millis(HOLD_STILL_MS)).await;
        if !current(&bench, generation).await {
            return;
        }

        set_stage(
            &ctx,
            &bench,
            "capture",
            "Capturing the ENV baseline.",
            generation,
        )
        .await;
        let _ = publish(&mqtt, &bench, "rover/env/command", "baseline", generation).await;
        tokio::time::sleep(std::time::Duration::from_millis(CAPTURE_SETTLE_MS)).await;
        if !current(&bench, generation).await {
            return;
        }

        let (captured, missing) = snapshot(&ctx).await;
        set_stage(
            &ctx,
            &bench,
            "motors",
            "Sensor baselines written. Starting the motor sequence.",
            generation,
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(ARM_SETTLE_MS)).await;
        if !current(&bench, generation).await {
            return;
        }
        // Same two moves as POST /api/calibration/start: the local FSM moves
        // now so the page shows warmup before the board answers, and the board
        // is told to drive the sequence.
        ctx.cal.write().await.start();
        broadcast(&ctx, crate::mqtt::calibration_payload(&ctx).await);
        let _ = publish(
            &mqtt,
            &bench,
            "rover/calibration/command",
            "start",
            generation,
        )
        .await;

        {
            let mut state = bench.write().await;
            if state.generation != generation {
                return;
            }
            state.running = false;
            state.stage = "done".into();
            state.detail =
                "Bench capture finished. The motor sequence runs on its own below.".into();
            state.finished_at = Some(crate::data::now_secs());
            state.captured = captured;
            state.missing = missing;
        }
        broadcast(&ctx, bench.read().await.payload());
    });

    start_state
}

/// False once this run has been cancelled or superseded.
async fn current(bench: &BenchSlot, generation: u64) -> bool {
    bench.read().await.generation == generation
}

async fn set_stage(ctx: &Ctx, bench: &BenchSlot, stage: &str, detail: &str, generation: u64) {
    {
        let mut state = bench.write().await;
        if state.generation != generation {
            return;
        }
        state.stage = stage.to_string();
        state.detail = detail.to_string();
    }
    broadcast(ctx, bench.read().await.payload());
}

/// Average the recent frames into sensor references. Same math the per-row
/// capture button uses, so both paths store the same shape.
pub async fn snapshot(ctx: &Ctx) -> (Vec<String>, Vec<String>) {
    // The log is a std Mutex, so nothing may be awaited while it is held.
    let (env, imu) = {
        let log = ctx.sensors.lock().unwrap();
        (
            log.env_recent(crate::sensor_cal::CAPTURE_WINDOW_S),
            log.imu_recent(crate::sensor_cal::CAPTURE_WINDOW_S),
        )
    };
    let pairs = crate::sensor_cal::all_references(&env, &imu);
    let mut captured = Vec::new();
    let mut missing = Vec::new();
    {
        let mut data = ctx.data.write().await;
        for (key, reference) in pairs {
            data.capture_sensor_baseline(key, reference);
            captured.push(key.to_string());
        }
        for spec in crate::sensor_cal::SPECS.iter() {
            if !captured.iter().any(|k| k == spec.key) {
                missing.push(spec.key.to_string());
            }
        }
    }
    (captured, missing)
}

/// Abort a run in progress. The spawned task keeps sleeping through its
/// remaining steps, so the generation counter is what actually stops it.
pub async fn cancel(bench: &BenchSlot) {
    let mut state = bench.write().await;
    state.generation += 1;
    state.running = false;
    state.stage = "cancelled".into();
    state.detail = "Stopped before it finished. Anything already captured was kept.".into();
    state.finished_at = Some(crate::data::now_secs());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done() -> BenchState {
        BenchState {
            running: false,
            stage: "done".into(),
            detail: "Bench capture finished.".into(),
            started_at: Some(1.0),
            finished_at: Some(2.0),
            captured: vec!["gas".into(), "imu".into()],
            missing: vec!["servo".into()],
            unsent: vec!["rover/imu/command imu_zero".into()],
            generation: 1,
        }
    }

    #[test]
    fn payload_is_tagged_for_the_socket() {
        let payload = done().payload();
        assert_eq!(payload["type"], "bench_capture");
        assert_eq!(payload["stage"], "done");
        assert_eq!(payload["captured"][0], "gas");
        assert_eq!(payload["missing"][0], "servo");
        assert!(payload.get("generation").is_none(), "internal counter");
    }

    #[tokio::test]
    async fn idle_state_is_not_running() {
        let bench = new_bench();
        assert!(!bench.read().await.running);
        assert_eq!(bench.read().await.stage, "idle");
    }

    #[tokio::test]
    async fn cancel_retires_the_run_in_progress() {
        let bench = new_bench();
        *bench.write().await = BenchState {
            running: true,
            stage: "capture".into(),
            generation: 1,
            ..Default::default()
        };
        cancel(&bench).await;
        let state = bench.read().await;
        assert!(!state.running);
        assert_eq!(state.stage, "cancelled");
        assert_eq!(state.generation, 2, "steps compare against this");
    }

    #[test]
    fn stillness_outlasts_every_moving_stage() {
        // The IMU zero has to finish before anything moves, or the rest
        // offsets capture motor noise instead of a rest pose.
        const { assert!(HOLD_STILL_MS >= 3000, "covers the 3 s average window") }
        const { assert!(CAPTURE_SETTLE_MS > 0 && ARM_SETTLE_MS > 0) }
    }
}
