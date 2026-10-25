//! Loopback JSON-lines control server: one request per line, one reply per line
//! (docs/control-protocol.md). Adapted from SolidCraft's `apps/solidcraft/src/control_server.rs`
//! (MIT OR Apache-2.0): the token on the first request, the line, connection and bad-line limits;
//! without its pending-request bookkeeping (a request waits for its reply, a long engine request
//! up to an hour).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use emcraft_ui_egui::ControlRequest;
use serde_json::{Value, json};

use crate::control_auth;

/// Longest request line (bytes).
pub const MAX_LINE: usize = 16 * 1024 * 1024;
/// Connections served at once, so a flood cannot spawn threads without end.
const MAX_CONNECTIONS: usize = 16;
/// Lines that are not JSON before the connection is closed.
const MAX_BAD_LINES: usize = 8;
/// How long a request waits for the app: most requests, and engine requests (a solve).
const SHORT: Duration = Duration::from_secs(60);
const LONG: Duration = Duration::from_secs(3600);

static OPEN: AtomicUsize = AtomicUsize::new(0);

/// Listens on 127.0.0.1:`port`; requests go to the returned queue, each answered through its
/// sender. `wake` repaints the window when a request arrives. The token file is removed when the
/// returned guard is dropped.
pub fn start(port: u16, wake: impl Fn() + Send + Sync + 'static) -> Result<(Receiver<ControlRequest>, Option<control_auth::TokenFile>), String> {
    let token = control_auth::env_token().or_else(control_auth::new_token).ok_or("no random token available")?;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let guard = match control_auth::token_path(port) {
        Some(path) => match control_auth::write_token_file(&path, &token) {
            Ok(()) => {
                eprintln!("emcraft: control channel on 127.0.0.1:{port} (token in {})", path.display());
                Some(control_auth::TokenFile(path))
            }
            Err(e) => {
                eprintln!(
                    "emcraft: control channel on 127.0.0.1:{port}, but the token file {} could not be written ({e}); clients need EMCRAFT_CONTROL_TOKEN",
                    path.display()
                );
                None
            }
        },
        None => {
            eprintln!("emcraft: control channel on 127.0.0.1:{port}; no data folder for its token, clients need EMCRAFT_CONTROL_TOKEN");
            None
        }
    };
    let (tx, rx) = channel::<ControlRequest>();
    let token = std::sync::Arc::new(token);
    let wake = std::sync::Arc::new(wake);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if OPEN.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                OPEN.fetch_sub(1, Ordering::Relaxed);
                let mut refused = stream;
                let _ = writeln!(refused, "{}", json!({"ok": false, "error": "too many connections"}));
                continue;
            }
            let (tx, token, wake) = (tx.clone(), token.clone(), wake.clone());
            std::thread::spawn(move || {
                serve(stream, &token, &tx, &*wake);
                OPEN.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Ok((rx, guard))
}

/// One line without its newline, or `None` at the end; a line longer than `max` is refused as
/// soon as it passes the limit.
fn read_line(r: &mut impl BufRead, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let (done, used) = {
            let available = r.fill_buf()?;
            if available.is_empty() {
                return Ok((!line.is_empty()).then_some(line));
            }
            match available.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    line.extend_from_slice(available.get(..i).unwrap_or(&[]));
                    (true, i + 1)
                }
                None => {
                    line.extend_from_slice(available);
                    (false, available.len())
                }
            }
        };
        r.consume(used);
        if done {
            return Ok(Some(line));
        }
        if line.len() > max {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request too large"));
        }
    }
}

fn unauthorized() -> Value {
    json!({"ok": false, "error": "unauthorized: send the control token as \"token\" (see docs/control-protocol.md)"})
}

