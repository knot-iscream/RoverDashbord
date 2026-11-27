//! Rover Control page (`/control`) — racing-style cockpit HUD with a live
//! camera mirror, tank drive pad, per-motor jog and a prominent STOP.
//!
//! The visual language borrows from racing HUDs: a gear/speed cluster, a tach
//! with a redline, throttle and brake bars, a steering indicator, a delta
//! readout that shows whether the wheels actually followed the command, and a
//! virtual mirror fed by the Pi camera. Styling stays Reanime: orange accent,
//! skewed plates, corner cuts and tracked uppercase labels.
//!
//! All commands still go through `POST /api/motor/control` via `post_motor`.
//! No backend route, MQTT topic, payload shape or firmware change. The
//! per-motor FLIP exists because the H-bridge truth table is only confirmed
//! on the bench, so a reversed wheel is corrected without a reflash.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Event, HtmlInputElement, WebSocket};

use crate::perception_ui::DetectionOverlay;
use crate::shell::app_shell;
use crate::ws::{
    camera_age_ms, camera_url, fetch_device_online, non_reactive, now_ms, post_autonomy,
    post_motor, post_pump, ws_url,
};

const SHORT: [&str; 6] = ["FL", "FR", "RL", "RR", "ML", "MR"];
const FULL: [&str; 6] = [
    "Front Left",
    "Front Right",
    "Rear Left",
    "Rear Right",
    "Middle Left",
    "Middle Right",
];
const LEFT_IDX: [usize; 3] = [0, 2, 4];
const RIGHT_IDX: [usize; 3] = [1, 3, 5];

// Keys use the low nibble, on-screen pad buttons the high nibble.
const K_FWD: u8 = 1;
const K_BACK: u8 = 2;
const K_LEFT: u8 = 4;
const K_RIGHT: u8 = 8;
const P_FWD: u8 = 16;
const P_BACK: u8 = 32;
const P_LEFT: u8 = 64;
const P_RIGHT: u8 = 128;
const FRESH_MS: f64 = 5000.0;
const TACH_SEGS: usize = 30;

#[derive(Clone, Copy, Default)]
struct Sample {
    speed: Option<i64>,
}

#[derive(Clone, Copy)]
struct Ctx {
    keys: RwSignal<u8>,
    pad: RwSignal<u8>,
    cmd: RwSignal<[i32; 6]>,
    invert: RwSignal<[bool; 6]>,
    actual: RwSignal<[Option<Sample>; 6]>,
    last_rx: RwSignal<[Option<f64>; 6]>,
    online: RwSignal<bool>,
    speed: RwSignal<i32>,
    drive_slider: NodeRef<leptos::html::Input>,
    jog_speeds: RwSignal<[i32; 6]>,
    cam_live: RwSignal<bool>,
    pump: RwSignal<bool>,
    tick: RwSignal<u64>,
    /// Latest `rover/perception/detections` frame.
    perception: RwSignal<serde_json::Value>,
    /// Latest `rover/autonomy/state`, Null when the autopilot is not running.
    auto: RwSignal<serde_json::Value>,
    auto_seen: RwSignal<f64>,
}

fn dir_of(c: &Ctx) -> u8 {
    c.keys.get() | (c.pad.get() >> 4)
}

fn gear(dir: u8) -> &'static str {
    match (dir & K_FWD != 0, dir & K_BACK != 0) {
        (true, false) => "F",
        (false, true) => "R",
        _ => "N",
    }
}

fn shift_on(dir: u8, i: usize) -> bool {
    match i {
        0 => dir & K_LEFT != 0,
        1 => dir & (K_FWD | K_BACK) != 0,
        _ => dir & K_RIGHT != 0,
    }
}

