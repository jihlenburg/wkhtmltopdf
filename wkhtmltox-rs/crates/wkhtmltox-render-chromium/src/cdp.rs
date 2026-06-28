// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use std::time::{Duration, Instant};
use tungstenite::{connect as ws_connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use std::net::TcpStream;
use wkhtmltox_core::{Result, WkError};

/// Maximum time a single CDP command is allowed to wait for a response.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

type Sock = WebSocket<MaybeTlsStream<TcpStream>>;

pub struct Cdp {
    sock: Sock,
    next_id: u64,
    /// Events and unmatched responses buffered while waiting for a command reply.
    ///
    /// `call()` discards non-matching messages by pushing them here instead of
    /// dropping them.  The event pump reads from this buffer first (via
    /// `read_message`) so `Fetch.requestPaused` events arriving during a `call`
    /// round-trip are not lost.
    msg_buf: Vec<serde_json::Value>,
}

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
    /// Send a CDP command and wait for the matching response.
    ///
    /// Any events or unmatched responses received while waiting are pushed into
    /// the internal `msg_buf` so the event pump can process them via
    /// [`read_message`].
    pub fn call(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        self.next_id += 1;
        let id = self.next_id;
        let deadline = Instant::now() + CALL_TIMEOUT;
        self.sock
            .send(Message::Text(request_frame(id, method, &params)))
            .map_err(|e| WkError::Engine(format!("cdp send: {e}")))?;
        loop {
            let msg = match self.sock.read() {
                Ok(m) => m,
                Err(e) if is_read_timeout(&e) => {
                    if Instant::now() >= deadline {
                        return Err(WkError::Engine(format!("cdp call timeout: {method}")));
                    }
                    continue;
                }
                Err(e) => return Err(WkError::Engine(format!("cdp read: {e}"))),
            };
            if let Message::Text(t) = msg {
                if let Some(res) = parse_response(&t, id) {
                    return res.map_err(WkError::Engine);
                }
                // Buffer events and unmatched responses so the event pump sees them.
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    self.msg_buf.push(v);
                }
            }
        }
    }

    /// Fire-and-forget: send a CDP command without waiting for the response.
    ///
    /// The Chrome response will arrive later and will be buffered or discarded
    /// by the next call/read_message.  Use this for `Fetch.continueRequest` and
    /// `Fetch.failRequest` inside the event pump where waiting for a response
    /// per-request would deadlock.
    pub fn send_only(&mut self, method: &str, params: serde_json::Value) -> Result<()> {
        self.next_id += 1;
        let id = self.next_id;
        self.sock
            .send(Message::Text(request_frame(id, method, &params)))
            .map_err(|e| WkError::Engine(format!("cdp send: {e}")))
    }

    /// Read one CDP message (event or response), with a timeout.
    ///
    /// Drains the internal buffer before reading from the socket, so events
    /// buffered by `call()` are returned first.  Returns `None` on timeout
    /// or unrecoverable socket error.
    pub fn read_message(&mut self, timeout: Duration) -> Option<serde_json::Value> {
        // Drain internal buffer first.
        if !self.msg_buf.is_empty() {
            return Some(self.msg_buf.remove(0));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let msg = match self.sock.read() {
                Ok(m) => m,
                Err(e) if is_read_timeout(&e) => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    continue;
                }
                Err(_) => return None,
            };
            if let Message::Text(t) = msg {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    return Some(v);
                }
            }
        }
    }

    /// Drain all messages currently in the internal buffer and return them.
    ///
    /// Used after `Fetch.disable` to process any `Fetch.requestPaused` events
    /// that Chrome sent while the disable command was in flight.
    pub fn drain_buffer(&mut self) -> Vec<serde_json::Value> {
        self.msg_buf.drain(..).collect()
    }

    /// Send a CDP command and, while awaiting its response, service every
    /// `Fetch.requestPaused` event that arrives by calling `on_fetch`.
    ///
    /// `on_fetch` receives the complete event JSON and returns a list of
    /// `(method, params)` pairs to fire immediately via `send_only` (e.g.
    /// `Fetch.continueRequest` / `Fetch.failRequest`).  The closure must not
    /// call any method on `self`; return the commands instead.
    ///
    /// Non-Fetch events received while waiting are pushed to `msg_buf` so the
    /// caller's event pump can process them later via [`read_message`].
    ///
    /// Errors from `send_only` (responding to Fetch events) are silently
    /// ignored — fail-closed: the paused request stays blocked rather than
    /// letting it through.
    pub fn call_pumping<F>(
        &mut self,
        method: &str,
        params: serde_json::Value,
        mut on_fetch: F,
    ) -> Result<serde_json::Value>
    where
        F: FnMut(&serde_json::Value) -> Result<Vec<(String, serde_json::Value)>>,
    {
        self.next_id += 1;
        let id = self.next_id;
        let deadline = Instant::now() + CALL_TIMEOUT;

        self.sock
            .send(Message::Text(request_frame(id, method, &params)))
            .map_err(|e| WkError::Engine(format!("cdp send: {e}")))?;

        // Drain internal buffer first — handle buffered Fetch events from prior
        // cdp.call() invocations that arrived while waiting for something else.
        let prior: Vec<serde_json::Value> = self.msg_buf.drain(..).collect();
        for msg in prior {
            if msg.get("method").and_then(|m| m.as_str()) == Some("Fetch.requestPaused") {
                if let Ok(cmds) = on_fetch(&msg) {
                    for (m, p) in cmds {
                        let _ = self.send_only(&m, p);
                    }
                }
            } else {
                self.msg_buf.push(msg);
            }
        }

        loop {
            let msg = match self.sock.read() {
                Ok(m) => m,
                Err(e) if is_read_timeout(&e) => {
                    if Instant::now() >= deadline {
                        return Err(WkError::Engine(format!(
                            "cdp call timeout: {method}"
                        )));
                    }
                    continue;
                }
                Err(e) => return Err(WkError::Engine(format!("cdp read: {e}"))),
            };
            if let Message::Text(t) = msg {
                if let Some(res) = parse_response(&t, id) {
                    return res.map_err(WkError::Engine);
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    if v.get("method").and_then(|m| m.as_str())
                        == Some("Fetch.requestPaused")
                    {
                        if let Ok(cmds) = on_fetch(&v) {
                            for (m, p) in cmds {
                                let _ = self.send_only(&m, p);
                            }
                        }
                    } else {
                        // Buffer everything else (Page.loadEventFired, etc.)
                        self.msg_buf.push(v);
                    }
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
    Ok(Cdp { sock, next_id: 0, msg_buf: Vec::new() })
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
