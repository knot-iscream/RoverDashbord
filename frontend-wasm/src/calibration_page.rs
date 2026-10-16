//! Calibration page (`/next/calibration`) — 1:1 port of `web/calibration/`.
//!
//! Baseline collection (warmup/sweep driven by backend status), manual motor
//! rows in MANUAL-idle, motor cards, history timeline, export download.
//! Like vanilla, the dynamic regions are HTML strings repainted imperatively;
//! only the static chrome is declarative. Vanilla stays at `/calibration/`.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::Element;

use crate::shell::app_shell;
use crate::ws::{fetch_device_online, get_json, now_ms, post_json, post_motor, ws_url};

const MOTOR_NAMES: [&str; 4] = ["FL", "FR", "RL", "RR"];
const WEEKDAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
const FRESH_LIVE_MS: f64 = 5000.0;

const PLAY_CONTENT: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\"><path d=\"M8 5v14l11-7z\"/></svg> Start";
const PAUSE_CONTENT: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"currentColor\"><rect x=\"6\" y=\"4\" width=\"4\" height=\"16\"/><rect x=\"14\" y=\"4\" width=\"4\" height=\"16\"/></svg> Pause";

fn el(id: &str) -> Option<Element> {
    web_sys::window()?
        .document()?
        .get_element_by_id(id)
        .and_then(|e| e.dyn_into::<Element>().ok())
}

fn set_text(id: &str, text: &str) {
    if let Some(e) = el(id) {
        e.set_text_content(Some(text));
    }
}

fn set_display(id: &str, show: &str) {
    if let Some(e) = el(id) {
        e.set_attribute("style", &format!("display:{show};")).ok();
    }
}

fn toggle(el: &Element, cls: &str, on: bool) {
    el.class_list().toggle_with_force(cls, on).ok();
}

#[derive(Debug, Clone, Copy, Default)]
struct CalSample {
    temp: Option<f64>,
    voltage: Option<f64>,
    current: Option<f64>,
    vibration: bool,
    vib_mag: Option<f64>,
    health: Option<f64>,
    vib_valid: bool,
    ina_ok: bool,
    temp_valid: bool,
}

#[derive(Debug, Clone, Default)]
struct Seg {
    start_ts: f64,
    start_iso: String,
    motor: u8,
    direction: String,
    speed_pct: f64,
    duration_s: f64,
    samples: u64,
    avg_temp: f64,
    max_temp: f64,
    avg_voltage: f64,
    avg_current: f64,
    vibration_count: u64,
}

impl Seg {
    fn from_value(v: &serde_json::Value) -> Self {
        let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
        Seg {
            start_ts: num("start_ts"),
            start_iso: v
                .get("start_iso")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            motor: v.get("motor").and_then(|x| x.as_u64()).unwrap_or(0) as u8,
            direction: v
                .get("direction")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            speed_pct: num("speed_pct"),
            duration_s: num("duration_s"),
            samples: v.get("samples").and_then(|x| x.as_u64()).unwrap_or(1),
            avg_temp: num("avg_temp"),
            max_temp: num("max_temp"),
            avg_voltage: num("avg_voltage"),
            avg_current: num("avg_current"),
            vibration_count: v
                .get("vibration_count")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
        }
    }
}

#[derive(Default)]
struct CalSt {
    cal_state: String, // idle | warmup | sweep
    samples: i64,
    data: [Option<CalSample>; 4],
    rx: [Option<f64>; 4],
    mode_manual: bool,
    speeds: [i32; 4],
    hist_day: String, // YYYY-MM-DD (local)
    expanded: Vec<String>,
    render_pending: bool,
    deb_seq: [u64; 4],
    anim_token: [u64; 4],
    frame_id: [Option<i32>; 4],
    // The 10s history refresh lives here (not a process static): exactly one
    // timer per page instance, dropped with it — vanilla keeps one global.
    hist_timer: Option<gloo_timers::callback::Interval>,
}

type Shared = std::rc::Rc<std::cell::RefCell<CalSt>>;

fn pad2(n: u32) -> String {
    format!("{n:02}")
}

/// Local-time `YYYY-MM-DD` from a ms timestamp (vanilla `dateStr`, local).
fn date_str(ts_ms: f64) -> String {
    let d = js_sys::Date::new(&ts_ms.into());
    format!(
        "{}-{}-{}",
        d.get_full_year(),
        pad2(d.get_month() + 1),
        pad2(d.get_date())
    )
}

fn today_str() -> String {
    date_str(js_sys::Date::now())
}

fn ease_out_expo(t: f64) -> f64 {
    if t >= 1.0 {
        1.0
    } else {
        1.0 - 2f64.powf(-10.0 * t)
    }
}

/// Holder for a live rAF glide closure (self-referential across frames).
type FrameCell = std::rc::Rc<std::cell::RefCell<Option<Closure<dyn FnMut(f64)>>>>;

// ---------------------------------------------------------------------------
// Motor cards (verbatim templates, repainted like vanilla).
// ---------------------------------------------------------------------------

fn card_html_empty() -> String {
    let mut html = String::new();
    for name in MOTOR_NAMES {
        html += &format!(
            "<div class=\"cal-motor-card\">\
             <div class=\"cal-motor-header\">\
             <span class=\"cal-motor-name\">Motor {name}</span>\
             <span class=\"cal-motor-status idle\">Idle</span>\
             </div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Temperature</span><span class=\"cal-sensor-value\">-- \u{b0}C</span></div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Voltage</span><span class=\"cal-sensor-value\">-- V</span></div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Current</span><span class=\"cal-sensor-value\">-- A</span></div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Vibration</span><span class=\"cal-sensor-value\">--</span></div>\
             </div>"
        );
    }
    html
}

