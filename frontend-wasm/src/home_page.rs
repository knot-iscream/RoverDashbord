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
use crate::ws::{
    camera_url, fetch_device_online, get_json, non_reactive, now_ms, post_json, ws_url,
};

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
    let _nz = non_reactive();
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

#[derive(Clone, Copy)]
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
    /// Latest RPLIDAR A1 revolution and its freshness. Null until the first
    /// full turn, same rule as the other feeds.
    lidar_msg: RwSignal<serde_json::Value>,
    lidar_seen: RwSignal<f64>,
    lidar_stale: RwSignal<bool>,
    /// ROS-built 3D cloud from `rover/lidar/cloud`, points in metres.
    lidar_cloud: RwSignal<serde_json::Value>,
    cloud_seen: RwSignal<f64>,
    cloud_stale: RwSignal<bool>,
    /// Manually stacked rings for the 3D view: (pitch_deg, points).
    lidar_rings: RwSignal<Vec<(f64, Vec<[f64; 3]>)>>,
    /// Merged recent turns by 1 degree angle bin, so a still sensor fills in
    /// its gaps instead of showing one sparse revolution.
    lidar_acc: RwSignal<Vec<[f64; 3]>>,
    lidar_view: RwSignal<u8>,
    lidar_pitch: RwSignal<f64>,
    lidar_wire: RwSignal<bool>,
    lidar_yaw: RwSignal<f64>,
    lidar_el: RwSignal<f64>,
    lidar_zoom: RwSignal<f64>,
    lidar_drag: RwSignal<Option<(f64, f64)>>,
    lidar_full: RwSignal<bool>,
    hud: NodeRef<leptos::html::Div>,
    lidar_canvas: NodeRef<leptos::html::Canvas>,
    lidar_canvas_big: NodeRef<leptos::html::Canvas>,
}

fn repaint(ctx: &HomeCtx) {
    let _nz = non_reactive();
    paint_hud(
        ctx.hud,
        &ctx.motors.get(),
        ctx.dot_cls,
        ctx.dot_txt,
        ctx.queue,
    );
}

/// Repaint both the card canvas and the expanded overlay canvas, if mounted.
fn paint_lidar(ctx: &HomeCtx) {
    if let Some(canvas) = ctx.lidar_canvas.get() {
        paint_lidar_canvas(ctx, &canvas);
    }
    if let Some(canvas) = ctx.lidar_canvas_big.get() {
        paint_lidar_canvas(ctx, &canvas);
    }
}

/// 2D polar plot of the latest A1 revolution. Imperative like the HUD: the
/// canvas is touched only when a scan arrives, not on every reactive tick.
fn paint_lidar_canvas(ctx: &HomeCtx, canvas: &web_sys::HtmlCanvasElement) {
    let _nz = non_reactive();
    let Ok(Some(rc)) = canvas.get_context("2d") else {
        return;
    };
    let Some(g) = rc.dyn_into::<web_sys::CanvasRenderingContext2d>().ok() else {
        return;
    };
    let w = canvas.width() as f64;
    let h = canvas.height() as f64;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let rmax = (h / 2.0 - 14.0).min(w / 2.0 - 14.0);
    g.clear_rect(0.0, 0.0, w, h);
    if ctx.lidar_view.get() == 1 {
        paint_lidar_3d(ctx, &g, cx, cy, rmax);
        return;
    }
    g.set_line_width(1.0);
    g.set_stroke_style_str("rgba(255,255,255,0.10)");
    for i in 1..=4 {
        g.begin_path();
        g.arc(
            cx,
            cy,
            rmax * i as f64 / 4.0,
            0.0,
            std::f64::consts::PI * 2.0,
        )
        .ok();
        g.stroke();
    }
    for a in 0..12 {
        let t = a as f64 * std::f64::consts::PI / 6.0;
        g.begin_path();
        g.move_to(cx, cy);
        g.line_to(cx + t.cos() * rmax, cy + t.sin() * rmax);
        g.stroke();
    }
    let acc = ctx.lidar_acc.get();
    let dmax = acc
        .iter()
        .fold(2000.0f64, |m, p| m.max(p[1]))
        .clamp(500.0, 6000.0);
    for p in &acc {
        if p[1] <= 0.0 {
            continue;
        }
        let rr = (p[1] / dmax).min(1.0) * rmax;
        let t = p[0].to_radians() - std::f64::consts::PI / 2.0;
        let alpha = (0.30 + (p[2] / 15.0).clamp(0.0, 1.0) * 0.70).min(1.0);
        g.set_fill_style_str(&format!("rgba(249,115,22,{alpha:.2})"));
        g.begin_path();
        g.arc(
            cx + t.cos() * rr,
            cy + t.sin() * rr,
            2.2,
            0.0,
            std::f64::consts::PI * 2.0,
        )
        .ok();
        g.fill();
    }
    g.set_fill_style_str("rgba(255,255,255,0.65)");
    g.set_font("11px sans-serif");
    let label = if ctx.lidar_stale.get() || acc.is_empty() {
        "NO SCAN - waiting for A1".to_string()
    } else {
        format!(
            "A1  {} pts accumulated   scale {:.1} m",
            acc.len(),
            dmax / 1000.0
        )
    };
    g.fill_text(&label, 10.0, h - 10.0).ok();
}

