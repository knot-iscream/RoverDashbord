//! Home dashboard (`/next/`) — 1:1 port of `web/index.html` + `web/app.js`.
//!
//! Vehicle-damage HUD (SVG, painted imperatively like vanilla), camera card,
//! arm placeholder, device badge. Telemetry is LIVE from WS when the device
//! streams; each motor falls back to the vanilla random walk until its first
//! real sample arrives (per-motor latch, so mixed hardware states degrade
//! gracefully). This settles the parked `simulateData()` question: live
//! wins, simulation is the no-device fallback, never shown as real data.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::Element;

use crate::env_ui;
use crate::shell::app_shell;
use crate::ws::{fetch_device_online, get_json, now_ms, ws_url};

const LABELS: [&str; 4] = ["FL", "FR", "RL", "RR"];
const PARTS: [&str; 4] = ["wheelFL", "wheelFR", "wheelRL", "wheelRR"];

#[derive(Debug, Clone, Copy)]
struct HomeMotor {
    health: f64,
    temp: f64,
    vibration: bool,
    voltage: f64,
    current: f64,
}

fn rnd() -> f64 {
    js_sys::Math::random()
}

fn temp_color(temp: f64) -> String {
    let t = temp.clamp(20.0, 60.0);
    let hue = ((1.0 - (t - 20.0) / 40.0) * 240.0).round() as i64;
    format!("hsl({hue}, 100%, 50%)")
}

fn q(hud: NodeRef<leptos::html::Div>, sel: &str) -> Option<Element> {
    hud.get()?.query_selector(sel).ok()?
}

/// `updatePart` verbatim: wheel fill from cx/fullW, chassis from total
/// height, temp color fill, `dmg-flash` restart on >3C jumps.
fn update_part(hud: NodeRef<leptos::html::Div>, part: &str, health_pct: f64, temp: f64) {
    let Some(el) = q(hud, &format!("[data-part=\"{part}\"]")) else {
        return;
    };
    let color = temp_color(temp);
    match part {
        p if p.starts_with("wheel") => {
            let (cx, full_w) = match p {
                "wheelFL" => (237.0, 49.58),
                "wheelFR" => (361.272, 17.76),
                "wheelRL" => (237.0, 28.86),
                _ => (361.272, 37.0),
            };
            let fill_w = full_w * health_pct / 100.0;
            el.set_attribute("x", &format!("{:.3}", cx + full_w - fill_w))
                .ok();
            el.set_attribute("width", &format!("{fill_w:.3}")).ok();
        }
        "chassis" => {
            let total_h: f64 = el
                .get_attribute("data-total-height")
                .and_then(|v| v.parse().ok())
                .unwrap_or(419.0);
            let new_h = total_h * health_pct / 100.0;
            el.set_attribute("y", &format!("{:.3}", 61.0 + total_h - new_h))
                .ok();
            el.set_attribute("height", &format!("{new_h:.3}")).ok();
        }
        _ => {}
    }
    el.set_attribute("fill", &color).ok();
    let prev: f64 = el
        .get_attribute("data-prev-temp")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    if (temp - prev).abs() > 3.0 {
        el.class_list().remove_1("dmg-flash").ok();
        let _ = el.get_bounding_client_rect();
        el.class_list().add_1("dmg-flash").ok();
    }
    el.set_attribute("data-prev-temp", &temp.to_string()).ok();
}

