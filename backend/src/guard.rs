// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Restricts the HTTP server to pages of the local Sorobo instance.
//!
//! The server only listens on 127.0.0.1, but any website open in the user's browser could still
//! send requests to it (cross-site requests, DNS rebinding). A request is therefore only accepted
//! when its `Host` header names the loopback interface and – if the browser sends an `Origin` –
//! the requesting page was loaded from the loopback interface as well.

use axum::Json;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub async fn local_only(req: Request, next: Next) -> Response {
    let headers = req.headers();
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).or_else(|| req.uri().authority().map(|a| a.as_str()));
    let host_ok = host.is_some_and(is_local_authority);
    let origin_ok = headers.get(header::ORIGIN).is_none_or(|o| o.to_str().is_ok_and(is_local_origin));
    if host_ok && origin_ok {
        return next.run(req).await;
    }
    tracing::warn!(
        "Anfrage abgewiesen: {} {} (Host {:?}, Origin {:?})",
        req.method(),
        req.uri().path(),
        headers.get(header::HOST),
        headers.get(header::ORIGIN)
    );
    (StatusCode::FORBIDDEN, Json(json!({ "error": "Zugriff nur über die lokale Sorobo-Oberfläche möglich." }))).into_response()
}

/// Host part of `host[:port]` / `[v6]:port`; None if the port is malformed.
fn host_of(authority: &str) -> Option<&str> {
    let port_ok = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        return (tail.is_empty() || tail.strip_prefix(':').is_some_and(port_ok)).then_some(host);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => port_ok(port).then_some(host),
        None => Some(authority),
    }
}

fn is_local_authority(authority: &str) -> bool {
    host_of(authority).is_some_and(|h| h.eq_ignore_ascii_case("localhost") || h == "127.0.0.1" || h == "::1")
}

/// `Origin` of a page served from the loopback interface (any port, e.g. the Vite dev server).
fn is_local_origin(origin: &str) -> bool {
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .is_some_and(|rest| !rest.contains('/') && is_local_authority(rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::routing::post;
    use tower::ServiceExt;

    #[test]
    fn authorities() {
        for ok in ["localhost", "localhost:8787", "LOCALHOST:5173", "127.0.0.1", "127.0.0.1:8787", "[::1]", "[::1]:8787"] {
            assert!(is_local_authority(ok), "{ok}");
        }
        for bad in ["", "evil.com", "evil.com:8787", "localhost.evil.com", "127.0.0.1.evil.com", "localhost:80x", "localhost:", "[::1]x", "user@localhost", "192.168.0.10:8787"] {
            assert!(!is_local_authority(bad), "{bad}");
        }
    }

    #[test]
    fn origins() {
        assert!(is_local_origin("http://localhost:8787"));
        assert!(is_local_origin("http://127.0.0.1:5173"));
        assert!(is_local_origin("http://[::1]:8787"));
        for bad in ["null", "https://evil.com", "http://localhost.evil.com", "http://localhost:8787/x", "file://", "localhost:8787"] {
            assert!(!is_local_origin(bad), "{bad}");
        }
    }

    async fn status(host: Option<&str>, origin: Option<&str>) -> StatusCode {
        let app = Router::new().route("/api/system/reset", post(|| async { "ok" })).layer(axum::middleware::from_fn(local_only));
        let mut req = Request::builder().method("POST").uri("/api/system/reset");
        if let Some(h) = host {
            req = req.header(header::HOST, h);
        }
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap().status()
    }

    #[tokio::test]
    async fn requests() {
        assert_eq!(status(Some("localhost:8787"), None).await, StatusCode::OK);
        assert_eq!(status(Some("localhost:8787"), Some("http://localhost:8787")).await, StatusCode::OK);
        assert_eq!(status(Some("localhost:5173"), Some("http://localhost:5173")).await, StatusCode::OK);
        // Cross-site request from a website open in the browser.
        assert_eq!(status(Some("localhost:8787"), Some("https://evil.example")).await, StatusCode::FORBIDDEN);
        assert_eq!(status(Some("localhost:8787"), Some("null")).await, StatusCode::FORBIDDEN);
        // DNS rebinding: the browser sends the attacker's host name.
        assert_eq!(status(Some("evil.example:8787"), Some("http://evil.example:8787")).await, StatusCode::FORBIDDEN);
        assert_eq!(status(None, None).await, StatusCode::FORBIDDEN);
    }
}