/// Left bank = FL/RL/ML, right = FR/RR/MR. A turn alone pivots the banks in
/// opposite directions; a turn with forward/back eases the inner bank.
fn drive_base(dir: u8, s: i32) -> [i32; 6] {
    let mut out = [0i32; 6];
    if dir == 0 {
        return out;
    }
    let fwd = dir & K_FWD != 0;
    let back = dir & K_BACK != 0;
    let left = dir & K_LEFT != 0;
    let right = dir & K_RIGHT != 0;
    let lin = match (fwd, back) {
        (true, false) => 1,
        (false, true) => -1,
        _ => 0,
    };
    let turn = match (left, right) {
        (true, false) => 1,
        (false, true) => -1,
        _ => 0,
    };
    let inner = (s as f64 * 0.4).round() as i32;
    let (l, r) = if turn == 0 {
        (lin * s, lin * s)
    } else if lin == 0 {
        (-turn * s, turn * s)
    } else if turn > 0 {
        (lin * inner, lin * s)
    } else {
        (lin * s, lin * inner)
    };
    for i in LEFT_IDX {
        out[i] = l;
    }
    for i in RIGHT_IDX {
        out[i] = r;
    }
    out
}

/// Apply the invert map, remember the commanded values and fire one POST per
/// motor. Fire-and-forget matches the existing test page.
fn send_speeds(c: &Ctx, base: [i32; 6]) {
    // Any real manual input takes the lease before the wheels get a command,
    // so the autopilot cannot keep republishing its own pair over this one.
    let moving = base.iter().any(|v| *v != 0);
    if moving && c.auto.get().get("mode").and_then(|v| v.as_str()) == Some("auto") {
        wasm_bindgen_futures::spawn_local(async {
            post_autonomy("manual").await;
        });
    }
    let inv = c.invert.get();
    let mut out = [0i32; 6];
    for (i, v) in base.iter().enumerate() {
        out[i] = if inv[i] { -*v } else { *v };
    }
    c.cmd.set(out);
    for (i, v) in out.iter().enumerate() {
        let motor = (i + 1) as u8;
        let speed = *v;
        wasm_bindgen_futures::spawn_local(async move {
            post_motor(motor, speed).await;
        });
    }
}

fn apply(c: &Ctx) {
    let dir = dir_of(c);
    let base = if dir == 0 {
        [0i32; 6]
    } else {
        drive_base(dir, c.speed.get())
    };
    send_speeds(c, base);
}

fn stop(c: &Ctx) {
    c.keys.set(0);
    c.pad.set(0);
    send_speeds(c, [0i32; 6]);
    // STOP must also drop the autopilot to idle, otherwise its next 5 Hz
    // republish would drive the rover again immediately.
    wasm_bindgen_futures::spawn_local(async {
        post_autonomy("stop").await;
    });
}

/// One motor only. The drive pad fans out to all six; jog must not disturb a
/// neighbour when you stop or reverse a single channel on the bench.
fn send_one(c: &Ctx, idx: usize, value: i32) {
    let out = if c.invert.get()[idx] { -value } else { value };
    let mut cmd = c.cmd.get();
    cmd[idx] = out;
    c.cmd.set(cmd);
    let motor = (idx + 1) as u8;
    wasm_bindgen_futures::spawn_local(async move {
        post_motor(motor, out).await;
    });
}

fn jog(c: &Ctx, idx: usize, sign: i32) {
    let base = if sign == 0 {
        0
    } else {
        sign * c.jog_speeds.get()[idx]
    };
    send_one(c, idx, base);
}

fn step_speed(c: &Ctx, idx: usize, by: i32) {
    let mut speeds = c.jog_speeds.get();
    speeds[idx] = (speeds[idx] + by).clamp(20, 255);
    c.jog_speeds.set(speeds);
}

/// Keep the range input in step with the throttle signal. Q/E and the
/// on-screen buttons change the signal without touching the DOM, so without
/// this the thumb stays at its initial position while the label moves.
fn paint_throttle(c: &Ctx) {
    if let Some(slider) = c.drive_slider.get() {
        slider.set_value(&c.speed.get().to_string());
    }
}

/// Throttle trim while driving. Re-applying when a direction is held is what
/// makes Q/E (and the on-screen pair) take effect without lifting a drive key.
/// With nothing held, only the value changes, so trimming cannot start a motor.
fn step_throttle(c: &Ctx, by: i32) {
    let next = (c.speed.get() + by).clamp(20, 255);
    c.speed.set(next);
    paint_throttle(c);
    if dir_of(c) != 0 {
        apply(c);
    }
}

