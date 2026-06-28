// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::path::PathBuf;

pub fn find_chrome() -> Option<PathBuf> {
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

pub fn launch_args(port: u16, user_data_dir: &str) -> Vec<String> {
    vec![
        "--headless=new".into(),
        format!("--remote-debugging-port={port}"),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-gpu".into(),
        format!("--user-data-dir={user_data_dir}"),
        "about:blank".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn args_include_port_and_headless() {
        let a = launch_args(9333, "/tmp/x");
        assert!(a.iter().any(|s| s == "--headless=new"));
        assert!(a.iter().any(|s| s == "--remote-debugging-port=9333"));
        assert!(a.iter().any(|s| s == "--user-data-dir=/tmp/x"));
        assert_eq!(a.last().unwrap(), "about:blank");
    }
}