/// Rebuild the damage queue + panel status. Returns (dot_class, dot_text).
fn damage_state(motors: &[HomeMotor; 4]) -> (String, String, Vec<String>) {
    let mut texts = vec![];
    for (i, m) in motors.iter().enumerate() {
        let label = LABELS[i];
        if m.health < 50.0 {
            texts.push(format!(
                "{label} motor critical ({}%)",
                m.health.round() as i64
            ));
        } else if m.health < 80.0 {
            texts.push(format!("{label} motor worn ({}%)", m.health.round() as i64));
        }
        if m.temp > 50.0 {
            texts.push(format!(
                "{label} overheating ({}\u{b0}C)",
                m.temp.round() as i64
            ));
        }
        if m.vibration {
            texts.push(format!("{label} abnormal vibration"));
        }
    }
    let mut min_v = 99.0;
    for m in motors.iter() {
        if m.voltage < min_v {
            min_v = m.voltage;
        }
    }
    if min_v < 11.0 {
        texts.push(format!("Battery critical ({min_v:.1}V)"));
    } else if min_v < 12.0 {
        texts.push(format!("Battery low ({min_v:.1}V)"));
    }
    if texts.is_empty() {
        texts.push("All systems nominal".to_string());
    }
    let mut worst = 0;
    for m in motors.iter() {
        if m.health < 50.0 || m.temp > 50.0 || m.vibration {
            worst = 2;
        } else if worst < 1 && (m.health < 80.0 || m.temp > 40.0) {
            worst = 1;
        }
    }
    if min_v < 11.0 {
        worst = 2;
    } else if worst < 1 && min_v < 12.0 {
        worst = 1;
    }
    let (cls, txt) = match worst {
        0 => ("status-dot dot-green pulse", "All systems nominal"),
        1 => ("status-dot dot-yellow pulse", "Service advised"),
        _ => ("status-dot dot-red pulse", "Damage detected"),
    };
    (cls.to_string(), txt.to_string(), texts)
}

/// Full HUD repaint: fills, corner panels, damage queue + panel status.
fn paint_hud(
    hud: NodeRef<leptos::html::Div>,
    motors: &[HomeMotor; 4],
    dot_cls: RwSignal<String>,
    dot_txt: RwSignal<String>,
    queue: RwSignal<Vec<String>>,
) {
    let avg_h: f64 = motors.iter().map(|m| m.health).sum::<f64>() / 4.0;
    let avg_t: f64 = motors.iter().map(|m| m.temp).sum::<f64>() / 4.0;
    update_part(hud, "chassis", avg_h, avg_t);
    for (i, m) in motors.iter().enumerate() {
        update_part(hud, PARTS[i], m.health, m.temp);
        let color = temp_color(m.temp);
        let key = LABELS[i].to_lowercase();
        if let Some(el) = q(hud, &format!("#temp-{key}")) {
            el.set_text_content(Some(&format!("{}\u{b0}C", m.temp.round() as i64)));
            el.set_attribute("fill", &color).ok();
        }
        if let Some(el) = q(hud, &format!("#health-{key}")) {
            el.set_text_content(Some(&format!("{}%", m.health.round() as i64)));
            el.set_attribute("fill", &color).ok();
        }
    }
    let (cls, txt, texts) = damage_state(motors);
    dot_cls.set(cls);
    dot_txt.set(txt);
    queue.set(texts);
}

/// `simulateData` random walk verbatim (fallback only, pre-live).
fn sim_step(m: &mut HomeMotor) {
    m.health = (m.health + (rnd() - 0.5) * 0.8).clamp(10.0, 100.0);
    m.temp = (m.temp + (rnd() - 0.5) * 1.5).clamp(30.0, 65.0);
    m.voltage = (m.voltage + (rnd() - 0.5) * 0.15).clamp(10.5, 14.0);
    m.current = (m.current + (rnd() - 0.5) * 0.1).clamp(0.0, 3.0);
    if rnd() < 0.02 {
        m.vibration = true;
    }
    if rnd() < 0.01 {
        m.vibration = false;
    }
    if m.health < 50.0 && rnd() < 0.03 {
        m.vibration = true;
    }
}

#[derive(Clone)]
struct HomeCtx {
    motors: RwSignal<[HomeMotor; 4]>,
    live: RwSignal<[bool; 4]>,
    device_online: RwSignal<bool>,
    dot_cls: RwSignal<String>,
    dot_txt: RwSignal<String>,
    queue: RwSignal<Vec<String>>,
    /// Latest flat `env_update` frame (fields + `alarms`), Null before the
    /// first one. `env_stale` flips when the feed stops for >4 s.
    env_msg: RwSignal<serde_json::Value>,
    env_seen: RwSignal<f64>,
    env_stale: RwSignal<bool>,
    hud: NodeRef<leptos::html::Div>,
}

