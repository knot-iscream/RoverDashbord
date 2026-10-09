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

    // API responses plant nothing.
    let (r, _g) = app().await;
    let (_, _, h) = req(r, "GET", "/api/health", None, None).await;
    assert!(!h.contains_key("set-cookie"));
}
