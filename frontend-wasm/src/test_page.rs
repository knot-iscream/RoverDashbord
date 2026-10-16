//! Motor Test page (`/next/test`) — 1:1 port of `web/test/`.
//!
//! Covers: 6-motor tabs (RR default; ML/MR are blind MID slots), slider + OFF + presets + ramp,
//! live telemetry panel, vibration beats, sys alerts, calibration guard,
//! freshness gates, `?motor=` deep-link. Vanilla stays default at `/test/`.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlInputElement};

use crate::shell::app_shell;
use crate::ws::{fetch_device_online, get_json, motor_param, now_ms, post_motor, ws_url};

const MOTOR_SHORT: [&str; 6] = ["FL", "FR", "RL", "RR", "ML", "MR"];
const MOTOR_FULL: [&str; 6] = [
    "Front Left",
    "Front Right",
    "Rear Left",
    "Rear Right",
    "Middle Left",
    "Middle Right",
];

const RAMP_SEQ: [i32; 9] = [64, 128, 191, 255, -255, -191, -128, -64, 0];
const RAMP_STEP_MS: f64 = 4000.0;
const RAMP_GLIDE_MS: f64 = 650.0;
const FRESH_LIVE_MS: f64 = 5000.0;
const FRESH_TIMEOUT_MS: f64 = 60000.0;

/// Live sample for one motor (only the fields this page renders).
#[derive(Debug, Clone, Copy, Default)]
struct TestSample {
    temp: Option<f64>,
    voltage: Option<f64>,
    current: Option<f64>,
    health: Option<f64>,
    speed: Option<i64>,
    vib_active: bool,
    vib_valid: bool,
    vib_mag: Option<f64>,
    temp_valid: bool,
    ina_ok: bool,
}

/// `fmt(x, dec, unit)` verbatim: `--` + unit when not a finite number.
fn fmt_val(v: Option<f64>, dec: usize, unit: &str) -> String {
    match v {
        Some(x) if x.is_finite() => format!("{x:.dec$}{unit}"),
        _ => format!("--{unit}"),
    }
}

fn ease_out_expo(t: f64) -> f64 {
    if t >= 1.0 {
        1.0
    } else {
        1.0 - 2f64.powf(-10.0 * t)
    }
}

fn ramp_label(speed: i32) -> String {
    let pct = (speed.abs() as f64 / 255.0 * 100.0).round() as i64;
    if speed > 0 {
        format!("FWD {pct}%")
    } else if speed < 0 {
        format!("REV {pct}%")
    } else {
        "STOP".to_string()
    }
}

fn toggle(el: &Element, cls: &str, on: bool) {
    el.class_list().toggle_with_force(cls, on).ok();
}

fn js_random() -> f64 {
    js_sys::Math::random()
}

#[derive(Clone, Copy, PartialEq)]
enum Fresh {
    Waiting,
    Live,
    Stale,
}

fn fresh_of(last_rx: Option<f64>, now: f64) -> (Fresh, Option<f64>) {
    match last_rx {
        None => (Fresh::Waiting, None),
        Some(t) => {
            let age = (now - t) / 1000.0;
            if age <= FRESH_LIVE_MS / 1000.0 {
                (Fresh::Live, Some(age))
            } else if age <= FRESH_TIMEOUT_MS / 1000.0 {
                (Fresh::Stale, Some(age))
            } else {
                (Fresh::Waiting, None)
            }
        }
    }
}

#[derive(Clone)]
struct Ctx {
    selected: RwSignal<u8>,
    speeds: RwSignal<[i32; 6]>,
    samples: RwSignal<[TestSample; 6]>,
    last_rx: RwSignal<[Option<f64>; 6]>,
    device_online: RwSignal<bool>,
    calibrating: RwSignal<bool>,
    cal_name: RwSignal<String>,
    has_baseline: RwSignal<bool>,
    tick: RwSignal<u64>,
    ramp_on: RwSignal<bool>,
    ramp_idx: RwSignal<usize>,
    ramp_txt: RwSignal<String>,
    ws_up: RwSignal<bool>,
    deb_seq: RwSignal<u64>,
    anim_token: RwSignal<u64>,
    frame_id: RwSignal<Option<i32>>,
    step_start: std::rc::Rc<std::cell::Cell<f64>>,
    // Interval is !Send, so it cannot live in a signal — RefCell instead.
    ramp_timer: std::rc::Rc<std::cell::RefCell<Option<gloo_timers::callback::Interval>>>,
    animate: std::rc::Rc<dyn Fn(i32, f64)>,
    cancel_ramp: std::rc::Rc<dyn Fn(bool)>,
    paint: std::rc::Rc<dyn Fn(i32)>,
    slider: NodeRef<leptos::html::Input>,
    row: NodeRef<leptos::html::Div>,
    val: NodeRef<leptos::html::Span>,
    fill: NodeRef<leptos::html::Div>,
    rev_label: NodeRef<leptos::html::Span>,
    fwd_label: NodeRef<leptos::html::Span>,
    beats: NodeRef<leptos::html::Div>,
    vib_read: NodeRef<leptos::html::Span>,
    health_bar: NodeRef<leptos::html::Div>,
    chip_cal: NodeRef<leptos::html::Span>,
    chip_conn: NodeRef<leptos::html::Span>,
}