fn render_motor_cards(st: &Shared) {
    let Some(grid) = el("cal-motor-grid") else {
        return;
    };
    let s = st.borrow();
    let idle = s.cal_state.is_empty() || s.cal_state == "idle";
    if idle && s.data.iter().all(|d| d.is_none()) {
        grid.set_inner_html(&card_html_empty());
        return;
    }
    let now = now_ms();
    let mut html = String::new();
    for (i, name) in MOTOR_NAMES.iter().enumerate() {
        let d = s.data[i];
        let fresh = s.rx[i].is_some_and(|t| now - t < FRESH_LIVE_MS);
        let live = fresh && d.is_some();
        let (status_class, status_text) = if idle {
            ("idle", "Idle")
        } else if s.cal_state == "sweep" {
            ("collecting", "Sweeping")
        } else {
            ("warmup", "Warming")
        };
        let temp_str = match (live, d) {
            (true, Some(x)) if x.temp_valid && x.temp.is_some_and(|v| v.is_finite()) => {
                format!("{:.1} \u{b0}C", x.temp.unwrap())
            }
            _ => "-- \u{b0}C".to_string(),
        };
        let volt_str = match (live, d) {
            (true, Some(x)) if x.ina_ok && x.voltage.is_some_and(|v| v.is_finite()) => {
                format!("{:.2} V", x.voltage.unwrap())
            }
            _ => "-- V".to_string(),
        };
        let curr_str = match (live, d) {
            (true, Some(x)) if x.ina_ok && x.current.is_some_and(|v| v.is_finite()) => {
                format!("{:.3} A", x.current.unwrap())
            }
            _ => "-- A".to_string(),
        };
        let vib_str = match (live, d) {
            (true, Some(x)) if x.vib_valid => {
                match x.vib_mag {
                    Some(g) if g.is_finite() => format!("{g:.2}g"),
                    // Old firmware sent the bool only.
                    _ if x.vibration => "YES".to_string(),
                    _ => "no".to_string(),
                }
            }
            (true, _) => "NO SIGNAL".to_string(),
            _ => "--".to_string(),
        };
        let health_str = match (live, d) {
            (true, Some(x)) if x.health.is_some_and(|v| v.is_finite()) => {
                format!("{:.1}%", x.health.unwrap())
            }
            _ => "--".to_string(),
        };
        html += &format!(
            "<div class=\"cal-motor-card\">\
             <div class=\"cal-motor-header\">\
             <span class=\"cal-motor-name\">Motor {name}</span>\
             <span class=\"cal-motor-status {status_class}\">{status_text}</span>\
             </div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Temperature</span><span class=\"cal-sensor-value\">{temp_str}</span></div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Voltage</span><span class=\"cal-sensor-value\">{volt_str}</span></div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Current</span><span class=\"cal-sensor-value\">{curr_str}</span></div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Vibration</span><span class=\"cal-sensor-value\">{vib_str}</span></div>\
             <div class=\"cal-sensor-row\"><span class=\"cal-sensor-label\">Health</span><span class=\"cal-sensor-value\">{health_str}</span></div>"
        );
        if let (true, Some(x)) = (live, d) {
            if let Some(h) = x.health {
                if h.is_finite() {
                    let hpct = h.clamp(0.0, 100.0);
                    html += &format!(
                        "<div style=\"margin-top:8px;height:3px;border-radius:9999px;background:var(--bg-white-05);overflow:hidden;\">\
                         <div style=\"height:100%;width:{hpct}%;border-radius:9999px;background:var(--primary);transition:width 0.5s ease;\"></div></div>"
                    );
                }
            }
        }
        html += "</div>";
    }
    grid.set_inner_html(&html);
}

/// 500ms-coalesced card repaint (vanilla `queueMotorRender`).
fn queue_motor_render(st: &Shared) {
    if st.borrow().render_pending {
        return;
    }
    st.borrow_mut().render_pending = true;
    let st = st.clone();
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(500).await;
        st.borrow_mut().render_pending = false;
        render_motor_cards(&st);
    });
}

// ---------------------------------------------------------------------------
// Calibration state (verbatim `updateCalState` + mode visibility).
// ---------------------------------------------------------------------------

fn set_start_pause(active: bool) {
    if let Some(btn) = el("cal-start-btn") {
        if let Ok(Some(content)) = btn.query_selector(".btn-content") {
            content.set_inner_html(if active { PAUSE_CONTENT } else { PLAY_CONTENT });
        }
    }
}

fn apply_mode_visibility(st: &Shared) {
    let s = st.borrow();
    let calibrating = !s.cal_state.is_empty() && s.cal_state != "idle";
    let show = !calibrating && s.mode_manual;
    drop(s);
    if let Some(g) = el("motor-control-grid") {
        // NOTE: the full style string, not display alone — a display-only
        // rewrite wipes the grid-template-columns rule and stacks the cards
        // vertically (vanilla sets the display *property*, preserving rest).
        g.set_attribute(
            "style",
            &format!(
                "display:{};grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:16px;margin-top:4px;",
                if show { "grid" } else { "none" }
            ),
        )
        .ok();
    }
    let label = if calibrating {
        "AUTO"
    } else if st.borrow().mode_manual {
        "MANUAL"
    } else {
        "AUTO"
    };
    set_text("mode-toggle-label", label);
    if let Some(btn) = el("mode-toggle-btn") {
        toggle(&btn, "mode-on", show);
        // The disabled property via attribute (vanilla sets .disabled).
        if calibrating {
            btn.set_attribute("disabled", "").ok();
        } else {
            btn.remove_attribute("disabled").ok();
        }
    }
    set_motor_controls_enabled(show);
}

fn update_cal_state(st: &Shared, status: &serde_json::Value) {
    let state = status
        .get("state")
        .and_then(|x| x.as_str())
        .unwrap_or("idle");
    let prev = st.borrow().cal_state.clone();
    st.borrow_mut().cal_state = state.to_string();
    st.borrow_mut().samples = status
        .get("samples_collected")
        .and_then(|x| x.as_i64())
        .unwrap_or(0);
    if state == "warmup" {
        let remaining = status
            .get("warmup_remaining_s")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0)
            .max(0.0)
            .round() as i64;
        let pct = status
            .get("warmup_pct")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        set_display("warmup-row", "flex");
        if let Some(e) = el("warmup-progress-fill") {
            e.set_attribute("style", &format!("width:{pct}%;")).ok();
        }
        set_text(
            "warmup-progress-text",
            &format!("{}m {}s remaining", remaining / 60, remaining % 60),
        );
        set_display("collect-row", "none");
        set_display("sweep-status-row", "none");
        set_text("cal-phase-label", "Warming up — no data recorded");
        set_start_pause(true);
    } else if state == "sweep" {
        set_display("warmup-row", "none");
        set_display("collect-row", "none");
        set_display("sweep-status-row", "flex");
        let spd = status
            .get("speed_pct")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let dir_n = status
            .get("direction")
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let dir = if dir_n > 0 {
            "FWD"
        } else if dir_n < 0 {
            "REV"
        } else {
            "--"
        };
        let rem = status
            .get("step_remaining_s")
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let cyc = status.get("cycle").and_then(|x| x.as_u64()).unwrap_or(1);
        set_text("sweep-status-label", &format!("Sweep {spd}%"));
        if let Some(e) = el("sweep-status-fill") {
            e.set_attribute("style", &format!("width:{}%;", spd.min(100.0)))
                .ok();
        }
        set_text(
            "sweep-status-text",
            &format!("{dir} \u{b7} {rem}s left \u{b7} cycle {cyc}"),
        );
        set_text("cal-phase-label", "Sweeping speeds — recording data");
        set_start_pause(true);
    } else {
        set_display("warmup-row", "none");
        set_display("collect-row", "none");
        set_display("sweep-status-row", "none");
        set_text("cal-phase-label", "Press Start to begin");
        set_start_pause(false);
    }
    let n = st.borrow().samples;
    set_text("cal-samples-label", &format!("{n} samples recorded"));
    apply_mode_visibility(st);
    refresh_history_loop(st);
    if prev != state {
        render_motor_cards(st);
    }
}

// ---------------------------------------------------------------------------
// Manual motor rows (rendered once, wired once — vanilla `renderMotorControls`).
// ---------------------------------------------------------------------------

