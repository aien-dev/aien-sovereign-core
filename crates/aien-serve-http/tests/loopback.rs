//! Loopback integration tests: a real TCP server with the stub engine and the
//! byte codec, driven by a hand-written HTTP/1.1 client (no extra crates).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use aien_serve_http::codec::ByteCodec;
use aien_serve_http::engine::StubEngine;
use aien_serve_http::server::{serve, ServerConfig};
use serde_json::Value;

struct Server {
    addr: SocketAddr,
    _rt: tokio::runtime::Runtime,
}

fn start(engine: StubEngine, cfg: ServerConfig) -> Server {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let cfg = ServerConfig {
        port: 0,
        model_id: "test-model".into(),
        ..cfg
    };
    let (addr, _handle) = rt
        .block_on(serve(cfg, Arc::new(engine), Arc::new(ByteCodec)))
        .expect("bind loopback");
    Server { addr, _rt: rt }
}

fn stub(reply: &str) -> StubEngine {
    StubEngine::new(reply.bytes().map(u32::from).collect())
}

struct Reply {
    status: u16,
    headers: String,
    body: String,
}

fn dechunk(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let line_end = raw[i..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .expect("chunk size line") + i;
        let size = usize::from_str_radix(
            std::str::from_utf8(&raw[i..line_end]).unwrap().trim(),
            16,
        )
        .expect("hex chunk size");
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&raw[line_end + 2..line_end + 2 + size]);
        i = line_end + 2 + size + 2;
    }
}

fn request(addr: SocketAddr, method: &str, path: &str, body: Option<&str>) -> Reply {
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let body = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).expect("read reply");
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("header end");
    let headers = String::from_utf8_lossy(&raw[..split]).to_string();
    let status: u16 = headers
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status code");
    let body_raw = &raw[split + 4..];
    let body = if headers.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        String::from_utf8(dechunk(body_raw)).unwrap()
    } else {
        String::from_utf8(body_raw.to_vec()).unwrap()
    };
    Reply {
        status,
        headers,
        body,
    }
}

fn chat(addr: SocketAddr, body: &str) -> Reply {
    request(addr, "POST", "/v1/chat/completions", Some(body))
}

fn json(r: &Reply) -> Value {
    serde_json::from_str(&r.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", r.body))
}

/// `data:` payloads of an SSE body, in order; comment lines are skipped.
fn sse_data(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:")))
        .map(|s| s.trim().to_string())
        .collect()
}

const PROMPT: &str = r#"{"model":"test-model","messages":[{"role":"system","content":"s"},{"role":"user","content":"hi"}]}"#;
// ByteCodec renders "<system>s</system>\n<user>hi</user>\n<assistant>" = 46 bytes.
const PROMPT_TOKENS: u64 = 46;

#[test]
fn models_lists_the_one_served_id() {
    let srv = start(stub("x"), ServerConfig::default());
    let r = request(srv.addr, "GET", "/v1/models", None);
    assert_eq!(r.status, 200);
    let v = json(&r);
    assert_eq!(v["object"], "list");
    assert_eq!(v["data"][0]["id"], "test-model");
    assert_eq!(v["data"][0]["owned_by"], "aien/stub");
    let r = request(srv.addr, "GET", "/v1/models/other", None);
    assert_eq!(r.status, 404);
    assert_eq!(json(&r)["error"]["type"], "not_found_error");
    let h = json(&request(srv.addr, "GET", "/health", None));
    assert_eq!(h["status"], "ok");
    assert_eq!(h["busy"], false);
}

#[test]
fn non_stream_completion_has_content_usage_and_finish() {
    let srv = start(stub("Hello, world!"), ServerConfig::default());
    let r = chat(srv.addr, PROMPT);
    assert_eq!(r.status, 200, "{}", r.body);
    let v = json(&r);
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["model"], "test-model");
    assert_eq!(v["choices"][0]["message"]["role"], "assistant");
    assert_eq!(v["choices"][0]["message"]["content"], "Hello, world!");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(v["usage"]["prompt_tokens"], PROMPT_TOKENS);
    assert_eq!(v["usage"]["completion_tokens"], 13);
    assert_eq!(v["usage"]["total_tokens"], PROMPT_TOKENS + 13);
    assert!(v["id"].as_str().unwrap().starts_with("chatcmpl-"));
}

#[test]
fn stream_chunks_parse_and_end_with_done() {
    let srv = start(stub("héllo wörld"), ServerConfig::default());
    let body = r#"{"messages":[{"role":"user","content":"hi"}],"stream":true}"#;
    let r = chat(srv.addr, body);
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(
        r.headers.to_ascii_lowercase().contains("content-type: text/event-stream"),
        "{}",
        r.headers
    );
    let data = sse_data(&r.body);
    assert!(data.len() >= 3, "{data:?}");
    assert_eq!(data.last().unwrap(), "[DONE]");
    let chunks: Vec<Value> = data[..data.len() - 1]
        .iter()
        .map(|d| serde_json::from_str(d).unwrap_or_else(|e| panic!("chunk not JSON ({e}): {d}")))
        .collect();
    assert!(chunks.iter().all(|c| c["object"] == "chat.completion.chunk"));
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    let text: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_eq!(text, "héllo wörld");
    assert!(
        chunks.iter().all(|c| !c.to_string().contains('\u{FFFD}')),
        "a chunk split a character"
    );
    let last = chunks.last().unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "stop");
    assert_eq!(last["usage"]["completion_tokens"], 13);
    assert_eq!(last["usage"]["prompt_tokens"], 27); // "<user>hi</user>\n<assistant>"
}