/// `updateMotorUI` verbatim (minus the slider element itself, which callers
/// drive). Clamps, paints fill/labels/val, stores per-motor speed memory.
fn paint_motor_ui(ctx: &Ctx, value: i32) {
    let value = value.clamp(-255, 255);
    let m = (ctx.selected.get() as usize).saturating_sub(1).min(5);
    let mag = value.abs() as f64 / 255.0;
    if let Some(row) = ctx.row.get() {
        let el: &Element = row.as_ref();
        el.set_attribute(
            "style",
            &format!("--mag:{mag:.3};--glow:{:.1}px", mag * 20.0),
        )
        .ok();
        toggle(el, "on", value != 0);
    }
    if let Some(fill) = ctx.fill.get() {
        let el: &Element = fill.as_ref();
        toggle(el, "rev", value < 0);
        toggle(el, "fwd", value > 0);
    }
    if let Some(v) = ctx.val.get() {
        let el: &Element = v.as_ref();
        toggle(el, "on", value != 0);
        v.set_text_content(Some(&value.to_string()));
    }
    if let Some(r) = ctx.rev_label.get() {
        toggle(r.as_ref(), "on", value < 0);
    }
    if let Some(f) = ctx.fwd_label.get() {
        toggle(f.as_ref(), "on", value > 0);
    }
    ctx.speeds.update(|arr| arr[m] = value);
}

/// `renderVib` verbatim + range suffix: NO SIGNAL / YES 1.8g / no 1.0g.
fn render_vib(ctx: &Ctx, active: bool, valid: bool, mag: Option<f64>) {
    let no_signal = !valid;
    let suffix = match mag {
        Some(g) if g.is_finite() => format!(" {g:.2}g"),
        _ => String::new(),
    };
    if let Some(read) = ctx.vib_read.get() {
        let el: &Element = read.as_ref();
        let text = if no_signal {
            "NO SIGNAL".to_string()
        } else if active {
            format!("YES{suffix}")
        } else {
            format!("no{suffix}")
        };
        read.set_text_content(Some(&text));
        el.set_attribute(
            "style",
            if no_signal {
                "color:var(--gray-500);"
            } else if active {
                "color:var(--dot-red);"
            } else {
                "color:var(--gray-400);"
            },
        )
        .ok();
    }
    if let Some(wrap) = ctx.beats.get() {
        let kids = wrap.child_nodes();
        for i in 0..kids.length() {
            if let Some(n) = kids.item(i) {
                if let Ok(b) = n.dyn_into::<web_sys::HtmlSpanElement>() {
                    let h = if active {
                        12.0 + js_random() * 10.0
                    } else {
                        2.0 + js_random() * 5.0
                    };
                    b.set_attribute("style", &format!("--h:{h:.1}px")).ok();
                    toggle(b.as_ref(), "on", active);
                }
            }
        }
    }
}

/// Health bar width + `low` class (mirrors the `t-health-bar` block).
fn paint_health_bar(ctx: &Ctx, h: Option<f64>) {
    let hpct = h.map(|x| x.clamp(0.0, 100.0)).unwrap_or(0.0);
    if let Some(bar) = ctx.health_bar.get() {
        let el: &Element = bar.as_ref();
        el.set_attribute("style", &format!("width:{hpct}%;")).ok();
        toggle(el, "low", h.is_some_and(|x| x < 60.0));
    }
}

/// The 5 verbatim sys-status strings + alert gating. Blind MID slots (5-6)
/// carry no sensors by design: steady "drive only" line, never an alarm.
fn sys_text(s: &TestSample, blind: bool) -> (bool, &'static str) {
    if blind {
        return (false, "Blind slot \u{2014} drive only, no sensors");
    }
    let vib_on = s.vib_active;
    let alert = (s.temp_valid && s.temp.is_some_and(|t| t > 50.0))
        || (s.ina_ok && s.voltage.is_some_and(|v| v > 0.0 && v < 11.5))
        || (s.vib_valid && vib_on)
        || !s.ina_ok
        || !s.temp_valid;
    let text = if !s.ina_ok && !s.temp_valid {
        "ALARM \u{2014} INA226 & temp probe not found"
    } else if !s.ina_ok {
        "ALARM \u{2014} INA226 not found"
    } else if !s.temp_valid {
        "ALARM \u{2014} temp probe not found"
    } else if alert {
        "ALARM \u{2014} check sensor reading"
    } else {
        "All systems nominal"
    };
    (alert, text)
}

fn fetch_baseline(ctx: &Ctx) {
    let ctx = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let m = ctx.selected.get();
        let has = get_json("/api/calibration")
            .await
            .and_then(|v| v.get("baselines").cloned())
            .is_some_and(|bl| {
                bl.get(m.to_string()).is_some_and(|x| !x.is_null())
                    || bl.get(m as usize - 1).is_some_and(|x| !x.is_null())
            });
        // Vanilla checks `bl[String] || bl[number]`; index form covers arrays.
        ctx.has_baseline.set(has);
    });
}