fn repaint(ctx: &HomeCtx) {
    paint_hud(
        ctx.hud,
        &ctx.motors.get(),
        ctx.dot_cls,
        ctx.dot_txt,
        ctx.queue,
    );
}

/// Home WS loop: motor_update feeds live motors (latching each one),
/// device_status drives the corner badge. No snapshot replay (vanilla home
/// never had WS at all — only fresh frames drive the HUD).
fn connect_home_ws(ctx: HomeCtx) {
    let url = ws_url();
    let Ok(socket) = web_sys::WebSocket::new(&url) else {
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(3000).await;
            connect_home_ws(ctx);
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
            match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                "motor_update" => {
                    let m = v.get("motor").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                    if !(1..=4).contains(&m) {
                        return;
                    }
                    let idx = m - 1;
                    let temp_valid = v
                        .get("temp_valid")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(true);
                    let ina_ok = v.get("ina_ok").and_then(|x| x.as_bool()).unwrap_or(true);
                    let vib = &v["vibration"];
                    let vib_on =
                        vib.as_bool().unwrap_or(false) || vib.as_u64().is_some_and(|x| x == 1);
                    ctx.motors.update(|arr| {
                        let mo = &mut arr[idx];
                        if temp_valid {
                            if let Some(t) = v.get("temp").and_then(|x| x.as_f64()) {
                                if t.is_finite() {
                                    mo.temp = t;
                                }
                            }
                        }
                        if ina_ok {
                            if let Some(x) = v.get("voltage").and_then(|x| x.as_f64()) {
                                if x.is_finite() {
                                    mo.voltage = x;
                                }
                            }
                            if let Some(x) = v.get("current").and_then(|x| x.as_f64()) {
                                if x.is_finite() {
                                    mo.current = x;
                                }
                            }
                        }
                        if let Some(h) = v.get("health").and_then(|x| x.as_f64()) {
                            if h.is_finite() {
                                mo.health = h.clamp(0.0, 100.0);
                            }
                        }
                        mo.vibration = vib_on;
                    });
                    ctx.live.update(|arr| arr[idx] = true);
                    repaint(&ctx);
                }
                "device_status" => {
                    if let Some(online) = v.get("online").and_then(|x| x.as_bool()) {
                        ctx.device_online.set(online);
                    }
                }
                "env_update" => {
                    ctx.env_msg.set(v.clone());
                    ctx.env_seen.set(now_ms());
                }
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
                connect_home_ws(ctx);
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

/// The HUD SVG, byte-identical to `web/index.html` (only the live-mutated
/// nodes are touched by paint: `[data-part]`, `#temp-*`, `#health-*`,
/// `#dmgOverlay`, `#dmgText`).
const HUD_SVG: &str = r###"<svg class="car-hud-svg" viewBox="0 0 575 503" fill="none" xmlns="http://www.w3.org/2000/svg">
<defs><clipPath id="chassis-clip"><path d="M339.153 87.0187C333.63 87.0187 329.153 91.4959 329.153 97.0187V163.77C329.153 169.293 333.63 173.77 339.153 173.77H364.546V362.256H339.153C333.63 362.256 329.153 366.733 329.153 372.256V439.006C329.153 444.529 333.63 449.006 339.153 449.006H364.546V454.764C364.546 468.571 353.353 479.764 339.546 479.764H232.24C218.433 479.764 207.24 468.571 207.24 454.764V449.006H232.634C238.157 449.006 242.634 444.529 242.634 439.006V372.256C242.634 366.733 238.157 362.256 232.634 362.256H207.24V173.77H232.634C238.157 173.77 242.634 169.292 242.634 163.77V97.0187C242.634 91.496 238.157 87.0188 232.634 87.0187H207.24V86.7823C207.24 72.9754 218.433 61.7826 232.24 61.7823H339.546C353.353 61.7824 364.546 72.9752 364.546 86.7823V87.0187H339.153Z"/></clipPath></defs>
<rect x="575" width="503" height="575" rx="25" transform="rotate(90 575 0)" fill="#222831"/>
<path d="M339.153 87.0187C333.63 87.0187 329.153 91.4959 329.153 97.0187V163.77C329.153 169.293 333.63 173.77 339.153 173.77H364.546V362.256H339.153C333.63 362.256 329.153 366.733 329.153 372.256V439.006C329.153 444.529 333.63 449.006 339.153 449.006H364.546V454.764C364.546 468.571 353.353 479.764 339.546 479.764H232.24C218.433 479.764 207.24 468.571 207.24 454.764V449.006H232.634C238.157 449.006 242.634 444.529 242.634 439.006V372.256C242.634 366.733 238.157 362.256 232.634 362.256H207.24V173.77H232.634C238.157 173.77 242.634 169.292 242.634 163.77V97.0187C242.634 91.496 238.157 87.0188 232.634 87.0187H207.24V86.7823C207.24 72.9754 218.433 61.7826 232.24 61.7823H339.546C353.353 61.7824 364.546 72.9752 364.546 86.7823V87.0187H339.153Z" fill="rgba(255,255,255,0.06)" stroke="rgba(255,255,255,0.08)" stroke-width="1.5"/>
<rect class="chassis-fill dmg-flashable" data-part="chassis" x="207" y="61" width="158" height="419" fill="transparent" clip-path="url(#chassis-clip)" data-total-height="419"/>
<line y1="-0.5" x2="39.4058" y2="-0.5" transform="matrix(-0.498994 -0.866605 0.865444 -0.501006 211.173 130.632)" stroke="rgba(255,255,255,0.25)" stroke-width="1"/>
<line x1="191.51" y1="96.9827" x2="141.958" y2="96.9827" stroke="rgba(255,255,255,0.25)" stroke-width="1"/>
<line y1="-0.5" x2="39.4058" y2="-0.5" transform="matrix(0.498994 -0.866605 -0.865444 -0.501006 362.187 130.632)" stroke="rgba(255,255,255,0.25)" stroke-width="1"/>
<line y1="-0.5" x2="49.5515" y2="-0.5" transform="matrix(1 0 0 -1 381.85 96.4827)" stroke="rgba(255,255,255,0.25)" stroke-width="1"/>
<line y1="-0.5" x2="39.4058" y2="-0.5" transform="matrix(-0.498994 -0.866605 0.865444 -0.501006 211.173 406.657)" stroke="rgba(255,255,255,0.25)" stroke-width="1"/>
<line x1="191.51" y1="373.008" x2="141.958" y2="373.008" stroke="rgba(255,255,255,0.25)" stroke-width="1"/>
<line y1="-0.5" x2="39.4058" y2="-0.5" transform="matrix(0.498994 -0.866605 -0.865444 -0.501006 362.187 406.657)" stroke="rgba(255,255,255,0.25)" stroke-width="1"/>
<line y1="-0.5" x2="49.5515" y2="-0.5" transform="matrix(1 0 0 -1 381.85 372.508)" stroke="rgba(255,255,255,0.25)" stroke-width="1"/>
<g><rect x="237.5" y="91.5" width="75" height="28" rx="5.5" transform="rotate(90 237.5 91.5)" fill="rgba(255,255,255,0.04)" stroke="rgba(255,255,255,0.10)" stroke-width="1.5"/><rect x="17.6862" y="18.4069" width="124.272" height="171.136" rx="20" fill="rgba(255,255,255,0.06)" stroke="rgba(255,255,255,0.10)" stroke-width="1.5"/><rect class="wheel-fill dmg-flashable" data-part="wheelFL" x="237" y="116.42" width="49.58" height="27" rx="5" transform="rotate(90 237 116.42)" fill="transparent" data-orig-width="49.58"/><text class="corner-name" x="80" y="88" text-anchor="middle">Front Left</text><text class="corner-temp" id="temp-fl" x="80" y="108" text-anchor="middle">38°C</text><text class="corner-health" id="health-fl" x="80" y="126" text-anchor="middle">92%</text></g>
<g><rect x="361.772" y="91.5" width="75" height="28" rx="5.5" transform="rotate(90 361.772 91.5)" fill="rgba(255,255,255,0.04)" stroke="rgba(255,255,255,0.10)" stroke-width="1.5"/><rect x="431.402" y="18.4069" width="124.272" height="171.136" rx="20" fill="rgba(255,255,255,0.06)" stroke="rgba(255,255,255,0.10)" stroke-width="1.5"/><rect class="wheel-fill dmg-flashable" data-part="wheelFR" x="361.272" y="148.24" width="17.76" height="27" rx="5" transform="rotate(90 361.272 148.24)" fill="transparent" data-orig-width="17.76"/><text class="corner-name" x="493" y="88" text-anchor="middle">Front Right</text><text class="corner-temp" id="temp-fr" x="493" y="108" text-anchor="middle">38°C</text><text class="corner-health" id="health-fr" x="493" y="126" text-anchor="middle">92%</text></g>
<g><rect x="237.5" y="367.525" width="75" height="28" rx="5.5" transform="rotate(90 237.5 367.525)" fill="rgba(255,255,255,0.04)" stroke="rgba(255,255,255,0.10)" stroke-width="1.5"/><rect x="17.6861" y="274.716" width="124.272" height="171.136" rx="20" fill="rgba(255,255,255,0.06)" stroke="rgba(255,255,255,0.10)" stroke-width="1.5"/><rect class="wheel-fill dmg-flashable" data-part="wheelRL" x="237" y="413.165" width="28.86" height="27" rx="5" transform="rotate(90 237 413.165)" fill="transparent" data-orig-width="28.86"/><text class="corner-name" x="80" y="345" text-anchor="middle">Rear Left</text><text class="corner-temp" id="temp-rl" x="80" y="365" text-anchor="middle">38°C</text><text class="corner-health" id="health-rl" x="80" y="383" text-anchor="middle">92%</text></g>
<g><rect x="361.772" y="367.525" width="75" height="28" rx="5.5" transform="rotate(90 361.772 367.525)" fill="rgba(255,255,255,0.04)" stroke="rgba(255,255,255,0.10)" stroke-width="1.5"/><rect x="431.402" y="274.716" width="124.272" height="171.136" rx="20" fill="rgba(255,255,255,0.06)" stroke="rgba(255,255,255,0.10)" stroke-width="1.5"/><rect class="wheel-fill dmg-flashable" data-part="wheelRR" x="361.272" y="405.025" width="37" height="27" rx="5" transform="rotate(90 361.272 405.025)" fill="transparent" data-orig-width="37"/><text class="corner-name" x="493" y="345" text-anchor="middle">Rear Right</text><text class="corner-temp" id="temp-rr" x="493" y="365" text-anchor="middle">38°C</text><text class="corner-health" id="health-rr" x="493" y="383" text-anchor="middle">92%</text></g>
<g id="dmgOverlay" opacity="0"><rect x="137" y="456" width="300" height="28" rx="6" fill="rgba(20,20,20,0.9)" stroke="rgba(255,255,255,0.25)" stroke-width="1"/><text id="dmgText" x="287" y="475" text-anchor="middle" fill="#fff" font-family="Inter,sans-serif" font-size="11" font-weight="600">All systems nominal</text></g>
</svg>"###;

#[component]
pub fn HomePage() -> impl IntoView {
    let motors = RwSignal::new([
        HomeMotor {
            health: 92.0,
            temp: 38.0,
            vibration: false,
            voltage: 12.3,
            current: 0.6,
        },
        HomeMotor {
            health: 87.0,
            temp: 42.0,
            vibration: false,
            voltage: 12.1,
            current: 0.8,
        },
        HomeMotor {
            health: 95.0,
            temp: 35.0,
            vibration: false,
            voltage: 12.4,
            current: 0.5,
        },
        HomeMotor {
            health: 78.0,
            temp: 48.0,
            vibration: true,
            voltage: 11.8,
            current: 1.2,
        },
    ]);
    let live = RwSignal::new([false; 4]);
    let device_online = RwSignal::new(false);
    let dot_cls = RwSignal::new("status-dot".to_string());
    let dot_txt = RwSignal::new("All systems nominal".to_string());
    let queue = RwSignal::new(Vec::<String>::new());
    let cam = RwSignal::new(0u8);
    let env_msg = RwSignal::new(serde_json::Value::Null);
    let env_seen = RwSignal::new(0.0f64);
    let env_stale = RwSignal::new(true);

    let hud = NodeRef::<leptos::html::Div>::new();
    let viewport = NodeRef::<leptos::html::Div>::new();
    let placeholder = NodeRef::<leptos::html::Div>::new();
    let cam_label = NodeRef::<leptos::html::Span>::new();

    let ctx = HomeCtx {
        motors,
        live,
        device_online,
        dot_cls,
        dot_txt,
        queue,
        env_msg,
        env_seen,
        env_stale,
        hud,
    };

    // -- setup: WS, polls, sim fallback, overlay ticker (refs post-mount) --
    {
        let ctx = ctx.clone();
        connect_home_ws(ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            ctx.device_online.set(fetch_device_online().await);
        });
        // ENV fetch folds `GET /api/env` into the flat `env_update` shape the
        // WS feeds, so the panel is populated even between frames.
        let c = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(v) = get_json("/api/env").await {
                let frame = env_ui::fold_api_env(&v);
                if !frame.is_null() {
                    c.env_msg.set(frame);
                    c.env_seen.set(now_ms());
                }
            }
        });
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            // Inject the static HUD, then paint the initial frame.
            if let Some(wrap) = ctx.hud.get() {
                wrap.set_inner_html(HUD_SVG);
            }
            repaint(&ctx);
            // Device poll every 3s.
            let c = ctx.clone();
            let _iv = gloo_timers::callback::Interval::new(3000, move || {
                let c = c.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    c.device_online.set(fetch_device_online().await);
                });
            });
            _iv.forget();
            // Sim fallback every 600ms: only motors with no live sample yet.
            let c = ctx.clone();
            let _iv = gloo_timers::callback::Interval::new(600, move || {
                let live = c.live.get();
                if live.iter().any(|l| !l) {
                    c.motors.update(|arr| {
                        for (i, m) in arr.iter_mut().enumerate() {
                            if !live[i] {
                                sim_step(m);
                            }
                        }
                    });
                    repaint(&c);
                }
            });
            _iv.forget();
            // Damage-text rotation every 2s (shift/push verbatim).
            let c = ctx.clone();
            let _iv = gloo_timers::callback::Interval::new(2000, move || {
                c.queue.update(|q| {
                    if q.is_empty() {
                        q.push("All systems nominal".to_string());
                    }
                    let txt = q.remove(0);
                    if let Some(wrap) = c.hud.get() {
                        if let Ok(Some(t)) = wrap.query_selector("#dmgText") {
                            t.set_text_content(Some(&txt));
                        }
                        if let Ok(Some(o)) = wrap.query_selector("#dmgOverlay") {
                            o.set_attribute("opacity", "1").ok();
                        }
                    }
                    q.push(txt);
                });
            });
            _iv.forget();
            // ENV staleness (4 s window, vanilla freshness rule).
            let c = ctx.clone();
            let _iv = gloo_timers::callback::Interval::new(1000, move || {
                let seen = c.env_seen.get();
                c.env_stale
                    .set(seen == 0.0 || now_ms() - seen > env_ui::FRESH_MS);
            });
            _iv.forget();
        });
    }

    let on_cam = move |mode: u8| {
        cam.set(mode);
        if mode == 1 {
            if let Some(p) = placeholder.get() {
                p.set_attribute("style", "opacity:0.4;").ok();
            }
            if let Some(v) = viewport.get() {
                v.set_attribute("style", "background:#050505;").ok();
            }
            if let Some(l) = cam_label.get() {
                l.set_text_content(Some("LiDAR map stream \u{2014} coming soon"));
            }
        } else {
            if let Some(p) = placeholder.get() {
                p.set_attribute("style", "opacity:1;").ok();
            }
            if let Some(v) = viewport.get() {
                v.set_attribute("style", "background:#0a0a0a;").ok();
            }
            if let Some(l) = cam_label.get() {
                l.set_text_content(Some("Waiting for camera stream..."));
            }
        }
    };

    view! {
        {app_shell(view! {
            <div class="body-wrap">
                <main class="main-content">
                    <div class="content-wrap">
                        <div class="page-header">
                            <div><h1 class="page-h1">"Dashboard"</h1></div>
                            <div class="live-badge" id="home-rt-badge">
                                <span class={move || if device_online.get() { "status-dot dot-green pulse" } else { "status-dot dot-red" }} id="home-rt-dot"></span>
                                <span id="home-rt-label">{move || if device_online.get() { "Online" } else { "Offline" }}</span>
                            </div>
                        </div>

                        <div class="hud-layout">
                            <div class="hud-stack">
                                <div class="panel panel-pad">
                                    <div class="panel-head">
                                        <h2 class="panel-title-muted">"Vehicle Damage"</h2>
                                        <div style="display:flex;align-items:center;gap:8px;">
                                            <span class={move || dot_cls.get()} id="dmg-status-dot"></span>
                                            <span id="dmg-status-text" style="font-size:10px;color:var(--gray-500);">{move || dot_txt.get()}</span>
                                        </div>
                                    </div>
                                    <div class="car-hud-wrap" node_ref=hud></div>
                                </div>

                                <div class="panel panel-pad" class:stale=move || env_stale.get()>
                                    <div class="panel-head">
                                        <h2 class="panel-title-muted">"Gas response"</h2>
                                        <div style="display:flex;align-items:center;gap:8px;">
                                            <span class={move || env_ui::level_dot(&env_msg.get(), env_stale.get())}></span>
                                            <span style="font-size:10px;color:var(--gray-500);">{move || env_ui::level_text(&env_msg.get(), env_stale.get())}</span>
                                        </div>
                                    </div>
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
                                    <div class="env-note">"Relative MQ cell response (R0/Rs), 1.00 = clean air. Indicative trend, not a gas concentration."</div>
                                </div>
                            </div>

                            <div class="hud-right-stack">
                                <div class="panel panel-pad">
                                    <div class="panel-head">
                                        <h2 class="panel-title-muted">"Camera"</h2>
                                        <div class="tab-group" id="cam-tabs">
                                            <button class={move || if cam.get() == 0 { "tab-btn active" } else { "tab-btn" }} data-cam="normal" on:click=move |_| on_cam(0)><span>"Normal"</span></button>
                                            <button class={move || if cam.get() == 1 { "tab-btn active" } else { "tab-btn" }} data-cam="lidar" on:click=move |_| on_cam(1)><span>"LiDAR"</span></button>
                                        </div>
                                    </div>
                                    <div class="camera-viewport" id="camera-viewport" node_ref=viewport>
                                        <div class="camera-placeholder" id="cam-placeholder" node_ref=placeholder>
                                            <div class="cam-dot-wrap">
                                                <span class="cam-dot"></span>
                                            </div>
                                            <span style="font-size:13px;font-weight:600;color:var(--gray-400);">"Live Feed"</span>
                                            <span class="cam-label" id="cam-label" node_ref=cam_label>"Waiting for camera stream..."</span>
                                        </div>
                                    </div>
                                </div>

                                <div class="panel panel-pad">
                                    <div class="panel-head">
                                        <h2 class="panel-title-muted">"Arm Movement"</h2>
                                        <div class="wip-badge">"WIP"</div>
                                    </div>
                                    <div class="arm-visual">
                                        <div class="arm-placeholder">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" style="color:var(--gray-600);">
                                                <path d="M18 5a4 4 0 0 0-4 4v12"/><path d="M9 5v16"/><path d="M3 5v12a4 4 0 0 0 4 4h12"/>
                                            </svg>
                                            <p class="arm-label">"3D arm visualization coming soon"</p>
                                            <p style="font-size:9px;color:var(--gray-600);margin-top:4px;">"Will display real-time joint angles"</p>
                                        </div>
                                    </div>
                                </div>

                            </div>
                        </div>

                    </div>
                </main>
            </div>
        })}
    }
}
