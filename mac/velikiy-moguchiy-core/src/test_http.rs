//! A small HTTP/1.1 server for tests: fixed routes, byte ranges, redirects
//! and a log of what was asked.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

pub enum Reply {
    /// Served with `Range` support.
    Body(Vec<u8>),
    /// Served whole, whatever `Range` says.
    IgnoreRange(Vec<u8>),
    Status(u16, Vec<(&'static str, String)>),
    Redirect(String),
}

#[derive(Debug, Clone)]
pub struct Request {
    pub path: String,
    pub range: Option<String>,
}

pub struct Server {
    pub base: String,
    log: Arc<Mutex<Vec<Request>>>,
}

impl Server {
    /// Serves `routes` (path with query -> reply) on 127.0.0.1 until the test ends.
    pub fn start(routes: HashMap<String, Reply>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let mut range = None;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.eq_ignore_ascii_case("range") {
                            range = Some(value.trim().to_string());
                        }
                    }
                }
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_string();
                seen.lock().unwrap().push(Request {
                    path: path.clone(),
                    range: range.clone(),
                });
                let mut stream = stream;
                let (status, headers, body) = respond(routes.get(&path), range.as_deref());
                let mut head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        Self { base, log }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.log.lock().unwrap().clone()
    }
}

fn respond(reply: Option<&Reply>, range: Option<&str>) -> (u16, Vec<(String, String)>, Vec<u8>) {
    match reply {
        None => (404, Vec::new(), Vec::new()),
        Some(Reply::IgnoreRange(body)) => (200, Vec::new(), body.clone()),
        Some(Reply::Status(code, headers)) => (
            *code,
            headers
                .iter()
                .map(|(n, v)| (n.to_string(), v.clone()))
                .collect(),
            Vec::new(),
        ),
        Some(Reply::Redirect(to)) => (302, vec![("Location".into(), to.clone())], Vec::new()),
        Some(Reply::Body(body)) => {
            let len = body.len();
            let Some(spec) = range.and_then(|r| r.strip_prefix("bytes=")) else {
                return (200, Vec::new(), body.clone());
            };
            let (from, to) = spec.split_once('-').unwrap_or((spec, ""));
            let (start, end) = if from.is_empty() {
                let n: usize = to.parse().unwrap_or(0);
                (len.saturating_sub(n), len)
            } else {
                let start: usize = from.parse().unwrap_or(0);
                let end = to.parse::<usize>().map(|e| (e + 1).min(len)).unwrap_or(len);
                (start, end)
            };
            if start >= len || start >= end {
                return (
                    416,
                    vec![("Content-Range".into(), format!("bytes */{len}"))],
                    Vec::new(),
                );
            }
            (
                206,
                vec![(
                    "Content-Range".into(),
                    format!("bytes {start}-{}/{len}", end - 1),
                )],
                body[start..end].to_vec(),
            )
        }
    }
}