fn paint_row(grid: &Element, motor: usize, value: i32) {
    let value = value.clamp(-255, 255);
    let Ok(Some(row)) = grid.query_selector(&format!("[data-motor-row=\"{motor}\"]")) else {
        return;
    };
    let mag = value.abs() as f64 / 255.0;
    row.set_attribute(
        "style",
        &format!("--mag:{mag:.3};--glow:{:.1}px", mag * 20.0),
    )
    .ok();
    if let Ok(Some(fill)) = row.query_selector(".motor-control-fill") {
        toggle(&fill, "rev", value < 0);
        toggle(&fill, "fwd", value > 0);
    }
    toggle(&row, "on", value != 0);
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Ok(Some(v)) = doc.query_selector(&format!("#motor-val-{motor}")) {
            toggle(&v, "on", value != 0);
            v.set_text_content(Some(&value.to_string()));
        }
    }
    if let Ok(Some(r)) = row.query_selector("[data-side=\"rev\"]") {
        toggle(&r, "on", value < 0);
    }
    if let Ok(Some(f)) = row.query_selector("[data-side=\"fwd\"]") {
        toggle(&f, "on", value > 0);
    }
}

fn animate_to(st: &Shared, motor: usize, target: i32, duration: f64) {
    let slider_id = format!("motor-slider-{motor}");
    let start: i32 = el(&slider_id)
        .and_then(|s| s.dyn_into::<web_sys::HtmlInputElement>().ok())
        .and_then(|s| s.value().parse().ok())
        .unwrap_or(0);
    // Cancel any running glide for this motor (vanilla motorAnim[motor]).
    if let Some(id) = st.borrow().frame_id[motor - 1] {
        if let Some(w) = web_sys::window() {
            w.cancel_animation_frame(id).ok();
        }
    }
    let token = st.borrow().anim_token[motor - 1] + 1;
    st.borrow_mut().anim_token[motor - 1] = token;
    if start == target {
        if let Some(grid) = el("motor-control-grid") {
            paint_row(&grid, motor, target);
        }
        st.borrow_mut().speeds[motor - 1] = target;
        return;
    }
    let t0 = std::rc::Rc::new(std::cell::Cell::new(None::<f64>));
    let frame: FrameCell = std::rc::Rc::new(std::cell::RefCell::new(None));
    let frame2 = frame.clone();
    let st2 = st.clone();
    *frame.borrow_mut() = Some(Closure::new(move |ts: f64| {
        if st2.borrow().anim_token[motor - 1] != token {
            return;
        }
        if t0.get().is_none() {
            t0.set(Some(ts));
        }
        let p = ((ts - t0.get().unwrap_or(ts)) / duration).min(1.0);
        let val = (start as f64 + (target - start) as f64 * ease_out_expo(p)).round() as i32;
        if let Some(s) = el(&slider_id).and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            s.set_value(&val.to_string());
        }
        if let Some(grid) = el("motor-control-grid") {
            paint_row(&grid, motor, val);
        }
        st2.borrow_mut().speeds[motor - 1] = val;
        if p < 1.0 {
            if let (Some(w), Some(f)) = (web_sys::window(), frame2.borrow().as_ref()) {
                if let Ok(id) = w.request_animation_frame(f.as_ref().unchecked_ref()) {
                    st2.borrow_mut().frame_id[motor - 1] = Some(id);
                }
            }
        } else {
            st2.borrow_mut().frame_id[motor - 1] = None;
            wasm_bindgen_futures::spawn_local(async move {
                post_motor(motor as u8, target).await;
            });
        }
    }));
    let first = if let (Some(w), Some(f)) = (web_sys::window(), frame.borrow().as_ref()) {
        w.request_animation_frame(f.as_ref().unchecked_ref()).ok()
    } else {
        None
    };
    std::mem::forget(frame);
    if let Some(id) = first {
        st.borrow_mut().frame_id[motor - 1] = Some(id);
    }
}

fn schedule_send(st: &Shared, motor: usize, value: i32) {
    let seq = st.borrow().deb_seq[motor - 1] + 1;
    st.borrow_mut().deb_seq[motor - 1] = seq;
    let st = st.clone();
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(80).await;
        if st.borrow().deb_seq[motor - 1] == seq {
            post_motor(motor as u8, value).await;
        }
    });
}

fn set_motor_controls_enabled(enabled: bool) {
    let Some(grid) = el("motor-control-grid") else {
        return;
    };
    if let Ok(rows) = grid.query_selector_all(".motor-control-row") {
        for i in 0..rows.length() {
            if let Some(n) = rows.item(i) {
                if let Ok(r) = n.dyn_into::<Element>() {
                    toggle(&r, "disabled", !enabled);
                }
            }
        }
    }
    // Sliders + OFF buttons share the enabled flag.
    for sel in ["input[type=\"range\"]", ".btn-off"] {
        if let Ok(nodes) = grid.query_selector_all(sel) {
            for i in 0..nodes.length() {
                if let Some(n) = nodes.item(i) {
                    // setAttribute/removeAttribute mirror the .disabled property.
                    if enabled {
                        n.dyn_into::<Element>()
                            .ok()
                            .map(|e| e.remove_attribute("disabled").ok());
                    } else {
                        n.dyn_into::<Element>()
                            .ok()
                            .map(|e| e.set_attribute("disabled", "").ok());
                    }
                }
            }
        }
    }
}

