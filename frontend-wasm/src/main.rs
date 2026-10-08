//! Phase B (opt-in) Leptos shell — CSR only, vanilla stays default.
//! Served at `/next/*` by Axum; the router picks the page from the path.
//! Nothing here is reachable unless the URL carries `?ui=next`.

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::StaticSegment;

mod ws;

#[component]
fn Placeholder(page: &'static str) -> impl IntoView {
    view! {
        <main style="font-family: system-ui; max-width: 40em; margin: 3em auto; padding: 0 1em;">
            <h1>{page} " (next)"</h1>
            <p>
                "This page has no Leptos UI yet — the live dashboard is the vanilla one at "
                <a href="/">"/"</a> "."
            </p>
            <p>"Shared WS client status: " {ws::status_text()}</p>
        </main>
    }
}

#[component]
fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes fallback=|| view! { <Placeholder page="unknown" /> }>
                <Route path=StaticSegment("") view=|| view! { <Placeholder page="home" /> } />
                <Route
                    path=StaticSegment("test")
                    view=|| view! { <Placeholder page="motor test" /> }
                />
                <Route
                    path=StaticSegment("detailed")
                    view=|| view! { <Placeholder page="detailed" /> }
                />
                <Route
                    path=StaticSegment("calibration")
                    view=|| view! { <Placeholder page="calibration" /> }
                />
            </Routes>
        </Router>
    }
}

fn main() {
    mount_to_body(App);
}
