//! Rover Rust frontend (Leptos CSR) — Phase B rewrite of the vanilla `web/`
//! dashboard. Vanilla stays default at `/`; this build serves at `/next/`
//! (home), `/next/detailed`, `/next/test`. Calibration + setup stay vanilla.

mod calibration_page;
mod env_ui;
mod home_page;
mod shell;
mod test_page;
mod ws;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::shell::{app_shell, route_path};
use ws::{fetch_device_online, get_json, motor_param, now_ms, ws_url, MotorSample};

use serde_json::Value;

const MAX_POINTS: usize = 60;
const FRESH_LIVE_MS: f64 = 5000.0;
const MOTOR_NAMES: [&str; 4] = ["FL Motor", "FR Motor", "RL Motor", "RR Motor"];
const MOTOR_SHORT: [&str; 4] = ["FL", "FR", "RL", "RR"];

/// Sensor chart series: (legend name, frame key, line color).
const ENV_GAS_SERIES: [(&str, &str, &str); 5] = [
    ("CO", "co", "#f97316"),
    ("LPG", "lpg", "#eab308"),
    ("CH4", "ch4", "#22c55e"),
    ("H2", "h2", "#38bdf8"),
    ("Air", "air", "#a1a1aa"),
];
const ENV_HEAT_SERIES: [(&str, &str, &str); 2] =
    [("L", "heat_l", "#f97316"), ("R", "heat_r", "#38bdf8")];
/// Chart range buttons: label + minutes.
const ENV_RANGES: [(&str, i64); 3] = [("1 min", 1), ("10 min", 10), ("1 hour", 60)];
/// Largest client-side ring kept: the 1 h window + slack. Frames past it
/// append live on top of the fetched sketch; the backend ring is the long
/// memory and the exporter reads that one.
const ENV_RING_MIN: i64 = 60;

/// Pinned chart ranges (verbatim from vanilla): [min, max] per metric.
/// Vibration rests at ~1.0g, so its window centers there.
fn chart_range(metric: usize) -> (f64, f64) {
    match metric {
        0 => (20.0, 80.0), // temp °C
        1 => (10.0, 14.0), // voltage V
        2 => (0.0, 3.0),   // current A
        3 => (0.0, 100.0), // health %
        _ => (0.5, 2.0),   // vibration g
    }
}

fn chart_color(metric: usize) -> &'static str {
    match metric {
        0 => "#f97316",
        1 => "#22c55e",
        2 => "#3b82f6",
        3 => "#a855f7",
        _ => "#ef4444",
    }
}

fn is_real(v: Option<f64>) -> bool {
    matches!(v, Some(x) if x.is_finite())
}

fn stale_of(last_rx: Option<f64>, now: f64) -> bool {
    match last_rx {
        None => true,
        Some(t) => now - t > FRESH_LIVE_MS,
    }
}

/// (display value, badge text, dot class) for one card.
fn card_state(
    metric: usize,
    s: &MotorSample,
    stale: bool,
    has_data: bool,
) -> (String, String, &'static str) {
    if stale || !has_data {
        let badge = match metric {
            0 => "no probe",
            1 | 2 => "no chip",
            _ => "no data",
        };
        return ("--".to_string(), badge.to_string(), "dot-red");
    }
    match metric {
        0 => {
            if s.temp_valid && is_real(s.temp) {
                (
                    format!("{:.1}", s.temp.unwrap()),
                    "normal".into(),
                    "dot-green",
                )
            } else {
                ("--".into(), "no probe".into(), "dot-red")
            }
        }
        1 => {
            if s.ina_ok && is_real(s.voltage) {
                let v = s.voltage.unwrap();
                let badge = if v >= 12.0 { "stable" } else { "low" };
                let dot = if v >= 12.0 { "dot-green" } else { "dot-yellow" };
                (format!("{v:.2}"), badge.into(), dot)
            } else {
                ("--".into(), "no chip".into(), "dot-red")
            }
        }
        2 => {
            if s.ina_ok && is_real(s.current) {
                let c = s.current.unwrap();
                let badge = if c < 1.5 { "nominal" } else { "high" };
                let dot = if c < 1.5 { "dot-green" } else { "dot-yellow" };
                (format!("{c:.3}"), badge.into(), dot)
            } else {
                ("--".into(), "no chip".into(), "dot-red")
            }
        }
        4 => {
            if s.vib_valid && is_real(s.vib_mag) {
                let g = s.vib_mag.unwrap();
                let dev = (g - 1.0).abs();
                let (badge, dot) = if dev < 0.25 {
                    ("calm", "dot-green")
                } else if dev < 0.5 {
                    ("elevated", "dot-yellow")
                } else {
                    ("shaking", "dot-red")
                };
                (format!("{g:.2}"), badge.into(), dot)
            } else {
                ("--".into(), "no signal".into(), "dot-red")
            }
        }
        _ => {
            if is_real(s.health) {
                let h = s.health.unwrap();
                let (badge, dot) = if h > 70.0 {
                    ("good", "dot-green")
                } else if h > 40.0 {
                    ("fair", "dot-yellow")
                } else {
                    ("critical", "dot-red")
                };
                (format!("{}", h.round() as i64), badge.into(), dot)
            } else {
                ("--".into(), "no data".into(), "dot-red")
            }
        }
    }
}

