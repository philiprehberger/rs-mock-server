# rs-mock-server

[![CI](https://github.com/philiprehberger/rs-mock-server/actions/workflows/ci.yml/badge.svg)](https://github.com/philiprehberger/rs-mock-server/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/philiprehberger-mock-server.svg)](https://crates.io/crates/philiprehberger-mock-server)
[![GitHub release](https://img.shields.io/github/v/release/philiprehberger/rs-mock-server)](https://github.com/philiprehberger/rs-mock-server/releases)
[![Last updated](https://img.shields.io/github/last-commit/philiprehberger/rs-mock-server)](https://github.com/philiprehberger/rs-mock-server/commits/main)
[![License](https://img.shields.io/github/license/philiprehberger/rs-mock-server)](LICENSE)
[![Bug Reports](https://img.shields.io/github/issues/philiprehberger/rs-mock-server/bug)](https://github.com/philiprehberger/rs-mock-server/issues?q=is%3Aissue+is%3Aopen+label%3Abug)
[![Feature Requests](https://img.shields.io/github/issues/philiprehberger/rs-mock-server/enhancement)](https://github.com/philiprehberger/rs-mock-server/issues?q=is%3Aissue+is%3Aopen+label%3Aenhancement)
[![Sponsor](https://img.shields.io/badge/sponsor-GitHub%20Sponsors-ec6cb9)](https://github.com/sponsors/philiprehberger)

Lightweight, per-test HTTP mock server for testing API integrations

## Installation

```toml
[dependencies]
philiprehberger-mock-server = "0.1.2"
```

## Usage

```rust
use philiprehberger_mock_server::{MockServer, Method};

#[tokio::test]
async fn test_api_call() {
    let server = MockServer::start().await;

    server
        .mock(Method::GET, "/users")
        .with_status(200)
        .with_body(r#"[{"id": 1, "name": "Alice"}]"#)
        .with_header("Content-Type", "application/json")
        .create();

    // Use server.url() as the base URL for your HTTP client
    let base_url = server.url(); // e.g. http://127.0.0.1:54321
}
```

### JSON response

```rust
use philiprehberger_mock_server::{MockServer, Method};
use serde_json::json;

#[tokio::test]
async fn test_json_response() {
    let server = MockServer::start().await;

    server
        .mock(Method::POST, "/api/data")
        .with_status(201)
        .with_json(json!({"status": "created", "id": 42}))
        .create();
}
```

### Simulated delay

```rust
use philiprehberger_mock_server::{MockServer, Method};
use std::time::Duration;

#[tokio::test]
async fn test_timeout_handling() {
    let server = MockServer::start().await;

    server
        .mock(Method::GET, "/slow")
        .with_status(200)
        .with_body("finally")
        .with_delay(Duration::from_millis(500))
        .create();
}
```

### Request inspection

```rust
use philiprehberger_mock_server::{MockServer, Method};

#[tokio::test]
async fn test_request_recording() {
    let server = MockServer::start().await;

    server
        .mock(Method::POST, "/webhook")
        .with_status(200)
        .create();

    // ... make requests to server.url() ...

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/webhook");
}
```

## API

| Item | Description |
|---|---|
| `MockServer::start()` | Start a mock server on a random port |
| `.url()` | Base URL of the running server |
| `.mock(method, path)` | Begin building a mock definition |
| `.requests()` | Get all recorded requests |
| `.reset()` | Clear all mocks and recorded requests |
| `MockBuilder::with_status(code)` | Set response status code |
| `MockBuilder::with_body(body)` | Set response body |
| `MockBuilder::with_json(value)` | Set JSON response body and content type |
| `MockBuilder::with_header(key, value)` | Add a response header |
| `MockBuilder::with_delay(duration)` | Add a delay before responding |
| `MockBuilder::expect(times)` | Expect exactly N calls (verified on drop) |
| `MockBuilder::create()` | Register the mock on the server |
| `Method` | HTTP method enum: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS |
| `RecordedRequest` | Captured request with method, path, headers, body, query |

## Development

```bash
cargo test
cargo clippy -- -D warnings
```

## Support

If you find this package useful, consider giving it a star on GitHub — it helps motivate continued maintenance and development.

[![LinkedIn](https://img.shields.io/badge/Philip%20Rehberger-LinkedIn-0A66C2?logo=linkedin)](https://www.linkedin.com/in/philiprehberger)
[![More packages](https://img.shields.io/badge/more-open%20source%20packages-blue)](https://philiprehberger.com/open-source-packages)

## License

[MIT](LICENSE)