fn render_motor_controls(st: &Shared) {
    let Some(grid) = el("motor-control-grid") else {
        return;
    };
    let mut html = String::new();
    for (i, name) in MOTOR_NAMES.iter().enumerate() {
        let m = i + 1;
        html += &format!(
            "<div class=\"motor-control-row\" data-motor-row=\"{m}\">\
             <div class=\"motor-control-head\">\
             <span class=\"cal-motor-name\">Motor {name}</span>\
             <div class=\"motor-control-meta\">\
             <span class=\"motor-control-val\" id=\"motor-val-{m}\">0</span>\
             <button class=\"btn-off\" type=\"button\" data-off=\"{m}\" title=\"Stop motor\"><span class=\"btn-off-label\">OFF</span></button>\
             </div></div>\
             <div class=\"motor-control-track\">\
             <div class=\"motor-control-groove\"></div>\
             <div class=\"motor-control-fill\"></div>\
             <div class=\"motor-control-tick\"></div>\
             <input type=\"range\" class=\"motor-control-input\" min=\"-255\" max=\"255\" step=\"1\" value=\"0\" id=\"motor-slider-{m}\" data-motor=\"{m}\">\
             </div>\
             <div class=\"motor-control-labels\">\
             <span class=\"motor-label\" data-side=\"rev\">REV</span>\
             <span class=\"motor-label\" data-side=\"fwd\">FWD</span>\
             </div></div>"
        );
    }
    grid.set_inner_html(&html);
    // Wire once (grid is static after this).
    if let Ok(sliders) = grid.query_selector_all("input[type=\"range\"]") {
        for i in 0..sliders.length() {
            if let Some(n) = sliders.item(i) {
                if let Ok(input) = n.dyn_into::<web_sys::HtmlInputElement>() {
                    let motor: usize = input
                        .get_attribute("data-motor")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1);
                    let st_in = st.clone();
                    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                        let v = e
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                            .and_then(|s| s.value().parse::<i32>().ok())
                            .unwrap_or(0);
                        // Cancel this motor's glide (vanilla motorAnim[motor]).
                        if let Some(id) = st_in.borrow().frame_id[motor - 1] {
                            if let Some(w) = web_sys::window() {
                                w.cancel_animation_frame(id).ok();
                            }
                            st_in.borrow_mut().frame_id[motor - 1] = None;
                        }
                        if let Some(g) = el("motor-control-grid") {
                            paint_row(&g, motor, v);
                        }
                        st_in.borrow_mut().speeds[motor - 1] = v.clamp(-255, 255);
                        schedule_send(&st_in, motor, v.clamp(-255, 255));
                    });
                    input
                        .add_event_listener_with_callback("input", cb.as_ref().unchecked_ref())
                        .ok();
                    cb.forget();
                    let st = st.clone();
                    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                        let v = e
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                            .and_then(|s| s.value().parse::<i32>().ok())
                            .unwrap_or(0);
                        let st = st.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            post_motor(motor as u8, v).await;
                            let _ = &st;
                        });
                    });
                    input
                        .add_event_listener_with_callback("change", cb.as_ref().unchecked_ref())
                        .ok();
                    cb.forget();
                }
            }
        }
    }
    if let Ok(btns) = grid.query_selector_all(".btn-off") {
        for i in 0..btns.length() {
            if let Some(n) = btns.item(i) {
                if let Ok(btn) = n.dyn_into::<Element>() {
                    let motor: usize = btn
                        .get_attribute("data-off")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1);
                    let st = st.clone();
                    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
                        animate_to(&st, motor, 0, 280.0);
                    });
                    btn.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
                        .ok();
                    cb.forget();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// History timeline (verbatim math + templates).
// ---------------------------------------------------------------------------

fn heat_cls(temp: f64) -> &'static str {
    if temp >= 50.0 {
        "alarm"
    } else if temp >= 45.0 {
        "warn"
    } else {
        "ok"
    }
}

fn hour_label(key: i64) -> String {
    let d = js_sys::Date::new(&(key as f64 * 3600.0 * 1000.0).into());
    format!("{:02}:00", d.get_hours())
}

fn metric_bar(label: &str, pct: f64, val: &str, tick_pct: f64, danger: bool) -> String {
    let c = pct.clamp(0.0, 100.0);
    let t = tick_pct.clamp(0.0, 100.0);
    format!(
        "<div class=\"metric-bar{}\"><span class=\"metric-label\">{label}</span>\
         <div class=\"metric-track\"><div class=\"metric-fill\" style=\"width:{c}%;\"></div>\
         <span class=\"metric-tick\" style=\"left:{t}%;\"></span></div>\
         <span class=\"metric-val\">{val}</span></div>",
        if danger { " danger" } else { "" }
    )
}

struct Hour {
    key: i64,
    runs: u64,
    minutes: i64,
    seconds: f64,
    max_temp: f64,
    avg_temp: f64,
    avg_volt: f64,
    avg_curr: f64,
    vib: u64,
    motors: std::collections::BTreeMap<u8, (f64, f64)>, // id -> (seconds, maxTemp)
    segs: Vec<Seg>,
    alert: bool,
}

fn aggregate_hours(segments: Vec<Seg>) -> Vec<Hour> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<i64, Hour> = BTreeMap::new();
    let mut samples_total: BTreeMap<i64, u64> = BTreeMap::new();
    let mut temp_sum: BTreeMap<i64, f64> = BTreeMap::new();
    let mut volt_sum: BTreeMap<i64, f64> = BTreeMap::new();
    let mut curr_sum: BTreeMap<i64, f64> = BTreeMap::new();
    for s in segments {
        let key = (s.start_ts / 3600.0).floor() as i64;
        let n = s.samples.max(1);
        let h = map.entry(key).or_insert_with(|| Hour {
            key,
            runs: 0,
            minutes: 0,
            seconds: 0.0,
            max_temp: 0.0,
            avg_temp: 0.0,
            avg_volt: 0.0,
            avg_curr: 0.0,
            vib: 0,
            motors: BTreeMap::new(),
            segs: vec![],
            alert: false,
        });
        h.runs += 1;
        *samples_total.entry(key).or_insert(0) += n;
        h.seconds += s.duration_s;
        *temp_sum.entry(key).or_insert(0.0) += s.avg_temp * n as f64;
        if s.max_temp > h.max_temp {
            h.max_temp = s.max_temp;
        }
        *volt_sum.entry(key).or_insert(0.0) += s.avg_voltage * n as f64;
        *curr_sum.entry(key).or_insert(0.0) += s.avg_current * n as f64;
        h.vib += s.vibration_count;
        let m = h.motors.entry(s.motor).or_insert((0.0, 0.0));
        m.0 += s.duration_s;
        if s.max_temp > m.1 {
            m.1 = s.max_temp;
        }
        if (s.max_temp > 50.0)
            || (s.vibration_count > 0)
            || (s.avg_voltage > 0.0 && s.avg_voltage < 11.5)
        {
            h.alert = true;
        }
        h.segs.push(s);
    }
    let mut hours: Vec<Hour> = map.into_values().collect();
    for h in hours.iter_mut() {
        let n = samples_total.get(&h.key).copied().unwrap_or(0).max(1) as f64;
        h.minutes = (h.seconds / 60.0).round() as i64;
        h.avg_temp = temp_sum.get(&h.key).copied().unwrap_or(0.0) / n;
        h.avg_volt = volt_sum.get(&h.key).copied().unwrap_or(0.0) / n;
        h.avg_curr = curr_sum.get(&h.key).copied().unwrap_or(0.0) / n;
    }
    hours
}

fn render_activity_strip(hist_day: &str, hours: &[Hour]) {
    let Some(strip) = el("sched-strip") else {
        return;
    };
    let now_hour = (js_sys::Date::now() / 3600.0 / 1000.0).floor() as i64;
    let is_today = hist_day == today_str();
    let today = js_sys::Date::new(&js_sys::Date::now().into());
    let midnight = js_sys::Date::new_with_year_month_day(
        today.get_full_year(),
        today.get_month() as i32,
        today.get_date() as i32,
    );
    let day_start = (midnight.get_time() / 3600000.0).floor() as i64;
    let mut html = String::new();
    for i in 0..24 {
        let key = day_start + i;
        let h = hours.iter().find(|x| x.key == key);
        let mut cls = "sched-strip-cell".to_string();
        let mut tip = String::new();
        if let Some(h) = h {
            cls += if h.alert { " alert" } else { " on" };
            tip = format!(
                " title=\"{} \u{b7} {} {} \u{b7} {} min\"",
                hour_label(key),
                h.runs,
                if h.runs > 1 { "runs" } else { "run" },
                h.minutes
            );
        }
        if is_today && key == now_hour {
            cls += " now";
        }
        html += &format!("<div class=\"{cls}\"{tip}></div>");
    }
    strip.set_inner_html(&html);
}