fn draw_chart(
    canvas: &HtmlCanvasElement,
    data: &[f64],
    color: &str,
    unit: &str,
    range: (f64, f64),
) {
    let w = canvas.width() as f64;
    let h = canvas.height() as f64;
    let Some(ctx) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    ctx.clear_rect(0.0, 0.0, w, h);
    if data.len() < 2 {
        ctx.set_fill_style_str("#52525b");
        ctx.set_font("12px Inter, sans-serif");
        let _ = ctx.fill_text("Waiting for live data...", 12.0, h / 2.0);
        return;
    }
    let (mut lo, mut hi) = range;
    let span = (hi - lo).max(1e-6);
    lo -= span * 0.15;
    hi += span * 0.15;
    let x = |i: usize| (i as f64) / ((data.len() - 1).max(1) as f64) * (w - 16.0) + 8.0;
    let y = |v: f64| h - 14.0 - ((v - lo) / (hi - lo).max(1e-6)) * (h - 28.0);
    // gridlines
    ctx.set_stroke_style_str("rgba(255,255,255,0.04)");
    ctx.set_line_width(1.0);
    for g in 0..5 {
        let gy = 8.0 + (g as f64) * (h - 22.0) / 4.0;
        ctx.begin_path();
        ctx.move_to(0.0, gy);
        ctx.line_to(w, gy);
        ctx.stroke();
    }
    // line + glow
    ctx.set_stroke_style_str(color);
    ctx.set_line_width(2.0);
    ctx.set_shadow_color(color);
    ctx.set_shadow_blur(6.0);
    ctx.begin_path();
    for (i, v) in data.iter().enumerate() {
        if i == 0 {
            ctx.move_to(x(i), y(*v));
        } else {
            ctx.line_to(x(i), y(*v));
        }
    }
    ctx.stroke();
    ctx.set_shadow_blur(0.0);
    // latest value top-right
    if let Some(last) = data.last() {
        ctx.set_fill_style_str("#e4e4e7");
        ctx.set_font("600 12px Inter, sans-serif");
        let _ = ctx.fill_text(&format!("{last:.1}{unit}"), w - 64.0, 16.0);
    }
}

/// Multi-line sensor chart for the Detailed ENV sections: one colored
/// polyline per series, shared y-window (`fixed` or auto around the data),
/// the same grid/axis look as the motor charts. Nulls break the line instead
/// of drawing a fake 0; a short window shows the waiting text.
/// A hole wider than this in the ENV feed breaks the polyline instead of
/// chording across it (feed stoppage, restart, node reboot).
const GAP_BREAK_MS: f64 = 5000.0;

fn draw_sensor_chart(
    canvas: &HtmlCanvasElement,
    frames: &[serde_json::Value],
    series: &[(&str, &str, &str)],
    unit: &str,
    fixed: Option<(f64, f64)>,
    span_ms: f64,
) {
    let w = canvas.width() as f64;
    let h = canvas.height() as f64;
    let Some(ctx) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    ctx.clear_rect(0.0, 0.0, w, h);
    let data: Vec<Vec<Option<f64>>> = series
        .iter()
        .map(|(_, key, _)| frames.iter().map(|f| env_ui::num(f, key)).collect())
        .collect();
    let present = data
        .iter()
        .map(|s| s.iter().filter(|v| v.is_some()).count())
        .sum::<usize>();
    if frames.len() < 2 || present < 2 {
        ctx.set_fill_style_str("#52525b");
        ctx.set_font("12px Inter, sans-serif");
        let _ = ctx.fill_text("Waiting for live data...", 12.0, h / 2.0);
        return;
    }
    let (mut lo, mut hi) = match fixed {
        Some(r) => r,
        None => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for v in data.iter().flatten().flatten() {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            if (hi - lo).abs() < 1e-9 {
                (lo - 0.5, hi + 0.5)
            } else {
                let pad = (hi - lo) * 0.15;
                (lo - pad, hi + pad)
            }
        }
    };
    let span = (hi - lo).max(1e-6);
    lo -= span * 0.05;
    hi += span * 0.05;
    // Time axis: place points by their `at` stamp over the selected window.
    // The old index spacing drew a 90 s feed hole as wide as a 1 s step and
    // stretched thinned history against fresh frames, so range switches
    // looked like a different (wrong) waveform. Falls back to index spacing
    // when fewer than two stamps are present.
    let ts: Vec<Option<f64>> = frames
        .iter()
        .map(|f| f.get("at").and_then(|v| v.as_f64()))
        .collect();
    let time_mode = ts.iter().flatten().count() >= 2;
    let t_end = ts.iter().flatten().last().copied().unwrap_or(0.0);
    let t_span = span_ms.max(1000.0);
    let x = |i: usize| {
        if time_mode {
            8.0 + ((ts[i].unwrap_or(t_end) - (t_end - t_span)) / t_span) * (w - 16.0)
        } else {
            (i as f64) / ((frames.len() - 1).max(1) as f64) * (w - 16.0) + 8.0
        }
    };
    let y = |v: f64| h - 14.0 - ((v - lo) / (hi - lo).max(1e-6)) * (h - 28.0);
    // gridlines
    ctx.set_stroke_style_str("rgba(255,255,255,0.04)");
    ctx.set_line_width(1.0);
    for g in 0..5 {
        let gy = 8.0 + (g as f64) * (h - 22.0) / 4.0;
        ctx.begin_path();
        ctx.move_to(0.0, gy);
        ctx.line_to(w, gy);
        ctx.stroke();
    }
    // one polyline per series, nulls break the line
    for (s, vals) in data.iter().enumerate() {
        let color = series.get(s).map(|d| d.2).unwrap_or("#a1a1aa");
        ctx.set_stroke_style_str(color);
        ctx.set_line_width(2.0);
        ctx.set_shadow_color(color);
        ctx.set_shadow_blur(6.0);
        ctx.begin_path();
        let mut started = false;
        let mut prev_at: Option<f64> = None;
        for (i, v) in vals.iter().enumerate() {
            match v {
                Some(val) => {
                    // A feed hole stays a hole; never chord across it.
                    if time_mode
                        && prev_at
                            .zip(ts[i])
                            .is_some_and(|(p, c)| c - p > GAP_BREAK_MS)
                    {
                        started = false;
                    }
                    if started {
                        ctx.line_to(x(i), y(*val));
                    } else {
                        ctx.move_to(x(i), y(*val));
                        started = true;
                    }
                    prev_at = ts[i];
                }
                None => started = false,
            }
        }
        ctx.stroke();
        ctx.set_shadow_blur(0.0);
    }
    // legend: swatch + name per series, top-left
    ctx.set_font("600 10px Inter, sans-serif");
    let mut lx = 10.0;
    for (name, _, color) in series {
        ctx.set_fill_style_str(color);
        let _ = ctx.fill_rect(lx, 9.0, 6.0, 6.0);
        ctx.set_fill_style_str("#a1a1aa");
        let _ = ctx.fill_text(name, lx + 10.0, 16.0);
        lx += 16.0 + name.chars().count() as f64 * 6.5;
    }
    // latest of the first series, top-right
    if let Some(last) = data.first().and_then(|s| s.iter().rev().flatten().next()) {
        ctx.set_fill_style_str("#e4e4e7");
        ctx.set_font("600 12px Inter, sans-serif");
        let _ = ctx.fill_text(&format!("{last:.1}{unit}"), w - 64.0, 16.0);
    }
    // Time axis ends; without them a short window reads as stretched.
    if time_mode {
        ctx.set_fill_style_str("#52525b");
        ctx.set_font("10px Inter, sans-serif");
        let _ = ctx.fill_text(&format!("-{}", fmt_span(t_span)), 8.0, h - 4.0);
        let _ = ctx.fill_text("now", w - 32.0, h - 4.0);
    }
}

