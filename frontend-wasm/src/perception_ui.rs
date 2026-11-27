//! Detector boxes shared by the Home and Control camera cards.
//!
//! Perception publishes normalised boxes on the 640x480 stream. Both camera
//! viewports are 16:9 with `object-fit: cover`, so the top and bottom 12.5%
//! of the frame is cropped: the vertical mapping below removes that crop.

use leptos::prelude::*;
use serde_json::Value;

fn box_style(x: f64, y: f64, w: f64, h: f64) -> String {
    let top = (y - 0.125) / 0.75;
    format!(
        "left:{:.2}%;top:{:.2}%;width:{:.2}%;height:{:.2}%;",
        x * 100.0,
        top * 100.0,
        w * 100.0,
        (h / 0.75) * 100.0
    )
}

/// `payload` is the latest `rover/perception/detections` frame, `show` is the
/// camera-live flag (and the NORMAL tab on Home). Boxes below 35% confidence
/// are skipped; the COCO model produces low-confidence noise on room objects.
#[component]
pub fn DetectionOverlay(
    payload: RwSignal<Value>,
    show: Signal<bool>,
) -> impl IntoView {
    view! {
        <div class="cam-overlay">
            {move || {
                if !show.get() {
                    return Vec::new();
                }
                let frame = payload.get();
                let items = frame
                    .get("items")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                items
                    .into_iter()
                    .filter_map(|it| {
                        let label = it.get("label")?.as_str()?.to_string();
                        let conf = it.get("p").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        if conf < 0.35 {
                            return None;
                        }
                        let x = it.get("x")?.as_f64()?;
                        let y = it.get("y")?.as_f64()?;
                        let w = it.get("w")?.as_f64()?;
                        let h = it.get("h")?.as_f64()?;
                        let cls = if label == "person" {
                            "det-box person"
                        } else {
                            "det-box"
                        };
                        let text = match it.get("range_m").and_then(|v| v.as_f64()) {
                            Some(range) => format!("{label} {range:.1} m {:.0}%", conf * 100.0),
                            None => format!("{label} {:.0}%", conf * 100.0),
                        };
                        Some(view! {
                            <div class=cls style=box_style(x, y, w, h)>
                                <span class="det-label">{text}</span>
                            </div>
                        })
                    })
                    .collect::<Vec<_>>()
            }}
        </div>
    }
}