/// `updateCalGuard` verbatim (chip paint imperative, the rest reactive).
fn update_cal_guard(ctx: &Ctx, state: &str) {
    let _was = ctx.calibrating.get();
    let cal = !state.is_empty() && state != "idle";
    ctx.calibrating.set(cal);
    if cal {
        (ctx.cancel_ramp)(false);
    }
    ctx.cal_name.set(if cal {
        format!("Calibration \u{2014} {state}")
    } else {
        "Calibration idle".to_string()
    });
    if let Some(dot) = ctx.chip_cal.get() {
        dot.set_attribute(
            "style",
            if cal {
                "background:var(--dot-yellow);"
            } else {
                "background:var(--gray-500);"
            },
        )
        .ok();
    }
    // Vanilla re-enables + refreshes whenever the guard opens; the
    // disabled state itself is reactive, so only the baseline refetch
    // needs an explicit call here.
    if !cal {
        fetch_baseline(ctx);
    }
}

fn set_conn_chip(ctx: &Ctx, up: bool) {
    ctx.ws_up.set(up);
    if let Some(dot) = ctx.chip_conn.get() {
        dot.set_attribute(
            "style",
            if up {
                "background:var(--dot-green);"
            } else {
                "background:var(--dot-red);"
            },
        )
        .ok();
    }
}

