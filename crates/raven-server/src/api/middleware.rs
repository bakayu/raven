use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Request},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub request_id: String,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
}

pub async fn with_request_context(mut req: Request<Body>, next: Next) -> Response {
    let headers = req.headers();
    let request_id = extract_request_id(headers).unwrap_or_else(|| Uuid::new_v4().to_string());
    let client_ip = extract_client_ip(&req, headers);
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());

    req.extensions_mut().insert(RequestContext {
        request_id: request_id.clone(),
        client_ip,
        user_agent,
    });

    let mut response = next.run(req).await;
    response.headers_mut().insert(
        "x-request-id",
        request_id
            .parse()
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("unknown")),
    );

    response
}

fn extract_request_id(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("x-request-id")?.to_str().ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 64 || !trimmed.is_ascii() {
        return None;
    }

    Some(trimmed.to_string())
}

fn extract_client_ip(req: &Request<Body>, headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("x-forwarded-for")
        && let Ok(raw) = value.to_str()
        && let Some(ip) = raw.split(',').map(|s| s.trim()).find(|s| !s.is_empty())
    {
        return Some(ip.to_string());
    }

    if let Some(value) = headers.get("x-real-ip")
        && let Ok(raw) = value.to_str()
    {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn accepts_valid_request_id() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static("req-123"));

        assert_eq!(extract_request_id(&headers).as_deref(), Some("req-123"));
    }

    #[test]
    fn rejects_invalid_request_id() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static(""));

        assert!(extract_request_id(&headers).is_none());
    }
}
