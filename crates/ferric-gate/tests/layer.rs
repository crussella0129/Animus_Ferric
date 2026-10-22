//! The gate as axum middleware: denials carry the header and no CORS, the
//! fallback is covered, and the credential never reaches a handler.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::routing::{get, post};
use ferric_gate::{DENIAL_HEADER, Gate, Token, protect};
use tower::ServiceExt as _;

/// Echo whether the handler saw an Authorization header.
async fn echo(headers: HeaderMap) -> String {
    format!("auth-seen={}", headers.contains_key(header::AUTHORIZATION))
}

fn app(token: &Token) -> Router {
    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/query", post(echo))
        .fallback(echo);
    protect(router, Gate::new(token, 3581).open_path("/health"))
}

fn request(method: &str, path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from("{}")).unwrap()
}

async fn body_text(response: axum::response::Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn admitted_requests_reach_the_handler_without_the_credential() {
    let token = Token::generate().unwrap();
    let auth = format!("Bearer {}", token.as_str());
    let response = app(&token)
        .oneshot(request(
            "POST",
            "/v1/query",
            &[
                ("host", "127.0.0.1:3581"),
                ("authorization", &auth),
                ("content-type", "application/json"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, "auth-seen=false");
}

#[tokio::test]
async fn denials_are_marked_and_never_carry_cors() {
    let token = Token::generate().unwrap();
    let response = app(&token)
        .oneshot(request(
            "POST",
            "/v1/query",
            &[
                ("host", "127.0.0.1:3581"),
                ("origin", "https://attacker.example"),
                ("content-type", "text/plain"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.headers()[DENIAL_HEADER], "browser-origin");
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    assert!(body_text(response).await.contains("ferric_gate"));
}

#[tokio::test]
async fn the_fallback_is_gated_too() {
    let token = Token::generate().unwrap();
    let response = app(&token)
        .oneshot(request("GET", "/slots", &[("host", "127.0.0.1:3581")]))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[DENIAL_HEADER], "missing-token");
    assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
}

#[tokio::test]
async fn the_health_check_is_open_but_still_host_checked() {
    let token = Token::generate().unwrap();
    let ok = app(&token)
        .oneshot(request("GET", "/health", &[("host", "localhost:3581")]))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let rebound = app(&token)
        .oneshot(request(
            "GET",
            "/health",
            &[("host", "attacker.example:3581")],
        ))
        .await
        .unwrap();
    assert_eq!(rebound.status(), StatusCode::FORBIDDEN);
    assert_eq!(rebound.headers()[DENIAL_HEADER], "foreign-host");
}
