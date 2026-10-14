//! Shared app shell for the Rust UI: header, dock-edge, dock, command
//! palette (Ctrl+K), Tauri frameless controls. 1:1 port of the behavioral
//! parts of `web/script.js` (ROVER URL bootstrap excluded — see `ws.rs`).
//!
//! View transitions need no code (CSS `navigation:auto` + `.no-vt`
//! fallback); the `.no-vt` probe + header scroll effect run once per page.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::Element;

fn ls_get(key: &str) -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(key)
        .ok()?
}

fn ls_set(key: &str, val: &str) {
    if let Some(win) = web_sys::window() {
        if let Ok(Some(ls)) = win.local_storage() {
            ls.set_item(key, val).ok();
        }
    }
}

fn evt_key(ev: &web_sys::Event) -> String {
    js_sys::Reflect::get(ev, &JsValue::from_str("key"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

fn evt_flag(ev: &web_sys::Event, name: &str) -> bool {
    js_sys::Reflect::get(ev, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq)]
enum DockSlot {
    Home,
    Calibration,
    Detailed,
    Test,
}

impl DockSlot {
    fn index(self) -> usize {
        match self {
            DockSlot::Home => 0,
            DockSlot::Calibration => 1,
            DockSlot::Detailed => 2,
            DockSlot::Test => 3,
        }
    }
}

fn dock_items() -> Vec<(&'static str, &'static str)> {
    // (href, title) in vanilla dock order: Home, Calibration, Detailed, Test.
    // All four are /next/* now; Setup stays vanilla (unported).
    vec![
        ("/next/", "Home"),
        ("/next/calibration", "Calibration"),
        ("/next/detailed", "Detailed"),
        ("/next/test", "Motor Test"),
    ]
}

#[derive(Clone)]
struct Entry {
    group: &'static str,
    title: String,
    keywords: String,
    href: String,
}

const MOTOR_SHORT: [&str; 4] = ["FL", "FR", "RL", "RR"];
const MOTOR_FULL: [&str; 4] = ["Front Left", "Front Right", "Rear Left", "Rear Right"];

/// Palette entries verbatim (groups/titles/keywords/hrefs adapted to the
/// /next/* tree; Setup + calibration actions stay vanilla).
fn all_entries() -> Vec<Entry> {
    let mut out = vec![];
    for (href, title) in dock_items() {
        out.push(Entry {
            group: "Pages",
            title: format!("Go to {title}"),
            keywords: format!("{} page open go", title.to_lowercase()),
            href: href.to_string(),
        });
    }
    out.push(Entry {
        group: "Pages",
        title: "Go to Setup".to_string(),
        keywords: "setup settings config broker token page".to_string(),
        href: "/setup/".to_string(),
    });
    for (i, short) in MOTOR_SHORT.iter().enumerate() {
        let id = i + 1;
        let kw = format!(
            "{} {} motor m{id}",
            short.to_lowercase(),
            MOTOR_FULL[i].to_lowercase()
        );
        out.push(Entry {
            group: "Motors",
            title: format!("View {} motor", MOTOR_FULL[i]),
            keywords: kw.clone() + " detailed telemetry chart analytics",
            href: format!("/next/detailed?motor={id}"),
        });
        out.push(Entry {
            group: "Motors",
            title: format!("Bench test {} motor", MOTOR_FULL[i]),
            keywords: kw.clone() + " test bench drive slider ramp manual",
            href: format!("/next/test?motor={id}"),
        });
        out.push(Entry {
            group: "Actions",
            title: format!("Ramp test {}", MOTOR_FULL[i]),
            keywords: kw + " ramp cycle fwd rev automatic run",
            href: format!("/next/test?motor={id}"),
        });
    }
    out.push(Entry {
        group: "Actions",
        title: "Start calibration sweep".to_string(),
        keywords: "calibration start sweep baseline begin run collect warmup".to_string(),
        href: "/next/calibration".to_string(),
    });
    out.push(Entry {
        group: "Actions",
        title: "Manual motor control".to_string(),
        keywords: "manual control drive slider motors calibration sliders".to_string(),
        href: "/next/calibration".to_string(),
    });
    out
}

/// Token-AND match over title + keywords; empty query returns everything.
fn match_entries(all: &[Entry], q: &str) -> Vec<Entry> {
    let tokens: Vec<String> = q
        .to_lowercase()
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    if tokens.is_empty() {
        return all.to_vec();
    }
    all.iter()
        .filter(|e| {
            let hay = format!("{} {}", e.title, e.keywords).to_lowercase();
            tokens.iter().all(|t| hay.contains(t))
        })
        .cloned()
        .collect()
}

/// Active dock slot by pathname containment, vanilla loop order (later
/// matches override; "/next/" only ever matches Home first).
fn active_for(path: &str) -> DockSlot {
    let mut active = DockSlot::Home;
    for (href, _) in dock_items() {
        if path.contains(href) {
            active = match href {
                "/next/detailed" => DockSlot::Detailed,
                "/next/test" => DockSlot::Test,
                "/next/calibration" => DockSlot::Calibration,
                _ => DockSlot::Home,
            };
        }
    }
    active
}

fn same_path(href: &str) -> bool {
    let path = web_sys::window()
        .map(|w| w.location().pathname().unwrap_or_default())
        .unwrap_or_default();
    let norm = |s: &str| s.trim_end_matches('/').to_string();
    norm(href) == norm(&path)
}

fn go(href: &str) {
    if let Some(win) = web_sys::window() {
        win.location().set_href(href).ok();
    }
}

fn set_inner_svg(el: &Element, inner: &str) {
    el.set_inner_html(inner);
}

// ---------------------------------------------------------------------------
// Tauri frameless controls (runs only under window.__TAURI__).
// Every bridge call degrades to hidden controls + one console warning.
// ---------------------------------------------------------------------------

fn tauri_win() -> Option<JsValue> {
    let win = web_sys::window()?;
    let tauri = js_sys::Reflect::get(&win, &JsValue::from_str("__TAURI__")).ok()?;
    if tauri.is_undefined() {
        return None;
    }
    let wobj = js_sys::Reflect::get(&tauri, &JsValue::from_str("window")).ok()?;
    let get_current = js_sys::Reflect::get(&wobj, &JsValue::from_str("getCurrentWindow")).ok()?;
    let func: js_sys::Function = get_current.dyn_into().ok()?;
    func.call0(&wobj).ok()
}

fn hide_controls(reason: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Ok(bars) = doc.query_selector_all(".win-controls") {
            for i in 0..bars.length() {
                if let Some(n) = bars.item(i) {
                    if let Ok(el) = n.dyn_into::<Element>() {
                        el.set_attribute("style", "display:none;").ok();
                    }
                }
            }
        }
    }
    web_sys::console::warn_1(&JsValue::from_str(&format!(
        "[win-controls] disabled: {reason}"
    )));
}

/// Call a Tauri window method; `after` receives the resolved value.
/// Rejections / missing methods hide the controls (never dead buttons).
fn bridge(win: &JsValue, name: &str, after: Option<std::rc::Rc<dyn Fn(JsValue)>>) {
    let func: Option<js_sys::Function> = js_sys::Reflect::get(win, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into().ok());
    let Some(func) = func else {
        hide_controls(&format!("no method: {name}"));
        return;
    };
    let out = func.call0(win);
    let Ok(out) = out else {
        hide_controls(&format!("call failed: {name}"));
        return;
    };
    let is_promise = js_sys::Reflect::get(&out, &JsValue::from_str("then"))
        .ok()
        .is_some_and(|t| t.is_function());
    if !is_promise {
        if let Some(f) = after {
            f(out);
        }
        return;
    }
    let then: js_sys::Function = js_sys::Reflect::get(&out, &JsValue::from_str("then"))
        .ok()
        .and_then(|v| v.dyn_into().ok())
        .expect("checked thenable");
    let name_ok = name.to_string();
    let ok_cb = Closure::<dyn FnMut(JsValue)>::new(move |v: JsValue| {
        if let Some(f) = after.as_ref() {
            f(v);
        }
        let _ = &name_ok;
    });
    let name_err = name.to_string();
    let err_cb = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
        hide_controls(&format!("denied: {name_err}"));
    });
    then.call2(
        &out,
        ok_cb.as_ref().unchecked_ref(),
        err_cb.as_ref().unchecked_ref(),
    )
    .ok();
    ok_cb.forget();
    err_cb.forget();
}