#[test]
fn second_request_while_busy_is_rejected_then_accepted() {
    let reply: String = "x".repeat(60);
    let srv = start(
        stub(&reply).with_delay(Duration::from_millis(25)),
        ServerConfig::default(),
    );
    let addr = srv.addr;
    let first = std::thread::spawn(move || {
        chat(addr, r#"{"messages":[{"role":"user","content":"a"}],"stream":true}"#)
    });
    std::thread::sleep(Duration::from_millis(300));
    let busy = chat(addr, PROMPT);
    assert_eq!(busy.status, 503, "{}", busy.body);
    assert_eq!(json(&busy)["error"]["type"], "server_busy");
    assert!(busy.headers.to_ascii_lowercase().contains("retry-after: 1"), "{}", busy.headers);
    assert_eq!(json(&request(addr, "GET", "/health", None))["busy"], true);
    let first = first.join().unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(sse_data(&first.body).last().unwrap(), "[DONE]");
    let after = chat(addr, PROMPT);
    assert_eq!(after.status, 200, "{}", after.body);
    assert_eq!(json(&after)["choices"][0]["message"]["content"], reply);
}

#[test]
fn bad_requests_get_openai_error_json() {
    let srv = start(
        stub("x"),
        ServerConfig {
            max_body_bytes: 2048,
            ..ServerConfig::default()
        },
    );
    let cases: Vec<(&str, String, u16, &str)> = vec![
        ("not json", "{nope".into(), 400, "invalid JSON"),
        ("no messages", r#"{"messages":[]}"#.into(), 400, "non-empty"),
        (
            "tool role",
            r#"{"messages":[{"role":"tool","content":"x"}]}"#.into(),
            400,
            "role tool",
        ),
        (
            "temperature",
            r#"{"messages":[{"role":"user","content":"x"}],"temperature":5}"#.into(),
            400,
            "temperature",
        ),
        (
            "tools",
            r#"{"messages":[{"role":"user","content":"x"}],"tools":[{"type":"function"}]}"#.into(),
            400,
            "tool calling",
        ),
        (
            "image part",
            r#"{"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"x"}}]}]}"#.into(),
            400,
            "text parts",
        ),
        (
            "wrong model",
            r#"{"model":"gpt-4","messages":[{"role":"user","content":"x"}]}"#.into(),
            404,
            "not found",
        ),
        (
            "too large",
            format!(r#"{{"messages":[{{"role":"user","content":"{}"}}]}}"#, "y".repeat(4000)),
            413,
            "exceeds",
        ),
    ];
    for (name, body, status, needle) in cases {
        let r = chat(srv.addr, &body);
        assert_eq!(r.status, status, "{name}: {}", r.body);
        let v = json(&r);
        let msg = v["error"]["message"].as_str().unwrap_or_default();
        assert!(msg.contains(needle), "{name}: {msg}");
    }
    let r = request(srv.addr, "GET", "/nope", None);
    assert_eq!(r.status, 404);
    assert_eq!(json(&r)["error"]["type"], "not_found_error");
}

#[test]
fn stop_sequences_and_max_tokens_apply() {
    let srv = start(stub("Hello, world!"), ServerConfig::default());
    let r = chat(
        srv.addr,
        r#"{"messages":[{"role":"user","content":"hi"}],"stop":["world"]}"#,
    );
    let v = json(&r);
    assert_eq!(v["choices"][0]["message"]["content"], "Hello, ");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");

    let r = chat(
        srv.addr,
        r#"{"messages":[{"role":"user","content":"hi"}],"max_tokens":5,"stream":true}"#,
    );
    let data = sse_data(&r.body);
    let chunks: Vec<Value> = data[..data.len() - 1]
        .iter()
        .map(|d| serde_json::from_str(d).unwrap())
        .collect();
    let text: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_eq!(text, "Hello");
    let last = chunks.last().unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "length");
    assert_eq!(last["usage"]["completion_tokens"], 5);
}

#[test]
fn prompt_that_fills_the_context_is_refused() {
    let srv = start(stub("x").with_max_context(30), ServerConfig::default());
    let r = chat(srv.addr, PROMPT);
    assert_eq!(r.status, 400, "{}", r.body);
    assert_eq!(json(&r)["error"]["code"], "context_length_exceeded");
}

#[test]
fn non_loopback_bind_is_refused() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let cfg = ServerConfig {
        bind: "0.0.0.0".parse().unwrap(),
        port: 0,
        ..ServerConfig::default()
    };
    let err = rt
        .block_on(serve(cfg, Arc::new(stub("x")), Arc::new(ByteCodec)))
        .err()
        .expect("must refuse");
    assert!(err.to_string().contains("loopback"), "{err}");
}