fn pad_down(c: &Ctx, bit: u8) {
    let mut p = c.pad.get();
    p |= bit;
    c.pad.set(p);
    apply(c);
}

fn pad_up(c: &Ctx, bit: u8) {
    let mut p = c.pad.get();
    p &= !bit;
    c.pad.set(p);
    apply(c);
}

fn range_value(ev: Event) -> Option<i32> {
    ev.current_target()?
        .dyn_into::<HtmlInputElement>()
        .ok()?
        .value()
        .parse()
        .ok()
}

fn pct(v: i32) -> i64 {
    (v as f64 / 255.0 * 100.0).round() as i64
}

/// (max commanded duty, mean actual speed). Only commanded motors count, so
/// idle channels cannot dilute the actual readout in the cluster.
fn readout(c: &Ctx) -> (i32, i32) {
    let _ = c.tick.get();
    let now = now_ms();
    let mut max = 0i32;
    let mut seen = 0i32;
    let mut n = 0i32;
    for i in 0..6 {
        let cmd = c.cmd.get()[i];
        max = max.max(cmd.abs());
        if cmd != 0 {
            let fresh = c.last_rx.get()[i].is_some_and(|t| now - t <= FRESH_MS);
            let actual = if fresh {
                c.actual.get()[i].and_then(|s| s.speed).unwrap_or(0)
            } else {
                0
            };
            seen += actual as i32;
            n += 1;
        }
    }
    if n == 0 {
        (0, 0)
    } else {
        (max, seen / n)
    }
}

fn tach_class(i: usize, speed: i32) -> &'static str {
    let lit = (speed as f64 / 255.0 * TACH_SEGS as f64).round() as usize;
    if i >= lit {
        return "ctl-tach-seg";
    }
    match i {
        0..=17 => "ctl-tach-seg lit a",
        18..=23 => "ctl-tach-seg lit b",
        _ => "ctl-tach-seg lit c",
    }
}

fn connect_ws(c: Ctx) {
    let Ok(socket) = WebSocket::new(&ws_url()) else {
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(3000).await;
            connect_ws(c);
        });
        return;
    };
    let onopen = Closure::<dyn FnMut(Event)>::new({
        let s = socket.clone();
        move |_| {
            let _ = s.send_with_str("ping");
        }
    });
    socket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let onmessage =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
            let _nz = non_reactive();
            let Some(text) = e.data().as_string() else {
                return;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                return;
            };
            // Pump state rides the MID frames: status every 2 s, motor data
            // every 250 ms. Trust the board, not the last click: a burst that
            // timed out on the firmware turns the button off by itself.
            let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            if kind == "perception_update" {
                c.perception.set(v.clone());
                return;
            }
            if kind == "autonomy_update" {
                c.auto.set(v.clone());
                c.auto_seen.set(now_ms());
                return;
            }
            if kind == "motor_status" {
                if v.get("source").and_then(|x| x.as_str()) == Some("mid") {
                    if let Some(p) = v.get("pump").and_then(|x| x.as_u64()) {
                        c.pump.set(p != 0);
                    }
                }
                return;
            }
            if kind != "motor_update" {
                return;
            }
            if let Some(p) = v.get("pump").and_then(|x| x.as_u64()) {
                c.pump.set(p != 0);
            }
            let Some(m) = v.get("motor").and_then(|x| x.as_u64()) else {
                return;
            };
            if !(1..=6).contains(&m) {
                return;
            }
            let sample = Sample {
                speed: v.get("speed").and_then(|x| x.as_i64()),
            };
            let idx = (m - 1) as usize;
            c.actual.update(|a| a[idx] = Some(sample));
            c.last_rx.update(|a| a[idx] = Some(now_ms()));
        });
    socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let reconnect = move || {
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(3000).await;
            connect_ws(c);
        });
    };
    let onclose = Closure::<dyn FnMut(Event)>::new(move |_| reconnect());
    socket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();
    let onerror = Closure::<dyn FnMut(Event)>::new(move |_| reconnect());
    socket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();
}

