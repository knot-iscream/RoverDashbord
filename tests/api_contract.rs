//! API contract tests: every route, method, status, and JSON shape, through
//! THE production router (`build_router`, not a copy) via `oneshot`.
//! Auth matrix included: no token → 401 on mutations, cookie/Bearer/query
//! all accepted. No broker needed (dead-port client, aborted retry loop).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use common::TEST_TOKEN;

async fn app() -> (axum::Router, PathBufGuard) {
    let (state, dir) = common::test_state("api").await;
    (
        digital_twin_dashboard::build_router(state),
        PathBufGuard(dir),
    )
}

struct PathBufGuard(std::path::PathBuf);
impl Drop for PathBufGuard {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[tokio::test]
async fn env_and_imu_routes_public_null_then_frame() {
    // Telemetry reads stay token-free like /api/motors, and a feed that never
    // spoke must answer null: an empty object or a 0 would read on the page as
    // "clean air, no fire", which is a fabricated reading.
    let (state, dir) = common::test_state("api-env").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    let (s, v, _) = req(r.clone(), "GET", "/api/env", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["env"].is_null(), "no frame yet -> null");
    assert_eq!(
        v["alarms"]["level"], "unknown",
        "no frame -> alarms unknown, never ok"
    );
    let (s, v, _) = req(r.clone(), "GET", "/api/imu", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["imu"].is_null(), "no frame yet -> null");

    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/env/telemetry",
        json!({"baseline_ok": 1, "co": 1.5, "flame_dir": 0}),
    )
    .await;
    let (s, v, _) = req(r.clone(), "GET", "/api/env/history?minutes=10", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["minutes"], 10);
    assert_eq!(v["samples"][0]["co"], 1.5, "ring snapshot in chart shape");
    assert!(v["samples"][0]["at"].is_i64(), "receive time stamped");
    let (s, v, _) = req(r, "GET", "/api/env", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["env"]["co"], 1.5);
    assert_eq!(v["env"]["baseline_ok"], 1);
    assert_eq!(v["env"]["o2_est"], 1, "no cell fitted: O2 is derived");
    assert_eq!(v["env"]["o2_src"], "gas");
    // One gas ratio at 1.5 -> load 0.07 -> 20.62 %.
    assert_eq!(v["env"]["o2_pct"], 20.62);
    assert_eq!(v["alarms"]["level"], "ok", "quiet frame -> ok");
    assert!(v["alarms"]["items"].as_array().unwrap().is_empty());
}

async fn req(
    router: axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value, axum::http::HeaderMap) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let resp = router.oneshot(builder.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes)
        .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()));
    (status, json, headers)
}

async fn req_bytes(
    router: axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let resp = router
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, bytes, headers)
}