const SVG_MIN: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"12\" height=\"12\" viewBox=\"0 0 12 12\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M1.5 6h9\"/></svg>";
const SVG_MAX: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"12\" height=\"12\" viewBox=\"0 0 12 12\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><rect x=\"2.5\" y=\"2.5\" width=\"7\" height=\"7\"/></svg>";
const SVG_RESTORE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"12\" height=\"12\" viewBox=\"0 0 12 12\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M4.5 1.5h6v6M1.5 4.5v6h6\"/></svg>";
const SVG_CLOSE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"12\" height=\"12\" viewBox=\"0 0 12 12\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M2 2l8 8M10 2l-8 8\"/></svg>";

fn sync_max(win: &JsValue) {
    let win = win.clone();
    bridge(
        &win,
        "isMaximized",
        Some(std::rc::Rc::new(move |m: JsValue| {
            let maxed = m.as_bool().unwrap_or(false);
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                if let Ok(btns) = doc.query_selector_all(".win-btn[data-win=\"max\"]") {
                    for i in 0..btns.length() {
                        if let Some(n) = btns.item(i) {
                            if let Ok(el) = n.dyn_into::<Element>() {
                                el.set_inner_html(if maxed { SVG_RESTORE } else { SVG_MAX });
                                el.set_attribute(
                                    "title",
                                    if maxed { "Restore" } else { "Maximize" },
                                )
                                .ok();
                                el.set_attribute(
                                    "aria-label",
                                    if maxed { "Restore" } else { "Maximize" },
                                )
                                .ok();
                            }
                        }
                    }
                }
            }
        })),
    );
}