#[component]
pub fn ControlPage() -> impl IntoView {
    let c = Ctx {
        keys: RwSignal::new(0),
        pad: RwSignal::new(0),
        cmd: RwSignal::new([0i32; 6]),
        invert: RwSignal::new([false; 6]),
        actual: RwSignal::new([None; 6]),
        last_rx: RwSignal::new([None; 6]),
        online: RwSignal::new(false),
        speed: RwSignal::new(128),
        drive_slider: NodeRef::<leptos::html::Input>::new(),
        jog_speeds: RwSignal::new([20i32; 6]),
        cam_live: RwSignal::new(false),
        pump: RwSignal::new(false),
        tick: RwSignal::new(0u64),
        perception: RwSignal::new(serde_json::Value::Null),
        auto: RwSignal::new(serde_json::Value::Null),
        auto_seen: RwSignal::new(0.0f64),
    };
    let cam_img = NodeRef::<leptos::html::Img>::new();

    // Keyboard: held keys drive; Space / Escape stop; blur releases everything.
    {
        wasm_bindgen_futures::spawn_local(async move {
            let Some(win) = web_sys::window() else {
                return;
            };
            let down = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
                move |e: web_sys::KeyboardEvent| {
                    let key = e.key().to_lowercase();
                    // Q/E trim throttle. Repeat is allowed on purpose so a
                    // held key ramps instead of stepping once.
                    if key == "q" || key == "e" {
                        e.prevent_default();
                        step_throttle(&c, if key == "e" { 16 } else { -16 });
                        return;
                    }
                    let bit = match key.as_str() {
                        "w" | "arrowup" => Some(K_FWD),
                        "s" | "arrowdown" => Some(K_BACK),
                        "a" | "arrowleft" => Some(K_LEFT),
                        "d" | "arrowright" => Some(K_RIGHT),
                        " " | "spacebar" | "escape" => None,
                        _ => return,
                    };
                    e.prevent_default();
                    match bit {
                        None => stop(&c),
                        Some(b) => {
                            if e.repeat() {
                                return;
                            }
                            let mut keys = c.keys.get();
                            keys |= b;
                            c.keys.set(keys);
                            apply(&c);
                        }
                    }
                },
            );
            win.add_event_listener_with_callback("keydown", down.as_ref().unchecked_ref())
                .ok();
            down.forget();

            let up = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
                move |e: web_sys::KeyboardEvent| {
                    let bit = match e.key().to_lowercase().as_str() {
                        "w" | "arrowup" => K_FWD,
                        "s" | "arrowdown" => K_BACK,
                        "a" | "arrowleft" => K_LEFT,
                        "d" | "arrowright" => K_RIGHT,
                        _ => return,
                    };
                    let mut keys = c.keys.get();
                    keys &= !bit;
                    c.keys.set(keys);
                    apply(&c);
                },
            );
            win.add_event_listener_with_callback("keyup", up.as_ref().unchecked_ref())
                .ok();
            up.forget();

            let blur = Closure::<dyn FnMut(Event)>::new(move |_| stop(&c));
            win.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref())
                .ok();
            blur.forget();
        });
    }

    connect_ws(c);
    {
        wasm_bindgen_futures::spawn_local(async move {
            c.online.set(fetch_device_online().await);
        });
    }
    {
        let poll = gloo_timers::callback::Interval::new(3000, move || {
            wasm_bindgen_futures::spawn_local(async move {
                c.online.set(fetch_device_online().await);
            });
        });
        poll.forget();
    }
    {
        let clock = gloo_timers::callback::Interval::new(1000, move || {
            c.tick.update(|t| *t += 1);
        });
        clock.forget();
    }
    // Camera URL follows the broker host from /api/config, so it works at
    // home and on the Saturday hotspot without a hardcoded address.
    {
        wasm_bindgen_futures::spawn_local(async move {
            let url = camera_url().await;
            if let Some(img) = cam_img.get() {
                img.set_attribute("src", &url).ok();
            }
            // The MJPEG <img> never reports a dead stream, so poll the service
            // and re-arm the source when frames stop arriving. Without this a
            // camera restart leaves a frozen frame with LIVE lit.
            let status_url = url.replace("/stream.mjpg", "/status");
            let _iv = gloo_timers::callback::Interval::new(2_000, move || {
                let status_url = status_url.clone();
                let base_url = url.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let fresh = camera_age_ms(&status_url)
                        .await
                        .is_some_and(|age| (0..2500).contains(&age));
                    c.cam_live.set(fresh);
                    if !fresh {
                        if let Some(img) = cam_img.get() {
                            let stamp = js_sys::Date::now() as u64;
                            img.set_attribute("src", &format!("{base_url}?r={stamp}"))
                                .ok();
                        }
                    }
                });
            });
            _iv.forget();
        });
    }

    let on_speed = move |ev: Event| {
        if let Some(v) = range_value(ev) {
            c.speed.set(v);
            paint_throttle(&c);
            if dir_of(&c) != 0 {
                apply(&c);
            }
        }
    };
    let on_thr_less = move |_: web_sys::MouseEvent| step_throttle(&c, -16);
    let on_thr_more = move |_: web_sys::MouseEvent| step_throttle(&c, 16);
    let on_stop = move |_: web_sys::MouseEvent| stop(&c);
    let on_take_manual = move |_: web_sys::MouseEvent| {
        wasm_bindgen_futures::spawn_local(async move {
            post_autonomy("manual").await;
        });
    };
    let on_hand_auto = move |_: web_sys::MouseEvent| {
        wasm_bindgen_futures::spawn_local(async move {
            post_autonomy("auto").await;
        });
    };
    let auto_fresh = move || {
        let _ = c.tick.get();
        let seen = c.auto_seen.get();
        seen > 0.0 && now_ms() - seen < 3000.0
    };
    let auto_mode = move || {
        if !auto_fresh() {
            return String::new();
        }
        c.auto
            .get()
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let auto_chip_cls = move || match auto_mode().as_str() {
        "auto" => "ctl-auto-chip auto",
        "manual" => "ctl-auto-chip manual",
        _ => "ctl-auto-chip",
    };
    let auto_chip_txt = move || match auto_mode().as_str() {
        "auto" => "AUTO",
        "manual" => "MANUAL",
        "idle" => "IDLE",
        _ => "OFFLINE",
    };
    let auto_state_line = move || {
        if !auto_fresh() {
            return "Autopilot not running".to_string();
        }
        let v = c.auto.get();
        let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("--");
        let reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("");
        if reason.is_empty() {
            state.to_string()
        } else {
            format!("{state} - {reason}")
        }
    };
    // Water: the click only sends the command. The button follows the ENV
    // frames, so it never claims "running" when the broker is down and it
    // turns itself off when the firmware burst ends.
    let on_pump_fire = move |_: web_sys::MouseEvent| {
        wasm_bindgen_futures::spawn_local(async move {
            post_pump(true, 10).await;
        });
    };
    let on_pump_stop = move |_: web_sys::MouseEvent| {
        wasm_bindgen_futures::spawn_local(async move {
            post_pump(false, 0).await;
        });
    };
    let btn_down = |bit: u8| move |_: web_sys::PointerEvent| pad_down(&c, bit);
    let btn_up = |bit: u8| move |_: web_sys::PointerEvent| pad_up(&c, bit);
    let on_cam_load = move |_: Event| c.cam_live.set(true);
    let on_cam_error = move |_: web_sys::ErrorEvent| {
        c.cam_live.set(false);
        // An MJPEG <img> never reconnects by itself, so a camera restart would
        // leave the mirror offline until the page is reloaded.
        let retry = gloo_timers::callback::Timeout::new(2_000, move || {
            if let Some(img) = cam_img.get() {
                let base = img
                    .get_attribute("src")
                    .unwrap_or_default()
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !base.is_empty() {
                    let stamp = js_sys::Date::now() as u64;
                    img.set_attribute("src", &format!("{base}?r={stamp}")).ok();
                }
            }
        });
        retry.forget();
    };
    let on_jog = |idx: usize| {
        let rev = move |_: web_sys::MouseEvent| jog(&c, idx, -1);
        let stop_one = move |_: web_sys::MouseEvent| jog(&c, idx, 0);
        let fwd = move |_: web_sys::MouseEvent| jog(&c, idx, 1);
        let less = move |_: web_sys::MouseEvent| step_speed(&c, idx, -16);
        let more = move |_: web_sys::MouseEvent| step_speed(&c, idx, 16);
        // Flip only the map. Re-applying the drive state here would stop a
        // motor someone is jogging; the next command picks up the new sign.
        let flip = move |_: web_sys::MouseEvent| {
            let mut inv = c.invert.get();
            inv[idx] = !inv[idx];
            c.invert.set(inv);
        };
        (rev, stop_one, fwd, less, more, flip)
    };

    view! {
        {app_shell(view! {
            <div class="body-wrap">
                <main class="main-content">
                    <div class="content-wrap ctl-wrap">
                        <div class="page-header">
                            <div><h1 class="page-h1">"Rover Control"</h1></div>
                            <div class="live-badge">
                                <span class={move || if c.online.get() { "status-dot dot-green pulse" } else { "status-dot dot-red" }}></span>
                                <span>{move || if c.online.get() { "Device online" } else { "Device offline" }}</span>
                            </div>
                        </div>

                        <div class="ctl-warn" style={move || if c.online.get() { "display:none" } else { "display:block" }}>
                            "No device online. Commands reach the broker but nothing will move until a board is connected."
                        </div>

                        <div class="ctl-auto">
                            <div class="ctl-auto-left">
                                <span class="ctl-auto-title">"AUTONOMY"</span>
                                <span class={move || auto_chip_cls()}>{move || auto_chip_txt()}</span>
                                <span class="ctl-auto-state">{auto_state_line}</span>
                            </div>
                            <div class="ctl-auto-right">
                                <button class="ctl-auto-btn" on:click=on_take_manual>"TAKE MANUAL"</button>
                                <button class="ctl-auto-btn accent" on:click=on_hand_auto>"HAND TO AUTOPILOT"</button>
                            </div>
                        </div>

                        <div class="ctl-cockpit">
                            <div class="ctl-mirror">
                                <div class="ctl-mirror-head">
                                    <span>"ROVER CAM"</span>
                                    <span class={move || if c.cam_live.get() { "ctl-live-chip on" } else { "ctl-live-chip" }}>"LIVE"</span>
                                </div>
                                <div class="ctl-mirror-view">
                                    <img
                                        node_ref=cam_img
                                        class="ctl-mirror-img"
                                        alt="Rover camera"
                                        style={move || if c.cam_live.get() { "display:block" } else { "display:none" }}
                                        on:load=on_cam_load
                                        on:error=on_cam_error
                                    />
                                    <div class="ctl-mirror-off" style={move || if c.cam_live.get() { "display:none" } else { "display:flex" }}>
                                        "CAMERA OFFLINE"
                                    </div>
                                    <DetectionOverlay
                                        payload=c.perception
                                        show=Signal::derive(move || c.cam_live.get())
                                    />
                                </div>
                            </div>

                            <div class="ctl-cluster">
                                <div class="ctl-shift">
                                    {(0..3).map(|i| view! {
                                        <i class={move || if shift_on(dir_of(&c), i) { "ctl-shift-lit" } else { "" }}></i>
                                    }).collect::<Vec<_>>()}
                                </div>
                                <div class="ctl-gear">{move || gear(dir_of(&c))}</div>
                                <div class="ctl-speed">
                                    <span class="ctl-speed-num">{move || readout(&c).0}</span>
                                    <span class="ctl-speed-label">"SPD"</span>
                                </div>
                                <div class="ctl-tach">
                                    {(0..TACH_SEGS).map(|i| view! {
                                        <i class={move || tach_class(i, c.speed.get())}></i>
                                    }).collect::<Vec<_>>()}
                                </div>
                                <div class="ctl-cluster-foot">
                                    <span>"ACT " {move || format!("{:+}", readout(&c).1)}</span>
                                    <span>"PWM " {move || format!("{}%", pct(c.speed.get()))}</span>
                                </div>
                            </div>

                            <div class="ctl-inputs">
                                <div class="ctl-pedal-row">
                                    <label>"THR"</label>
                                    <div class="ctl-pedal-track"><i style={move || format!("width:{}%", if dir_of(&c) & K_FWD != 0 { pct(c.speed.get()) } else { 0 })}></i></div>
                                    <span>{move || if dir_of(&c) & K_FWD != 0 { format!("{}%", pct(c.speed.get())) } else { "0%".to_string() }}</span>
                                </div>
                                <div class="ctl-pedal-row rev">
                                    <label>"BRK"</label>
                                    <div class="ctl-pedal-track"><i style={move || format!("width:{}%", if dir_of(&c) & K_BACK != 0 { pct(c.speed.get()) } else { 0 })}></i></div>
                                    <span>{move || if dir_of(&c) & K_BACK != 0 { format!("{}%", pct(c.speed.get())) } else { "0%".to_string() }}</span>
                                </div>
                                <div class="ctl-steer">
                                    <div class="ctl-steer-labels"><span>"◀ LEFT"</span><span>"STEER"</span><span>"RIGHT ▶"</span></div>
                                    <div class="ctl-steer-track"><i style={move || {
                                        let d = dir_of(&c);
                                        let v = if d & K_RIGHT != 0 && d & K_LEFT == 0 { 100 } else if d & K_LEFT != 0 && d & K_RIGHT == 0 { 0 } else { 50 };
                                        format!("left:{v}%")
                                    }}></i></div>
                                </div>
                            </div>
                        </div>

                        <div class="ctl-deck">
                            <div class="ctl-keys">
                                {[
                                    ("W", K_FWD),
                                    ("A", K_LEFT),
                                    ("S", K_BACK),
                                    ("D", K_RIGHT),
                                ]
                                .into_iter()
                                .map(|(k, bit)| view! {
                                    <span
                                        class=format!("ctl-keycap key-{}", k.to_lowercase())
                                        class:armed=move || dir_of(&c) & bit != 0
                                    >{k}</span>
                                })
                                .collect::<Vec<_>>()}
                            </div>
                            <div class="ctl-pad">
                                <button
                                    class="ctl-pad-btn"
                                    class:armed=move || dir_of(&c) & K_FWD != 0
                                    on:pointerdown=btn_down(P_FWD)
                                    on:pointerup=btn_up(P_FWD)
                                    on:pointerleave=btn_up(P_FWD)
                                    on:pointercancel=btn_up(P_FWD)
                                >"FWD ▶"</button>
                                <button
                                    class="ctl-pad-btn"
                                    class:armed=move || dir_of(&c) & K_LEFT != 0
                                    on:pointerdown=btn_down(P_LEFT)
                                    on:pointerup=btn_up(P_LEFT)
                                    on:pointerleave=btn_up(P_LEFT)
                                    on:pointercancel=btn_up(P_LEFT)
                                >"◀ LEFT"</button>
                                <button class="ctl-stop" on:click=on_stop>"■ STOP"</button>
                                <button
                                    class="ctl-pad-btn"
                                    class:armed=move || dir_of(&c) & K_RIGHT != 0
                                    on:pointerdown=btn_down(P_RIGHT)
                                    on:pointerup=btn_up(P_RIGHT)
                                    on:pointerleave=btn_up(P_RIGHT)
                                    on:pointercancel=btn_up(P_RIGHT)
                                >"RIGHT ▶"</button>
                                <button
                                    class="ctl-pad-btn"
                                    class:armed=move || dir_of(&c) & K_BACK != 0
                                    on:pointerdown=btn_down(P_BACK)
                                    on:pointerup=btn_up(P_BACK)
                                    on:pointerleave=btn_up(P_BACK)
                                    on:pointercancel=btn_up(P_BACK)
                                >"◀ REV"</button>
                            </div>
                            <div class="ctl-throttle">
                                <label for="ctl-drive-speed">"THROTTLE"</label>
                                <button title="Raise throttle (E)" on:click=on_thr_more>"+"</button>
                                <div
                                    class="motor-control-row on ctl-throttle-row"
                                    style={move || format!("--glow:8px;--mag:{:.3}", c.speed.get() as f64 / 255.0)}
                                >
                                    <div class="motor-control-track ctl-throttle-track">
                                        <div class="motor-control-groove ctl-throttle-groove"></div>
                                        <div class="motor-control-fill fwd ctl-throttle-fill"></div>
                                        <div class="motor-control-tick ctl-throttle-tick"></div>
                                        <input
                                            id="ctl-drive-speed"
                                            class="motor-control-input ctl-throttle-input"
                                            type="range"
                                            min="20"
                                            max="255"
                                            step="1"
                                            value="128"
                                            node_ref=c.drive_slider
                                            on:input=on_speed
                                        />
                                    </div>
                                </div>
                                <button title="Lower throttle (Q)" on:click=on_thr_less>"−"</button>
                                <span class="ctl-thr-pct">{move || format!("{}%", pct(c.speed.get()))}</span>
                                <span class="ctl-thr-keys">"E / Q"</span>
                            </div>
                        </div>

                        <div class="ctl-pump">
                            <div class="ctl-pump-copy">
                                <span class="ctl-pump-title">"WATER PUMP"</span>
                                <span class="ctl-pump-hint">{move || if c.pump.get() {
                                    "running - the ESP-3 stops it at the end of the burst".to_string()
                                } else {
                                    "10 s burst, about 0.7 L. ESP-3 GPIO 25.".to_string()
                                }}</span>
                            </div>
                            <button
                                class=move || if c.pump.get() { "ctl-water on" } else { "ctl-water" }
                                on:click=move |ev| if c.pump.get() { on_pump_stop(ev) } else { on_pump_fire(ev) }
                            >{move || if c.pump.get() { "■ STOP WATER" } else { "THROW WATER" }}</button>
                        </div>

                        <div class="ctl-wheel-grid">
                            {(1usize..=6usize).map(|m| {
                                let idx = m - 1;
                                let (rev, stop_one, fwd, less, more, flip) = on_jog(idx);
                                view! {
                                    <div class="ctl-wheel">
                                        <div class="ctl-wheel-head-row">
                                            <span class="ctl-wheel-name"><b>{SHORT[idx]}</b> {FULL[idx]}</span>
                                            <button
                                                class={move || if c.invert.get()[idx] { "ctl-flip on" } else { "ctl-flip" }}
                                                title="Wheel runs backwards? Flip this. Takes effect on the next command."
                                                on:click=flip
                                            >"FLIP"</button>
                                        </div>
                                        <div class="ctl-wheel-bar">
                                            <i style={move || {
                                                let v = c.cmd.get()[idx];
                                                let w = (v.abs() as f64 / 255.0 * 50.0).round() as i64;
                                                if v >= 0 { format!("left:50%;width:{w}%") } else { format!("left:{}%;width:{w}%", 50 - w) }
                                            }}></i>
                                            <span class="ctl-wheel-zero"></span>
                                        </div>
                                        <div class="ctl-wheel-buttons">
                                            <button class="ctl-jog-btn" title="Reverse" on:click=rev>"◀ REV"</button>
                                            <button class="ctl-jog-btn stop" title="Stop this motor" on:click=stop_one>"■ 0"</button>
                                            <button class="ctl-jog-btn" title="Forward" on:click=fwd>"FWD ▶"</button>
                                        </div>
                                        <div class="ctl-wheel-speed">
                                            <button title="Lower jog speed" on:click=less>"−"</button>
                                            <span>{move || format!("{}%", pct(c.jog_speeds.get()[idx]))}</span>
                                            <button title="Raise jog speed" on:click=more>"+"</button>
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    </div>
                </main>
            </div>
        })}
    }
}