/// Compact span label for the chart time axis: 45s / 10m / 2h.
fn fmt_span(ms: f64) -> String {
    let s = ms / 1000.0;
    if s >= 3600.0 {
        format!("{:.0}h", s / 3600.0)
    } else if s >= 90.0 {
        format!("{:.0}m", s / 60.0)
    } else {
        format!("{:.0}s", s)
    }
}

fn size_canvas(canvas: &HtmlCanvasElement) {
    if let Some(parent) = canvas.parent_element() {
        let w = parent.client_width().max(50);
        // Backing store only; CSS class .chart-canvas sizes the element.
        // (Vanilla uses raw client width at 140px fixed height — kept 1:1.)
        canvas.set_width(w as u32);
        canvas.set_height(140);
    }
}

/// Clonable handle bundle for the WS loop. All fields are `Copy` signals
/// except the redraw callbacks (`Rc`, cloned per reconnect).
#[derive(Clone)]
struct WsCtx {
    samples: RwSignal<[MotorSample; 4]>,
    last_rx: RwSignal<[Option<f64>; 4]>,
    has_data: RwSignal<[bool; 4]>,
    hist: RwSignal<Vec<Vec<Vec<f64>>>>,
    device_online: RwSignal<bool>,
    /// ENV/IMU feeds: latest frames, the ENV chart ring (flat `env_update`
    /// payloads, oldest first) and per-feed freshness stamps. `env_status`
    /// carries the node's state + burn-in.
    env_msg: RwSignal<Value>,
    env_status: RwSignal<Value>,
    env_last_rx: RwSignal<Option<f64>>,
    env_ring: RwSignal<Vec<Value>>,
    imu_msg: RwSignal<Value>,
    imu_last_rx: RwSignal<Option<f64>>,
    redraw: std::rc::Rc<dyn Fn()>,
    redraw_env: std::rc::Rc<dyn Fn()>,
}

