//! Host, Origin, and bearer-token checks for every web request.

use super::{errors::error, WebState};
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};

pub(crate) async fn security(
    State(state): State<WebState>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    if !headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| allowed_host(host, &state.config.listen_host))
    {
        return error(StatusCode::FORBIDDEN, "host", "Host is not allowed");
    }
    if headers.contains_key(header::ORIGIN)
        && !headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|origin| {
                state
                    .config
                    .allow_origins
                    .iter()
                    .any(|allowed| allowed == origin)
            })
    {
        return error(StatusCode::FORBIDDEN, "origin", "Origin is not allowed");
    }
    if request.uri().path() != "/v1/health"
        && state.config.token.as_ref().is_some_and(|token| {
            !constant_time_eq(
                format!("Bearer {token}").as_bytes(),
                headers
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .as_bytes(),
            )
        })
    {
        return error(StatusCode::UNAUTHORIZED, "auth", "Bearer token required");
    }
    next.run(request).await
}

fn allowed_host(host: &str, configured: &str) -> bool {
    host == configured
        || host
            .split(':')
            .next()
            .is_some_and(|value| matches!(value, "127.0.0.1" | "localhost"))
        || host.starts_with("[::1]")
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.iter()
        .zip(right)
        .fold(left.len() ^ right.len(), |difference, (a, b)| {
            difference | usize::from(a ^ b)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use axum::{
        body::Body,
        http::{header, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn security_rejects_bad_host_and_origin_and_enforces_token() {
        let (service, _, path) = test_service().await;
        let app = app(service, Some("secret"));
        let mut bad_host = request("GET", "/v1/status", Body::empty());
        bad_host
            .headers_mut()
            .insert(header::HOST, "attacker.example".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(bad_host).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let mut origin = request("GET", "/v1/status", Body::empty());
        origin
            .headers_mut()
            .insert(header::ORIGIN, "https://attacker.example".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(origin).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            app.clone()
                .oneshot(request("GET", "/v1/status", Body::empty()))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let mut authorized = request("GET", "/v1/status", Body::empty());
        authorized
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(authorized).await.unwrap().status(),
            StatusCode::OK
        );
        assert_eq!(
            app.oneshot(request("GET", "/v1/health", Body::empty()))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let _ = std::fs::remove_file(path);
    }
}
