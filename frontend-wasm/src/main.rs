//! Rover Rust frontend (Leptos CSR) — Phase B rewrite of the vanilla `web/`
//! dashboard. Vanilla stays default at `/`; this build serves at `/next/`
//! (home), `/next/detailed`, `/next/test`. Calibration + setup stay vanilla.

mod calibration_page;
mod home_page;
mod shell;
mod test_page;
mod ws;

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::shell::app_shell;
use ws::{fetch_device_online, motor_param, ws_url, MotorSample};

const MAX_POINTS: usize = 60;
const FRESH_LIVE_MS: f64 = 5000.0;
const MOTOR_NAMES: [&str; 4] = ["FL Motor", "FR Motor", "RL Motor", "RR Motor"];
const MOTOR_SHORT: [&str; 4] = ["FL", "FR", "RL", "RR"];

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
/// except `redraw` (`Rc`, cloned per reconnect).
#[derive(Clone)]
struct WsCtx {
    samples: RwSignal<[MotorSample; 4]>,
    last_rx: RwSignal<[Option<f64>; 4]>,
    has_data: RwSignal<[bool; 4]>,
    hist: RwSignal<Vec<Vec<Vec<f64>>>>,
    device_online: RwSignal<bool>,
    redraw: std::rc::Rc<dyn Fn()>,
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

    let temp_ref = NodeRef::<leptos::html::Canvas>::new();
    let volt_ref = NodeRef::<leptos::html::Canvas>::new();
    let curr_ref = NodeRef::<leptos::html::Canvas>::new();
    let health_ref = NodeRef::<leptos::html::Canvas>::new();
    let vib_ref = NodeRef::<leptos::html::Canvas>::new();

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

    // ---- WS connect with 3s reconnect (vanilla behavior) ----
    // Plain setup statements (run once on mount, CSR only): no Effect guard
    // needed, and the forgotten JS closures + intervals are 'static.
    connect_ws(WsCtx {
        samples,
        last_rx,
        has_data,
        hist,
        device_online,
        redraw: redraw_rc.clone(),
    });

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
        let _iv2 = gloo_timers::callback::Interval::new(1000, move || {
            tick.update(|t| *t += 1);
            rt();
        });
        _iv2.forget();
        wasm_bindgen_futures::spawn_local(async move {
            device_online.set(fetch_device_online().await);
        });
    }

    // ---- window resize -> resize + rerender ----
    {
        let r = redraw_rc.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_: web_sys::Event| {
            r();
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
                            <div class="live-badge">
                                <span class={move || if device_online.get() { "status-dot dot-green pulse" } else { "status-dot dot-red" }} id="detail-live-dot"></span>
                                <span id="detail-live-label">{move || if device_online.get() { "Online" } else { "Offline" }}</span>
                            </div>
                        </div>
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
                    </div>
                </main>
            </div>
        })}
    }
}

#[component]
fn App() -> impl IntoView {
    // Manual pathname routing (leptos_router deferred): /next/test -> Motor
    // Test, /next/detailed -> Detailed, everything else (/next/) -> Home.
    // Plain <a> links keep deep-links and back-button behavior.
    // Page-level once-per-load effects: .no-vt fallback flag + header
    // scroll shading (both need only `document`, no mount wait).
    // Page-level once-per-load effects (deferred past mount): `.no-vt`
    // fallback flag + header scroll shading, both verbatim from vanilla.
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
    if path.starts_with("/next/test") {
        view! { <test_page::TestPage/> }.into_any()
    } else if path.starts_with("/next/detailed") {
        view! { <DetailedPage/> }.into_any()
    } else if path.starts_with("/next/calibration") {
        view! { <calibration_page::CalibrationPage/> }.into_any()
    } else {
        view! { <home_page::HomePage/> }.into_any()
    }
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App/> });
}