/// WS loop for the test page: motor_update drives the selected motor,
/// calibration_status drives the guard, snapshot only refreshes freshness.
fn connect_test_ws(ctx: Ctx) {
    let url = ws_url();
    let Ok(socket) = web_sys::WebSocket::new(&url) else {
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(3000).await;
            connect_test_ws(ctx);
        });
        return;
    };
    let onopen = Closure::<dyn FnMut(_)>::new({
        let s = socket.clone();
        let ctx = ctx.clone();
        move |_: web_sys::Event| {
            set_conn_chip(&ctx, true);
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
                    if !(1..=6).contains(&m) {
                        return;
                    }
                    let vib = &v["vibration"];
                    let sample = TestSample {
                        temp: v.get("temp").and_then(|x| x.as_f64()),
                        voltage: v.get("voltage").and_then(|x| x.as_f64()),
                        current: v.get("current").and_then(|x| x.as_f64()),
                        health: v.get("health").and_then(|x| x.as_f64()),
                        speed: v.get("speed").and_then(|x| x.as_i64()),
                        vib_active: vib.as_bool().unwrap_or(false)
                            || vib.as_u64().is_some_and(|x| x == 1)
                            || vib.as_str().is_some_and(|x| x == "1"),
                        vib_valid: v
                            .get("vibration_valid")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true),
                        vib_mag: v.get("vib_mag").and_then(|x| x.as_f64()),
                        temp_valid: v
                            .get("temp_valid")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true),
                        ina_ok: v.get("ina_ok").and_then(|x| x.as_bool()).unwrap_or(true),
                    };
                    ctx.samples.update(|arr| arr[m - 1] = sample);
                    ctx.last_rx.update(|arr| arr[m - 1] = Some(now_ms()));
                    if m as u8 == ctx.selected.get() {
                        paint_health_bar(&ctx, sample.health);
                        render_vib(&ctx, sample.vib_active, sample.vib_valid, sample.vib_mag);
                    }
                }
                "snapshot" => {
                    // Intentionally ignored (no stale replay); freshness text
                    // still refreshes on the next tick.
                    ctx.tick.update(|t| *t += 1);
                }
                "calibration_status" => {
                    let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("");
                    update_cal_guard(&ctx, state);
                }
                "device_status" => {
                    if let Some(online) = v.get("online").and_then(|x| x.as_bool()) {
                        ctx.device_online.set(online);
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
        let ctx = ctx.clone();
        move |_: web_sys::CloseEvent| {
            set_conn_chip(&ctx, false);
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(3000).await;
                connect_test_ws(ctx);
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

/// Holder for the live rAF glide closure (self-referential across frames).
type FrameCell = std::rc::Rc<std::cell::RefCell<Option<Closure<dyn FnMut(f64)>>>>;

#[component]
pub fn TestPage() -> impl IntoView {
    let selected = RwSignal::new(motor_param(4));
    let speeds = RwSignal::new([0i32, 0, 0, 0, 0, 0]);
    let samples = RwSignal::new([TestSample::default(); 6]);
    let last_rx = RwSignal::new([None, None, None, None, None, None]);
    let device_online = RwSignal::new(false);
    let calibrating = RwSignal::new(false);
    let cal_name = RwSignal::new("Calibration idle".to_string());
    let has_baseline = RwSignal::new(false);
    let tick = RwSignal::new(0u64);
    let ramp_on = RwSignal::new(false);
    let ramp_idx = RwSignal::new(0usize);
    let ramp_txt = RwSignal::new(
        "FWD 25 \u{2192} 50 \u{2192} 75 \u{2192} 100 \u{2192} REV \u{2192} 0".to_string(),
    );
    let ws_up = RwSignal::new(false);
    let deb_seq = RwSignal::new(0u64);
    let anim_token = RwSignal::new(0u64);
    let frame_id = RwSignal::new(None::<i32>);
    let step_start = std::rc::Rc::new(std::cell::Cell::new(0.0f64));
    let ramp_timer = std::rc::Rc::new(std::cell::RefCell::new(None));

    let slider = NodeRef::<leptos::html::Input>::new();
    let row = NodeRef::<leptos::html::Div>::new();
    let val = NodeRef::<leptos::html::Span>::new();
    let fill = NodeRef::<leptos::html::Div>::new();
    let rev_label = NodeRef::<leptos::html::Span>::new();
    let fwd_label = NodeRef::<leptos::html::Span>::new();
    let beats = NodeRef::<leptos::html::Div>::new();
    let vib_read = NodeRef::<leptos::html::Span>::new();
    let health_bar = NodeRef::<leptos::html::Div>::new();
    let chip_cal = NodeRef::<leptos::html::Span>::new();
    let chip_conn = NodeRef::<leptos::html::Span>::new();

    // Base context (closures that need animate/cancel_ramp/paint are added
    // after those are built).
    let base = || Ctx {
        selected,
        speeds,
        samples,
        last_rx,
        device_online,
        calibrating,
        cal_name,
        has_baseline,
        tick,
        ramp_on,
        ramp_idx,
        ramp_txt,
        ws_up,
        deb_seq,
        anim_token,
        frame_id,
        step_start: step_start.clone(),
        ramp_timer: ramp_timer.clone(),
        animate: std::rc::Rc::new(|_, _| {}),
        cancel_ramp: std::rc::Rc::new(|_| {}),
        paint: std::rc::Rc::new(|_| {}),
        slider,
        row,
        val,
        fill,
        rev_label,
        fwd_label,
        beats,
        vib_read,
        health_bar,
        chip_cal,
        chip_conn,
    };
    // `RwSignal::clone` is cheap (Copy, but the struct holds Rc fields).
    let ctx = base();

    // ---- animateMotorTo: rAF glide with easeOutExpo, final POST on land ----
    let animate = {
        let ctx = ctx.clone();
        std::rc::Rc::new(move |target: i32, duration: f64| {
            let ctx = ctx.clone();
            if let Some(id) = ctx.frame_id.get() {
                if let Some(w) = web_sys::window() {
                    w.cancel_animation_frame(id).ok();
                }
            }
            let token = ctx.anim_token.get() + 1;
            ctx.anim_token.set(token);
            let start = ctx
                .slider
                .get()
                .and_then(|s| s.value().parse::<i32>().ok())
                .unwrap_or(0);
            if start == target {
                paint_motor_ui(&ctx, target);
                return;
            }
            let t0 = std::rc::Rc::new(std::cell::Cell::new(None::<f64>));
            let frame: FrameCell = std::rc::Rc::new(std::cell::RefCell::new(None));
            let frame2 = frame.clone();
            let ctx2 = ctx.clone();
            *frame.borrow_mut() = Some(Closure::new(move |ts: f64| {
                if ctx2.anim_token.get() != token {
                    return;
                }
                if t0.get().is_none() {
                    t0.set(Some(ts));
                }
                let p = ((ts - t0.get().unwrap_or(ts)) / duration).min(1.0);
                let val =
                    (start as f64 + (target - start) as f64 * ease_out_expo(p)).round() as i32;
                if let Some(s) = ctx2.slider.get() {
                    s.set_value(&val.to_string());
                }
                paint_motor_ui(&ctx2, val);
                if p < 1.0 {
                    if let (Some(w), Some(f)) = (web_sys::window(), frame2.borrow().as_ref()) {
                        if let Ok(id) = w.request_animation_frame(f.as_ref().unchecked_ref()) {
                            ctx2.frame_id.set(Some(id));
                        }
                    }
                } else {
                    ctx2.frame_id.set(None);
                    let (m, t) = (ctx2.selected.get(), target);
                    wasm_bindgen_futures::spawn_local(async move {
                        post_motor(m, t).await;
                    });
                }
            }));
            // First frame: scoped borrow, then forget the holder so the
            // closure stays alive across rAF turns (superseded via token).
            let first_id = if let (Some(w), Some(f)) = (web_sys::window(), frame.borrow().as_ref())
            {
                w.request_animation_frame(f.as_ref().unchecked_ref()).ok()
            } else {
                None
            };
            std::mem::forget(frame);
            if let Some(id) = first_id {
                ctx.frame_id.set(Some(id));
            }
        })
    };

    // ---- cancelRamp ----
    let cancel_ramp = {
        let ctx = ctx.clone();
        std::rc::Rc::new(move |finished: bool| {
            if !ctx.ramp_on.get() {
                return;
            }
            ctx.ramp_on.set(false);
            ctx.ramp_timer.borrow_mut().take();
            ctx.ramp_txt.set(if finished {
                "Ramp complete \u{2014} motor stopped".to_string()
            } else {
                "Ramp cancelled".to_string()
            });
        })
    };

    // ---- paint entry used by tab switches (same paint fn) ----
    let paint = {
        let ctx = ctx.clone();
        std::rc::Rc::new(move |value: i32| paint_motor_ui(&ctx, value))
    };

    // Full context for WS/polls/select handlers.
    let full = || {
        let mut c = base();
        c.animate = animate.clone();
        c.cancel_ramp = cancel_ramp.clone();
        c.paint = paint.clone();
        c
    };
    let ctx = full();

    // ---- ramp ticker (250ms): advance steps, countdown label ----
    let ramp_tick = {
        let ctx = ctx.clone();
        move || {
            let elapsed = now_ms() - ctx.step_start.get();
            if elapsed >= RAMP_STEP_MS {
                let next = ctx.ramp_idx.get() + 1;
                if next >= RAMP_SEQ.len() {
                    (ctx.cancel_ramp)(true);
                    return;
                }
                ctx.ramp_idx.set(next);
                ctx.step_start.set(now_ms());
                (ctx.animate)(RAMP_SEQ[next], RAMP_GLIDE_MS);
            }
            let rem = ((RAMP_STEP_MS - (now_ms() - ctx.step_start.get())) / 1000.0)
                .ceil()
                .max(0.0) as i64;
            let label = ramp_label(RAMP_SEQ[ctx.ramp_idx.get()]);
            ctx.ramp_txt
                .set(format!("Ramping \u{2192} {label} \u{b7} {rem}s"));
        }
    };

    // ---- startRamp ----
    let start_ramp = {
        let ctx = ctx.clone();
        let ramp_tick = std::rc::Rc::new(ramp_tick);
        move || {
            if ctx.calibrating.get() || ctx.ramp_on.get() {
                return;
            }
            ctx.ramp_on.set(true);
            ctx.ramp_idx.set(0);
            ctx.step_start.set(now_ms());
            (ctx.animate)(RAMP_SEQ[0], RAMP_GLIDE_MS);
            let rt = ramp_tick.clone();
            let iv = gloo_timers::callback::Interval::new(250, move || rt());
            *ctx.ramp_timer.borrow_mut() = Some(iv);
        }
    };
    let start_ramp = std::rc::Rc::new(start_ramp);

    // ---- selectMotor ----
    let select_motor = {
        let ctx = ctx.clone();
        move |id: u8| {
            if id == ctx.selected.get() {
                return;
            }
            if ctx.ramp_on.get() {
                (ctx.cancel_ramp)(false);
            }
            ctx.selected.set(id);
            let m = (id as usize).saturating_sub(1).min(5);
            let spd = ctx.speeds.get()[m];
            if let Some(s) = ctx.slider.get() {
                s.set_value(&spd.to_string());
            }
            (ctx.paint)(spd);
            // Blank, then re-render the cached sample like vanilla.
            paint_health_bar(&ctx, None);
            let cached = ctx.samples.get()[m];
            let live = ctx.last_rx.get()[m].is_some_and(|t| now_ms() - t <= FRESH_LIVE_MS);
            if live {
                paint_health_bar(&ctx, cached.health);
                render_vib(&ctx, cached.vib_active, cached.vib_valid, cached.vib_mag);
            }
            fetch_baseline(&ctx);
        }
    };
    let select_motor = std::rc::Rc::new(select_motor);

    // ---- slider POST paths ----
    let send_now = {
        let ctx = ctx.clone();
        move |value: i32| {
            let (m, v) = (ctx.selected.get(), value);
            wasm_bindgen_futures::spawn_local(async move {
                post_motor(m, v).await;
            });
        }
    };
    let send_now = std::rc::Rc::new(send_now);
    let schedule_send = {
        let ctx = ctx.clone();
        let send_now = send_now.clone();
        move |value: i32| {
            let seq = ctx.deb_seq.get() + 1;
            ctx.deb_seq.set(seq);
            let ctx = ctx.clone();
            let send_now = send_now.clone();
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(80).await;
                if ctx.deb_seq.get() == seq {
                    send_now(value);
                }
            });
        }
    };
    let schedule_send = std::rc::Rc::new(schedule_send);

    // ---- setup: polls, WS, intervals (DOM-free; refs touched post-mount) ----
    {
        let ctx = ctx.clone();
        connect_test_ws(ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let up = fetch_device_online().await;
            ctx.device_online.set(up);
        });
        fetch_baseline(&ctx);
        // Post-mount init: beats, slider paint, recurring polls.
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(wrap) = ctx.beats.get() {
                let doc = wrap.owner_document().unwrap();
                for _ in 0..20 {
                    if let Ok(b) = doc.create_element("span") {
                        b.set_attribute("class", "test-beat").ok();
                        let h = 2.0 + js_random() * 5.0;
                        b.set_attribute("style", &format!("--h:{h:.1}px")).ok();
                        wrap.append_child(&b).ok();
                    }
                }
            }
            if let Some(s) = ctx.slider.get() {
                s.set_value("0");
            }
            (ctx.paint)(0);
            // Recurring polls (handles forgotten: 'static by construction).
            let c = ctx.clone();
            let _iv = gloo_timers::callback::Interval::new(2000, move || {
                let c = c.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Some(st) = get_json("/api/calibration/status").await {
                        let state = st.get("state").and_then(|x| x.as_str()).unwrap_or("");
                        update_cal_guard(&c, state);
                    }
                });
            });
            _iv.forget();
            let c = ctx.clone();
            let _iv = gloo_timers::callback::Interval::new(1000, move || {
                c.tick.update(|t| *t += 1);
                // Vanilla updateFreshness side effects beyond text: re-render
                // idle/stale beat bars every second.
                let m = (c.selected.get() as usize).saturating_sub(1).min(5);
                let (fresh, _) = fresh_of(c.last_rx.get()[m], now_ms());
                if !matches!(fresh, Fresh::Live) {
                    render_vib(&c, false, false, None);
                }
            });
            _iv.forget();
            let c = ctx.clone();
            let _iv = gloo_timers::callback::Interval::new(3000, move || {
                let c = c.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    c.device_online.set(fetch_device_online().await);
                });
            });
            _iv.forget();
        });
    }

    // ---- reactive derivations (mirror the vanilla render call sites) ----
    let fresh = move || {
        let _ = tick.get();
        let m = (selected.get() as usize).saturating_sub(1).min(5);
        fresh_of(last_rx.get()[m], now_ms())
    };
    let tele_dot_cls = move || {
        let (f, _) = fresh();
        match f {
            Fresh::Live => "status-dot dot-green pulse",
            Fresh::Stale => "status-dot dot-yellow",
            Fresh::Waiting => "status-dot dot-off",
        }
    };
    let age_txt = move || {
        let m = (selected.get() as usize).saturating_sub(1).min(5);
        match fresh() {
            (Fresh::Live, Some(age)) => format!("Live \u{b7} {age:.1}s ago"),
            (Fresh::Stale, Some(age)) => format!("Stale \u{b7} {}s ago", age.round() as i64),
            _ => format!("Waiting for {} stream\u{2026}", MOTOR_SHORT[m]),
        }
    };
    // Sensor values: waiting -> '--' (blankTelemetry), else validity-gated.
    let val_txt = move |metric: usize| {
        let _ = tick.get();
        let m = (selected.get() as usize).saturating_sub(1).min(5);
        let s = samples.get()[m];
        let (f, _) = fresh_of(last_rx.get()[m], now_ms());
        if matches!(f, Fresh::Waiting) {
            return if metric == 4 {
                speeds.get()[m].to_string()
            } else {
                "--".to_string()
            };
        }
        match metric {
            0 => {
                if s.temp_valid {
                    fmt_val(s.temp, 1, " \u{b0}C")
                } else {
                    "-- \u{b0}C".to_string()
                }
            }
            1 => {
                if s.ina_ok {
                    fmt_val(s.voltage, 2, " V")
                } else {
                    "-- V".to_string()
                }
            }
            2 => {
                if s.ina_ok {
                    fmt_val(s.current, 3, " A")
                } else {
                    "-- A".to_string()
                }
            }
            3 => {
                let h = if has_baseline.get() { s.health } else { None };
                match h {
                    Some(x) if x.is_finite() => format!("{x:.1}%"),
                    _ => "--".to_string(),
                }
            }
            _ => {
                let spd = s.speed.map(|x| x.to_string());
                spd.unwrap_or_else(|| speeds.get()[m].to_string())
            }
        }
    };
    let stale_cls = move |base: &'static str| {
        let _ = tick.get();
        let m = (selected.get() as usize).saturating_sub(1).min(5);
        let (f, _) = fresh_of(last_rx.get()[m], now_ms());
        if matches!(f, Fresh::Stale) {
            format!("{base} stale")
        } else {
            base.to_string()
        }
    };
    let sys_cls = move || {
        let _ = tick.get();
        let m = (selected.get() as usize).saturating_sub(1).min(5);
        match fresh_of(last_rx.get()[m], now_ms()).0 {
            Fresh::Waiting => "test-sys-ok test-sys-wait".to_string(),
            Fresh::Stale => "test-sys-ok test-sys-alert".to_string(),
            Fresh::Live => {
                let blind = selected.get() > 4;
                let (alert, _) = sys_text(&samples.get()[m], blind);
                if alert {
                    "test-sys-ok test-sys-alert".to_string()
                } else {
                    "test-sys-ok".to_string()
                }
            }
        }
    };
    let sys_txt = move || {
        let _ = tick.get();
        let m = (selected.get() as usize).saturating_sub(1).min(5);
        match fresh_of(last_rx.get()[m], now_ms()).0 {
            Fresh::Waiting => {
                format!(
                    "No live data \u{2014} awaiting {} stream\u{2026}",
                    MOTOR_SHORT[m]
                )
            }
            Fresh::Stale => {
                let age = last_rx.get()[m].map(|t| ((now_ms() - t) / 1000.0).round() as i64);
                format!(
                    "Telemetry stale \u{2014} last sample {}s ago",
                    age.unwrap_or(0)
                )
            }
            Fresh::Live => sys_text(&samples.get()[m], selected.get() > 4)
                .1
                .to_string(),
        }
    };
    // run-status: Offline > Ramp > calibrating ? Sweeping : live sample ?
    // Running : commanded Sweeping — mirrors applyDeviceStatus/render sites.
    let run_txt = move || {
        let _ = tick.get();
        if !device_online.get() {
            return "Offline";
        }
        if ramp_on.get() {
            return "Ramp";
        }
        if calibrating.get() {
            return if speeds.get()[(selected.get() as usize).saturating_sub(1).min(5)] != 0 {
                "Sweeping"
            } else {
                "Idle"
            };
        }
        let m = (selected.get() as usize).saturating_sub(1).min(5);
        let (f, _) = fresh_of(last_rx.get()[m], now_ms());
        if matches!(f, Fresh::Live) {
            if samples.get()[m].speed.unwrap_or(0) != 0 {
                "Running"
            } else {
                "Idle"
            }
        } else if speeds.get()[m] != 0 {
            "Sweeping"
        } else {
            "Idle"
        }
    };
    let run_cls = move || {
        let t = run_txt();
        if t == "Idle" || t == "Offline" {
            "cal-motor-status idle"
        } else {
            "cal-motor-status collecting"
        }
    };
    let disabled = move || calibrating.get() || ramp_on.get();

    // ---- event handlers ----
    let on_tab = {
        let select_motor = select_motor.clone();
        move |id: u8| select_motor(id)
    };
    let on_input = {
        let ctx = ctx.clone();
        let schedule_send = schedule_send.clone();
        let cancel_ramp = cancel_ramp.clone();
        move |ev: web_sys::Event| {
            let v = ev
                .target()
                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                .and_then(|i| i.value().parse::<i32>().ok())
                .unwrap_or(0);
            if ctx.ramp_on.get() {
                (cancel_ramp)(false);
            }
            (ctx.paint)(v);
            schedule_send(v);
        }
    };
    let on_change = {
        let send_now = send_now.clone();
        move |_| {
            let v = slider
                .get()
                .and_then(|s| s.value().parse::<i32>().ok())
                .unwrap_or(0);
            send_now(v);
        }
    };
    let on_off = {
        let ctx = ctx.clone();
        let cancel_ramp = cancel_ramp.clone();
        move |_| {
            if ctx.ramp_on.get() {
                (cancel_ramp)(false);
            }
            (ctx.animate)(0, 280.0);
        }
    };
    let on_preset = {
        let ctx = ctx.clone();
        let cancel_ramp = cancel_ramp.clone();
        move |speed: i32| {
            if ctx.ramp_on.get() {
                (cancel_ramp)(false);
            }
            (ctx.animate)(speed, 300.0);
        }
    };
    let on_ramp = {
        let start_ramp = start_ramp.clone();
        let cancel_ramp = cancel_ramp.clone();
        let ctx = ctx.clone();
        move |_| {
            if ctx.ramp_on.get() {
                (cancel_ramp)(false);
                return;
            }
            start_ramp();
        }
    };

    view! {
        {app_shell(view! {
            <div class="body-wrap">
                <main class="main-content">
                    <div class="content-wrap">
                        <div class="page-header">
                            <div><h1 class="page-h1">"Motor Test"</h1></div>
                            <div class="live-badge">
                                <span class={move || if device_online.get() { "status-dot dot-green pulse" } else { "status-dot dot-red" }} id="test-status-dot"></span>
                                <span id="test-status-label">{move || if device_online.get() { "Online" } else { "Offline" }}</span>
                            </div>
                        </div>
                        <div class="detail-motor-tabs" id="test-motor-tabs">
                            {(1u8..=6u8).map(|m| {
                                let ot = on_tab.clone();
                                view! {
                                    <button
                                        class={move || if selected.get() == m { "detail-motor-tab active" } else { "detail-motor-tab" }}
                                        data-motor={m.to_string()}
                                        disabled={move || disabled()}
                                        on:click=move |_| ot(m)
                                    >{format!("{} Motor", MOTOR_SHORT[(m - 1) as usize])}</button>
                                }
                            }).collect::<Vec<_>>()}
                        </div>

                        <div class="hud-layout">
                            <div class="hud-panel">
                                <span class="hud-corner hud-corner-tl" aria-hidden="true"></span>
                                <span class="hud-corner hud-corner-br" aria-hidden="true"></span>
                                <span class="hud-vents" aria-hidden="true"></span>
                                <div class="hud-content">
                                    <div class="panel-head">
                                        <h2 class="panel-title-muted" id="test-motor-title">
                                            {move || { let m = selected.get() as usize; format!("Motor {} \u{b7} #{}", MOTOR_SHORT[m - 1], m) }}
                                        </h2>
                                        <span class={run_cls} id="run-status">{run_txt}</span>
                                    </div>

                                    <div class="motor-control-row test-hero" data-motor-row={move || selected.get().to_string()}>
                                        <div class="motor-control-head">
                                            <span class="cal-motor-name" id="test-motor-name">
                                                {move || MOTOR_FULL[(selected.get() as usize).saturating_sub(1).min(5)].to_string()}
                                            </span>
                                            <div class="motor-control-meta">
                                                <span class="motor-control-val test-hero-val" id="motor-val" node_ref=val>"0"</span>
                                                <button class="btn-off" type="button" data-off={move || selected.get().to_string()} title="Stop motor" disabled={move || disabled()} on:click=on_off>
                                                    <span class="btn-off-label">"OFF"</span>
                                                </button>
                                            </div>
                                        </div>
                                        <div class="motor-control-track" node_ref=row>
                                            <div class="motor-control-groove"></div>
                                            <div class="motor-control-fill" node_ref=fill></div>
                                            <div class="motor-control-tick"></div>
                                            <input type="range" class="motor-control-input" min="-255" max="255" step="1" value="0" id="motor-slider" data-motor={move || selected.get().to_string()} disabled={move || disabled()} node_ref=slider on:input=on_input on:change=on_change />
                                        </div>
                                        <div class="motor-control-labels">
                                            <span class="motor-label" data-side="rev" node_ref=rev_label>"REV"</span>
                                            <span class="motor-label" data-side="fwd" node_ref=fwd_label>"FWD"</span>
                                        </div>
                                    </div>

                                    <div class="test-presets" id="preset-fwd">
                                        <span class="test-preset-tag">"FWD"</span>
                                        {[64i32, 128, 191, 255].into_iter().map(|s| {
                                            let on_preset = on_preset.clone();
                                            view! {
                                                <button class="test-preset" data-speed={s.to_string()} disabled={move || disabled()} on:click=move |_| on_preset(s)>
                                                    {format!("{}%", (s as f64 / 255.0 * 100.0).round() as i64)}
                                                </button>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                    <div class="test-presets" id="preset-rev">
                                        <span class="test-preset-tag">"REV"</span>
                                        {[-64i32, -128, -191, -255].into_iter().map(|s| {
                                            let on_preset = on_preset.clone();
                                            view! {
                                                <button class="test-preset" data-speed={s.to_string()} disabled={move || disabled()} on:click=move |_| on_preset(s)>
                                                    {format!("{}%", (s.abs() as f64 / 255.0 * 100.0).round() as i64)}
                                                </button>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>

                                    <div class="test-ramp">
                                        <button class="btn-primary" id="ramp-btn" style="padding:8px 22px;" on:click=on_ramp>
                                            <span class="btn-corner-tl"></span>
                                            <span class="btn-corner-br"></span>
                                            <span class="btn-sweep"><span class="btn-sweep-inner"></span></span>
                                            <span class="btn-content" style="gap:8px;">
                                                <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/></svg>
                                                "Ramp Test"
                                            </span>
                                        </button>
                                        <span class={move || if ramp_on.get() { "test-ramp-status running" } else { "test-ramp-status" }} id="ramp-status">{move || ramp_txt.get()}</span>
                                    </div>
                                </div>
                            </div>

                            <div class="hud-right-stack">
                                <div class="hud-panel">
                                    <span class="hud-corner hud-corner-tl" aria-hidden="true"></span>
                                    <span class="hud-corner hud-corner-br" aria-hidden="true"></span>
                                    <span class="hud-vents" aria-hidden="true"></span>
                                    <div class="hud-content" id="telemetry-panel">
                                        <div class="panel-head">
                                            <h2 class="panel-title-muted">"Live Telemetry"</h2>
                                            <div style="display:flex;align-items:center;gap:8px;">
                                                <span class="test-age" id="t-age">{age_txt}</span>
                                                <span class={tele_dot_cls} id="tele-dot"></span>
                                            </div>
                                        </div>

                                        <div id="sys-status" class={sys_cls}>{sys_txt}</div>

                                        <div class="cal-sensor-row"><span class="cal-sensor-label">"Temperature"</span><span class={move || stale_cls("cal-sensor-value")} id="t-temp">{move || val_txt(0)}</span></div>
                                        <div class="cal-sensor-row"><span class="cal-sensor-label">"Voltage"</span><span class={move || stale_cls("cal-sensor-value")} id="t-volt">{move || val_txt(1)}</span></div>
                                        <div class="cal-sensor-row"><span class="cal-sensor-label">"Current"</span><span class={move || stale_cls("cal-sensor-value")} id="t-curr">{move || val_txt(2)}</span></div>
                                        <div class="cal-sensor-row"><span class="cal-sensor-label">"Health"</span><span class={move || stale_cls("cal-sensor-value")} id="t-health">{move || val_txt(3)}</span></div>
                                        <div class="cal-sensor-row"><span class="cal-sensor-label">"Commanded"</span><span class={move || stale_cls("cal-sensor-value")} id="t-speed">{move || val_txt(4)}</span></div>

                                        <div class="test-health-bar-wrap">
                                            <div class="test-health-bar" id="t-health-bar" style="width:0%;" node_ref=health_bar></div>
                                        </div>

                                        <div class="test-beat-wrap">
                                            <span class="test-beat-label">"Vibration"</span>
                                            <div class="test-beats" id="vib-bars" node_ref=beats></div>
                                            <span class="test-beat-read" id="vib-read" node_ref=vib_read>"--"</span>
                                        </div>
                                    </div>
                                </div>

                                <div class="hud-panel">
                                    <span class="hud-corner hud-corner-tl" aria-hidden="true"></span>
                                    <span class="hud-corner hud-corner-br" aria-hidden="true"></span>
                                    <span class="hud-vents" aria-hidden="true"></span>
                                    <div class="hud-content">
                                        <div class={move || if calibrating.get() { "test-guard show" } else { "test-guard" }} id="test-guard">
                                            <span style="font-size:14px;">"\u{26a0}"</span>
                                            <span style="font-size:12px;color:var(--gray-300);" id="test-guard-text">"Motors auto-run during calibration — manual control disabled."</span>
                                        </div>
                                        <div class="cal-info-chips" style="margin-top:12px;">
                                            <div class="cal-info-chip">
                                                <span class="chip-dot" id="chip-cal" style="background:var(--gray-500);" node_ref=chip_cal></span>
                                                <span id="chip-cal-label">{move || cal_name.get()}</span>
                                            </div>
                                            <div class="cal-info-chip">
                                                <span class="chip-dot" id="chip-conn" style="background:var(--dot-red);" node_ref=chip_conn></span>
                                                <span id="chip-conn-label">{move || if ws_up.get() { "Server connected" } else { "Disconnected" }}</span>
                                            </div>
                                        </div>
                                    </div>
                                </div>

                            </div>
                        </div>

                        <div class="panel panel-pad" id="conn-lost-banner" style="display:none;border:1px solid var(--dot-red);margin-top:16px;">
                            <div style="display:flex;align-items:center;gap:12px;">
                                <span style="font-size:14px;">"\u{26a0}"</span>
                                <span style="font-size:12px;color:var(--gray-300);">"Connection lost — motor control is still enabled but telemetry is stale."</span>
                            </div>
                        </div>

                    </div>
                </main>
            </div>
        })}
    }
}
