// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::path::PathBuf;

pub fn find_chrome() -> Option<PathBuf> {
    // 1. Explicit override wins (fail-loud if set-but-missing — preserved).
    if let Ok(p) = std::env::var("WKHTMLTOX_CHROME") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Some(pb);
        }
        eprintln!(
            "wkhtmltox: WKHTMLTOX_CHROME={p:?} does not exist; \
             platform-default Chrome search is disabled when the env var is set"
        );
        return None;
    }
    // 2. A chrome-headless-shell bundled next to our executable (self-contained artifact).
    if let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.to_path_buf()))
    {
        if let Some(bundled) = bundled_chrome_in(&exe) {
            return Some(bundled);
        }
    }
    // 3. System Chrome candidates (config fallback).
    const CANDIDATES: &[&str] = &[
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    ];
    CANDIDATES.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Look for a bundled `chrome-headless-shell` under `dir` (the directory holding
/// our executable). Checks a sibling binary and a `chrome-headless-shell/`
/// subdirectory layout (Chrome for Testing unzips into a versioned subdir, which
/// the packaging script flattens to one of these two shapes).
fn bundled_chrome_in(dir: &std::path::Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "chrome-headless-shell.exe"
    } else {
        "chrome-headless-shell"
    };
    let candidates = [dir.join(name), dir.join("chrome-headless-shell").join(name)];
    candidates.into_iter().find(|p| p.is_file())
}

pub fn launch_args(port: u16, user_data_dir: &str, proxy: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--headless=new".into(),
        format!("--remote-debugging-port={port}"),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-gpu".into(),
        format!("--user-data-dir={user_data_dir}"),
    ];
    if let Some(p) = proxy {
        args.push(format!("--proxy-server={p}"));
    }
    args.push("about:blank".into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sibling_chrome_headless_shell() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "chrome-headless-shell.exe"
        } else {
            "chrome-headless-shell"
        };
        let p = dir.path().join(name);
        std::fs::write(&p, b"#!/bin/sh\n").unwrap();
        let found = bundled_chrome_in(dir.path()).expect("sibling should be found");
        assert_eq!(found, p);
    }

    #[test]
    fn finds_chrome_in_subdir_layout() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "chrome-headless-shell.exe"
        } else {
            "chrome-headless-shell"
        };
        let sub = dir.path().join("chrome-headless-shell");
        std::fs::create_dir_all(&sub).unwrap();
        let p = sub.join(name);
        std::fs::write(&p, b"#!/bin/sh\n").unwrap();
        assert_eq!(bundled_chrome_in(dir.path()).unwrap(), p);
    }

    #[test]
    fn none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(bundled_chrome_in(dir.path()).is_none());
    }

    #[test]
    fn args_include_port_and_headless() {
        let a = launch_args(9333, "/tmp/x", None);
        assert!(a.iter().any(|s| s == "--headless=new"));
        assert!(a.iter().any(|s| s == "--remote-debugging-port=9333"));
        assert!(a.iter().any(|s| s == "--user-data-dir=/tmp/x"));
        assert_eq!(a.last().unwrap(), "about:blank");
    }

    #[test]
    fn args_include_proxy_when_set() {
        let a = launch_args(9333, "/tmp/x", Some("http://proxy.example:8080"));
        assert!(
            a.iter()
                .any(|s| s == "--proxy-server=http://proxy.example:8080"),
            "expected --proxy-server in args: {a:?}"
        );
        assert_eq!(a.last().unwrap(), "about:blank");
    }
}
