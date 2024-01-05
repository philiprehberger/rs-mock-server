//! Lightweight, per-test HTTP mock server for testing API integrations.
//!
//! Spins up a real TCP server on a random port using raw tokio TCP — no external
//! HTTP framework required. Each test gets its own isolated server instance.
//!
//! # Example
//!
//! ```rust,no_run
//! use philiprehberger_mock_server::{MockServer, Method};
//!
//! #[tokio::main]
//! async fn main() {
//!     let server = MockServer::start().await;
//!
//!     server
//!         .mock(Method::GET, "/hello")
//!         .with_status(200)
//!         .with_body("world")
//!         .create();
//!
//!     println!("Mock server running at {}", server.url());
//! }
//! ```

use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// HTTP method for mock matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    /// HTTP GET
    GET,
    /// HTTP POST
    POST,
    /// HTTP PUT
    PUT,
    /// HTTP DELETE
    DELETE,
    /// HTTP PATCH
    PATCH,
    /// HTTP HEAD
    HEAD,
    /// HTTP OPTIONS
    OPTIONS,
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Method::GET => "GET",
            Method::POST => "POST",
            Method::PUT => "PUT",
            Method::DELETE => "DELETE",
            Method::PATCH => "PATCH",
            Method::HEAD => "HEAD",
            Method::OPTIONS => "OPTIONS",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for Method {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "GET" => Ok(Method::GET),
            "POST" => Ok(Method::POST),
            "PUT" => Ok(Method::PUT),
            "DELETE" => Ok(Method::DELETE),
            "PATCH" => Ok(Method::PATCH),
            "HEAD" => Ok(Method::HEAD),
            "OPTIONS" => Ok(Method::OPTIONS),
            other => Err(format!("Unknown HTTP method: {}", other)),
        }
    }
}

/// A recorded incoming HTTP request.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    /// The HTTP method (e.g. "GET", "POST").
    pub method: String,
    /// The request path (e.g. "/api/users").
    pub path: String,
    /// Request headers as key-value pairs.
    pub headers: Vec<(String, String)>,
    /// The request body as a string.
    pub body: String,
    /// The query string (e.g. "foo=bar&baz=1"), empty if none.
    pub query: String,
}

/// Internal mock definition used for matching and responding.
struct MockDefinition {
    method: Method,
    path: String,
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<String>,
    delay: Option<Duration>,
    times: Option<usize>,
    call_count: usize,
}

