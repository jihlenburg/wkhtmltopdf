// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::time::Duration;
use tungstenite::{connect as ws_connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use std::net::TcpStream;
use wkhtmltox_core::{Result, WkError};

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

pub struct Cdp { sock: Sock, next_id: u64 }

/// Build a CDP request frame. Pure + unit-testable.
pub fn request_frame(id: u64, method: &str, params: &serde_json::Value) -> String {
    serde_json::json!({ "id": id, "method": method, "params": params }).to_string()
}

/// Classify an incoming CDP frame: Some((id, result_or_error)) for responses, None for events.
pub fn parse_response(txt: &str, want_id: u64) -> Option<std::result::Result<serde_json::Value, String>> {
    let v: serde_json::Value = serde_json::from_str(txt).ok()?;
    if v.get("id").and_then(|x| x.as_u64()) == Some(want_id) {
        if let Some(err) = v.get("error") { return Some(Err(err.to_string())); }
        return Some(Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null)));
    }
    None
}

fn set_poll_timeout(sock: &mut Sock) {
    if let MaybeTlsStream::Plain(s) = sock.get_mut() {
        let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(250)));
    }
}

fn is_read_timeout(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io)
        if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

impl Cdp {
    pub fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.sock
            .send(Message::Text(request_frame(id, method, &params)))
            .map_err(|e| WkError::Engine(format!("cdp send: {e}")))?;
        loop {
            let msg = match self.sock.read() {
                Ok(m) => m,
                Err(e) if is_read_timeout(&e) => continue,
                Err(e) => return Err(WkError::Engine(format!("cdp read: {e}"))),
            };
            if let Message::Text(t) = msg {
                if let Some(res) = parse_response(&t, id) {
                    return res.map_err(WkError::Engine);
                }
            }
        }
    }

    pub fn wait_event(&mut self, method: &str, timeout: Duration) -> Result<serde_json::Value> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let msg = match self.sock.read() {
                Ok(m) => m,
                Err(e) if is_read_timeout(&e) => continue,
                Err(e) => return Err(WkError::Engine(format!("cdp read: {e}"))),
            };
            if let Message::Text(t) = msg {
                let v: serde_json::Value = serde_json::from_str(&t)
                    .map_err(|e| WkError::Engine(format!("cdp json: {e}")))?;
                if v.get("method").and_then(|m| m.as_str()) == Some(method) {
                    return Ok(v.get("params").cloned().unwrap_or(serde_json::Value::Null));
                }
            }
        }
        Err(WkError::Engine(format!("timeout waiting for event {method}")))
    }
}

pub fn connect(ws_url: &str) -> Result<Cdp> {
    let (mut sock, _resp) = ws_connect(ws_url).map_err(|e| WkError::Engine(format!("cdp connect: {e}")))?;
    set_poll_timeout(&mut sock);
    Ok(Cdp { sock, next_id: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_and_response_roundtrip() {
        let f = request_frame(7, "Page.enable", &serde_json::json!({}));
        assert!(f.contains("\"id\":7"));
        assert!(f.contains("Page.enable"));
        let resp = r#"{"id":7,"result":{"ok":true}}"#;
        let got = parse_response(resp, 7).unwrap().unwrap();
        assert_eq!(got["ok"], serde_json::json!(true));
        // an event frame is not a response
        assert!(parse_response(r#"{"method":"Page.loadEventFired","params":{}}"#, 7).is_none());
    }
}