fn init_tauri() {
    let Some(win) = tauri_win() else { return };
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(slots) = doc.query_selector_all(".header-actions") else {
        return;
    };
    if slots.length() == 0 {
        return;
    }
    for i in 0..slots.length() {
        let Some(slot) = slots.item(i) else { continue };
        let bar = doc.create_element("div").expect("create div");
        bar.set_attribute("class", "win-controls").ok();
        bar.set_inner_html(&format!(
            "<button type=\"button\" class=\"win-btn\" data-win=\"min\" title=\"Minimize\" aria-label=\"Minimize\">{SVG_MIN}</button>\
             <button type=\"button\" class=\"win-btn\" data-win=\"max\" title=\"Maximize\" aria-label=\"Maximize\">{SVG_MAX}</button>\
             <button type=\"button\" class=\"win-btn\" data-win=\"close\" title=\"Close\" aria-label=\"Close\">{SVG_CLOSE}</button>"
        ));
        slot.append_child(&bar).ok();
    }
    if let Ok(btns) = doc.query_selector_all(".win-btn") {
        for i in 0..btns.length() {
            if let Some(n) = btns.item(i) {
                if let Ok(el) = n.dyn_into::<Element>() {
                    let el2 = el.clone();
                    let win = win.clone();
                    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                        e.stop_propagation();
                        let k = el2.get_attribute("data-win").unwrap_or_default();
                        if k == "min" {
                            bridge(&win, "minimize", None);
                        } else if k == "max" {
                            sync_max(&win);
                            bridge(&win, "toggleMaximize", None);
                            sync_max(&win);
                        } else if k == "close" {
                            bridge(&win, "close", None);
                        }
                    });
                    el.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
                        .ok();
                    cb.forget();
                }
            }
        }
    }
    if let Some(header) = doc.get_element_by_id("site-header") {
        let w1 = win.clone();
        let down = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let button = js_sys::Reflect::get(&e, &JsValue::from_str("button"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);
            if button != 0.0 {
                return;
            }
            let skip = e
                .target()
                .and_then(|t| t.dyn_into::<Element>().ok())
                .and_then(|el| {
                    el.closest("button, a, input, .search-bar, .win-controls")
                        .ok()
                        .flatten()
                })
                .is_some();
            if !skip {
                bridge(&w1, "startDragging", None);
            }
        });
        header
            .add_event_listener_with_callback("mousedown", down.as_ref().unchecked_ref())
            .ok();
        down.forget();
        let w2 = win.clone();
        let dbl = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let skip = e
                .target()
                .and_then(|t| t.dyn_into::<Element>().ok())
                .and_then(|el| el.closest("button, a, input, .search-bar").ok().flatten())
                .is_some();
            if !skip {
                bridge(&w2, "toggleMaximize", None);
                sync_max(&w2);
            }
        });
        header
            .add_event_listener_with_callback("dblclick", dbl.as_ref().unchecked_ref())
            .ok();
        dbl.forget();
    }
    // Max-glyph honesty on external resizes (250ms throttle).
    let w3 = win.clone();
    if let Ok(cb_prop) = js_sys::Reflect::get(&win, &JsValue::from_str("onResized")) {
        if cb_prop.is_function() {
            let func: &js_sys::Function = cb_prop.unchecked_ref();
            let last = std::rc::Rc::new(std::cell::Cell::new(0f64));
            let sub = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
                let now = js_sys::Date::now();
                if now - last.get() > 250.0 {
                    last.set(now);
                    sync_max(&w3);
                }
            });
            let _ = func.call1(&win, sub.as_ref().unchecked_ref());
            sub.forget();
        }
    }
    sync_max(&win);
}

