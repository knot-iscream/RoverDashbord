//! Axum 1:1 port of the v1 Python backend (`backend/server.py`).
//! Single port — `/api/*` + `/ws` + `/setup` first, vendored `web/` pages
//! via static fallback last. Always binds 127.0.0.1 (this PC only —
//! localhost-only by design, no LAN mode). No CORS headers are ever sent:
//! our pages are same-origin, and cross-origin JS gets nothing.

use digital_twin_dashboard::{build_state, init_logging, serve_forever};

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("shutting down");
}

#[tokio::main]
async fn main() {
    init_logging();
    let state = build_state().await;
    let http_port = state.cfg.read().await.http_port;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], http_port));
    tracing::info!("listening on {addr}");
    match serve_forever(state, addr, shutdown_signal()).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            tracing::error!(
                "cannot listen on {addr} ({e}) — is another Rover app already \
                 running? Run Doctor.bat to diagnose."
            );
            std::process::exit(1);
        }
        Err(e) => tracing::error!("server error: {e}"),
    }
}