fn render_hourly_summary(st: &Shared, segments: Vec<Seg>) {
    let hours = aggregate_hours(segments);
    let hist_day = st.borrow().hist_day.clone();
    render_activity_strip(&hist_day, &hours);
    let Some(list) = el("sched-list") else { return };
    if hours.is_empty() {
        list.set_inner_html("<div class=\"sched-empty\">No data recorded on this day</div>");
        return;
    }
    let now_hour = (js_sys::Date::now() / 3600.0 / 1000.0).floor() as i64;
    let is_today = hist_day == today_str();
    let mut rows: Vec<String> = vec![];
    let mut now_idx: Option<usize> = None;
    for h in &hours {
        if is_today && now_idx.is_none() && h.key > now_hour {
            now_idx = Some(rows.len());
        }
        let expanded = st
            .borrow()
            .expanded
            .contains(&format!("{}:{}", hist_day, h.key));
        let motors = {
            let mut ids: Vec<u8> = h.motors.keys().copied().collect();
            ids.sort_unstable();
            ids.iter()
                .map(|m| {
                    MOTOR_NAMES
                        .get((*m as usize).saturating_sub(1))
                        .copied()
                        .unwrap_or("M")
                })
                .collect::<Vec<_>>()
                .join(" \u{b7} ")
        };
        // NOTE: vanilla prints motor names for known ids and `M{n}` otherwise;
        // unknown ids render as "M" here — the backend only ever sends 1-4.
        let title = format!(
            "{} \u{b7} {} {} \u{b7} {} min",
            if motors.is_empty() {
                "Motors".to_string()
            } else {
                motors
            },
            h.runs,
            if h.runs > 1 { "runs" } else { "run" },
            h.minutes
        );
        let mut reading = if h.max_temp > 0.0 {
            format!("{:.1}\u{b0}C max", h.max_temp)
        } else {
            "--\u{b0}C".to_string()
        };
        reading += &format!(" \u{b7} {:.2}A", h.avg_curr);
        let mut row = format!(
            "<div class=\"sched-row hour{}{}\" data-hour=\"{}\">",
            if h.alert { " is-alert" } else { "" },
            if expanded { " expanded" } else { "" },
            h.key
        );
        if h.alert {
            row += "<div class=\"sched-alert-badge\">ALERT</div>";
        }
        row += &format!(
            "<div class=\"sched-hour-head\"><div class=\"sched-row-main\">\
             <span class=\"sched-time\">{}</span><span class=\"sched-marker\"></span>\
             <span class=\"sched-title\">{title}</span></div>\
             <div class=\"sched-meta\"><span class=\"sched-tag{}\">{}</span>\
             <span class=\"sched-reading\">{reading}</span>\
             <svg class=\"sched-hour-chev\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"m6 9 6 6 6-6\"></path></svg>\
             </div></div>",
            hour_label(h.key),
            if h.alert { " alert" } else { "" },
            if h.alert { "ALERT" } else { "OK" }
        );
        row += "<div class=\"sched-hour-detail\"><div class=\"heat-cells\">";
        for (i, name) in MOTOR_NAMES.iter().enumerate() {
            let mid = (i + 1) as u8;
            let cell = match h.motors.get(&mid) {
                Some((secs, mt)) => format!(
                    "<div class=\"heat-fill {}\" style=\"width:{}%;\"></div>",
                    heat_cls(*mt),
                    (secs / 36.0 * 100.0).min(100.0)
                ),
                None => "<div class=\"heat-fill none\" style=\"width:0%;\"></div>".to_string(),
            };
            row += &format!(
                "<div class=\"heat-cell\"><span class=\"heat-label\">{name}</span>\
                 <div class=\"heat-track\">{cell}</div></div>"
            );
        }
        row += "</div><div class=\"metric-bars\">";
        row += &metric_bar(
            "T",
            h.avg_temp / 60.0 * 100.0,
            &format!("{:.1}\u{b0}", h.avg_temp),
            83.3,
            h.avg_temp > 50.0,
        );
        row += &metric_bar(
            "A",
            h.avg_curr / 3.0 * 100.0,
            &format!("{:.2}A", h.avg_curr),
            66.7,
            h.avg_curr > 2.0,
        );
        row += &metric_bar(
            "V",
            (h.avg_volt - 10.0) / 4.0 * 100.0,
            &format!("{:.1}V", h.avg_volt),
            37.5,
            h.avg_volt > 0.0 && h.avg_volt < 11.5,
        );
        if h.vib > 0 {
            row += &format!(
                "<span class=\"metric-vib\" title=\"Vibrations detected\">{} vib</span>",
                h.vib
            );
        }
        row += "</div></div><div class=\"sched-hour-segs\">";
        for s in &h.segs {
            let seg_alert = (s.max_temp > 50.0)
                || (s.vibration_count > 0)
                || (s.avg_voltage > 0.0 && s.avg_voltage < 11.5);
            let m_name = MOTOR_NAMES
                .get((s.motor as usize).saturating_sub(1))
                .copied()
                .unwrap_or("M");
            let dir = if s.direction == "IDLE" {
                "IDLE".to_string()
            } else {
                format!("{} {}%", s.direction, s.speed_pct)
            };
            let mut seg_reading = format!(
                "{} \u{b7} {:.2}A",
                if s.max_temp > 0.0 {
                    format!("{:.1}\u{b0}C max", s.max_temp)
                } else {
                    "--".to_string()
                },
                s.avg_current
            );
            if s.vibration_count > 0 {
                seg_reading += &format!(" \u{b7} {} vib", s.vibration_count);
            }
            row += &format!(
                "<div class=\"sched-seg{}\"><span class=\"sched-seg-motor\">{m_name}</span>\
                 <span class=\"sched-seg-info\">{dir} \u{b7} {} \u{b7} {:.1} min</span>\
                 <span class=\"sched-seg-reading\">{seg_reading}</span></div>",
                if seg_alert { " is-alert" } else { "" },
                s.start_iso,
                s.duration_s / 60.0
            );
        }
        row += "</div></div>";
        rows.push(row);
    }
    let idx = now_idx.unwrap_or(rows.len());
    if is_today {
        rows.insert(
            idx.min(rows.len()),
            "<div class=\"sched-now\"><span class=\"sched-now-diamond\" aria-hidden=\"true\"></span><span class=\"sched-now-line\" aria-hidden=\"true\"></span></div>"
                .to_string(),
        );
    }
    list.set_inner_html(&rows.join(""));
    // Re-bind expands (rows are rebuilt each render, like vanilla).
    if let Ok(nodes) = list.query_selector_all(".sched-row.hour") {
        for i in 0..nodes.length() {
            if let Some(n) = nodes.item(i) {
                if let Ok(r) = n.dyn_into::<Element>() {
                    let st = st.clone();
                    let r2 = r.clone();
                    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
                        let key = format!(
                            "{}:{}",
                            st.borrow().hist_day,
                            r2.get_attribute("data-hour").unwrap_or_default()
                        );
                        // DOMTokenList.toggle flips and returns the new state,
                        // exactly like vanilla's toggle-then-record.
                        let on = r2.class_list().toggle("expanded").unwrap_or(false);
                        if on {
                            st.borrow_mut().expanded.push(key);
                        } else {
                            st.borrow_mut().expanded.retain(|k| k != &key);
                        }
                    });
                    r.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
                        .ok();
                    cb.forget();
                }
            }
        }
    }
}

