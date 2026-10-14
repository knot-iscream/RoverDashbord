//! xtask: repo task runner (`cargo launch | dev | doctor | package`).
//!
//! 1:1 replacement for the deleted batch launchers, minus the batch quirks:
//! real sleeps instead of ping-delays, identical behavior with or without a
//! console attached, parsed health output instead of raw dumps. Zero deps.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const HEALTH_PATH: &str = "/api/health";
const DEVICE_PATH: &str = "/api/device/status";

/// Workspace root (xtask's manifest dir is `<root>/xtask`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in <root>/xtask")
        .to_path_buf()
}

/// Split a raw HTTP/1.x response into (status code, body).
fn parse_response(raw: &[u8]) -> Result<(u16, String), String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let code = head
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .parse::<u16>()
        .map_err(|_| "bad status line".to_string())?;
    Ok((code, body.to_string()))
}

/// Plain HTTP/1.0 GET over std TCP (no client dependency needed for checks).
fn http_get(host: &str, port: u16, path: &str, secs: u64) -> Result<(u16, String), String> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| format!("bad addr: {e}"))?;
    let mut sock = TcpStream::connect_timeout(&addr, Duration::from_secs(secs))
        .map_err(|e| format!("connect: {e}"))?;
    sock.set_read_timeout(Some(Duration::from_secs(secs)))
        .map_err(|e| format!("timeout: {e}"))?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    sock.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut raw = Vec::new();
    sock.read_to_end(&mut raw)
        .map_err(|e| format!("read: {e}"))?;
    parse_response(&raw)
}

/// Naive `"key": value` lookup for flat JSON (bools, numbers, strings).
/// The backend shapes are fixed and tiny; a parser crate would be overkill.
fn json_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let at = body.find(&needle)?;
    let after = body[at + needle.len()..].trim_start().strip_prefix(':')?;
    let token: String = after
        .trim_start()
        .chars()
        .take_while(|c| *c != ',' && *c != '}' && *c != '\n')
        .collect();
    let token = token.trim().trim_matches('"').to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn port_open(host: &str, port: u16) -> bool {
    let addr: SocketAddr = match format!("{host}:{port}").parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_ok()
}

/// `cargo` on PATH, else the per-user install (same fallback as the bats).
fn find_cargo() -> Option<PathBuf> {
    if Command::new("cargo")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return Some(PathBuf::from("cargo"));
    }
    let home = std::env::var("USERPROFILE").ok()?;
    let p = PathBuf::from(home)
        .join(".cargo")
        .join("bin")
        .join("cargo.exe");
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