/// Merge a fresh turn into the 2D accumulation, binned by whole degree so
/// successive scans fill gaps instead of stacking duplicates on one angle.
fn accumulate_lidar(ctx: &HomeCtx, frame: &serde_json::Value) {
    let Some(ps) = frame.get("points").and_then(|p| p.as_array()) else {
        return;
    };
    ctx.lidar_acc.update(|acc| {
        for p in ps {
            let (Some(a), Some(d)) = (
                p.get(0).and_then(|x| x.as_f64()),
                p.get(1).and_then(|x| x.as_f64()),
            ) else {
                continue;
            };
            if d <= 0.0 {
                continue;
            }
            let q = p.get(2).and_then(|x| x.as_f64()).unwrap_or(15.0);
            let bin = (a * 4.0).round() as i64 % 1440;
            if let Some(old) = acc
                .iter_mut()
                .find(|e| ((e[0] * 4.0).round() as i64 % 1440) == bin)
            {
                if q >= old[2] {
                    *old = [a, d, q];
                }
            } else {
                acc.push([a, d, q]);
            }
        }
        if acc.len() > 3000 {
            acc.drain(0..acc.len() - 3000);
        }
    });
}

/// Orbit 3D view of stacked rings. Drag to rotate, wheel to zoom, depth-sorted
/// points and optional wireframe. No angle is invented: the A1 gives yaw, the
/// hand tilt gives pitch, and the floor grid gives orientation.
fn paint_lidar_3d(
    ctx: &HomeCtx,
    g: &web_sys::CanvasRenderingContext2d,
    cx: f64,
    cy: f64,
    rmax: f64,
) {
    let rings = ctx.lidar_rings.get();
    let cloud = ctx.lidar_cloud.get();
    let cloud_pts = cloud.get("points").and_then(|p| p.as_array());
    let ring_count: usize = rings.iter().map(|(_, p)| p.len()).sum();
    let cloud_count = cloud_pts.map_or(0, |p| p.len());
    let yaw = ctx.lidar_yaw.get();
    let el = ctx.lidar_el.get();
    let zoom = ctx.lidar_zoom.get();
    let wire = ctx.lidar_wire.get();
    if ring_count == 0 && cloud_count == 0 {
        g.set_fill_style_str("rgba(255,255,255,0.65)");
        g.fill_text(
            "3D - waiting for ROS cloud or capture a ring at each tilt",
            10.0,
            cy + rmax * 0.55 + 16.0,
        )
        .ok();
        return;
    }
    let dmax = rings
        .iter()
        .flat_map(|(_, pts)| pts.iter().map(|p| p[1]))
        .chain(cloud_pts.into_iter().flatten().filter_map(|p| {
            p.get(0)
                .and_then(|v| v.as_f64())
                .map(|v| (v * 1000.0).abs())
        }))
        .fold(1000.0f64, f64::max)
        .clamp(500.0, 6000.0);
    let scale = rmax / dmax;
    let proj = |x: f64, y: f64, z: f64| -> (f64, f64, f64) {
        let (cyaw, syaw) = (yaw.cos(), yaw.sin());
        let x1 = x * cyaw - y * syaw;
        let y1 = x * syaw + y * cyaw;
        let (cel, sel) = (el.cos(), el.sin());
        let y2 = y1 * cel - z * sel;
        let z2 = y1 * sel + z * cel;
        (cx + x1 * scale * zoom, cy - z2 * scale * zoom, y2)
    };
    // Floor grid on the z = 0 plane.
    g.set_line_width(1.0);
    g.set_stroke_style_str("rgba(255,255,255,0.09)");
    for i in -4..=4 {
        let v = i as f64 * dmax / 4.0;
        let (x1, y1, _) = proj(-dmax, v, 0.0);
        let (x2, y2, _) = proj(dmax, v, 0.0);
        g.begin_path();
        g.move_to(x1, y1);
        g.line_to(x2, y2);
        g.stroke();
        let (x1, y1, _) = proj(v, -dmax, 0.0);
        let (x2, y2, _) = proj(v, dmax, 0.0);
        g.begin_path();
        g.move_to(x1, y1);
        g.line_to(x2, y2);
        g.stroke();
    }
    // Project every point, world coordinates: x from the A1 angle, y from the
    // tilt plane, z as height.
    let mut points: Vec<(f64, f64, f64, f64)> = Vec::new();
    for (pitch, pts) in &rings {
        let pr = pitch.to_radians();
        let (cp, sp) = (pr.cos(), pr.sin());
        for p in pts {
            if p[1] <= 0.0 {
                continue;
            }
            let a = p[0].to_radians();
            let (x, y_plane) = (p[1] * a.cos(), p[1] * a.sin());
            let (sx, sy, depth) = proj(x, y_plane * cp, y_plane * sp);
            points.push((depth, sx, sy, p[2]));
        }
    }
    // ROS cloud is already in 3D metres; convert to mm for the shared scale.
    if let Some(ps) = cloud_pts {
        for p in ps {
            let (Some(x), Some(y), Some(z)) = (
                p.get(0).and_then(|v| v.as_f64()),
                p.get(1).and_then(|v| v.as_f64()),
                p.get(2).and_then(|v| v.as_f64()),
            ) else {
                continue;
            };
            let (sx, sy, depth) = proj(x * 1000.0, y * 1000.0, z * 1000.0);
            points.push((depth, sx, sy, 15.0));
        }
    }
    // Wireframe first, behind the points. Adjacent samples in a ring are
    // joined only within a small angular and range gap, so doorways and
    // occlusions do not get false walls drawn across them.
    if wire {
        g.set_line_width(1.0);
        for (pitch, pts) in &rings {
            let pr = pitch.to_radians();
            let (cp, sp) = (pr.cos(), pr.sin());
            let mut sorted = pts.clone();
            sorted.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
            let mut prev: Option<[f64; 3]> = None;
            for p in sorted {
                if p[1] <= 0.0 {
                    continue;
                }
                let a = p[0].to_radians();
                if let Some(o) = prev {
                    let da = (p[0] - o[0]).abs();
                    let dr = (p[1] - o[1]).abs();
                    if da < 4.0 && dr < 1500.0 {
                        let oa = o[0].to_radians();
                        let op = proj(o[1] * oa.cos(), o[1] * oa.sin() * cp, o[1] * oa.sin() * sp);
                        let np = proj(p[1] * a.cos(), p[1] * a.sin() * cp, p[1] * a.sin() * sp);
                        g.set_stroke_style_str("rgba(249,115,22,0.28)");
                        g.begin_path();
                        g.move_to(op.0, op.1);
                        g.line_to(np.0, np.1);
                        g.stroke();
                    }
                }
                prev = Some(p);
            }
        }
    }
    // Painter's algorithm: far points first.
    points.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (depth, sx, sy, q) in points {
        let near = (-depth / dmax).clamp(0.0, 1.0);
        let light = 42.0 + near * 22.0;
        let alpha = (0.45 + (q / 15.0).clamp(0.0, 1.0) * 0.55).min(1.0);
        g.set_fill_style_str(&format!("hsla(24,95%,{light:.0}%,{alpha:.2})"));
        g.begin_path();
        g.arc(sx, sy, 2.6, 0.0, std::f64::consts::PI * 2.0).ok();
        g.fill();
    }
    g.set_fill_style_str("rgba(255,255,255,0.65)");
    let label = if cloud_count > 0 {
        if ctx.cloud_stale.get() {
            format!(
                "ROS cloud offline - {} pts (stale) - drag orbit",
                cloud_count
            )
        } else {
            format!("ROS cloud - {} pts - drag orbit / wheel zoom", cloud_count)
        }
    } else {
        format!(
            "3D room - {} rings / {} pts - tilt {:+.0}..{:+.0} - drag orbit",
            rings.len(),
            ring_count,
            rings.first().map_or(0.0, |r| r.0),
            rings.last().map_or(0.0, |r| r.0)
        )
    };
    g.fill_text(&label, 10.0, cy + rmax * 0.55 + 16.0).ok();
}