fn render_day_nav(st: &Shared, days: &[String]) {
    let Some(wrap) = el("sched-days") else { return };
    let cur = st.borrow().hist_day.clone();
    let cur_ms = day_start_ms(&cur);
    let mut html = String::new();
    for off in -1..=1 {
        let ts = cur_ms + off as f64 * 86400000.0;
        let key = date_str(ts);
        let d = js_sys::Date::new(&ts.into());
        let sel = key == cur;
        let has = days.contains(&key);
        html += &format!(
            "<button class=\"day-tile{}{}\" data-day=\"{key}\" type=\"button\">\
             <span class=\"day-label\">{}</span>\
             <span class=\"h-badge\"><span class=\"h-badge-inner\">{}</span></span></button>",
            if sel { " active" } else { "" },
            if has { " has-data" } else { "" },
            WEEKDAYS[d.get_day() as usize % 7],
            d.get_date()
        );
    }
    wrap.set_inner_html(&html);
    if let Ok(tiles) = wrap.query_selector_all(".day-tile") {
        for i in 0..tiles.length() {
            if let Some(n) = tiles.item(i) {
                if let Ok(t) = n.dyn_into::<Element>() {
                    let st = st.clone();
                    let t2 = t.clone();
                    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
                        if let Some(k) = t2.get_attribute("data-day") {
                            st.borrow_mut().hist_day = k;
                            render_history(&st);
                        }
                    });
                    t.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
                        .ok();
                    cb.forget();
                }
            }
        }
    }
}

/// Local-midnight ms for a YYYY-MM-DD key (vanilla `new Date(y,m,d)`).
fn day_start_ms(day: &str) -> f64 {
    let parts: Vec<u32> = day.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() == 3 {
        js_sys::Date::new_with_year_month_day(parts[0], (parts[1] as i32) - 1, parts[2] as i32)
            .get_time()
    } else {
        js_sys::Date::now()
    }
}

fn render_history(st: &Shared) {
    let st2 = st.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let days: Vec<String> = get_json("/api/history/days")
            .await
            .and_then(|v| v.get("days").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        render_day_nav(&st2, &days);
        let day = st2.borrow().hist_day.clone();
        let segs: Vec<Seg> = get_json(&format!("/api/history/segments?day={day}"))
            .await
            .and_then(|v| v.get("segments").cloned())
            .and_then(|v| serde_json::from_value::<Vec<serde_json::Value>>(v).ok())
            .map(|arr| arr.iter().map(Seg::from_value).collect())
            .unwrap_or_default();
        render_hourly_summary(&st2, segs);
    });
    // Today chip + foot clock paint immediately (vanilla does both here).
    let is_today = st.borrow().hist_day == today_str();
    if let Some(btn) = el("hist-today") {
        toggle(&btn, "show", !is_today);
    }
    update_foot_clock();
}

fn now_label() -> String {
    let d = js_sys::Date::new(&js_sys::Date::now().into());
    format!(
        "{}/{}/{} {:02}:{:02}:{:02}",
        d.get_full_year(),
        pad2(d.get_month() + 1),
        pad2(d.get_date()),
        pad2(d.get_hours()),
        pad2(d.get_minutes()),
        pad2(d.get_seconds())
    )
}

fn update_foot_clock() {
    set_text("hist-foot-ts", &now_label());
}

fn reload_history(st: &Shared) {
    render_history(st);
}

fn refresh_history_loop(st: &Shared) {
    let calibrating = {
        let s = st.borrow();
        !s.cal_state.is_empty() && s.cal_state != "idle"
    };
    let running = st.borrow().hist_timer.is_some();
    if calibrating {
        if !running {
            let st2 = st.clone();
            st.borrow_mut().hist_timer =
                Some(gloo_timers::callback::Interval::new(10000, move || {
                    render_history(&st2);
                }));
            render_history(st);
        }
    } else if running {
        st.borrow_mut().hist_timer = None;
    }
}

// ---------------------------------------------------------------------------
// Actions + WS + init.
// ---------------------------------------------------------------------------

fn toggle_calibration(st: &Shared) {
    let idle = {
        let s = st.borrow();
        s.cal_state.is_empty() || s.cal_state == "idle"
    };
    let st2 = st.clone();
    if !idle {
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(res) = post_json("/api/calibration/stop", serde_json::json!({})).await {
                let n = res
                    .get("samples_collected")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
                st2.borrow_mut().samples = n;
            }
            st2.borrow_mut().cal_state = "idle".to_string();
            update_cal_state(&st2, &serde_json::json!({"state": "idle"}));
            show_loss_banner(false);
        });
    } else {
        wasm_bindgen_futures::spawn_local(async move {
            if post_json("/api/calibration/start", serde_json::json!({}))
                .await
                .is_some()
            {
                poll_status(&st2);
            }
        });
    }
}

fn download_data() {
    if let Some(win) = web_sys::window() {
        let url = format!("{}/api/calibration/export", crate::ws::api_base());
        win.open_with_url_and_target(&url, "_blank").ok();
    }
}

fn do_reset(st: &Shared) {
    set_display("warmup-row", "none");
    set_display("collect-row", "none");
    set_text("cal-phase-label", "Press Start to begin");
    set_start_pause(false);
    set_text("cal-samples-label", "0 samples collected");
    st.borrow_mut().cal_state = "idle".to_string();
    st.borrow_mut().samples = 0;
    st.borrow_mut().data = [None, None, None, None];
    show_loss_banner(false);
    render_motor_cards(st);
}

fn reset_calibration(st: &Shared) {
    let calibrating = {
        let s = st.borrow();
        !s.cal_state.is_empty() && s.cal_state != "idle"
    };
    let st = st.clone();
    if calibrating {
        wasm_bindgen_futures::spawn_local(async move {
            post_json("/api/calibration/stop", serde_json::json!({})).await;
            do_reset(&st);
        });
    } else {
        do_reset(&st);
    }
}

fn poll_status(st: &Shared) {
    let idle = {
        let s = st.borrow();
        s.cal_state.is_empty() || s.cal_state == "idle"
    };
    if idle {
        return;
    }
    let st = st.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Some(status) = get_json("/api/calibration/status").await {
            update_cal_state(&st, &status);
        }
    });
}

