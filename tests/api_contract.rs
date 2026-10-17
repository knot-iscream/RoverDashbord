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
    // Same state: the baseline round-trips through GET.
    let (_, v, _) = req(router, "GET", "/api/calibration", None, None).await;
    assert_eq!(v, json!({"baselines": {"2": {"avg_temp": 36.5}}}));
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
    for path in ["/test", "/detailed", "/calibration"] {
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
    ] {
        let (r, _g) = app().await;
        let (s, _, _) = req(r, "GET", path, None, None).await;
        assert_eq!(s, StatusCode::OK, "SPA route {path}");
    }

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