#[tokio::test]
async fn health_shape_and_public() {
    let (r, _g) = app().await;
    let (s, v, h) = req(r, "GET", "/api/health", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["clients"], 0);
    assert_eq!(v["mqtt_connected"], false);
    assert_eq!(v["device_online"], false);
    assert!(!v["version"].as_str().unwrap_or("").is_empty());
    assert!(!v["build"].as_str().unwrap_or("").is_empty());
    assert!(v["mqtt_broker"].as_str().unwrap().contains("127.0.0.1:9"));
    // No CORS headers, ever.
    assert!(!h.contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn motors_empty() {
    let (r, _g) = app().await;
    let (s, v, _) = req(r, "GET", "/api/motors", None, None).await;
    assert_eq!((s, v), (StatusCode::OK, json!({"motors": []})));
}

#[tokio::test]
async fn motor_control_auth_matrix() {
    let (r, _g) = app().await;
    // No token → 401 with the exact error shape.
    let (s, v, _) = req(
        r,
        "POST",
        "/api/motor/control",
        None,
        Some(json!({"motor": 1, "speed": 100})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(v["status"], "error");
    assert_eq!(v["detail"], "missing or invalid token");

    // Bearer → through to the handler. No broker here, so the MQTT-down
    // shape answers — note HTTP 200 with an error BODY, exactly like v1
    // (FastAPI returns 200 unless told otherwise; clients key on `sent`).
    let (r, _g) = app().await;
    let (s, v, _) = req(
        r,
        "POST",
        "/api/motor/control",
        Some(TEST_TOKEN),
        Some(json!({"motor": 1, "speed": 999})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "error");
    assert_eq!(v["sent"], false);
    assert_eq!(v["detail"], "MQTT not connected");
    assert_eq!(v["mqtt_connected"], false);
}

#[tokio::test]
async fn pump_control_auth_matrix() {
    let (r, _g) = app().await;
    // No token → same 401 shape as every other mutation.
    let (s, v, _) = req(
        r,
        "POST",
        "/api/pump",
        None,
        Some(json!({"on": true, "seconds": 10})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(v["status"], "error");
    assert_eq!(v["detail"], "missing or invalid token");

    // Dead broker: HTTP 200 with sent:false, like motor_control.
    let (r, _g) = app().await;
    let (s, v, _) = req(
        r,
        "POST",
        "/api/pump",
        Some(TEST_TOKEN),
        Some(json!({"on": true, "seconds": 10})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "error");
    assert_eq!(v["sent"], false);
    assert_eq!(v["detail"], "MQTT not connected");
    assert_eq!(v["mqtt_connected"], false);

    // A malformed body must not start water either.
    let (r, _g) = app().await;
    let (s, v, _) = req(r, "POST", "/api/pump", Some(TEST_TOKEN), Some(json!({}))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sent"], false);
}

#[tokio::test]
async fn motor_control_cookie_and_query_tokens() {
    // Cookie.
    let (state, dir) = common::test_state("api-cookie").await;
    let _guard = PathBufGuard(dir);
    let router = digital_twin_dashboard::build_router(state);
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/motor/control")
                .header("content-type", "application/json")
                .header("cookie", format!("rover_token={TEST_TOKEN}"))
                .body(Body::from(r#"{"motor":2,"speed":-50}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // ?token=.
    let (state, dir) = common::test_state("api-query").await;
    let _guard = PathBufGuard(dir);
    let router = digital_twin_dashboard::build_router(state);
    let (s, v, _) = req(
        router,
        "POST",
        &format!("/api/motor/control?token={TEST_TOKEN}"),
        None,
        Some(json!({"motor": 2, "speed": -50})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sent"], false); // passed auth, failed only on the dead broker
    assert_eq!(v["mqtt_connected"], false);
}

#[tokio::test]
async fn calibration_lifecycle_guarded() {
    let (r, _g) = app().await;
    let (s, _, _) = req(r, "POST", "/api/calibration/start", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Start → status → stop against ONE shared state (router clones).
    let (state, dir) = common::test_state("api-cal").await;
    let _guard = PathBufGuard(dir);
    let router = digital_twin_dashboard::build_router(state);
    let (s, v, _) = req(
        router.clone(),
        "POST",
        "/api/calibration/start",
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!((s, v["state"].clone()), (StatusCode::OK, json!("warmup")));
    let (s, v, _) = req(router.clone(), "GET", "/api/calibration/status", None, None).await;
    assert_eq!((s, v["state"].clone()), (StatusCode::OK, json!("warmup")));
    let (s, v, _) = req(
        router,
        "POST",
        "/api/calibration/stop",
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!((s, v["state"].clone()), (StatusCode::OK, json!("idle")));
}

#[tokio::test]
async fn calibration_status_public_and_baseline_roundtrip() {
    let (state, dir) = common::test_state("api-base").await;
    let _guard = PathBufGuard(dir);
    let router = digital_twin_dashboard::build_router(state);

    let (s, v, _) = req(router, "GET", "/api/calibration/status", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["state"], "idle");
    assert_eq!(v["warmup_remaining_s"], 120.0);

    let (state, dir) = common::test_state("api-base2").await;
    let _guard = PathBufGuard(dir);
    let router = digital_twin_dashboard::build_router(state);
    let (s, v, _) = req(
        router.clone(),
        "POST",
        "/api/calibration/baseline/2",
        Some(TEST_TOKEN),
        Some(json!({"avg_temp": 36.5})),
    )
    .await;
    assert_eq!((s, v["status"].clone()), (StatusCode::OK, json!("ok")));
    // Same state: the baseline round-trips through GET. The sensor half and
    // the bench stage come along so the page needs one request.
    let (_, v, _) = req(router, "GET", "/api/calibration", None, None).await;
    assert_eq!(v["baselines"], json!({"2": {"avg_temp": 36.5}}));
    assert_eq!(v["sensor_baselines"], json!({}));
    assert_eq!(v["bench"]["stage"], "idle");
    assert_eq!(v["bench"]["running"], false);
}

/// Feed frames the way a board would, so a capture has something to average.
async fn feed(state: &digital_twin_dashboard::AppState) {
    use digital_twin_dashboard::mqtt::on_mqtt_message;
    for co in [1.0, 1.4, 1.2] {
        on_mqtt_message(
            &state.ctx(),
            "rover/env/telemetry",
            json!({
                "co": co, "lpg": 0.9, "ch4": 1.1, "h2": 0.8, "air": 1.0,
                "flame_lvl": 0.04,
                "heat_l": 24.5, "heat_r": 24.0, "heat_l_amb": 21.0, "heat_r_amb": 21.1,
            }),
        )
        .await;
    }
    for (ax, ay) in [(0.0, 0.3), (0.02, 0.28)] {
        on_mqtt_message(
            &state.ctx(),
            "rover/imu",
            json!({"imu_ok": 1, "ax": ax, "ay": ay, "az": 0.98, "gz": -1.2}),
        )
        .await;
    }
}

#[tokio::test]
async fn sensor_capture_averages_the_window_and_stores_a_timestamp() {
    let (state, dir) = common::test_state("api-cap").await;
    let _guard = PathBufGuard(dir);
    feed(&state).await;
    let router = digital_twin_dashboard::build_router(state);

    let (s, v, _) = req(
        router.clone(),
        "POST",
        "/api/calibration/sensor/gas/capture",
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sensor"], "gas");
    // Mean of 1.0 / 1.4 / 1.2, not the last frame alone.
    assert_eq!(v["reference"]["co"], 1.2);
    assert_eq!(v["reference"]["h2"], 0.8);
    // The board was told too; the test fixture's broker is a dead port, so the
    // reply must admit it rather than claim the rover moved.
    assert_eq!(v["board_command"]["sent"], false);

    let (_, v, _) = req(router, "GET", "/api/calibration", None, None).await;
    let stored = &v["sensor_baselines"]["gas"];
    assert_eq!(stored["reference"]["co"], 1.2);
    assert_eq!(stored["window_s"], 10);
    assert!(stored["captured_at"].as_f64().unwrap() > 0.0);
}

#[tokio::test]
async fn sensor_capture_covers_every_key_and_the_imu_too() {
    let (state, dir) = common::test_state("api-cap-all").await;
    let _guard = PathBufGuard(dir);
    feed(&state).await;
    let router = digital_twin_dashboard::build_router(state);

    for key in ["gas", "flame", "heat", "imu"] {
        let (s, v, _) = req(
            router.clone(),
            "POST",
            &format!("/api/calibration/sensor/{key}/capture"),
            Some(TEST_TOKEN),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{key}");
        assert_eq!(v["sensor"], key);
        // The IMU has no board command yet (its rest offsets need the
        // bench-day firmware), so its capture is the record and nothing else.
        let has_command = v.get("board_command").is_some();
        assert_eq!(has_command, key != "imu", "{key}");
    }
    let (_, v, _) = req(router, "GET", "/api/calibration", None, None).await;
    let stored = v["sensor_baselines"].as_object().unwrap();
    assert_eq!(stored.len(), 4, "every spec key is stored: {stored:?}");
    assert_eq!(v["sensor_baselines"]["imu"]["reference"]["ay"], 0.29);
    assert!(
        v["sensor_baselines"].get("servo").is_none(),
        "no angle encoder fitted, there is nothing to capture"
    );
    assert!(
        v["sensor_baselines"].get("o2").is_none(),
        "the derived O2 is a model output, there is nothing to capture"
    );
}

#[tokio::test]
async fn sensor_capture_needs_a_reading_and_a_known_key() {
    let (state, dir) = common::test_state("api-cap-empty").await;
    let _guard = PathBufGuard(dir);
    let router = digital_twin_dashboard::build_router(state);

    // Silent board: nothing to average, and the page must be told why.
    let (s, v, _) = req(
        router.clone(),
        "POST",
        "/api/calibration/sensor/gas/capture",
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["sensor"], "gas");
    assert!(v["detail"].as_str().unwrap().contains("not sent a reading"));

    let (s, v, _) = req(
        router.clone(),
        "POST",
        "/api/calibration/sensor/nope/capture",
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(v["keys"], json!(["gas", "flame", "heat", "imu"]));
}

#[tokio::test]
async fn sensor_capture_and_bench_are_token_guarded() {
    let (state, dir) = common::test_state("api-cap-auth").await;
    let _guard = PathBufGuard(dir);
    feed(&state).await;
    let router = digital_twin_dashboard::build_router(state);
    for path in [
        "/api/calibration/sensor/gas/capture",
        "/api/calibration/bench/start",
        "/api/calibration/bench/stop",
    ] {
        let (s, _, _) = req(router.clone(), "POST", path, None, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn bench_start_is_staged_and_refuses_a_second_press() {
    let (state, dir) = common::test_state("api-bench").await;
    let _guard = PathBufGuard(dir);
    let router = digital_twin_dashboard::build_router(state);

    let (s, v, _) = req(
        router.clone(),
        "POST",
        "/api/calibration/bench/start",
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    // IMU first: the rest pose needs stillness, so nothing moves yet.
    assert_eq!(v["stage"], "hold_still");
    assert_eq!(v["running"], true);
    assert_eq!(v["type"], "bench_capture");

    let (s, _, _) = req(
        router.clone(),
        "POST",
        "/api/calibration/bench/start",
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (s, v, _) = req(
        router,
        "POST",
        "/api/calibration/bench/stop",
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["running"], false);
    assert_eq!(v["stage"], "cancelled");
}

#[tokio::test]
async fn servo_is_public_and_null_until_the_arm_speaks() {
    let (r, _g) = app().await;
    let (s, v, _) = req(r, "GET", "/api/servo", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"servo": null}), "null means `--`, not zeros");
}

#[tokio::test]
async fn lidar_is_public_and_keeps_the_full_scan() {
    let (state, dir) = common::test_state("api-lidar").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    let (s, v, _) = req(r.clone(), "GET", "/api/lidar", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"lidar": null}), "no turn yet -> null");
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/lidar/scan",
        json!({"ok": 1, "model": 24, "count": 1, "points": [[15.5, 800.0, 15]]}),
    )
    .await;
    let (s, v, _) = req(r, "GET", "/api/lidar", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["lidar"]["points"][0][0], 15.5);
}

#[tokio::test]
async fn lidar_cloud_route_is_public_and_null_until_published() {
    let (state, dir) = common::test_state("api-lidar-cloud").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    let (s, v, _) = req(r.clone(), "GET", "/api/lidar/cloud", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"cloud": null}));
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/lidar/cloud",
        json!({"count": 1, "points": [[0.1, 0.2, 0.3]]}),
    )
    .await;
    let (s, v, _) = req(r, "GET", "/api/lidar/cloud", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["cloud"]["points"][0][1], 0.2);
}

#[tokio::test]
async fn lidar_display_cloud_route_and_clear() {
    let (state, dir) = common::test_state("api-display-cloud").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    let (s, v, _) = req(r.clone(), "GET", "/api/lidar/display_cloud", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"cloud": null}));
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/lidar/display_cloud",
        json!({"count": 2, "points": [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]]}),
    )
    .await;
    let (s, v, _) = req(r.clone(), "GET", "/api/lidar/display_cloud", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["cloud"]["count"], 2);
    let (s, _, _) = req(
        r.clone(),
        "POST",
        "/api/lidar/cloud/clear",
        Some(TEST_TOKEN),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v, _) = req(r, "GET", "/api/lidar/display_cloud", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"cloud": null}));
}

#[tokio::test]
async fn lidar_service_routes_report_status_and_require_token() {
    let (state, dir) = common::test_state("api-lidar-service").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    let (s, v, _) = req(r.clone(), "GET", "/api/lidar/service/status", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"service": null}));
    for action in ["start", "stop", "clear"] {
        let path = format!("/api/lidar/service/{action}");
        let (s, _, _) = req(r.clone(), "POST", &path, None, Some(json!({}))).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, v, _) = req(r.clone(), "POST", &path, Some(TEST_TOKEN), Some(json!({}))).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["sent"], false);
        let expected = if action == "clear" {
            "clear_map"
        } else {
            action
        };
        assert_eq!(v["action"], expected);
    }
    // Clear map also drops the backend caches, so a page reload cannot
    // replay the old surface while the Pi resets OctoMap.
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/lidar/display_cloud",
        json!({"count": 1, "points": [[0.0, 0.0, 0.0]]}),
    )
    .await;
    let (_, v, _) = req(r.clone(), "GET", "/api/lidar/display_cloud", None, None).await;
    assert_eq!(v["cloud"]["count"], 1);
    let (s, _, _) = req(
        r.clone(),
        "POST",
        "/api/lidar/service/clear",
        Some(TEST_TOKEN),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v, _) = req(r, "GET", "/api/lidar/display_cloud", None, None).await;
    assert_eq!(v, json!({"cloud": null}));
}

#[tokio::test]
async fn lidar_tilt_and_clear_require_the_token_and_report_offline() {
    let (state, dir) = common::test_state("api-lidar-clear").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    let (s, _, _) = req(
        r.clone(),
        "POST",
        "/api/lidar/tilt",
        None,
        Some(json!({"angle": 15})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/lidar/tilt",
        Some(TEST_TOKEN),
        Some(json!({"angle": 15})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["angle"], 15.0);
    assert_eq!(v["sent"], false, "dead broker in tests");
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/lidar/cloud",
        json!({"count": 1, "rings": 1, "points": [[0.1, 0.2, 0.3]]}),
    )
    .await;
    let (s, v, _) = req(r.clone(), "GET", "/api/lidar/cloud", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["cloud"]["count"], 1);
    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/lidar/cloud/clear",
        Some(TEST_TOKEN),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sent"], false);
    let (s, v, _) = req(r, "GET", "/api/lidar/cloud", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v, json!({"cloud": null}));
}

#[tokio::test]
async fn scan_archive_list_cloud_download_label_delete() {
    let (state, dir) = common::test_state("api-scans").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    let (s, v, _) = req(r.clone(), "GET", "/api/scans", None, None).await;
    assert_eq!((s, v), (StatusCode::OK, json!({"scans": []})));
    let (s, v, _) = req(r.clone(), "GET", "/api/lidar/scan_state", None, None).await;
    assert_eq!((s, v), (StatusCode::OK, json!({"state": null})));

    // Seed one saved session in the laptop archive.
    let id = "20261125-190000";
    let scan_dir = state.scans.read().await.root().join(id);
    std::fs::create_dir_all(&scan_dir).unwrap();
    std::fs::write(
        scan_dir.join("meta.json"),
        json!({"id": id, "start": "x", "sweeps": 3, "partial": false, "points": 2}).to_string(),
    )
    .unwrap();
    std::fs::write(
        scan_dir.join("display.json"),
        json!({"count": 2, "rings": 0, "points": [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]]}).to_string(),
    )
    .unwrap();

    let (s, v, _) = req(r.clone(), "GET", "/api/scans", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["scans"][0]["id"], id);
    assert_eq!(v["scans"][0]["sweeps"], 3);
    let (s, v, _) = req(r.clone(), "GET", &format!("/api/scans/{id}"), None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["scan"]["points"], 2);
    let (s, v, _) = req(
        r.clone(),
        "GET",
        &format!("/api/scans/{id}/cloud"),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["cloud"]["points"][1][2], 0.6);

    let (s, bytes, headers) =
        req_bytes(r.clone(), "GET", &format!("/api/scans/{id}/download"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/zip");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("meta.json").is_ok());
    assert!(zip.by_name("display.json").is_ok());

    let (s, _, _) = req(
        r.clone(),
        "POST",
        &format!("/api/scans/{id}/label"),
        None,
        Some(json!({"label": "Living room"})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v, _) = req(
        r.clone(),
        "POST",
        &format!("/api/scans/{id}/label"),
        Some(TEST_TOKEN),
        Some(json!({"label": "  Living room  "})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["scan"]["label"], "Living room");
    let (_, v, _) = req(r.clone(), "GET", "/api/scans", None, None).await;
    assert_eq!(v["scans"][0]["label"], "Living room");

    let (s, _, _) = req(r.clone(), "DELETE", &format!("/api/scans/{id}"), None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = req(
        r.clone(),
        "DELETE",
        &format!("/api/scans/{id}"),
        Some(TEST_TOKEN),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v, _) = req(r, "GET", "/api/scans", None, None).await;
    assert_eq!(v, json!({"scans": []}));
}

#[tokio::test]
async fn scan_missing_routes_and_sync_without_pi() {
    let (state, dir) = common::test_state("api-scans-missing").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    for path in [
        "/api/scans/nope",
        "/api/scans/nope/cloud",
        "/api/scans/nope/download",
        "/api/scans/download_all",
    ] {
        let (s, _, _) = req(r.clone(), "GET", path, None, None).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (s, _, _) = req(r.clone(), "POST", "/api/scans/sync", None, Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = req(r.clone(), "POST", "/api/scans/save", None, Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/scans/save",
        Some(TEST_TOKEN),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sent"], false);
    assert_eq!(v["action"], "save");
    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/scans/sync",
        Some(TEST_TOKEN),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert_eq!(v["status"], "error");

    let (s, _, _) = req(
        r.clone(),
        "POST",
        "/api/lidar/auto",
        None,
        Some(json!({"enabled": true})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v, _) = req(
        r,
        "POST",
        "/api/lidar/auto",
        Some(TEST_TOKEN),
        Some(json!({"enabled": true})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sent"], false);
    assert_eq!(v["enabled"], true);
}

#[tokio::test]
async fn autonomy_feeds_are_public_and_mode_is_guarded() {
    let (state, dir) = common::test_state("api-autonomy").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state);

    for (path, key) in [
        ("/api/pose", "pose"),
        ("/api/autonomy", "autonomy"),
        ("/api/nav/grid", "grid"),
    ] {
        let (s, v, _) = req(r.clone(), "GET", path, None, None).await;
        assert_eq!(s, StatusCode::OK, "{path}");
        assert!(v[key].is_null(), "{path} is null before the Pi speaks");
    }

    for path in ["/api/autonomy/mode", "/api/nav/goal", "/api/nav/cancel"] {
        let body = if path.ends_with("goal") {
            Some(json!({"x": 1.0, "y": 2.0}))
        } else {
            Some(json!({}))
        };
        let (s, _, _) = req(r.clone(), "POST", path, None, body).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{path}");
    }

    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/autonomy/mode",
        Some(TEST_TOKEN),
        Some(json!({"mode": "fly"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["detail"].as_str().unwrap().contains("mode must be"));

    // Broker is down in tests: the shape must stay honest.
    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/autonomy/mode",
        Some(TEST_TOKEN),
        Some(json!({"mode": "auto"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "error");
    assert_eq!(v["sent"], false);
    assert_eq!(v["mode"], "auto");

    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/autonomy/mode",
        Some(TEST_TOKEN),
        Some(json!({"mode": "stop"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["mode"], "idle", "stop publishes the idle lease");

    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/nav/goal",
        Some(TEST_TOKEN),
        Some(json!({"x": 1.0, "y": 2.0})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sent"], false);

    let (s, _, _) = req(
        r.clone(),
        "POST",
        "/api/nav/goal",
        Some(TEST_TOKEN),
        Some(json!({"x": 999.0, "y": 0.0})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, v, _) = req(
        r,
        "POST",
        "/api/nav/cancel",
        Some(TEST_TOKEN),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["action"], "cancel");
}

#[tokio::test]
async fn manual_hold_stores_and_clears() {
    let (state, dir) = common::test_state("api-manual-hold").await;
    let _g = PathBufGuard(dir);
    let probe = state.clone();
    let r = digital_twin_dashboard::build_router(state);

    let (s, v, _) = req(
        r.clone(),
        "POST",
        "/api/motor/control",
        Some(TEST_TOKEN),
        Some(json!({"motor": 0, "speed": 120})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["sent"], false, "broker is dead in tests");
    assert_eq!(probe.manual.lock().unwrap().snapshot(), Some([120; 6]));

    let (s, _, _) = req(
        r.clone(),
        "POST",
        "/api/motor/control",
        Some(TEST_TOKEN),
        Some(json!({"motor": 5, "speed": 40})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        probe.manual.lock().unwrap().snapshot(),
        Some([120, 120, 120, 120, 40, 120])
    );

    let (s, _, _) = req(
        r.clone(),
        "POST",
        "/api/motor/control",
        Some(TEST_TOKEN),
        Some(json!({"motor": 0, "speed": 0})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(probe.manual.lock().unwrap().snapshot(), None);

    probe.manual.lock().unwrap().set(2, 50);
    let (s, _, _) = req(
        r,
        "POST",
        "/api/autonomy/mode",
        Some(TEST_TOKEN),
        Some(json!({"mode": "stop"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(probe.manual.lock().unwrap().snapshot(), None);
}

#[tokio::test]
async fn perception_route_public_null_then_boxes() {
    let (state, dir) = common::test_state("api-perception").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());

    let (s, v, _) = req(r.clone(), "GET", "/api/perception", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["perception"].is_null(), "no frame yet -> null");

    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/perception/detections",
        json!({"ok": 1, "w": 640, "h": 480,
               "items": [{"c": 0, "label": "person", "p": 0.8,
                          "x": 0.4, "y": 0.3, "w": 0.2, "h": 0.4}]}),
    )
    .await;

    let (s, v, _) = req(r, "GET", "/api/perception", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["perception"]["ok"], 1);
    assert_eq!(v["perception"]["items"][0]["label"], "person");
    assert_eq!(v["perception"]["items"][0]["p"], 0.8);
}

#[tokio::test]
async fn history_empty_and_export_400() {
    let (r, _g) = app().await;
    let (s, v, _) = req(r, "GET", "/api/history/days", None, None).await;
    assert_eq!((s, v), (StatusCode::OK, json!({"days": []})));

    let (r, _g) = app().await;
    let (s, v, _) = req(r, "GET", "/api/history/segments?day=2099-01-01", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["segments"], json!([]));

    let (r, _g) = app().await;
    let (s, _, _) = req(r, "GET", "/api/calibration/export", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn config_get_public_post_guarded() {
    let (r, _g) = app().await;
    let (s, v, _) = req(r, "GET", "/api/config", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        v.get("auth_token").is_none(),
        "token must never leak via config"
    );

    let (r, _g) = app().await;
    let (s, _, _) = req(
        r,
        "POST",
        "/api/config",
        None,
        Some(json!({"lan_access": true})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (r, _g) = app().await;
    let (s, v, _) = req(
        r,
        "POST",
        "/api/config",
        Some(TEST_TOKEN),
        Some(json!({"mqtt_broker": "192.168.0.5"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["changed"], true);
    assert_eq!(v["mqtt_broker"], "192.168.0.5");
}

#[tokio::test]
async fn token_endpoints_locked() {
    let (r, _g) = app().await;
    let (s, _, _) = req(r, "GET", "/api/auth/token", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (r, _g) = app().await;
    let (s, v, _) = req(r, "GET", "/api/auth/token", Some(TEST_TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["token"], TEST_TOKEN);
}

#[tokio::test]
async fn static_pages_and_cookie_plant() {
    let (r, _g) = app().await;
    let (s, _, h) = req(r, "GET", "/", None, None).await;
    assert_eq!(s, StatusCode::OK);
    let cookie = h
        .get("set-cookie")
        .expect("entry GET plants the cookie")
        .to_str()
        .unwrap();
    assert!(
        cookie.starts_with("rover_token="),
        "cookie carries the token"
    );
    assert!(cookie.contains("HttpOnly"));

    let (r, _g) = app().await;
    let (s, _, _) = req(r, "GET", "/setup/", None, None).await;
    assert_eq!(s, StatusCode::OK);

    // Root-URL SPA routes are entry paths too. The no-slash variants used
    // to be missing from ENTRY_PATHS: a first visit there planted no
    // cookie, so the page's WS upgrade and mutations 401ed.
    for path in ["/test", "/detailed", "/calibration", "/control"] {
        let (r, _g) = app().await;
        let (s, _, h) = req(r, "GET", path, None, None).await;
        assert_eq!(s, StatusCode::OK, "SPA root route {path}");
        let cookie = h
            .get("set-cookie")
            .expect("root SPA entry GET plants the cookie")
            .to_str()
            .unwrap();
        assert!(
            cookie.starts_with("rover_token="),
            "cookie carries the token"
        );
    }

    // SPA routes serve the trunk bundle (built in CI before tests).
    for path in [
        "/next/",
        "/next/detailed",
        "/next/test",
        "/next/calibration",
        "/next/control",
    ] {
        let (r, _g) = app().await;
        let (s, _, _) = req(r, "GET", path, None, None).await;
        assert_eq!(s, StatusCode::OK, "SPA route {path}");
    }

    // The bench control route serves the real WASM entry, not a stub.
    let (r, _g) = app().await;
    let (s, v, _) = req(r, "GET", "/next/control", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        v.as_str().is_some_and(|b| b.contains("Rover Digital Twin")),
        "control route serves the WASM entry"
    );

    // API responses plant nothing.
    let (r, _g) = app().await;
    let (_, _, h) = req(r, "GET", "/api/health", None, None).await;
    assert!(!h.contains_key("set-cookie"));
}

#[tokio::test]
async fn fire_page_is_gone() {
    // The old ENV page was redistributed into the Rust UI (home gas panel +
    // Detailed sensor sections); both old paths must be dead — no stale page,
    // no cookie planted, no cache headers pretending it exists.
    for path in ["/fire", "/fire/"] {
        let (r, _g) = app().await;
        let (s, _, h) = req(r, "GET", path, None, None).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path} must 404 after removal");
        assert!(!h.contains_key("set-cookie"), "{path} plants nothing");
    }
}

#[tokio::test]
async fn theme_css_revalidates() {
    // The shared /style.css changes as often as the markup it styles. A
    // heuristically cached copy once left the live window showing the new gas
    // panel as unstyled stacked text while every fresh client looked correct.
    // CSS must revalidate on each load instead of going stale for hours.
    let (r, _g) = app().await;
    let (s, v, h) = req(r, "GET", "/style.css", None, None).await;
    assert_eq!(s, StatusCode::OK, "vanilla theme still served");
    assert!(
        v.as_str().is_some_and(|c| c.contains(".env-gas")),
        "served theme carries the ENV block"
    );
    assert_eq!(
        h.get("cache-control").and_then(|c| c.to_str().ok()),
        Some("no-cache"),
        "must revalidate every load"
    );
}

#[tokio::test]
async fn history_export_sheets_and_empty_guard() {
    // Detailed-page export: 400 while nothing is recorded, else a real xlsx
    // (zip magic) built from motor history + the sensor rings.
    let (state, dir) = common::test_state("api-export").await;
    let _g = PathBufGuard(dir);
    let r = digital_twin_dashboard::build_router(state.clone());
    let (s, _, _) = req(r.clone(), "GET", "/api/history/export", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "nothing recorded yet");

    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/motor/1",
        json!({"motor": 1, "speed": 128, "temp": 40.0, "temp_valid": true,
               "voltage": 12.0, "current": 0.5, "ina_ok": true,
               "vibration": 0, "vibration_valid": true}),
    )
    .await;
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/env/telemetry",
        json!({"baseline_ok": 1, "co": 1.2, "o2_pct": 20.9, "flame_dir": 0}),
    )
    .await;
    digital_twin_dashboard::mqtt::on_mqtt_message(
        &state.ctx(),
        "rover/imu",
        json!({"imu_ok": 1, "yaw": 3.0}),
    )
    .await;

    let (s, body, h) = req(r, "GET", "/api/history/export", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        h.get("content-type")
            .and_then(|c| c.to_str().ok())
            .is_some_and(|c| c.contains("spreadsheetml")),
        "xlsx content type"
    );
    assert!(
        h.get("content-disposition")
            .and_then(|c| c.to_str().ok())
            .is_some_and(|c| c.contains("attachment")),
        "downloads as an attachment"
    );
    let bytes = body.as_str().unwrap_or_default();
    assert!(bytes.starts_with("PK"), "xlsx is a real zip container");
}