fn show_loss_banner(show: bool) {
    if let Some(e) = el("conn-lost-banner") {
        // Full style string: a display-only rewrite would drop the red
        // border and spacing below (same wipe class as the motor grid).
        e.set_attribute(
            "style",
            &format!(
                "display:{};border:1px solid var(--dot-red);margin-top:16px;",
                if show { "block" } else { "none" }
            ),
        )
        .ok();
    }
}

fn set_conn_chip(up: bool) {
    // NOTE: vanilla distinguishes the WS link chip from the device badge;
    // the chip text lives here, the badge in apply_device_status.
    set_text(
        "cal-conn-label",
        if up {
            "Connected to server"
        } else {
            "Disconnected from server"
        },
    );
}

fn render_status_badge(online: bool) {
    if let Some(dot) = el("cal-status-dot") {
        dot.set_attribute(
            "class",
            if online {
                "status-dot dot-green pulse"
            } else {
                "status-dot dot-red"
            },
        )
        .ok();
    }
    set_text(
        "cal-status-label",
        if online { "Online" } else { "Offline" },
    );
}

fn connect_cal_ws(st: Shared) {
    let url = ws_url();
    let Ok(socket) = web_sys::WebSocket::new(&url) else {
        let st = st.clone();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(3000).await;
            connect_cal_ws(st);
        });
        return;
    };
    let onopen = Closure::<dyn FnMut(_)>::new({
        let s = socket.clone();
        let _st = st.clone();
        move |_: web_sys::Event| {
            set_conn_chip(true);
            show_loss_banner(false);
            let _ = s.send_with_str("ping");
        }
    });
    socket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let onmessage = Closure::<dyn FnMut(_)>::new({
        let st = st.clone();
        move |e: web_sys::MessageEvent| {
            let Some(text) = e.data().as_string() else {
                return;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                return;
            };
            match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                "motor_update" => {
                    let m = v.get("motor").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                    if !(1..=4).contains(&m) {
                        return;
                    }
                    let vib = &v["vibration"];
                    st.borrow_mut().data[m - 1] = Some(CalSample {
                        temp: v.get("temp").and_then(|x| x.as_f64()),
                        voltage: v.get("voltage").and_then(|x| x.as_f64()),
                        current: v.get("current").and_then(|x| x.as_f64()),
                        vibration: vib.as_bool().unwrap_or(false)
                            || vib.as_u64().is_some_and(|x| x == 1)
                            || vib.as_str().is_some_and(|x| x == "1"),
                        vib_mag: v.get("vib_mag").and_then(|x| x.as_f64()),
                        health: v.get("health").and_then(|x| x.as_f64()),
                        vib_valid: v
                            .get("vibration_valid")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true),
                        temp_valid: v
                            .get("temp_valid")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true),
                        ina_ok: v.get("ina_ok").and_then(|x| x.as_bool()).unwrap_or(true),
                    });
                    st.borrow_mut().rx[m - 1] = Some(now_ms());
                    let warm = {
                        let s = st.borrow();
                        s.cal_state == "sweep" || s.cal_state == "warmup"
                    };
                    if warm {
                        queue_motor_render(&st);
                    }
                }
                "snapshot" => {}
                "calibration_status" => update_cal_state(&st, &v),
                "device_status" => {
                    if let Some(online) = v.get("online").and_then(|x| x.as_bool()) {
                        render_status_badge(online);
                    }
                }
                "pong" => {}
                _ => {}
            }
        }
    });
    socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let onclose = Closure::<dyn FnMut(_)>::new({
        let st = st.clone();
        move |_: web_sys::CloseEvent| {
            set_conn_chip(false);
            let calibrating = {
                let s = st.borrow();
                !s.cal_state.is_empty() && s.cal_state != "idle"
            };
            if calibrating {
                show_loss_banner(true);
            }
            let st = st.clone();
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(3000).await;
                connect_cal_ws(st);
            });
        }
    });
    socket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
    let onerror = Closure::<dyn FnMut(_)>::new({
        let s = socket.clone();
        move |_: web_sys::Event| {
            let _ = s.close();
        }
    });
    socket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();
}