/// Refuses the connection: the reply, the end of what we send, then what the client had already
/// sent read and dropped (briefly), so a reset does not overtake the refusal.
fn refuse(out: &mut TcpStream, reader: &mut BufReader<TcpStream>) {
    use std::io::Read;
    let _ = writeln!(out, "{}", unauthorized());
    let _ = out.shutdown(std::net::Shutdown::Write);
    let _ = reader.get_ref().set_read_timeout(Some(Duration::from_millis(500)));
    let _ = std::io::copy(&mut reader.by_ref().take(64 * 1024), &mut std::io::sink());
}

pub(crate) fn serve(stream: TcpStream, token: &str, tx: &Sender<ControlRequest>, wake: &dyn Fn()) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    let mut bad = 0usize;
    let mut authenticated = false;
    loop {
        let line = match read_line(&mut reader, MAX_LINE) {
            Ok(Some(l)) => String::from_utf8_lossy(&l).into_owned(),
            Ok(None) => break,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": e.to_string()}));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
                if !authenticated {
                    match msg.get("token").and_then(Value::as_str) {
                        Some(t) if control_auth::tokens_match(t, token) => authenticated = true,
                        _ => {
                            refuse(&mut out, &mut reader);
                            break;
                        }
                    }
                }
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_owned();
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let wait = if method.starts_with("engine.") { LONG } else { SHORT };
                let (req, rrx) = ControlRequest::new(method.clone(), params);
                if tx.send(req).is_err() {
                    break;
                }
                wake();
                let mut r = rrx.recv_timeout(wait).unwrap_or_else(|_| json!({"ok": false, "error": format!("timeout: `{method}` did not answer")}));
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
            Err(_) if !authenticated => {
                refuse(&mut out, &mut reader);
                break;
            }
            Err(e) => {
                bad += 1;
                json!({"ok": false, "error": format!("bad JSON: {e}")})
            }
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
        if bad >= MAX_BAD_LINES {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A connection runs nothing until a request carries the token; a wrong or missing token,
    /// or garbage, is refused and the connection closed; too long a line is refused.
    #[test]
    fn requests_run_only_after_the_token() {
        use std::io::Read;
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = ran.clone();
        std::thread::spawn(move || {
            for req in rx {
                seen.lock().unwrap().push(req.method.clone());
                let _ = req.reply.send(json!({"ok": true, "result": null}));
            }
        });
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve(stream, token, &tx, &|| {}));
            }
        });
        let session = |lines: &[String]| {
            let mut s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            for l in lines {
                let _ = writeln!(s, "{l}");
            }
            let _ = s.shutdown(std::net::Shutdown::Write);
            let mut text = String::new();
            let _ = s.read_to_string(&mut text);
            text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect::<Vec<_>>()
        };
        for first in [
            json!({"id": 1, "method": "app.quit"}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": "0123456789abcdef0123456789abcdee"}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": 7}).to_string(),
            "not json".to_string(),
        ] {
            let replies = session(&[first.clone(), json!({"id": 2, "method": "app.quit", "token": token}).to_string()]);
            assert_eq!(replies.len(), 1, "{first}: {replies:?}");
            assert!(replies[0]["error"].as_str().is_some_and(|e| e.starts_with("unauthorized")), "{first}: {replies:?}");
        }
        assert!(ran.lock().unwrap().is_empty(), "nothing ran without the token");
        let replies = session(&[
            json!({"id": 1, "method": "ui.inspect", "token": token}).to_string(),
            json!({"id": 2, "method": "ui.inspect"}).to_string(),
            "garbage".to_string(),
        ]);
        assert_eq!(replies.len(), 3);
        assert_eq!((&replies[0]["id"], &replies[1]["id"]), (&json!(1), &json!(2)));
        assert!(replies[2]["error"].as_str().is_some_and(|e| e.starts_with("bad JSON")));
        assert_eq!(*ran.lock().unwrap(), ["ui.inspect", "ui.inspect"]);
        // A line past the limit is refused without being kept.
        let mut r = std::io::Cursor::new(vec![b'x'; 300]);
        assert!(read_line(&mut BufReader::with_capacity(16, &mut r), 100).is_err());
    }
}