/// WS connect with 3s reconnect (vanilla behavior). Recurses through a free
/// function — no self-referential `Rc`, so reconnect always typechecks.
fn connect_ws(ctx: WsCtx) {
    let url = ws_url();
    let Ok(socket) = web_sys::WebSocket::new(&url) else {
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(3000).await;
            connect_ws(ctx);
        });
        return;
    };
    let onopen = Closure::<dyn FnMut(_)>::new({
        let s = socket.clone();
        move |_: web_sys::Event| {
            let _ = s.send_with_str("ping");
        }
    });
    socket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let onmessage = Closure::<dyn FnMut(_)>::new({
        let ctx = ctx.clone();
        move |e: web_sys::MessageEvent| {
            let Some(text) = e.data().as_string() else {
                return;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                return;
            };
            let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            match t {
                "motor_update" => {
                    let m = v.get("motor").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                    if !(1..=4).contains(&m) {
                        return;
                    }
                    let idx = m - 1;
                    let sample = MotorSample {
                        temp: v.get("temp").and_then(|x| x.as_f64()),
                        voltage: v.get("voltage").and_then(|x| x.as_f64()),
                        current: v.get("current").and_then(|x| x.as_f64()),
                        health: v.get("health").and_then(|x| x.as_f64()),
                        temp_valid: v
                            .get("temp_valid")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true),
                        ina_ok: v.get("ina_ok").and_then(|x| x.as_bool()).unwrap_or(true),
                        vib_mag: v.get("vib_mag").and_then(|x| x.as_f64()),
                        vib_valid: v
                            .get("vibration_valid")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true),
                    };
                    ctx.samples.update(|arr| arr[idx] = sample);
                    ctx.last_rx
                        .update(|arr| arr[idx] = Some(js_sys::Date::now()));
                    ctx.has_data.update(|arr| arr[idx] = true);
                    ctx.hist.update(|h| {
                        let mut push = |metric: usize, val: Option<f64>, valid: bool| {
                            if valid {
                                if let Some(x) = val {
                                    if x.is_finite() {
                                        h[metric][idx].push(x);
                                        if h[metric][idx].len() > MAX_POINTS {
                                            let excess = h[metric][idx].len() - MAX_POINTS;
                                            h[metric][idx].drain(..excess);
                                        }
                                    }
                                }
                            }
                        };
                        push(0, sample.temp, sample.temp_valid);
                        push(1, sample.voltage, sample.ina_ok);
                        push(2, sample.current, sample.ina_ok);
                        push(3, sample.health, true);
                        push(4, sample.vib_mag, sample.vib_valid);
                    });
                    (ctx.redraw)();
                }
                "device_status" => {
                    if let Some(online) = v.get("online").and_then(|x| x.as_bool()) {
                        ctx.device_online.set(online);
                    }
                }
                "snapshot" | "pong" | "calibration_status" | "motor_status" => {}
                "env_update" => {
                    ctx.env_msg.set(v.clone());
                    ctx.env_last_rx.set(Some(now_ms()));
                    let cap = ((ENV_RING_MIN + 5) * 60) as usize;
                    ctx.env_ring.update(|r| {
                        // Stamp the client wall clock (epoch ms, same unit as
                        // the backend ring rows) so seeded history and live
                        // frames share one time axis. Do not use now_ms()
                        // here: it is performance.now(), monotonic since page
                        // load, and would send every live point off-window.
                        let mut item = v.clone();
                        if let Some(o) = item.as_object_mut() {
                            o.insert("at".to_string(), js_sys::Date::now().into());
                        }
                        r.push(item);
                        if r.len() > cap {
                            let over = r.len() - cap;
                            r.drain(..over);
                        }
                    });
                    (ctx.redraw_env)();
                }
                "imu_update" => {
                    ctx.imu_msg.set(v.clone());
                    ctx.imu_last_rx.set(Some(now_ms()));
                }
                "env_status" => ctx.env_status.set(v.clone()),
                _ => {}
            }
        }
    });
    socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let onclose = Closure::<dyn FnMut(_)>::new({
        let ctx = ctx.clone();
        move |_: web_sys::CloseEvent| {
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(3000).await;
                connect_ws(ctx);
            });
        }
    });
    socket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
    let onerror = Closure::<dyn FnMut(_)>::new(move |_: web_sys::Event| {});
    socket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();
}