#[component]
pub fn CalibrationPage() -> impl IntoView {
    let st: Shared = std::rc::Rc::new(std::cell::RefCell::new(CalSt {
        hist_day: today_str(),
        ..Default::default()
    }));

    // -- setup: static renders, polls, WS (refs resolve post-mount) --
    {
        let st = st.clone();
        connect_cal_ws(st.clone());
        wasm_bindgen_futures::spawn_local(async move {
            render_motor_cards(&st);
            render_motor_controls(&st);
            apply_mode_visibility(&st);
            render_history(&st);
            // Recurring polls (handles forgotten: 'static by construction).
            let c = st.clone();
            let _iv = gloo_timers::callback::Interval::new(2000, move || poll_status(&c));
            _iv.forget();
            let _iv = gloo_timers::callback::Interval::new(3000, move || {
                wasm_bindgen_futures::spawn_local(async move {
                    render_status_badge(fetch_device_online().await);
                });
            });
            _iv.forget();
            let _iv = gloo_timers::callback::Interval::new(1000, update_foot_clock);
            _iv.forget();
            let c = st.clone();
            let _iv = gloo_timers::callback::Interval::new(1000, move || {
                let (warm, any) = {
                    let s = c.borrow();
                    let warm = s.cal_state == "sweep" || s.cal_state == "warmup";
                    let any = s.data.iter().any(|d| d.is_some());
                    (warm, any)
                };
                if warm {
                    queue_motor_render(&c);
                } else if any {
                    render_motor_cards(&c);
                }
            });
            _iv.forget();
        });
    }

    let on_start = {
        let st = st.clone();
        move |_| toggle_calibration(&st)
    };
    let on_download = move |_| download_data();
    let on_reset = {
        let st = st.clone();
        move |_| reset_calibration(&st)
    };
    let on_mode = {
        let st = st.clone();
        move |_| {
            let calibrating = {
                let s = st.borrow();
                !s.cal_state.is_empty() && s.cal_state != "idle"
            };
            if calibrating {
                return;
            }
            // NOTE: read-then-write in separate statements — a single
            // `borrow_mut().f = !borrow().f` expression panics the RefCell.
            let next = !st.borrow().mode_manual;
            st.borrow_mut().mode_manual = next;
            apply_mode_visibility(&st);
        }
    };
    let on_prev = {
        let st = st.clone();
        move |_| {
            let next = day_start_ms(&st.borrow().hist_day) - 86400000.0;
            st.borrow_mut().hist_day = date_str(next);
            reload_history(&st);
        }
    };
    let on_next = {
        let st = st.clone();
        move |_| {
            let next = day_start_ms(&st.borrow().hist_day) + 86400000.0;
            st.borrow_mut().hist_day = date_str(next);
            reload_history(&st);
        }
    };
    let on_today = {
        let st = st.clone();
        move |_| {
            st.borrow_mut().hist_day = today_str();
            render_history(&st);
            if let Some(list) = el("sched-list") {
                list.set_scroll_top(0);
            }
        }
    };
    // "More" jumps to the detailed view (adapted from vanilla '../detailed/').
    let on_more = move |_| {
        if let Some(win) = web_sys::window() {
            win.location().set_href("/next/detailed").ok();
        }
    };

    view! {
        {app_shell(view! {
            <div class="body-wrap">
                <main class="main-content">
                    <div class="content-wrap">
                        <div class="page-header">
                            <div><h1 class="page-h1">"Calibration"</h1></div>
                            <div class="live-badge" id="cal-badge">
                                <span class="status-dot dot-red" id="cal-status-dot"></span>
                                <span id="cal-status-label">"Offline"</span>
                            </div>
                        </div>

                        <div class="panel panel-pad" style="margin-bottom:24px;">
                            <div class="panel-head">
                                <h2 class="panel-title-muted">"Baseline Collection"</h2>
                                <div style="display:flex;gap:8px;">
                                    <button class="btn-primary" id="cal-start-btn" style="padding:6px 16px;" on:click=on_start>
                                        <span class="btn-corner-tl"></span>
                                        <span class="btn-corner-br"></span>
                                        <span class="btn-sweep"><span class="btn-sweep-inner"></span></span>
                                        <span class="btn-content" style="font-size:10px;gap:6px;">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                                            "Start"
                                        </span>
                                    </button>
                                    <button class="btn-reanime" id="cal-download-btn" style="padding:6px 16px;" on:click=on_download>
                                        <div class="btn-glow"></div>
                                        <span class="btn-content" style="font-size:10px;gap:6px;">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                                            "Download"
                                        </span>
                                    </button>
                                    <button class="btn-reanime" id="cal-reset-btn" style="padding:6px 16px;" on:click=on_reset>
                                        <div class="btn-glow"></div>
                                        <span class="btn-content" style="font-size:10px;gap:6px;">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
                                            "Reset"
                                        </span>
                                    </button>
                                </div>
                            </div>

                            <div class="cal-progress-row" id="warmup-row" style="display:none;">
                                <span style="font-size:12px;color:var(--gray-500);font-weight:500;">"Warming up"</span>
                                <div class="cal-progress-bar">
                                    <div class="cal-progress-fill cal-warmup-fill" id="warmup-progress-fill" style="width:0%;"></div>
                                </div>
                                <span class="cal-progress-text" id="warmup-progress-text">"--:-- remaining"</span>
                            </div>

                            <div class="cal-progress-row" id="collect-row" style="display:none;">
                                <span style="font-size:12px;color:var(--gray-500);font-weight:500;">"Recording"</span>
                                <div class="cal-progress-bar">
                                    <div class="cal-progress-fill" id="collect-progress-fill" style="width:0%;"></div>
                                </div>
                                <span class="cal-progress-text" id="collect-progress-text">"0 samples"</span>
                            </div>

                            <div class="cal-progress-row" id="sweep-status-row" style="display:none;">
                                <span style="font-size:12px;color:var(--primary);font-weight:700;" id="sweep-status-label">"Sweep --%"</span>
                                <div class="cal-progress-bar">
                                    <div class="cal-progress-fill cal-sweep-fill" id="sweep-status-fill" style="width:0%;"></div>
                                </div>
                                <span class="cal-progress-text" id="sweep-status-text">"--"</span>
                            </div>

                            <div class="cal-info-chips">
                                <div class="cal-info-chip">
                                    <span class="chip-dot" style="background:var(--primary);"></span>
                                    <span id="cal-phase-label">"Press Start to begin"</span>
                                </div>
                                <div class="cal-info-chip">
                                    <span class="chip-dot" style="background:var(--dot-yellow);"></span>
                                    <span id="cal-samples-label">"0 samples collected"</span>
                                </div>
                                <div class="cal-info-chip">
                                    <span class="chip-dot" style="background:var(--dot-green);"></span>
                                    <span id="cal-conn-label">"Disconnected from server"</span>
                                </div>
                            </div>
                        </div>

                        <div class="panel panel-pad" style="margin-bottom:24px;">
                            <div class="panel-head">
                                <h2 class="panel-title-muted">"Manual Motor Control"</h2>
                                <div style="display:flex;align-items:center;gap:12px;">
                                    <button class="btn-reanime mode-toggle" id="mode-toggle-btn" style="padding:6px 12px;" title="Toggle AUTO / MANUAL" on:click=on_mode>
                                        <div class="btn-glow"></div>
                                        <span class="btn-content" style="font-size:10px;gap:6px;">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="4" y="4" width="16" height="16" rx="2"/><circle cx="9" cy="9" r="1.5"/><circle cx="15" cy="15" r="1.5"/><path d="m9 15 6-6"/></svg>
                                            <span id="mode-toggle-label">"AUTO"</span>
                                        </span>
                                    </button>
                                </div>
                            </div>
                            <div id="motor-control-grid" style="display:none;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:16px;margin-top:4px;"></div>
                        </div>

                        <div class="hud-panel history-panel" style="margin-bottom:24px;">
                            <span class="hud-corner hud-corner-tl" aria-hidden="true"></span>
                            <span class="hud-corner hud-corner-br" aria-hidden="true"></span>
                            <span class="hud-vents" aria-hidden="true"></span>
                            <div class="hud-content">
                                <div class="sched-head">
                                    <button class="day-tile day-nav" id="hist-prev" aria-label="Previous day" on:click=on_prev>
                                        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>
                                    </button>
                                    <div class="sched-days" id="sched-days"></div>
                                    <button class="day-tile day-today" id="hist-today" type="button" on:click=on_today>
                                        <span class="day-label">"today"</span>
                                    </button>
                                    <button class="day-tile day-nav" id="hist-next" aria-label="Next day" on:click=on_next>
                                        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>
                                    </button>
                                </div>
                                <div class="sched-strip" id="sched-strip" aria-hidden="true"></div>
                                <div class="sched-list" id="sched-list"></div>
                                <div class="sched-foot">
                                    <span class="sched-foot-ts" id="hist-foot-ts">"--"</span>
                                    <button class="sched-more" id="hist-more" type="button" on:click=on_more>
                                        <span>"More"</span>
                                        <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"></path></svg>
                                    </button>
                                </div>
                            </div>
                        </div>

                        <div class="cal-motor-grid" id="cal-motor-grid"></div>

                        <div class="panel panel-pad" id="conn-lost-banner" style="display:none;border:1px solid var(--dot-red);margin-top:16px;">
                            <div style="display:flex;align-items:center;gap:12px;">
                                <span style="font-size:14px;">"\u{26a0}"</span>
                                <span style="font-size:12px;color:var(--gray-300);">"Connection lost — data is not being recorded. Reconnect and try again."</span>
                            </div>
                        </div>

                    </div>
                </main>
            </div>
        })}
    }
}