// ---------------------------------------------------------------------------

/// Chevron/lock glyph swaps (verbatim paths from vanilla).
const CHEV_OPEN: &str = "<path d=\"m15 18-6-6 6-6\"/>";
const CHEV_CLOSED: &str = "<path d=\"m9 18 6-6-6-6\"/>";
const LOCK_LOCKED: &str = "<rect width=\"18\" height=\"11\" x=\"3\" y=\"11\" rx=\"2\" ry=\"2\"/><path d=\"M7 11V7a5 5 0 0 1 10 0v4\"/>";
const LOCK_OPEN: &str = "<rect width=\"18\" height=\"11\" x=\"3\" y=\"11\" rx=\"2\" ry=\"2\"/><path d=\"M7 11V7a5 5 0 0 1 9.9-1\"/>";

/// Shared shell as a plain function (NOT a `#[component]`): the `Children`
/// prop type requires `Send`, but page content holds `Rc` handles. Taking
/// the built view by value needs no such bound.
pub fn app_shell(content: impl IntoView) -> impl IntoView {
    let path = web_sys::window()
        .map(|w| w.location().pathname().unwrap_or_default())
        .unwrap_or_default();
    let dock_active = RwSignal::new(active_for(&path).index());
    let dock_hidden = RwSignal::new(ls_get("dock-open").is_some_and(|v| v == "false"));
    let dock_locked = RwSignal::new(ls_get("dock-locked").is_some_and(|v| v == "true"));
    let navigating = std::rc::Rc::new(std::cell::Cell::new(false));

    let dock_ref = NodeRef::<leptos::html::Aside>::new();
    let indicator_ref = NodeRef::<leptos::html::Div>::new();
    let chev_ref = NodeRef::<leptos::html::Button>::new();
    let lock_ref = NodeRef::<leptos::html::Button>::new();
    let pal_input = NodeRef::<leptos::html::Input>::new();
    let pal_list = NodeRef::<leptos::html::Div>::new();

    let pal_open = RwSignal::new(false);
    let pal_rows = RwSignal::new(Vec::<Entry>::new());
    let pal_sel = RwSignal::new(0usize);
    let entries = all_entries();

    // -- dock paint helpers (imperative, refs post-mount) --
    let paint_chev = {
        move |visible: bool| {
            if let Some(btn) = chev_ref.get() {
                if let Ok(Some(svg)) = btn.query_selector("svg") {
                    set_inner_svg(&svg, if visible { CHEV_OPEN } else { CHEV_CLOSED });
                }
            }
        }
    };
    let paint_lock = {
        move |locked: bool| {
            if let Some(btn) = lock_ref.get() {
                if let Ok(Some(svg)) = btn.query_selector("svg") {
                    set_inner_svg(&svg, if locked { LOCK_LOCKED } else { LOCK_OPEN });
                }
                btn.set_attribute(
                    "title",
                    if locked {
                        "Unlock dock"
                    } else {
                        "Lock dock hidden"
                    },
                )
                .ok();
            }
        }
    };
    let place_indicator = {
        move |instant: bool| {
            let y = format!("translateY({}px)", dock_active.get() * (42 + 12));
            if let Some(bar) = indicator_ref.get() {
                let el: &Element = bar.as_ref();
                if instant {
                    el.set_attribute("style", "transition:none;").ok();
                    el.set_attribute("style", &format!("transition:none;transform:{y};"))
                        .ok();
                    let _ = el.get_bounding_client_rect();
                    el.remove_attribute("style").ok();
                    el.set_attribute("style", &format!("transform:{y};")).ok();
                } else {
                    // Preserve any stylesheet transition; just move.
                    let cur = el.get_attribute("style").unwrap_or_default();
                    let base = cur
                        .split(';')
                        .filter(|p| {
                            let p = p.trim();
                            !(p.starts_with("transform") || p.starts_with("transition"))
                                && !p.is_empty()
                        })
                        .collect::<Vec<_>>()
                        .join(";");
                    el.set_attribute("style", &format!("{base};transform:{y};"))
                        .ok();
                }
            }
        }
    };

    let set_visible = {
        move |visible: bool, persist: bool| {
            dock_hidden.set(!visible);
            if persist {
                ls_set("dock-open", if visible { "true" } else { "false" });
            }
            paint_chev(visible);
        }
    };

    // -- post-mount init (refs live now) --
    {
        wasm_bindgen_futures::spawn_local(async move {
            let locked = dock_locked.get();
            let hidden = dock_hidden.get();
            set_visible(!hidden && !locked, false);
            if locked {
                dock_hidden.set(true);
                paint_lock(true);
            }
            place_indicator(true);
            init_tauri();
        });
    }

    // -- outside-click closes (lock exempts) --
    {
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                    if dock_locked.get() {
                        return;
                    }
                    let in_dock = e
                        .target()
                        .and_then(|t| t.dyn_into::<Element>().ok())
                        .is_some_and(|el| dock_ref.get().is_some_and(|d| d.contains(Some(&el))));
                    let in_edge = e
                        .target()
                        .and_then(|t| t.dyn_into::<Element>().ok())
                        .and_then(|el| el.closest("#dock-edge").ok().flatten())
                        .is_some();
                    if !in_dock && !in_edge {
                        set_visible(false, true);
                    }
                });
                doc.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
                    .ok();
                cb.forget();
            }
        });
    }

    // -- palette open/close/go --
    let pal_go = move |href: String| {
        pal_open.set(false);
        go(&href);
    };
    let pal_select = {
        move |i: usize| {
            let rows = pal_rows.get();
            if rows.is_empty() {
                return;
            }
            let i = if i >= rows.len() { 0 } else { i };
            pal_sel.set(i);
            if let Some(list) = pal_list.get() {
                if let Ok(kids) = list.query_selector_all(".cmdk-row") {
                    for k in 0..kids.length() {
                        if let Some(n) = kids.item(k) {
                            if let Ok(el) = n.dyn_into::<Element>() {
                                el.class_list()
                                    .toggle_with_force("selected", k as usize == i)
                                    .ok();
                                if k as usize == i {
                                    el.scroll_into_view_with_bool(false);
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    let pal_render = {
        move |q: String| {
            let hits = match_entries(&entries, &q);
            pal_rows.set(hits.clone());
            if let Some(list) = pal_list.get() {
                if hits.is_empty() {
                    list.set_text_content(Some(&format!("No results for \u{201c}{q}\u{201d}")));
                } else {
                    list.set_text_content(None);
                    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                        let mut last_group = "";
                        for e in &hits {
                            if e.group != last_group {
                                last_group = e.group;
                                if let Ok(g) = doc.create_element("div") {
                                    g.set_attribute("class", "cmdk-group").ok();
                                    g.set_text_content(Some(e.group));
                                    list.append_child(&g).ok();
                                }
                            }
                            if let Ok(b) = doc.create_element("button") {
                                b.set_attribute("class", "cmdk-row").ok();
                                b.set_attribute("type", "button").ok();
                                b.set_attribute("data-href", &e.href).ok();
                                let href = e.href.clone();
                                let pal_go = pal_go;
                                let go_cb = Closure::<dyn FnMut(web_sys::Event)>::new(
                                    move |_: web_sys::Event| {
                                        pal_go(href.clone());
                                    },
                                );
                                b.add_event_listener_with_callback(
                                    "click",
                                    go_cb.as_ref().unchecked_ref(),
                                )
                                .ok();
                                go_cb.forget();
                                if let Ok(label) = doc.create_element("span") {
                                    label.set_text_content(Some(&e.title));
                                    b.append_child(&label).ok();
                                }
                                list.append_child(&b).ok();
                            }
                        }
                    }
                }
            }
            pal_select(0);
        }
    };
    let pal_open_fn = {
        let pal_render = pal_render.clone();
        move || {
            pal_open.set(true);
            pal_render(String::new());
            if let Some(inp) = pal_input.get() {
                inp.set_value("");
                inp.focus().ok();
            }
        }
    };
    let pal_close = move || {
        pal_open.set(false);
        if let Some(win) = web_sys::window() {
            if let Some(doc) = win.document() {
                if let Some(active) = doc.active_element() {
                    if let Ok(html) = active.dyn_into::<web_sys::HtmlElement>() {
                        html.blur().ok();
                    }
                }
            }
        }
    };

    // -- global Ctrl+K / Esc (generic Event + Reflect, no KeyboardEvent type) --
    {
        let pal_open_fn = pal_open_fn.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                    let key = evt_key(&e);
                    let modu = evt_flag(&e, "ctrlKey") || evt_flag(&e, "metaKey");
                    if modu && (key == "k" || key == "K") {
                        e.prevent_default();
                        if pal_open.get() {
                            pal_close();
                        } else {
                            pal_open_fn();
                        }
                    } else if key == "Escape" && pal_open.get() {
                        pal_close();
                    }
                });
                doc.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
                    .ok();
                cb.forget();
            }
        });
    }

    // -- dock item tap: glide-then-navigate (300ms), same-page no-op --
    let on_dock = {
        let navigating = navigating.clone();
        move |idx: usize| {
            if navigating.get() {
                return;
            }
            let href = dock_items()[idx].0;
            if same_path(href) {
                return;
            }
            dock_active.set(idx);
            place_indicator(false);
            navigating.set(true);
            let href = href.to_string();
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(300).await;
                go(&href);
            });
        }
    };

    let on_chev = {
        move |_| {
            if dock_locked.get() {
                return;
            }
            set_visible(dock_hidden.get(), true);
        }
    };
    let on_lock = {
        move |_| {
            let locked = !dock_locked.get();
            dock_locked.set(locked);
            ls_set("dock-locked", &locked.to_string());
            if locked {
                set_visible(false, true);
            }
            paint_lock(locked);
            if !locked {
                set_visible(true, true);
            }
        }
    };

    // Palette input events (typed: input=Event per Leptos 0.8 descriptors).
    let pal_input_ev = {
        let pal_render = pal_render.clone();
        move |ev: web_sys::Event| {
            let q = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                .map(|i| i.value())
                .unwrap_or_default();
            pal_render(q);
        }
    };
    let pal_key = {
        move |ev: web_sys::KeyboardEvent| {
            // Key readout via Reflect so no KeyboardEvent feature is needed.
            let key = evt_key(&ev);
            if key == "ArrowDown" {
                ev.prevent_default();
                pal_select(pal_sel.get() + 1);
            } else if key == "ArrowUp" {
                ev.prevent_default();
                pal_select(pal_sel.get().saturating_sub(1));
            } else if key == "Enter" {
                let rows = pal_rows.get();
                if let Some(href) = rows.get(pal_sel.get()).map(|e| e.href.clone()) {
                    pal_go(href);
                }
            } else if key == "Escape" {
                pal_close();
            }
        }
    };

    let search_open = {
        let pal_open_fn = pal_open_fn.clone();
        move |_| pal_open_fn()
    };
    let search_key = {
        let pal_open_fn = pal_open_fn.clone();
        move |ev: web_sys::KeyboardEvent| {
            let key = evt_key(&ev);
            if key == "Enter" || key == " " {
                ev.prevent_default();
                pal_open_fn();
            }
        }
    };

    view! {
        <div class="app-wrap">
            <header class="header" id="site-header">
                <div class="header-bg"></div>
                <div class="header-glow-left"></div>
                <div class="header-glow-right"></div>
                <div class="header-inner">
                    <div
                        class="search-bar"
                        role="button"
                        tabindex="0"
                        aria-label="Search dashboard (Ctrl+K)"
                        on:click=search_open
                        on:keydown=search_key
                    >
                        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="search-icon">
                            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.34-4.34"/>
                        </svg>
                        <span class="search-text">"Search dashboard..."</span>
                        <div class="search-kbd">
                            <kbd>"Ctrl"</kbd>
                            <kbd>"K"</kbd>
                        </div>
                    </div>
                    <div class="header-actions"></div>
                </div>
            </header>
            <div class="dock-edge" id="dock-edge">
                <button class="dock-edge-btn dock-edge-chevron" id="dock-edge-toggle" title="Toggle dock" node_ref=chev_ref on:click=on_chev>
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>
                </button>
                <button class="dock-edge-btn dock-edge-sm" id="dock-edge-lock" title="Lock dock hidden" node_ref=lock_ref on:click=on_lock>
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/></svg>
                </button>
                <a href="#" class="dock-edge-btn dock-edge-sm" title="Status">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2"/></svg>
                </a>
            </div>
            <aside
                class={move || {
                    let mut c = "nav-dock".to_string();
                    if dock_hidden.get() {
                        c += " dock-hidden";
                    }
                    if dock_locked.get() {
                        c += " dock-locked";
                    }
                    c
                }}
                id="nav-dock"
                node_ref=dock_ref
            >
                <div class="dock-inner">
                    <div class="dock-indicator" id="dock-indicator" node_ref=indicator_ref><div class="dock-accent"></div></div>
                    {dock_items()
                        .into_iter()
                        .enumerate()
                        .map(|(i, (href, title))| {
                            let on_dock = on_dock.clone();
                            // Dock glyphs, byte-identical paths to vanilla.
                            // Branches differ in type, so each is erased to
                            // AnyView (same pattern as the page router).
                            let icon: AnyView = match i {
                                0 => view! {
                                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
                                }
                                .into_any(),
                                1 => view! {
                                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v18h18"/><path d="m19 9-5 5-4-4-3 3"/></svg>
                                }
                                .into_any(),
                                2 => view! {
                                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20V10"/><path d="M18 20V4"/><path d="M6 20v-4"/></svg>
                                }
                                .into_any(),
                                _ => view! {
                                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/></svg>
                                }
                                .into_any(),
                            };
                            view! {
                                <a
                                    class={move || if dock_active.get() == i { "dock-item active" } else { "dock-item" }}
                                    href={href}
                                    title={title}
                                    on:click=move |ev: web_sys::MouseEvent| {
                                        ev.prevent_default();
                                        on_dock(i);
                                    }
                                >
                                    {icon}
                                    <div class="dock-tooltip">{title}</div>
                                </a>
                            }
                        })
                        .collect::<Vec<_>>()}
                    <div class="dock-sep"></div>
                </div>
            </aside>
            {content}
            <div class={move || if pal_open.get() { "cmdk-backdrop open" } else { "cmdk-backdrop" }} id="cmdk-backdrop">
                <div class="cmdk-panel" role="dialog" aria-label="Search dashboard">
                    <div class="cmdk-input-row">
                        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.34-4.34"/></svg>
                        <input id="cmdk-input" type="text" placeholder="Search pages, motors, actions..." autocomplete="off" spellcheck="false" node_ref=pal_input on:input=pal_input_ev on:keydown=pal_key />
                        <kbd>"esc"</kbd>
                    </div>
                    <div class="cmdk-list" id="cmdk-list" node_ref=pal_list></div>
                    <div class="cmdk-foot"><span><kbd>"↑↓"</kbd>" navigate"</span><span><kbd>"⏎"</kbd>" open"</span><span><kbd>"esc"</kbd>" close"</span></div>
                </div>
            </div>
        </div>
    }
}