/// A lightweight HTTP mock server bound to a random local port.
///
/// Each instance runs its own TCP listener in a background tokio task.
/// The server is automatically shut down when dropped.
pub struct MockServer {
    addr: SocketAddr,
    mocks: Arc<Mutex<Vec<MockDefinition>>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl MockServer {
    /// Start a new mock server on a random available port.
    ///
    /// The server begins accepting connections immediately.
    pub async fn start() -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
        let addr = listener.local_addr().expect("Failed to get local addr");

        let mocks: Arc<Mutex<Vec<MockDefinition>>> = Arc::new(Mutex::new(Vec::new()));
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let mocks_clone = Arc::clone(&mocks);
        let requests_clone = Arc::clone(&requests);

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                let mocks = Arc::clone(&mocks_clone);
                                let requests = Arc::clone(&requests_clone);
                                tokio::spawn(async move {
                                    handle_connection(stream, mocks, requests).await;
                                });
                            }
                            Err(_) => break,
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        });

        MockServer {
            addr,
            mocks,
            requests,
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    /// Returns the base URL of the mock server (e.g. `http://127.0.0.1:12345`).
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    /// Begin building a mock for the given HTTP method and path.
    pub fn mock(&self, method: Method, path: &str) -> MockBuilder<'_> {
        MockBuilder {
            server: self,
            method,
            path: path.to_string(),
            status: 200,
            headers: Vec::new(),
            body: None,
            delay: None,
            times: None,
        }
    }

    /// Returns a snapshot of all recorded requests received by the server.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        // Use try_lock for non-async contexts; callers should ensure no contention.
        let guard = self.requests.try_lock().expect("Failed to lock requests");
        guard.clone()
    }

    /// Clear all registered mocks and recorded requests.
    pub fn reset(&self) {
        {
            let mut guard = self.mocks.try_lock().expect("Failed to lock mocks");
            guard.clear();
        }
        {
            let mut guard = self.requests.try_lock().expect("Failed to lock requests");
            guard.clear();
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        // Check expectations before shutting down.
        if let Ok(mocks) = self.mocks.try_lock() {
            for mock in mocks.iter() {
                if let Some(expected) = mock.times {
                    if mock.call_count != expected {
                        eprintln!(
                            "Mock expectation failed: {} {} expected {} calls, got {}",
                            mock.method, mock.path, expected, mock.call_count
                        );
                    }
                }
            }
        }

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

/// Fluent builder for configuring a mock response.
pub struct MockBuilder<'a> {
    server: &'a MockServer,
    method: Method,
    path: String,
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<String>,
    delay: Option<Duration>,
    times: Option<usize>,
}

impl<'a> MockBuilder<'a> {
    /// Set the HTTP response status code.
    pub fn with_status(mut self, code: u16) -> Self {
        self.status = code;
        self
    }

    /// Set the response body as a plain string.
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set the response body to a JSON value and add `Content-Type: application/json`.
    pub fn with_json(mut self, value: serde_json::Value) -> Self {
        self.body = Some(value.to_string());
        self.headers
            .push(("Content-Type".to_string(), "application/json".to_string()));
        self
    }

    /// Add a custom response header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// Add a delay before the server sends the response.
    pub fn with_delay(mut self, duration: Duration) -> Self {
        self.delay = Some(duration);
        self
    }

    /// Expect this mock to be called exactly `times` times.
    ///
    /// A warning is printed to stderr on server drop if the count does not match.
    pub fn expect(mut self, times: usize) -> Self {
        self.times = Some(times);
        self
    }

    /// Register the mock on the server.
    pub fn create(&self) {
        let definition = MockDefinition {
            method: self.method.clone(),
            path: self.path.clone(),
            status: self.status,
            headers: self.headers.clone(),
            body: self.body.clone(),
            delay: self.delay,
            times: self.times,
            call_count: 0,
        };
        let mut guard = self
            .server
            .mocks
            .try_lock()
            .expect("Failed to lock mocks");
        guard.push(definition);
    }
}

/// Handle a single TCP connection: parse the HTTP request, match against mocks,
/// and send the response.
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    mocks: Arc<Mutex<Vec<MockDefinition>>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
) {
    let mut buf = vec![0u8; 8192];
    let mut total = 0usize;

    // Read until we have the full headers (look for \r\n\r\n).
    loop {
        if total >= buf.len() {
            buf.resize(buf.len() * 2, 0);
        }
        match stream.read(&mut buf[total..]).await {
            Ok(0) => return,
            Ok(n) => {
                total += n;
                // Check if we have the header delimiter.
                if let Some(header_end) = find_subsequence(&buf[..total], b"\r\n\r\n") {
                    let header_bytes = &buf[..header_end];
                    let header_str = String::from_utf8_lossy(header_bytes).to_string();
                    let body_start = header_end + 4;

                    // Parse request line.
                    let mut lines = header_str.lines();
                    let request_line = match lines.next() {
                        Some(line) => line,
                        None => return,
                    };

                    let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
                    if parts.len() < 2 {
                        return;
                    }
                    let method_str = parts[0];
                    let raw_path = parts[1];

                    // Split path and query.
                    let (path, query) = match raw_path.split_once('?') {
                        Some((p, q)) => (p.to_string(), q.to_string()),
                        None => (raw_path.to_string(), String::new()),
                    };

                    // Parse headers.
                    let mut headers = Vec::new();
                    let mut content_length: usize = 0;
                    for line in lines {
                        if let Some((key, value)) = line.split_once(':') {
                            let key = key.trim().to_string();
                            let value = value.trim().to_string();
                            if key.eq_ignore_ascii_case("content-length") {
                                content_length = value.parse().unwrap_or(0);
                            }
                            headers.push((key, value));
                        }
                    }

                    // Read remaining body if needed.
                    let body_already = total - body_start;
                    if body_already < content_length {
                        let remaining = content_length - body_already;
                        if total + remaining > buf.len() {
                            buf.resize(total + remaining, 0);
                        }
                        let mut read_so_far = 0;
                        while read_so_far < remaining {
                            match stream.read(&mut buf[total + read_so_far..total + remaining]).await
                            {
                                Ok(0) => break,
                                Ok(n) => read_so_far += n,
                                Err(_) => break,
                            }
                        }
                        total += read_so_far;
                    }

                    let body_end = body_start + content_length;
                    let body_end = body_end.min(total);
                    let body =
                        String::from_utf8_lossy(&buf[body_start..body_end]).to_string();

                    // Record the request.
                    {
                        let mut req_guard = requests.lock().await;
                        req_guard.push(RecordedRequest {
                            method: method_str.to_string(),
                            path: path.clone(),
                            headers: headers.clone(),
                            body: body.clone(),
                            query: query.clone(),
                        });
                    }

                    // Match against mocks.
                    let parsed_method = Method::from_str(method_str).ok();

                    let (status, resp_headers, resp_body, delay) = {
                        let mut mocks_guard = mocks.lock().await;
                        let mut matched = None;

                        for (i, mock) in mocks_guard.iter().enumerate() {
                            if Some(&mock.method) == parsed_method.as_ref() && mock.path == path {
                                // Check if this mock has exceeded its expected calls.
                                if let Some(times) = mock.times {
                                    if mock.call_count >= times {
                                        continue;
                                    }
                                }
                                matched = Some(i);
                                break;
                            }
                        }

                        if let Some(idx) = matched {
                            let mock = &mut mocks_guard[idx];
                            mock.call_count += 1;
                            (
                                mock.status,
                                mock.headers.clone(),
                                mock.body.clone().unwrap_or_default(),
                                mock.delay,
                            )
                        } else {
                            (404, Vec::new(), "No mock matched".to_string(), None)
                        }
                    };

                    // Apply delay.
                    if let Some(d) = delay {
                        tokio::time::sleep(d).await;
                    }

                    // Build and send response.
                    let reason = status_reason(status);
                    let mut response = format!(
                        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n",
                        status,
                        reason,
                        resp_body.len()
                    );
                    for (key, value) in &resp_headers {
                        response.push_str(&format!("{}: {}\r\n", key, value));
                    }
                    response.push_str("\r\n");
                    response.push_str(&resp_body);

                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.flush().await;
                    break;
                }
            }
            Err(_) => return,
        }
    }
}

