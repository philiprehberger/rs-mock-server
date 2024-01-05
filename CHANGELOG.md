# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-03-19

### Added

- Initial release
- `MockServer` with automatic port allocation
- `MockBuilder` with fluent API for configuring responses
- Support for GET, POST, PUT, DELETE, PATCH, HEAD, and OPTIONS methods
- JSON response helper via `with_json`
- Response delay simulation via `with_delay`
- Request recording and inspection
- Call count expectations via `expect`
- Automatic cleanup on drop
