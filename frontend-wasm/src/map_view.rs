//! Flat map tab: nav occupancy grid, rover arrow and active goal.
//!
//! Self-contained on purpose. It polls `/api/nav/grid`, `/api/pose` and
//! `/api/autonomy` once a second and owns its canvas, so a problem here
//! cannot touch the Home websocket path again.

use leptos::prelude::*;
use serde_json::Value;
use wasm_bindgen::JsCast;

use crate::ws::get_json;

#[component]
pub fn MapView(cam: RwSignal<u8>) -> impl IntoView {
    let grid = RwSignal::new(Value::Null);
    let pose = RwSignal::new(Value::Null);
    let auto = RwSignal::new(Value::Null);
    let canvas = NodeRef::<leptos::html::Canvas>::new();

    let _iv = gloo_timers::callback::Interval::new(1000, move || {
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(v) = get_json("/api/nav/grid").await {
                if let Some(g) = v.get("grid") {
                    grid.set(g.clone());
                }
            }
            if let Some(v) = get_json("/api/pose").await {
                if let Some(p) = v.get("pose") {
                    pose.set(p.clone());
                }
            }
            if let Some(v) = get_json("/api/autonomy").await {
                if let Some(a) = v.get("autonomy") {
                    auto.set(a.clone());
                }
            }
            if cam.get() == 2 {
                if let Some(cv) = canvas.get() {
                    paint_map(&cv, &grid.get(), &pose.get(), &auto.get());
                }
            }
        });
    });
    _iv.forget();

    view! {
        <canvas
            class="lidar-canvas"
            id="map-canvas"
            width="600"
            height="450"
            style=move || if cam.get() == 2 { "display:block" } else { "display:none" }
            node_ref=canvas
        ></canvas>
    }
}

fn paint_map(canvas: &web_sys::HtmlCanvasElement, grid: &Value, pose: &Value, auto: &Value) {
    let css_w = canvas.client_width() as f64;
    let css_h = canvas.client_height() as f64;
    if css_w < 2.0 || css_h < 2.0 {
        return;
    }
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0)
        .max(1.0);
    let bw = (css_w * dpr).round() as u32;
    let bh = (css_h * dpr).round() as u32;
    if canvas.width() != bw {
        canvas.set_width(bw);
    }
    if canvas.height() != bh {
        canvas.set_height(bh);
    }
    let Ok(Some(rc)) = canvas.get_context("2d") else {
        return;
    };
    let Some(g) = rc.dyn_into::<web_sys::CanvasRenderingContext2d>().ok() else {
        return;
    };
    g.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0).ok();
    g.clear_rect(0.0, 0.0, css_w, css_h);
    g.set_fill_style_str("#050505");
    g.fill_rect(0.0, 0.0, css_w, css_h);

    let Some(res) = grid.get("resolution").and_then(|v| v.as_f64()) else {
        g.set_fill_style_str("#6b7280");
        g.set_font("12px Inter, sans-serif");
        g.fill_text(
            "Waiting for the nav grid. Start the lidar.",
            16.0,
            css_h / 2.0,
        )
        .ok();
        return;
    };
    let (Some(w), Some(h)) = (
        grid.get("width").and_then(|v| v.as_u64()),
        grid.get("height").and_then(|v| v.as_u64()),
    ) else {
        return;
    };
    let origin = grid.get("origin").and_then(|v| v.as_array());
    let (Some(ox), Some(oy)) = (
        origin.and_then(|a| a.first()).and_then(|v| v.as_f64()),
        origin.and_then(|a| a.get(1)).and_then(|v| v.as_f64()),
    ) else {
        return;
    };
    let Some(rle) = grid.get("rle").and_then(|v| v.as_array()) else {
        return;
    };
    let wm = w as f64 * res;
    let hm = h as f64 * res;
    let s = ((css_w - 24.0) / wm).min((css_h - 24.0) / hm);
    let mx = (css_w - wm * s) / 2.0;
    let my = (css_h - hm * s) / 2.0;

    let mut cells = vec![0u8; (w * h) as usize];
    let mut idx = 0usize;
    for run in rle {
        let Some(pair) = run.as_array() else {
            continue;
        };
        let count = pair.first().and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let value = pair.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as u8;
        for _ in 0..count {
            if idx >= cells.len() {
                break;
            }
            cells[idx] = value;
            idx += 1;
        }
    }
    let cell = res * s;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let colour = match cells[y * w as usize + x] {
                2 => "#ef4444",
                1 => "#1f2937",
                _ => "#0a0a0a",
            };
            g.set_fill_style_str(colour);
            let px = mx + (x as f64 * res) * s;
            let py = css_h - my - ((y as f64 + 1.0) * res) * s;
            g.fill_rect(px, py, (cell - 0.3).max(0.5), (cell - 0.3).max(0.5));
        }
    }
    g.set_stroke_style_str("rgba(255,255,255,0.16)");
    g.set_line_width(1.0);
    g.stroke_rect(mx, css_h - my - hm * s, wm * s, hm * s);

    let to_px = |x: f64| mx + (x - ox) * s;
    let to_py = |y: f64| css_h - my - (y - oy) * s;

    if let Some(goal) = auto.get("goal") {
        if let (Some(gx), Some(gy)) = (
            goal.get("x").and_then(|v| v.as_f64()),
            goal.get("y").and_then(|v| v.as_f64()),
        ) {
            g.set_stroke_style_str("#22c55e");
            g.set_line_width(1.5);
            g.begin_path();
            g.arc(to_px(gx), to_py(gy), 5.0, 0.0, std::f64::consts::TAU)
                .ok();
            g.stroke();
        }
    }

    if let (Some(px), Some(py), Some(yaw)) = (
        pose.get("x").and_then(|v| v.as_f64()),
        pose.get("y").and_then(|v| v.as_f64()),
        pose.get("yaw").and_then(|v| v.as_f64()),
    ) {
        g.save();
        let _ = g.translate(to_px(px), to_py(py));
        let _ = g.rotate(-yaw.to_radians());
        g.set_fill_style_str("#f97316");
        g.begin_path();
        g.move_to(9.0, 0.0);
        g.line_to(-6.0, 5.0);
        g.line_to(-6.0, -5.0);
        g.close_path();
        g.fill();
        g.restore();
    }
}
