//! Boot splash, owned by Rust: this constant is the single source of truth
//! for the Tauri splash window. Cargo regenerates `web/splash.html` from it
//! on every build (the file is gitignored); Tauri bundles/loads the
//! generated file exactly like the old static one, so the boot sequence is
//! byte-identical with zero vanilla sources in the tree.

fn main() {
    tauri_build::build();

    const SPLASH_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Starting RoverDash…</title>
<style>
  html, body {
    margin: 0; height: 100%; background: #0b0d10; color: #e8e8e8;
    font-family: system-ui, sans-serif; display: flex;
    align-items: center; justify-content: center;
  }
  .box { text-align: center; }
  .ring {
    width: 44px; height: 44px; margin: 0 auto 14px;
    border: 4px solid #2a2e35; border-top-color: #ffb020;
    border-radius: 50%; animation: spin 0.9s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .t { font-size: 15px; font-weight: 600; }
  .s { font-size: 12px; color: #8a8f98; margin-top: 4px; }
</style>
</head>
<body>
<div class="box">
  <div class="ring"></div>
  <div class="t">RoverDash</div>
  <div class="s">starting broker and backend…</div>
</div>
</body>
</html>
"#;

    let out = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("..")
        .join("web")
        .join("splash.html");
    let fresh = std::fs::read_to_string(&out).is_ok_and(|s| s == SPLASH_HTML);
    if !fresh {
        std::fs::write(&out, SPLASH_HTML).expect("write generated web/splash.html");
    }
    println!("cargo::rerun-if-changed=build.rs");
}