fn append_log(path: &Path, line: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn cargo_version_string(cargo: &Path) -> Option<String> {
    let out = Command::new(cargo).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// ---------------------------------------------------------------------------
// launch — Rover.bat: packaged exe, else backend from source + browser.
// ---------------------------------------------------------------------------

fn cmd_launch() -> i32 {
    let root = repo_root();
    let app = root.join("dist").join("RoverDash").join("roverdash.exe");
    let log = root.join("Rover.log");
    append_log(&log, "----------------------------------------");
    append_log(&log, "RoverDash start");
    if app.is_file() {
        println!("[Rover] Starting RoverDash...");
        match Command::new(&app).current_dir(&root).spawn() {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("[ERROR] Could not start {}: {e}", app.display());
                1
            }
        }
    } else {
        dev_fallback(&root, &log)
    }
}

fn dev_fallback(root: &Path, log: &Path) -> i32 {
    let Some(cargo) = find_cargo() else {
        eprintln!();
        eprintln!("[ERROR] No packaged app (dist\\RoverDash missing) and no Rust toolchain.");
        eprintln!("Either run cargo package on a dev PC, or install Rust from https://rustup.rs/");
        eprintln!();
        return 1;
    };
    if !port_open("127.0.0.1", 1883) {
        // Same fallback as the old bat: Program Files broker, if present.
        // (Service install needs admin; the app sidecar covers the rest.)
        let prog = PathBuf::from(r"C:\Program Files\mosquitto\mosquitto.exe");
        if prog.is_file() {
            let mut cmd = Command::new(prog);
            cmd.arg("-v");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            if cmd.spawn().is_err() {
                append_log(log, "[Rover] broker autostart failed, continuing anyway");
            }
        }
    }
    println!("[Rover] Dev mode: backend from source, dashboard at http://localhost:8000/");
    println!("[Rover] (Run cargo package once for the native window.)");
    let logfile = OpenOptions::new().create(true).append(true).open(log);
    let Ok(logfile) = logfile else {
        eprintln!("[ERROR] Cannot write {}", log.display());
        return 1;
    };
    let child = Command::new(&cargo)
        .arg("run")
        .current_dir(root)
        .stdout(Stdio::from(logfile.try_clone().unwrap_or_else(|_| {
            OpenOptions::new()
                .write(true)
                .open("NUL")
                .expect("NUL exists")
        })))
        .stderr(Stdio::from(logfile))
        .spawn();
    if child.is_err() {
        eprintln!("[ERROR] cargo run failed to start.");
        return 1;
    }
    // Real sleep, not the old ping-delay: identical with/without console.
    for _ in 0..30 {
        if http_get("127.0.0.1", 8000, HEALTH_PATH, 2).is_ok() {
            open_browser("http://localhost:8000/");
            return 0;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    println!("[Rover] Backend is slow to answer - opening anyway, hit refresh if needed.");
    open_browser("http://localhost:8000/");
    0
}

fn open_browser(url: &str) {
    // `start` with an unquoted URL (no spaces possible) opens the browser.
    Command::new("cmd").args(["/C", "start", url]).spawn().ok();
}

// ---------------------------------------------------------------------------
// dev — Rover-dev.bat: desktop shell from source, foreground.
// ---------------------------------------------------------------------------

fn cmd_dev() -> i32 {
    println!("[Rover-dev] Debug build + native windows (main + Rust live view)...");
    println!("[Rover-dev] Real rover data only - no simulation. Close windows or Ctrl+C to stop.");
    let Some(cargo) = find_cargo() else {
        eprintln!();
        eprintln!("[ERROR] Rust toolchain not found - install from https://rustup.rs/");
        eprintln!();
        return 1;
    };
    match Command::new(&cargo)
        .args(["run", "-p", "roverdash"])
        .current_dir(repo_root())
        .status()
    {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("[ERROR] cargo run failed: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// doctor — Doctor.bat: plain-language health checks.
// ---------------------------------------------------------------------------

const PAGES: [(&str, &str); 6] = [
    ("/", "dashboard (Rust UI)"),
    ("/next/", "rust home alias"),
    ("/next/detailed", "detailed"),
    ("/next/test", "motor test"),
    ("/next/calibration", "calibration"),
    ("/setup/", "setup page"),
];

fn cmd_doctor() -> i32 {
    println!("================ Rover Doctor =================");
    println!();
    match find_cargo().and_then(|c| cargo_version_string(&c)) {
        Some(v) => println!("[OK] {v}"),
        None => println!("[X] Rust toolchain: NOT FOUND - install from https://rustup.rs/"),
    }

    println!();
    println!("--- Rust backend on port 8000 ---");
    match http_get("127.0.0.1", 8000, HEALTH_PATH, 5) {
        Ok((200, body)) => {
            println!("{body}");
            let mqtt = json_field(&body, "mqtt_connected").as_deref() == Some("true");
            let dev = json_field(&body, "device_online").as_deref() == Some("true");
            if !mqtt {
                println!("mqtt_connected is false: broker is down - cargo launch starts it, or open the Setup page.");
            }
            if !dev {
                println!(
                    "device_online is false: ESP32 is not publishing - check rover power and WiFi."
                );
            }
        }
        Ok((code, _)) => println!("Backend answered {code} - expected 200 with a JSON body."),
        Err(_) => {
            println!("Backend is NOT running (no answer on :8000). Run cargo launch first.");
        }
    }

    println!();
    println!("--- MQTT broker on port 1883 ---");
    if port_open("127.0.0.1", 1883) {
        println!("[OK] Something is listening on 1883.");
    } else {
        println!("[X] Nothing listening on port 1883.");
        println!("cargo launch starts Mosquitto for you - run it. Service install needs admin.");
    }

    println!();
    println!("--- Dashboard pages, expect 200 each ---");
    for (path, name) in PAGES {
        match http_get("127.0.0.1", 8000, path, 5) {
            Ok((code, _)) => println!("{name}:       {code}"),
            Err(_) => println!("{name}:       unreachable"),
        }
    }

    println!();
    println!("--- ESP32 heartbeat ---");
    match http_get("127.0.0.1", 8000, DEVICE_PATH, 5) {
        Ok((200, body)) => {
            println!("{body}");
            println!();
            println!("online true means the rover is publishing. online false means check rover power and WiFi.");
        }
        _ => println!("No device status - backend may be down."),
    }
    println!();
    println!("================ Done =================");
    0
}

// ---------------------------------------------------------------------------
// package — package.bat: assemble portable dist\RoverDash\.
// ---------------------------------------------------------------------------

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    // xcopy semantics: implied directories are created, not just leaves.
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_tree(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

fn cmd_package() -> i32 {
    let root = repo_root();
    let exe = root.join("target").join("release").join("roverdash.exe");
    if !exe.is_file() {
        eprintln!("[ERROR] target\\release\\roverdash.exe missing - run cargo build --release -p roverdash first.");
        return 1;
    }
    let bundle = root.join("dist").join("next").join("index.html");
    if !bundle.is_file() {
        eprintln!(
            "[ERROR] dist\\next\\index.html missing - run trunk build in frontend-wasm\\ first."
        );
        return 1;
    }
    let dist = root.join("dist").join("RoverDash");
    if dist.exists() {
        std::fs::remove_dir_all(&dist).ok();
    }
    if std::fs::create_dir_all(&dist).is_err() {
        eprintln!("[ERROR] Cannot create {}", dist.display());
        return 1;
    }
    let cp = |from: PathBuf, name: &str| -> bool {
        if std::fs::copy(&from, dist.join(name)).is_err() {
            eprintln!("[ERROR] Copy failed: {}", from.display());
            false
        } else {
            true
        }
    };
    let bin = root.join("desktop").join("binaries");
    if !cp(exe, "roverdash.exe") {
        return 1;
    }
    for f in [
        "mosquitto-x86_64-pc-windows-msvc.exe",
        "mosquitto.dll",
        "pthreadVC3.dll",
    ] {
        if !cp(bin.join(f), f) {
            return 1;
        }
    }
    if copy_tree(&root.join("web"), &dist.join("web")).is_err()
        || copy_tree(
            &root.join("dist").join("next"),
            &dist.join("dist").join("next"),
        )
        .is_err()
    {
        eprintln!("[ERROR] Asset copy failed.");
        return 1;
    }
    println!("[OK] Portable app ready: {}", dist.display());
    println!("     Double-click roverdash.exe. First run creates rover_config.json + history\\ beside it.");
    0
}

fn usage() -> i32 {
    println!("xtask — RoverDash repo tasks:");
    println!("  cargo launch   start the app (packaged exe, else backend from source + browser)");
    println!("  cargo dev      desktop shell from source (was Rover-dev.bat)");
    println!("  cargo doctor   plain-language health checks (was Doctor.bat)");
    println!("  cargo pack     assemble portable dist\\RoverDash\\ (was package.bat;");
    println!("                 `package` alone is cargo's built-in, so the alias is `pack`)");
    0
}

fn main() {
    let code = match std::env::args().nth(1).as_deref() {
        Some("launch") => cmd_launch(),
        Some("dev") => cmd_dev(),
        Some("doctor") => cmd_doctor(),
        Some("package") => cmd_package(),
        Some("-h" | "--help" | "help") | None => usage(),
        Some(other) => {
            eprintln!("[ERROR] Unknown task: {other}");
            usage()
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_splits_status_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        let (code, body) = parse_response(raw).unwrap();
        assert_eq!(code, 200);
        assert_eq!(body, "{\"a\":1}");
    }

    #[test]
    fn response_404_keeps_code() {
        let raw = b"HTTP/1.0 404 Not Found\r\n\r\nnope";
        let (code, body) = parse_response(raw).unwrap();
        assert_eq!(code, 404);
        assert_eq!(body, "nope");
    }

    #[test]
    fn response_garbage_is_an_error() {
        assert!(parse_response(b"hello").is_err());
    }

    #[test]
    fn json_field_reads_bool_number_and_string() {
        let body =
            "{\"mqtt_connected\":true,\"device_online\":false,\"http_port\":8000,\"v\":\"0.1.0\"}";
        assert_eq!(json_field(body, "mqtt_connected").as_deref(), Some("true"));
        assert_eq!(json_field(body, "device_online").as_deref(), Some("false"));
        assert_eq!(json_field(body, "http_port").as_deref(), Some("8000"));
        assert_eq!(json_field(body, "v").as_deref(), Some("0.1.0"));
        assert_eq!(json_field(body, "missing"), None);
    }

    #[test]
    fn page_list_covers_current_routes() {
        let paths: Vec<&str> = PAGES.iter().map(|(p, _)| *p).collect();
        for want in [
            "/",
            "/next/",
            "/next/detailed",
            "/next/test",
            "/next/calibration",
            "/setup/",
        ] {
            assert!(paths.contains(&want), "missing {want}");
        }
    }

    #[test]
    fn response_without_body_is_ok() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
        let (code, body) = parse_response(raw).unwrap();
        assert_eq!(code, 200);
        assert_eq!(body, "");
    }

    #[test]
    fn copy_tree_creates_implied_dirs() {
        let base = std::env::temp_dir().join(format!(
            "xtask-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src = base.join("src");
        let dst = base.join("dst").join("nested");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("top.txt"), "top").unwrap();
        std::fs::write(src.join("sub").join("inner.txt"), "inner").unwrap();
        copy_tree(&src, &dst).unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub").join("inner.txt")).unwrap(),
            "inner"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn json_field_tolerates_spaces() {
        let body = "{ \"online\" : true }";
        assert_eq!(json_field(body, "online").as_deref(), Some("true"));
    }
}