#[component]
fn DetailedPage() -> impl IntoView {
    let initial_motor = motor_param(1);
    let current_motor = RwSignal::new(initial_motor);
    let device_online = RwSignal::new(false);
    let tick = RwSignal::new(0u64);
    // latest sample + last-rx wall time (ms) per motor 1-4.
    let samples = RwSignal::new([
        MotorSample::default(),
        MotorSample::default(),
        MotorSample::default(),
        MotorSample::default(),
    ]);
    let last_rx = RwSignal::new([None, None, None, None]);
    let has_data = RwSignal::new([false, false, false, false]);
    // per-metric history per motor: [metric][motor].
    let hist: RwSignal<Vec<Vec<Vec<f64>>>> =
        RwSignal::new(vec![
            vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()];
            5
        ]);
    // ENV/IMU feeds: latest frames, the ENV chart ring (flat `env_update`
    // payloads, oldest first), per-feed freshness and the chart range.
    let env_msg = RwSignal::new(Value::Null);
    let env_status = RwSignal::new(Value::Null);
    let env_last_rx = RwSignal::new(None::<f64>);
    let imu_msg = RwSignal::new(Value::Null);
    let imu_last_rx = RwSignal::new(None::<f64>);
    let env_ring: RwSignal<Vec<Value>> = RwSignal::new(Vec::new());
    let env_range = RwSignal::new(1i64);

    let temp_ref = NodeRef::<leptos::html::Canvas>::new();
    let volt_ref = NodeRef::<leptos::html::Canvas>::new();
    let curr_ref = NodeRef::<leptos::html::Canvas>::new();
    let health_ref = NodeRef::<leptos::html::Canvas>::new();
    let vib_ref = NodeRef::<leptos::html::Canvas>::new();
    let gas_ref = NodeRef::<leptos::html::Canvas>::new();
    let o2_ref = NodeRef::<leptos::html::Canvas>::new();
    let flame_ref = NodeRef::<leptos::html::Canvas>::new();
    let heat_ref = NodeRef::<leptos::html::Canvas>::new();

    let redraw = move || {
        let m = (current_motor.get() as usize).saturating_sub(1).min(3);
        let refs = [temp_ref, volt_ref, curr_ref, health_ref, vib_ref];
        let h = hist.get();
        for (metric, r) in refs.iter().enumerate() {
            if let Some(c) = r.get() {
                size_canvas(&c);
                let data = h
                    .get(metric)
                    .and_then(|v| v.get(m))
                    .cloned()
                    .unwrap_or_default();
                let now = js_sys::Date::now();
                let rx = last_rx.get();
                let stale = stale_of(rx[m], now);
                let units = ["°C", "V", "A", "%", "g"];
                if stale {
                    draw_chart(
                        &c,
                        &[],
                        chart_color(metric),
                        units[metric],
                        chart_range(metric),
                    );
                } else {
                    draw_chart(
                        &c,
                        &data,
                        chart_color(metric),
                        units[metric],
                        chart_range(metric),
                    );
                }
            }
        }
    };
    let redraw_rc = std::rc::Rc::new(redraw);
    let redraw_tab = redraw_rc.clone();

    let redraw_env = move || {
        let full = env_ring.get();
        let range_ms = env_range.get().max(1) as f64 * 60_000.0;
        let keep = (env_range.get().max(1) as usize) * 60 + 30;
        let mut tail: Vec<Value> = full
            .iter()
            .skip(full.len().saturating_sub(keep))
            .cloned()
            .collect();
        // Anchor the window to the newest sample and drop the slice overhang
        // so the time axis always shows exactly the selected range.
        if let Some(t_end) = tail
            .last()
            .and_then(|f| f.get("at").and_then(|v| v.as_f64()))
        {
            tail.retain(|f| {
                f.get("at")
                    .and_then(|v| v.as_f64())
                    .is_none_or(|t| t >= t_end - range_ms)
            });
        }
        let draw = |r: NodeRef<leptos::html::Canvas>,
                    series: &[(&str, &str, &str)],
                    unit: &str,
                    fixed: Option<(f64, f64)>| {
            if let Some(c) = r.get() {
                size_canvas(&c);
                if tail.len() < 2 {
                    draw_sensor_chart(&c, &[], &[], "", None, range_ms);
                } else {
                    draw_sensor_chart(&c, &tail, series, unit, fixed, range_ms);
                }
            }
        };
        draw(gas_ref, &ENV_GAS_SERIES, "", None);
        draw(
            o2_ref,
            &[("O2", "o2_pct", "#22c55e")],
            "%",
            Some((19.0, 22.0)),
        );
        draw(
            flame_ref,
            &[("Flame", "flame_lvl", "#ef4444")],
            "V",
            Some((0.0, 5.0)),
        );
        draw(heat_ref, &ENV_HEAT_SERIES, "°C", None);
    };
    let redraw_env_rc = std::rc::Rc::new(redraw_env);
    // Fetch race guard: range switches (and the initial seed) each take a
    // ticket; a response that lands after a newer request is dropped.
    let fetch_seq = std::rc::Rc::new(std::cell::Cell::new(0u64));

    // ---- WS connect with 3s reconnect (vanilla behavior) ----
    // Plain setup statements (run once on mount, CSR only): no Effect guard
    // needed, and the forgotten JS closures + intervals are 'static.
    connect_ws(WsCtx {
        samples,
        last_rx,
        has_data,
        hist,
        device_online,
        env_msg,
        env_status,
        env_last_rx,
        env_ring,
        imu_msg,
        imu_last_rx,
        redraw: redraw_rc.clone(),
        redraw_env: redraw_env_rc.clone(),
    });

    // Seed the ENV chart ring from the backend roll-up (1 min window), then
    // live frames append; the range buttons re-fetch the wider windows.
    {
        let rr = redraw_env_rc.clone();
        let seq = fetch_seq.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mine = seq.get() + 1;
            seq.set(mine);
            if let Some(v) = get_json("/api/env/history?minutes=1").await {
                if seq.get() != mine {
                    return; // a range click asked for different data
                }
                if let Some(samples) = v.get("samples").and_then(|s| s.as_array()) {
                    env_ring.set(samples.clone());
                }
            }
            rr();
        });
    }

    // ---- polls: device 3s, clock+freshness 1s ----
    {
        let dev = device_online;
        let _iv = gloo_timers::callback::Interval::new(3000, move || {
            wasm_bindgen_futures::spawn_local(async move {
                dev.set(fetch_device_online().await);
            });
        });
        _iv.forget();
        let rt = redraw_rc.clone();
        let rt_env = redraw_env_rc.clone();
        let _iv2 = gloo_timers::callback::Interval::new(1000, move || {
            tick.update(|t| *t += 1);
            rt();
            rt_env();
        });
        _iv2.forget();
        wasm_bindgen_futures::spawn_local(async move {
            device_online.set(fetch_device_online().await);
        });
    }

    // ---- window resize -> resize + rerender ----
    {
        let r = redraw_rc.clone();
        let r_env = redraw_env_rc.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_: web_sys::Event| {
            r();
            r_env();
        });
        if let Some(w) = web_sys::window() {
            let _ = w.add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref());
        }
        closure.forget();
    }

    view! {
        {app_shell(view! {
            <div class="body-wrap">
                <main class="main-content">
                    <div class="content-wrap">
                        <div class="page-header">
                            <div><h1 class="page-h1">"Detailed View"</h1></div>
                            <div style="display:flex;align-items:center;gap:10px;">
                                <div class="live-badge">
                                    <span class={move || if device_online.get() { "status-dot dot-green pulse" } else { "status-dot dot-red" }} id="detail-live-dot"></span>
                                    <span id="detail-live-label">{move || if device_online.get() { "Online" } else { "Offline" }}</span>
                                </div>
                                <button class="export-btn" on:click=move |_| {
                                    if let Some(w) = web_sys::window() {
                                        let _ = w.location().set_href("/api/history/export");
                                    }
                                }>"Export Excel"</button>
                            </div>
                        </div>
                        <h2 class="detail-section-title">"Motor data"</h2>
                        <div class="detail-motor-tabs" id="motor-tabs">
                            {(1u8..=4u8).map(|m| {
                                let label = MOTOR_NAMES[(m - 1) as usize];
                                let rt = redraw_tab.clone();
                                view! {
                                    <button
                                        class={move || if current_motor.get() == m { "detail-motor-tab active" } else { "detail-motor-tab" }}
                                        data-motor={m.to_string()}
                                        on:click=move |_| { current_motor.set(m); rt(); }
                                    >{label}</button>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                        <div style="margin:-16px 0 24px;font-size:11px;color:var(--gray-500);font-weight:500;">
                            "Updated "
                            <span id="update-time">
                                {move || { let _ = tick.get(); js_sys::Date::new_0().to_locale_time_string("en-US").as_string().unwrap_or_default() }}
                            </span>
                            <span style="margin-left:12px;color:var(--gray-600);">"Rust UI · /next/ · vanilla at /detailed/"</span>
                            <span style="margin-left:8px;">{move || { let m = current_motor.get() as usize; format!("· {} #{}", MOTOR_SHORT[m-1], m) }}</span>
                        </div>
                        <div class="detail-values-grid" id="value-cards" style="margin-bottom:24px;">
                            {(0usize..5usize).map(|metric| {
                                let titles = ["Temperature (°C)", "Voltage (V)", "Current (A)", "Health (%)", "Vibration (g)"];
                                view! {
                                    <div class="value-card">
                                        <div class="value-label">{titles[metric]}</div>
                                        <div class="value-number">
                                            {move || {
                                                let _ = tick.get();
                                                let m = (current_motor.get() as usize).saturating_sub(1).min(3);
                                                let s = samples.get()[m];
                                                let rx = last_rx.get()[m];
                                                let hd = has_data.get()[m];
                                                let stale = stale_of(rx, js_sys::Date::now());
                                                card_state(metric, &s, stale, hd).0
                                            }}
                                        </div>
                                        <div class="value-badge">
                                            <span class={move || {
                                                let _ = tick.get();
                                                let m = (current_motor.get() as usize).saturating_sub(1).min(3);
                                                let s = samples.get()[m];
                                                let rx = last_rx.get()[m];
                                                let hd = has_data.get()[m];
                                                let stale = stale_of(rx, js_sys::Date::now());
                                                format!("status-dot {}", card_state(metric, &s, stale, hd).2)
                                            }}></span>
                                            {move || {
                                                let _ = tick.get();
                                                let m = (current_motor.get() as usize).saturating_sub(1).min(3);
                                                let s = samples.get()[m];
                                                let rx = last_rx.get()[m];
                                                let hd = has_data.get()[m];
                                                let stale = stale_of(rx, js_sys::Date::now());
                                                card_state(metric, &s, stale, hd).1
                                            }}
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                        <div class="detail-chart-grid" id="chart-grid">
                            <div class="chart-card"><div class="chart-title">"Temperature "</div><canvas class="chart-canvas" id="chart-temp" node_ref=temp_ref></canvas></div>
                            <div class="chart-card"><div class="chart-title">"Voltage "</div><canvas class="chart-canvas" id="chart-voltage" node_ref=volt_ref></canvas></div>
                            <div class="chart-card"><div class="chart-title">"Current "</div><canvas class="chart-canvas" id="chart-current" node_ref=curr_ref></canvas></div>
                            <div class="chart-card"><div class="chart-title">"Health "</div><canvas class="chart-canvas" id="chart-health" node_ref=health_ref></canvas></div>
                            <div class="chart-card"><div class="chart-title">"Vibration "</div><canvas class="chart-canvas" id="chart-vib" node_ref=vib_ref></canvas></div>
                        </div>

                        <h2 class="detail-section-title">"Gas sensor"</h2>
                        <div class="sensor-charts">
                            <div class="panel panel-pad" class:stale=move || { let _ = tick.get(); env_ui::stale_at(env_last_rx.get(), now_ms()) }>
                                {(0..env_ui::GAS.len()).map(|i| {
                                    let (key, label) = env_ui::GAS[i];
                                    view! {
                                        <div class="env-gas">
                                            <span class="env-gas-name">{label}</span>
                                            <div class="env-track">
                                                <i class="base"></i>
                                                <i class="fill" style=move || env_ui::gas_fill_style(&env_msg.get(), key)></i>
                                            </div>
                                            <span class="env-gas-val" style=move || format!("color:{}", env_ui::gas_color(&env_msg.get(), key))>
                                                {move || env_ui::gas_text(&env_msg.get(), key)}
                                            </span>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                                <div class="env-note">"Relative MQ cell response (R0/Rs), 1.00 = clean air. Colors follow the node's own lines: 2.0 warn, 3.5 high."</div>
                            </div>
                            <div class="chart-card"><div class="chart-title">"Gas response (R0/Rs) "</div><canvas class="chart-canvas" id="chart-gas" node_ref=gas_ref></canvas></div>
                        </div>

                        <h2 class="detail-section-title">"O2 sensor"</h2>
                        <div class="sensor-charts">
                            <div class="panel panel-pad" class:stale=move || { let _ = tick.get(); env_ui::stale_at(env_last_rx.get(), now_ms()) }>
                                <div class="sensor-big" style=move || {
                                    let f = env_msg.get();
                                    format!("color:{}", env_ui::guarded_color(&f, env_ui::num(&f, "o2_pct"), &["o2_low"]))
                                }>
                                    {move || env_ui::fmt(env_ui::num(&env_msg.get(), "o2_pct"), 2)}<small>"% O2"</small>
                                </div>
                                <div class="sensor-row">
                                    <span class="k">"Cell output"</span>
                                    <span class="v" style=move || format!("color:{}", env_ui::guarded_color(&env_msg.get(), env_ui::num(&env_msg.get(), "o2_mv"), &[]))>
                                        {move || format!("{} mV", env_ui::fmt(env_ui::num(&env_msg.get(), "o2_mv"), 2))}
                                    </span>
                                </div>
                                <div class="env-note">"Galvanic cell: clean air reads about 10 to 13 mV and 20.9 %. Warn below 20.0 %, alarm below 19.5 %."</div>
                            </div>
                            <div class="chart-card"><div class="chart-title">"O2 (%) "</div><canvas class="chart-canvas" id="chart-o2" node_ref=o2_ref></canvas></div>
                        </div>

                        <h2 class="detail-section-title">"Flame sensor"</h2>
                        <div class="sensor-charts">
                            <div class="panel panel-pad" class:stale=move || { let _ = tick.get(); env_ui::stale_at(env_last_rx.get(), now_ms()) }>
                                <div class="sensor-big" style=move || {
                                    let f = env_msg.get();
                                    format!("color:{}", env_ui::guarded_color(&f, env_ui::num(&f, "flame_lvl"), &["flame"]))
                                }>
                                    {move || env_ui::fmt(env_ui::num(&env_msg.get(), "flame_lvl"), 2)}<small>"V"</small>
                                </div>
                                <div class="sensor-row">
                                    <span class="k">"Direction"</span>
                                    <span class="v" style=move || {
                                        let f = env_msg.get();
                                        format!("color:{}", env_ui::guarded_color(&f, env_ui::num(&f, "flame_dir"), &["flame"]))
                                    }>
                                        {move || {
                                            let f = env_msg.get();
                                            match env_ui::num(&f, "flame_dir") {
                                                Some(d) if d > 0.0 => format!("FLAME on direction {}", d as i64),
                                                Some(_) => "no flame seen".to_string(),
                                                None => "--".to_string(),
                                            }
                                        }}
                                    </span>
                                </div>
                                <div class="sensor-row">
                                    <span class="k">"Array"</span>
                                    <span class="env-dirs">
                                        {(1u8..=5u8).map(|d| view! {
                                            <span class="dir" class:hit=move || env_ui::num(&env_msg.get(), "flame_dir").is_some_and(|v| v as u8 == d)></span>
                                        }).collect::<Vec<_>>()}
                                    </span>
                                </div>
                                <div class="sensor-row">
                                    <span class="k">"Module"</span>
                                    <span class="v" style=move || match env_ui::ok_flag(&env_msg.get(), "flame_ok") {
                                        Some(true) => "color:var(--dot-green)".to_string(),
                                        Some(false) => "color:var(--dot-red)".to_string(),
                                        None => "color:var(--gray-500)".to_string(),
                                    }>
                                        {move || match env_ui::ok_flag(&env_msg.get(), "flame_ok") {
                                            Some(true) => "ok".to_string(),
                                            Some(false) => "no answer".to_string(),
                                            None => "--".to_string(),
                                        }}
                                    </span>
                                </div>
                                <div class="env-note">"Level is the module's analog output. A lit dot is the direction whose line fired (1 = GPIO 34, 2 = 35, 3 = 36, 4 = 39, 5 = 32)."</div>
                            </div>
                            <div class="chart-card"><div class="chart-title">"Flame level (V) "</div><canvas class="chart-canvas" id="chart-flame" node_ref=flame_ref></canvas></div>
                        </div>

                        <h2 class="detail-section-title">"Heat sensor (MLX pair)"</h2>
                        <div class="sensor-charts">
                            <div class="panel panel-pad" class:stale=move || { let _ = tick.get(); env_ui::stale_at(env_last_rx.get(), now_ms()) }>
                                <div class="sensor-big" style=move || {
                                    let f = env_msg.get();
                                    format!("color:{}", env_ui::guarded_color(&f, env_ui::num(&f, "heat_l"), &["heat_delta"]))
                                }>
                                    {move || format!("{} / {}", env_ui::fmt(env_ui::num(&env_msg.get(), "heat_l"), 1), env_ui::fmt(env_ui::num(&env_msg.get(), "heat_r"), 1))}<small>"°C L / R object"</small>
                                </div>
                                <div class="sensor-row">
                                    <span class="k">"Ambient L / R"</span>
                                    <span class="v">{move || format!("{} / {} °C", env_ui::fmt(env_ui::num(&env_msg.get(), "heat_l_amb"), 1), env_ui::fmt(env_ui::num(&env_msg.get(), "heat_r_amb"), 1))}</span>
                                </div>
                                <div class="sensor-row">
                                    <span class="k">"Difference L-R"</span>
                                    <span class="v">{move || {
                                        let f = env_msg.get();
                                        match (env_ui::num(&f, "heat_l"), env_ui::num(&f, "heat_r")) {
                                            (Some(l), Some(r)) => format!("{:.1} °C", l - r),
                                            _ => "--".to_string(),
                                        }
                                    }}</span>
                                </div>
                                <div class="env-note">"Warn when |L-R| >= 8 °C, alarm >= 15 °C. Aim both at the same wall to sanity-check the pair."</div>
                            </div>
                            <div class="chart-card"><div class="chart-title">"Heat L / R (°C) "</div><canvas class="chart-canvas" id="chart-heat" node_ref=heat_ref></canvas></div>
                        </div>

                        <div class="sensor-range">
                            <span class="lbl">"Chart range"</span>
                            {ENV_RANGES.iter().copied().map(|(label, mins)| {
                                let rr = redraw_env_rc.clone();
                                let rr2 = redraw_env_rc.clone();
                                let seq = fetch_seq.clone();
                                view! {
                                    <button
                                        class={move || if env_range.get() == mins { "range-btn active" } else { "range-btn" }}
                                        on:click=move |_| {
                                            env_range.set(mins);
                                            let rr = rr.clone();
                                            let seq = seq.clone();
                                            wasm_bindgen_futures::spawn_local(async move {
                                                let mine = seq.get() + 1;
                                                seq.set(mine);
                                                if let Some(v) = get_json(&format!("/api/env/history?minutes={mins}")).await {
                                                    if seq.get() != mine {
                                                        return; // a newer click already won
                                                    }
                                                    if let Some(samples) = v.get("samples").and_then(|s| s.as_array()) {
                                                        env_ring.set(samples.clone());
                                                    }
                                                }
                                                rr();
                                            });
                                            rr2();
                                        }
                                    >{label}</button>
                                }
                            }).collect::<Vec<_>>()}
                        </div>

                        <h2 class="detail-section-title">"Node"</h2>
                        <div class="panel panel-pad" class:stale=move || { let _ = tick.get(); env_ui::stale_at(env_last_rx.get(), now_ms()) }>
                            <div style="display:flex;flex-wrap:wrap;gap:8px;align-items:center;">
                                {[("ADS", "ads_ok"), ("MLX pair", "mlx_ok"), ("baseline", "baseline_ok")].iter().copied().map(|(label, key)| view! {
                                    <span class={move || match env_ui::ok_flag(&env_msg.get(), key) {
                                        Some(true) => "chip-sm ok",
                                        Some(false) => "chip-sm bad",
                                        None => "chip-sm",
                                    }}>
                                        {label}<b>{move || match env_ui::ok_flag(&env_msg.get(), key) {
                                            Some(true) => "ok".to_string(),
                                            Some(false) => "no".to_string(),
                                            None => "--".to_string(),
                                        }}</b>
                                    </span>
                                }).collect::<Vec<_>>()}
                                <span class="chip-sm">"burn-in "<b>{move || match env_ui::num(&env_status.get(), "burn_in_s") {
                                    Some(s) if s >= 3600.0 => format!("{:.1} h", s / 3600.0),
                                    Some(s) => format!("{} min", (s / 60.0).round() as i64),
                                    None => "--".to_string(),
                                }}</b></span>
                                <span class="chip-sm">"state "<b>{move || env_status.get().get("state").and_then(|v| v.as_str()).unwrap_or("--").to_string()}</b></span>
                            </div>
                            <div class="env-note">"Burn-in is the node's own powered time. MQ cells need 24 to 48 h before the baseline means anything. Green means the board answered, not that a gas is safe."</div>
                        </div>

                        <h2 class="detail-section-title">"IMU (mid board)"</h2>
                        <div class="panel panel-pad" class:stale=move || { let _ = tick.get(); env_ui::stale_at(imu_last_rx.get(), now_ms()) }>
                            <div class="sensor-row"><span class="k">"Yaw"</span><span class="v">{move || format!("{} deg", env_ui::fmt(env_ui::num(&imu_msg.get(), "yaw"), 1))}</span></div>
                            <div class="sensor-row"><span class="k">"Pitch / roll"</span><span class="v">{move || format!("{} / {} deg", env_ui::fmt(env_ui::num(&imu_msg.get(), "pitch"), 1), env_ui::fmt(env_ui::num(&imu_msg.get(), "roll"), 1))}</span></div>
                            <div class="sensor-row"><span class="k">"Accel x / y / z"</span><span class="v">{move || format!("{} / {} / {} g", env_ui::fmt(env_ui::num(&imu_msg.get(), "ax"), 2), env_ui::fmt(env_ui::num(&imu_msg.get(), "ay"), 2), env_ui::fmt(env_ui::num(&imu_msg.get(), "az"), 2))}</span></div>
                            <div class="sensor-row"><span class="k">"Turn rate gz"</span><span class="v">{move || format!("{} deg/s", env_ui::fmt(env_ui::num(&imu_msg.get(), "gz"), 1))}</span></div>
                            <div class="sensor-row"><span class="k">"Module"</span><span class="v">{move || match env_ui::ok_flag(&imu_msg.get(), "imu_ok") {
                                Some(true) => "ok".to_string(),
                                Some(false) => "no answer".to_string(),
                                None => "--".to_string(),
                            }}</span></div>
                            <div class="env-note">"Localization feed, 4 Hz; recorded with the ENV log into the Excel export (IMU sheet). The map view lands with the Pi work."</div>
                        </div>
                    </div>
                </main>
            </div>
        })}
    }
}

#[component]
fn App() -> impl IntoView {
    // Manual pathname routing (leptos_router deferred): /test -> Motor Test,
    // /detailed -> Detailed, /calibration -> Calibration, everything else ->
    // Home. The server serves this same index.html at the root routes AND
    // under /next/*, so match on the alias-normalized path (route_path).
    // Plain <a> links keep deep-links and back-button behavior.
    // Page-level once-per-load effects: `.no-vt` fallback flag + header
    // scroll shading, both verbatim from vanilla, no mount wait needed.
    wasm_bindgen_futures::spawn_local(async move {
        let Some(win) = web_sys::window() else { return };
        let Some(doc) = win.document() else { return };
        let has_vt = js_sys::Reflect::get(
            doc.unchecked_ref(),
            &JsValue::from_str("startViewTransition"),
        )
        .ok()
        .is_some_and(|v| !v.is_undefined());
        if !has_vt {
            if let Some(root) = doc.document_element() {
                root.class_list().add_1("no-vt").ok();
            }
        }
        if let Some(header) = doc.get_element_by_id("site-header") {
            let ticking = std::rc::Rc::new(std::cell::Cell::new(false));
            let (h, w) = (header.clone(), win.clone());
            let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                if ticking.get() {
                    return;
                }
                ticking.set(true);
                let header = h.clone();
                let ticking = ticking.clone();
                let w = w.clone();
                let wf = w.clone();
                let frame = Closure::<dyn FnMut()>::new(move || {
                    let y = wf.scroll_y().unwrap_or(0.0);
                    header
                        .class_list()
                        .toggle_with_force("scrolled", y > 10.0)
                        .ok();
                    ticking.set(false);
                });
                w.request_animation_frame(frame.as_ref().unchecked_ref())
                    .ok();
                frame.forget();
            });
            win.add_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref())
                .ok();
            cb.forget();
        }
    });
    let path = web_sys::window()
        .map(|w| w.location().pathname().unwrap_or_default())
        .unwrap_or_default();
    let view_path = route_path(&path);
    if view_path.starts_with("/test") {
        view! { <test_page::TestPage/> }.into_any()
    } else if view_path.starts_with("/detailed") {
        view! { <DetailedPage/> }.into_any()
    } else if view_path.starts_with("/calibration") {
        view! { <calibration_page::CalibrationPage/> }.into_any()
    } else {
        view! { <home_page::HomePage/> }.into_any()
    }
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App/> });
}
