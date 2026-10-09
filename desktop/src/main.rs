//! RoverDash desktop shell: native window around the in-process backend.
//! The backend serves API + WS + pages on 127.0.0.1:8000 exactly as the
//! standalone binary does; this shell just hosts it and shows it. No browser,
//! no address bar.
//!
//! Supervisor duties (this file): start the bundled Mosquitto sidecar when
//! nothing listens on 1883, start the backend, wait for health, then swap the
//! splash window for the main one. The sidecar is killed on app exit.
//! (Tray + single-instance arrive next.)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use digital_twin_dashboard::{build_state, init_logging, serve_forever};
use tauri::Manager;

type ChildSlot = Arc<Mutex<Option<std::process::Child>>>;

/// Where the sidecar broker lives: `<dir>/mosquitto-*.exe` with its DLLs
/// beside it. Searches the exe folder, then up to 3 ancestors, then the CWD
/// the same way — covers the packaged layout (beside the exe), a dev
/// double-click from `target/debug` (via `desktop/binaries` two up), and
/// `cargo run` from anywhere in the checkout.
fn sidecar_paths() -> Option<(PathBuf, PathBuf)> {
    const EXE: &str = "mosquitto-x86_64-pc-windows-msvc.exe";
    let mut roots = vec![];
    if let Ok(me) = std::env::current_exe() {
        if let Some(d) = me.parent() {
            roots.push(d.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    for root in roots {
        let mut dir = Some(root.as_path());
        for _ in 0..4 {
            let Some(d) = dir else { break };
            if d.join(EXE).is_file() {
                return Some((d.join(EXE), d.to_path_buf()));
            }
            let dev = d.join("desktop").join("binaries");
            if dev.join(EXE).is_file() {
                return Some((dev.join(EXE), dev));
            }
            dir = d.parent();
        }
    }
    None
}

async fn tcp_open(addr: &str) -> bool {
    tokio::net::TcpStream::connect(addr).await.is_ok()
}

/// Start bundled Mosquitto unless something already listens on 1883
/// (external broker wins — never fight it). Returns the child when WE
/// started it, so exit can kill exactly what we own.
fn ensure_broker() -> Option<std::process::Child> {
    if std::net::TcpStream::connect("127.0.0.1:1883").is_ok() {
        tracing::info!("broker already on 1883, sidecar not started");
        return None;
    }
    let Some((exe, workdir)) = sidecar_paths() else {
        tracing::warn!(
            "no broker on 1883 and no sidecar found — dashboard will show NOT connected"
        );
        return None;
    };
    let mut cmd = std::process::Command::new(&exe);
    cmd.current_dir(&workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    match cmd.spawn() {
        Ok(child) => {
            tracing::info!("mosquitto sidecar started (pid {})", child.id());
            Some(child)
        }
        Err(e) => {
            tracing::warn!("could not start mosquitto sidecar: {e}");
            None
        }
    }
}

/// Drain the sidecar's piped output into the log so broker errors are visible.
fn drain_logs(mut child: Option<std::process::Child>) -> Option<std::process::Child> {
    let c = child.as_mut()?;
    let out = c.stdout.take();
    let err = c.stderr.take();
    tauri::async_runtime::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        if let Some(o) = out {
            if let Ok(std) = tokio::process::ChildStdout::from_std(o) {
                let mut lines = BufReader::new(std).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    tracing::debug!(target: "mosquitto", "{l}");
                }
            }
        }
        if let Some(e) = err {
            if let Ok(std) = tokio::process::ChildStderr::from_std(e) {
                let mut lines = BufReader::new(std).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    tracing::warn!(target: "mosquitto", "{l}");
                }
            }
        }
    });
    child
}

/// Boot order: broker → backend → navigate + show main, close splash.
/// Runs to completion once; failures leave the splash up with the log.
async fn bring_up(handle: tauri::AppHandle, port: u16, broker: ChildSlot) {
    if let Some(child) = drain_logs(ensure_broker()) {
        *broker.lock().unwrap() = Some(child);
    }
    let url: url::Url = format!("http://127.0.0.1:{port}")
        .parse()
        .expect("app url parses");
    let probe = format!("127.0.0.1:{port}");
    for _ in 0..300 {
        if tcp_open(&probe).await {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if let Some(win) = handle.get_webview_window("main") {
        let _ = win.navigate(url);
        let _ = win.show();
    }
    if let Some(splash) = handle.get_webview_window("splash") {
        let _ = splash.close();
    }
}

/// Show (and focus) the main window. Used by the tray, single-instance,
// and anywhere else that needs to summon the app.
fn show_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Tray icon: left-click or Show summons the window, Quit exits through the
/// normal path (which stops the sidecar broker).
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = tauri::menu::MenuItem::with_id(app, "show", "Show RoverDash", true, None::<&str>)?;
    let quit = tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&show, &quit])?;
    // Reuse the bundled window icon — no separate tray asset to maintain.
    let Some(icon) = app.default_window_icon().cloned() else {
        tracing::warn!("no window icon bundled; tray icon skipped");
        return Ok(());
    };
    tauri::tray::TrayIconBuilder::new()
        .icon(icon)
        .tooltip("RoverDash")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn main() {
    init_logging();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Second launch: summon the running instance instead of starting over.
            show_main(app);
        }))
        .setup(|app| {
            build_tray(app.handle())?;
            let port = digital_twin_dashboard::config::AppConfig::load().http_port;
            let handle = app.handle().clone();
            let broker: ChildSlot = Arc::new(Mutex::new(None));
            app.manage(broker.clone());
            tauri::async_runtime::spawn(async move {
                let state = build_state().await;
                let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
                tracing::info!("backend listening on {addr}");
                let _ = serve_forever(state, addr, std::future::pending::<()>()).await;
            });
            tauri::async_runtime::spawn(bring_up(handle, port, broker));
            Ok(())
        })
        .on_window_event(|win, event| {
            // X minimizes to the tray instead of quitting (quit lives in the
            // tray menu, which runs the normal exit path incl. sidecar stop).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = win.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("tauri runtime failed")
        .run(|app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                // Kill exactly the sidecar we started, if any.
                if let Some(slot) = app_handle.try_state::<ChildSlot>() {
                    if let Ok(mut guard) = slot.lock() {
                        if let Some(mut child) = guard.take() {
                            let _ = child.kill();
                            let _ = child.wait();
                            tracing::info!("mosquitto sidecar stopped");
                        }
                    }
                }
            }
        });
}
