//! The axum middleware: apply a [`Gate`] to every route of a router,
//! including its fallback.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use serde_json::json;

use crate::DENIAL_HEADER;
use crate::guard::{Denial, Gate};

/// Wrap `router` so every request passes `gate` first. Call it last, after
/// all routes and the fallback are added, so the gate is the outermost layer.
///
/// An admitted request has its `Authorization` header removed before any
/// handler runs: the gate credential is never forwarded upstream.
pub fn protect(router: Router, gate: Gate) -> Router {
    router.layer(middleware::from_fn_with_state(Arc::new(gate), enforce))
}

async fn enforce(State(gate): State<Arc<Gate>>, mut request: Request, next: Next) -> Response {
    match gate.check(request.method(), request.uri().path(), request.headers()) {
        Ok(()) => {
            request.headers_mut().remove(header::AUTHORIZATION);
            next.run(request).await
        }
        Err(denial) => {
            tracing::info!(
                code = denial.code(),
                "ferric-gate refused: {}",
                denial.message()
            );
            denial_response(&denial)
        }
    }
}

/// The refusal: a JSON error, the denial header, no CORS headers, no caching.
pub fn denial_response(denial: &Denial) -> Response {
    let body = json!({
        "error": {
            "type": "ferric_gate",
            "code": denial.code(),
            "message": denial.message(),
        }
    });
    let mut response = Response::builder()
        .status(denial.status())
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .header(DENIAL_HEADER, HeaderValue::from_static(denial.code()));
    if matches!(denial, Denial::MissingToken | Denial::BadToken) {
        response = response.header(header::WWW_AUTHENTICATE, "Bearer realm=\"ferric\"");
    }
    response
        .body(Body::from(body.to_string()))
        .unwrap_or_default()
}