/// Find the position of a subsequence in a byte slice.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Return a reason phrase for common HTTP status codes.
fn status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    /// Helper: send a raw HTTP request and return (status_code, body).
    async fn http_request(
        url: &str,
        method: &str,
        path: &str,
        request_body: Option<&str>,
    ) -> (u16, String, String) {
        let addr = url
            .strip_prefix("http://")
            .expect("URL must start with http://");
        let mut stream = TcpStream::connect(addr).await.expect("Failed to connect");

        let body_bytes = request_body.unwrap_or("");
        let request = if request_body.is_some() {
            format!(
                "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                method,
                path,
                addr,
                body_bytes.len(),
                body_bytes
            )
        } else {
            format!(
                "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                method, path, addr
            )
        };

        stream
            .write_all(request.as_bytes())
            .await
            .expect("Failed to write");

        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("Failed to read");

        let response_str = String::from_utf8_lossy(&response).to_string();

        // Parse status code.
        let status_line = response_str.lines().next().unwrap_or("");
        let status: u16 = status_line
            .splitn(3, ' ')
            .nth(1)
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        // Parse body (after \r\n\r\n).
        let body = response_str
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_string())
            .unwrap_or_default();

        // Collect response headers.
        let headers_section = response_str
            .split_once("\r\n\r\n")
            .map(|(h, _)| h.to_string())
            .unwrap_or_default();

        (status, body, headers_section)
    }

    async fn http_get(url: &str, path: &str) -> (u16, String) {
        let (status, body, _) = http_request(url, "GET", path, None).await;
        (status, body)
    }

    #[tokio::test]
    async fn test_start_server_and_get_url() {
        let server = MockServer::start().await;
        let url = server.url();
        assert!(url.starts_with("http://127.0.0.1:"));
        let port: u16 = url.rsplit(':').next().unwrap().parse().unwrap();
        assert!(port > 0);
    }

    #[tokio::test]
    async fn test_basic_get_mock() {
        let server = MockServer::start().await;

        server
            .mock(Method::GET, "/hello")
            .with_status(200)
            .with_body("Hello, world!")
            .create();

        let (status, body) = http_get(&server.url(), "/hello").await;
        assert_eq!(status, 200);
        assert_eq!(body, "Hello, world!");
    }

    #[tokio::test]
    async fn test_json_response() {
        let server = MockServer::start().await;

        server
            .mock(Method::GET, "/data")
            .with_status(200)
            .with_json(json!({"key": "value", "num": 42}))
            .create();

        let (status, body, headers) = http_request(&server.url(), "GET", "/data", None).await;
        assert_eq!(status, 200);
        assert!(headers.contains("Content-Type: application/json"));

        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["num"], 42);
    }

    #[tokio::test]
    async fn test_404_when_no_mock_matches() {
        let server = MockServer::start().await;

        let (status, body) = http_get(&server.url(), "/unknown").await;
        assert_eq!(status, 404);
        assert_eq!(body, "No mock matched");
    }

    #[tokio::test]
    async fn test_multiple_mocks_different_paths() {
        let server = MockServer::start().await;

        server
            .mock(Method::GET, "/one")
            .with_status(200)
            .with_body("first")
            .create();

        server
            .mock(Method::GET, "/two")
            .with_status(201)
            .with_body("second")
            .create();

        let (status1, body1) = http_get(&server.url(), "/one").await;
        assert_eq!(status1, 200);
        assert_eq!(body1, "first");

        let (status2, body2) = http_get(&server.url(), "/two").await;
        assert_eq!(status2, 201);
        assert_eq!(body2, "second");
    }

    #[tokio::test]
    async fn test_post_with_body_recording() {
        let server = MockServer::start().await;

        server
            .mock(Method::POST, "/submit")
            .with_status(201)
            .with_body("created")
            .create();

        let (status, body, _) =
            http_request(&server.url(), "POST", "/submit", Some(r#"{"name":"test"}"#)).await;
        assert_eq!(status, 201);
        assert_eq!(body, "created");

        // Allow a moment for the request to be recorded.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/submit");
        assert_eq!(requests[0].body, r#"{"name":"test"}"#);
    }

    #[tokio::test]
    async fn test_requests_returns_recorded() {
        let server = MockServer::start().await;

        server
            .mock(Method::GET, "/a")
            .with_status(200)
            .with_body("ok")
            .create();

        http_get(&server.url(), "/a").await;
        http_get(&server.url(), "/a").await;

        tokio::time::sleep(Duration::from_millis(50)).await;

        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|r| r.path == "/a"));
    }

    #[tokio::test]
    async fn test_reset_clears_mocks() {
        let server = MockServer::start().await;

        server
            .mock(Method::GET, "/temp")
            .with_status(200)
            .with_body("temporary")
            .create();

        let (status, body) = http_get(&server.url(), "/temp").await;
        assert_eq!(status, 200);
        assert_eq!(body, "temporary");

        tokio::time::sleep(Duration::from_millis(50)).await;

        server.reset();

        let (status, body) = http_get(&server.url(), "/temp").await;
        assert_eq!(status, 404);
        assert_eq!(body, "No mock matched");

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Requests should be cleared too (only the post-reset request remains).
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
    }

    #[tokio::test]
    async fn test_custom_headers_in_response() {
        let server = MockServer::start().await;

        server
            .mock(Method::GET, "/headers")
            .with_status(200)
            .with_body("ok")
            .with_header("X-Custom", "hello")
            .with_header("X-Another", "world")
            .create();

        let (status, body, headers) =
            http_request(&server.url(), "GET", "/headers", None).await;
        assert_eq!(status, 200);
        assert_eq!(body, "ok");
        assert!(headers.contains("X-Custom: hello"));
        assert!(headers.contains("X-Another: world"));
    }

    #[tokio::test]
    async fn test_different_http_methods() {
        let server = MockServer::start().await;

        server
            .mock(Method::PUT, "/resource")
            .with_status(200)
            .with_body("updated")
            .create();

        server
            .mock(Method::DELETE, "/resource")
            .with_status(204)
            .with_body("")
            .create();

        server
            .mock(Method::PATCH, "/resource")
            .with_status(200)
            .with_body("patched")
            .create();

        let (status, body, _) =
            http_request(&server.url(), "PUT", "/resource", Some("data")).await;
        assert_eq!(status, 200);
        assert_eq!(body, "updated");

        let (status, _, _) =
            http_request(&server.url(), "DELETE", "/resource", None).await;
        assert_eq!(status, 204);

        let (status, body, _) =
            http_request(&server.url(), "PATCH", "/resource", Some("patch")).await;
        assert_eq!(status, 200);
        assert_eq!(body, "patched");
    }
}