/// Home WS loop: motor_update feeds live motors (latching each one),
/// device_status drives the corner badge. No snapshot replay (vanilla home
/// never had WS at all — only fresh frames drive the HUD).
fn connect_home_ws(ctx: HomeCtx) {
    let url = ws_url();
    let Ok(socket) = web_sys::WebSocket::new(&url) else {
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
                "lidar_update" => {
                    ctx.lidar_msg.set(v.clone());
                    ctx.lidar_seen.set(now_ms());
                    ctx.lidar_stale.set(false);
                    accumulate_lidar(&ctx, &v);
                    paint_lidar(&ctx);
                }
                "lidar_cloud_update" => {
                    ctx.lidar_cloud.set(v.clone());
                    ctx.cloud_seen.set(now_ms());
                    ctx.cloud_stale.set(false);
                    paint_lidar(&ctx);
                }
                _ => {}
            }
        }
    });
    socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let onclose = Closure::<dyn FnMut(_)>::new({
        move |_: web_sys::CloseEvent| {
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
    let cam_live = RwSignal::new(false);
    let env_msg = RwSignal::new(serde_json::Value::Null);
    let env_seen = RwSignal::new(0.0f64);
    let env_stale = RwSignal::new(true);
    let lidar_msg = RwSignal::new(serde_json::Value::Null);
    let lidar_seen = RwSignal::new(0.0f64);
    let lidar_stale = RwSignal::new(true);
    let lidar_cloud = RwSignal::new(serde_json::Value::Null);
    let cloud_seen = RwSignal::new(0.0f64);
    let cloud_stale = RwSignal::new(true);
    let lidar_rings = RwSignal::new(Vec::new());
    let lidar_acc = RwSignal::new(Vec::new());
    let lidar_view = RwSignal::new(1u8);
    let lidar_pitch = RwSignal::new(0.0f64);
    let lidar_wire = RwSignal::new(true);
    let lidar_yaw = RwSignal::new(0.0f64);
    let lidar_el = RwSignal::new(0.35f64);
    let lidar_zoom = RwSignal::new(1.0f64);
    let lidar_drag = RwSignal::new(None);
    let lidar_full = RwSignal::new(false);

    let hud = NodeRef::<leptos::html::Div>::new();
    let lidar_canvas = NodeRef::<leptos::html::Canvas>::new();
    let lidar_canvas_big = NodeRef::<leptos::html::Canvas>::new();
    let viewport = NodeRef::<leptos::html::Div>::new();
    let placeholder = NodeRef::<leptos::html::Div>::new();
    let cam_label = NodeRef::<leptos::html::Span>::new();
    let cam_img = NodeRef::<leptos::html::Img>::new();

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
        lidar_msg,
        lidar_seen,
        lidar_stale,
        lidar_cloud,
        cloud_seen,
        cloud_stale,
        lidar_rings,
        lidar_acc,
        lidar_view,
        lidar_pitch,
        lidar_wire,
        lidar_yaw,
        lidar_el,
        lidar_zoom,
        lidar_drag,
        lidar_full,
        hud,
        lidar_canvas,
        lidar_canvas_big,
    };

    // -- setup: WS, polls, sim fallback, overlay ticker (refs post-mount) --
    {
        connect_home_ws(ctx);
        wasm_bindgen_futures::spawn_local(async move {
            ctx.device_online.set(fetch_device_online().await);
        });
        // ENV fetch folds `GET /api/env` into the flat `env_update` shape the
        // WS feeds, so the panel is populated even between frames.
        let c = ctx;
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(v) = get_json("/api/env").await {
                let frame = env_ui::fold_api_env(&v);
                if !frame.is_null() {
                    c.env_msg.set(frame);
                    c.env_seen.set(now_ms());
                }
            }
        });
        // Same for the A1 scan: a page opened between revolutions still shows
        // the last full turn instead of an empty canvas.
        let c = ctx;
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(v) = get_json("/api/lidar").await {
                if let Some(frame) = v.get("lidar") {
                    if !frame.is_null() {
                        c.lidar_msg.set(frame.clone());
                        c.lidar_seen.set(now_ms());
                        accumulate_lidar(&c, frame);
                        paint_lidar(&c);
                    }
                }
            }
        });
        // And the ROS point cloud, so the 3D view is populated on page load.
        let c = ctx;
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(v) = get_json("/api/lidar/cloud").await {
                if let Some(frame) = v.get("cloud") {
                    if !frame.is_null() {
                        c.lidar_cloud.set(frame.clone());
                        c.cloud_seen.set(now_ms());
                        c.cloud_stale.set(false);
                        paint_lidar(&c);
                    }
                }
            }
        });
        wasm_bindgen_futures::spawn_local(async move {
            let _nz = non_reactive();
            // Inject the static HUD, then paint the initial frame.
            if let Some(wrap) = ctx.hud.get() {
                wrap.set_inner_html(HUD_SVG);
            }
            repaint(&ctx);
            // Device poll every 3s.
            let c = ctx;
            let _iv = gloo_timers::callback::Interval::new(3000, move || {
                wasm_bindgen_futures::spawn_local(async move {
                    c.device_online.set(fetch_device_online().await);
                });
            });
            _iv.forget();
            // Sim fallback every 600ms: only motors with no live sample yet.
            let c = ctx;
            let _iv = gloo_timers::callback::Interval::new(600, move || {
                let _nz = non_reactive();
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
            let c = ctx;
            let _iv = gloo_timers::callback::Interval::new(2000, move || {
                let _nz = non_reactive();
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
            let c = ctx;
            let _iv = gloo_timers::callback::Interval::new(1000, move || {
                let _nz = non_reactive();
                let seen = c.env_seen.get();
                c.env_stale
                    .set(seen == 0.0 || now_ms() - seen > env_ui::FRESH_MS);
            });
            _iv.forget();
            // A1 scan is stale after 5 s (sensor turns ~5.5 times per second).
            let c = ctx;
            let _iv = gloo_timers::callback::Interval::new(1000, move || {
                let seen = c.lidar_seen.get();
                c.lidar_stale.set(seen == 0.0 || now_ms() - seen > 5000.0);
                let cseen = c.cloud_seen.get();
                c.cloud_stale.set(cseen == 0.0 || now_ms() - cseen > 5000.0);
                paint_lidar(&c);
            });
            _iv.forget();
        });
    }

    let push_tilt = move |angle: f64| {
        wasm_bindgen_futures::spawn_local(async move {
            let _ = post_json("/api/lidar/tilt", serde_json::json!({ "angle": angle })).await;
        });
    };
    let on_lidar_view = move |v: u8| {
        ctx.lidar_view.set(v);
        paint_lidar(&ctx);
    };
    let on_lidar_tilt = move |d: f64| {
        let next = (ctx.lidar_pitch.get() + d).clamp(-90.0, 90.0);
        ctx.lidar_pitch.set(next);
        push_tilt(next);
    };
    let on_lidar_capture = move |_| {
        let pts: Vec<[f64; 3]> = ctx
            .lidar_acc
            .get()
            .iter()
            .copied()
            .filter(|p| p[1] > 0.0)
            .collect();
        if pts.is_empty() {
            return;
        }
        let pitch = ctx.lidar_pitch.get();
        push_tilt(pitch);
        ctx.lidar_rings.update(|rings| {
            if let Some(ring) = rings.iter_mut().find(|(p, _)| (*p - pitch).abs() <= 2.0) {
                // Same tilt pressed again: densify that ring instead of
                // stacking a duplicate at a slightly different angle.
                for np in pts {
                    if !ring
                        .1
                        .iter()
                        .any(|e| (e[0] - np[0]).abs() < 1.0 && (e[1] - np[1]).abs() < 80.0)
                    {
                        ring.1.push(np);
                    }
                }
            } else {
                rings.push((pitch, pts));
            }
        });
        paint_lidar(&ctx);
    };
    let on_lidar_clear = move |_| {
        ctx.lidar_rings.set(Vec::new());
        ctx.lidar_acc.set(Vec::new());
        ctx.lidar_cloud.set(serde_json::Value::Null);
        ctx.cloud_seen.set(0.0);
        ctx.cloud_stale.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let _ = post_json("/api/lidar/cloud/clear", serde_json::json!({})).await;
        });
        paint_lidar(&ctx);
    };
    let on_lidar_expand = move |_| {
        ctx.lidar_full.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(60).await;
            paint_lidar(&ctx);
        });
    };
    let on_lidar_close = move |_| {
        ctx.lidar_full.set(false);
    };
    let on_lidar_wire = move |_| {
        ctx.lidar_wire.update(|w| *w = !*w);
        paint_lidar(&ctx);
    };
    let on_lidar_reset_view = move |_| {
        ctx.lidar_yaw.set(0.0);
        ctx.lidar_el.set(0.35);
        ctx.lidar_zoom.set(1.0);
        paint_lidar(&ctx);
    };
    let on_pd = move |ev: web_sys::PointerEvent| {
        ctx.lidar_drag
            .set(Some((ev.client_x() as f64, ev.client_y() as f64)));
    };
    let on_pm = move |ev: web_sys::PointerEvent| {
        if let Some((px, py)) = ctx.lidar_drag.get() {
            let dx = ev.client_x() as f64 - px;
            let dy = ev.client_y() as f64 - py;
            ctx.lidar_yaw.update(|y| *y += dx * 0.01);
            ctx.lidar_el
                .update(|e| *e = (*e - dy * 0.01).clamp(-1.2, 1.2));
            ctx.lidar_drag
                .set(Some((ev.client_x() as f64, ev.client_y() as f64)));
            paint_lidar(&ctx);
        }
    };
    let on_pu = move |_: web_sys::PointerEvent| ctx.lidar_drag.set(None);
    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        let factor = if ev.delta_y() > 0.0 { 0.9 } else { 1.1 };
        ctx.lidar_zoom
            .update(|z| *z = (*z * factor).clamp(0.25, 4.0));
        paint_lidar(&ctx);
    };
    let c = ctx;
    let paint_cam = move || {
        let _nz = non_reactive();
        let mode = cam.get();
        let live = cam_live.get();
        if let Some(img) = cam_img.get() {
            img.set_attribute(
                "style",
                if mode == 0 && live {
                    "display:block"
                } else {
                    "display:none"
                },
            )
            .ok();
        }
        if let Some(cv) = lidar_canvas.get() {
            cv.set_attribute(
                "style",
                if mode == 1 {
                    "display:block"
                } else {
                    "display:none"
                },
            )
            .ok();
        }
        if mode == 1 {
            paint_lidar(&c);
        }
        if let Some(p) = placeholder.get() {
            p.set_attribute(
                "style",
                if mode == 1 {
                    "display:none;"
                } else if !live {
                    "opacity:1;"
                } else {
                    "opacity:0.25;"
                },
            )
            .ok();
        }
        if let Some(l) = cam_label.get() {
            let text = if mode == 1 {
                let pts = c
                    .lidar_msg
                    .get()
                    .get("points")
                    .and_then(|p| p.as_array())
                    .map_or(0, |p| p.len());
                if c.lidar_stale.get() {
                    "A1 scan offline - waiting for 360 turn".to_string()
                } else {
                    format!("A1 live - {pts} points per turn")
                }
            } else if live {
                "Live feed connected".to_string()
            } else {
                "Waiting for camera stream...".to_string()
            };
            l.set_text_content(Some(&text));
        }
    };
    let on_cam = move |mode: u8| {
        cam.set(mode);
        if mode == 1 && ctx.lidar_view.get() == 0 {
            ctx.lidar_view.set(1);
        }
        paint_cam();
    };
    let on_cam_load = move |_: web_sys::Event| {
        cam_live.set(true);
        paint_cam();
    };
    let on_cam_error = move |_: web_sys::ErrorEvent| {
        cam_live.set(false);
        paint_cam();
    };
    // The Pi serves MJPEG on the broker host; /api/config tells us which host
    // that is, so no address is hardcoded for home vs the Saturday hotspot.
    {
        wasm_bindgen_futures::spawn_local(async move {
            let url = camera_url().await;
            if let Some(img) = cam_img.get() {
                img.set_attribute("src", &url).ok();
            }
        });
    }

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
                                        <div class="panel-title-row">
                                            <h2 class="panel-title-muted">"Gas response"</h2>
                                            <span class="env-info" tabindex="0">"i"
                                                <span class="env-tip">"Relative MQ cell response (R0/Rs), 1.00 = clean air. Indicative trend, not a gas concentration."</span>
                                            </span>
                                        </div>
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
                                                    {(0..env_ui::GAS_SEGMENTS).map(|s| view! {
                                                        <i class=move || env_ui::gas_seg_class(&env_msg.get(), key, s)></i>
                                                    }).collect::<Vec<_>>()}
                                                </div>
                                                <span class="env-gas-val" style=move || format!("color:{}", env_ui::gas_color(&env_msg.get(), key))>
                                                    {move || env_ui::gas_text(&env_msg.get(), key)}
                                                </span>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
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
                                        <img
                                            class="cam-feed"
                                            alt="Rover camera"
                                            style="display:none"
                                            node_ref=cam_img
                                            on:load=on_cam_load
                                            on:error=on_cam_error
                                        />
                                        <canvas
                                            class="lidar-canvas"
                                            id="lidar-canvas"
                                            width="600"
                                            height="450"
                                            style="display:none"
                                            node_ref=lidar_canvas
                                            on:pointerdown=on_pd
                                            on:pointermove=on_pm
                                            on:pointerup=on_pu
                                            on:pointerleave=on_pu
                                            on:wheel=on_wheel
                                        ></canvas>
                                        <div class="lidar-controls" style=move || if cam.get() == 1 { "display:flex" } else { "display:none" }>
                                            <button class="lidar-btn" on:click=move |_| on_lidar_view(0)>"2D"</button>
                                            <button class="lidar-btn" on:click=move |_| on_lidar_view(1)>"3D"</button>
                                            <button class="lidar-btn" on:click=on_lidar_wire>"Wire"</button>
                                            <button class="lidar-btn" on:click=move |_| on_lidar_tilt(-5.0)>"-"</button>
                                            <span class="lidar-pitch">{move || format!("{:+.0}", ctx.lidar_pitch.get())}</span>
                                            <button class="lidar-btn" on:click=move |_| on_lidar_tilt(5.0)>"+"</button>
                                            <button class="lidar-btn" on:click=on_lidar_capture>"Capture"</button>
                                            <button class="lidar-btn" on:click=on_lidar_clear>"Clear"</button>
                                            <button class="lidar-btn" on:click=on_lidar_reset_view>"Reset"</button>
                                            <button class="lidar-btn" on:click=on_lidar_expand>"Expand"</button>
                                        </div>
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
        <div class="lidar-modal" style=move || if ctx.lidar_full.get() { "display:flex" } else { "display:none" }>
            <div class="lidar-modal-card">
                <div class="lidar-modal-head">
                    <div>
                        <h2 class="lidar-modal-title">"LiDAR 3D view"</h2>
                        <span class="lidar-modal-sub">"Tilt by hand, set the angle, wait for the ring, repeat"</span>
                    </div>
                    <button class="lidar-btn" on:click=on_lidar_close>"Close"</button>
                </div>
                <div class="lidar-modal-view">
                    <canvas
                        class="lidar-canvas-big"
                        width="1400"
                        height="1000"
                        node_ref=lidar_canvas_big
                        on:pointerdown=on_pd
                        on:pointermove=on_pm
                        on:pointerup=on_pu
                        on:pointerleave=on_pu
                        on:wheel=on_wheel
                    ></canvas>
                </div>
                <div class="lidar-controls lidar-controls-modal">
                    <button class="lidar-btn" on:click=move |_| on_lidar_view(0)>"2D"</button>
                    <button class="lidar-btn" on:click=move |_| on_lidar_view(1)>"3D"</button>
                    <button class="lidar-btn" on:click=on_lidar_wire>"Wire"</button>
                    <button class="lidar-btn" on:click=move |_| on_lidar_tilt(-5.0)>"-"</button>
                    <span class="lidar-pitch">{move || format!("{:+.0}", ctx.lidar_pitch.get())}</span>
                    <button class="lidar-btn" on:click=move |_| on_lidar_tilt(5.0)>"+"</button>
                    <button class="lidar-btn" on:click=on_lidar_capture>"Capture"</button>
                    <button class="lidar-btn" on:click=on_lidar_clear>"Clear"</button>
                    <button class="lidar-btn" on:click=on_lidar_reset_view>"Reset"</button>
                    <button class="lidar-btn" on:click=on_lidar_close>"Close"</button>
                </div>
            </div>
        </div>
    }
}
