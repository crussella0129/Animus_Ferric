//! The request check. Pure: method, path and headers in, a decision out.
//!
//! The checks run in a fixed order, and each one stands on its own. A stolen
//! token does not help a web page (the browser check still fires), and a
//! rebinding page does not get past the `Host` check even if it guessed the
//! token.
//!
//! 1. **Host.** The `Host` header must name this listener exactly
//!    (`127.0.0.1:<port>`, `localhost:<port>`, `[::1]:<port>`, or an address
//!    added with [`Gate::allow_host`]). DNS rebinding sends the attacker's
//!    hostname, so it stops here.
//! 2. **No browsers.** Any `Origin` or `Sec-Fetch-*` header is refused.
//!    Browsers attach them to every cross-site and same-site request;
//!    Ferric's clients (Hermes, curl, the IDE, scripts) never do. This is
//!    what closes the drive-by form POST and the no-cors `fetch`.
//! 3. **Token.** `Authorization: Bearer <token>`, compared as SHA-256
//!    digests in constant time. Paths opened with [`Gate::open_path`] skip
//!    this for `GET` only (the health check).
//! 4. **JSON bodies.** `POST`/`PUT`/`PATCH` must declare
//!    `application/json`. The CORS-safelisted types a page can send without a
//!    preflight (`text/plain`, form encodings) never reach a handler.
//!
//! No CORS header is ever emitted, so a browser cannot read a response even
//! if a request somehow got through.

use axum::http::{HeaderMap, Method, StatusCode, header};

use crate::token::Token;

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denial {
    MissingHost,
    ForeignHost(String),
    BrowserOrigin(String),
    BrowserFetch(String),
    MissingToken,
    BadToken,
    NotJson(String),
}

impl Denial {
    pub fn status(&self) -> StatusCode {
        match self {
            Denial::MissingHost
            | Denial::ForeignHost(_)
            | Denial::BrowserOrigin(_)
            | Denial::BrowserFetch(_) => StatusCode::FORBIDDEN,
            Denial::MissingToken | Denial::BadToken => StatusCode::UNAUTHORIZED,
            Denial::NotJson(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        }
    }

    /// Stable machine code, also the value of the denial header.
    pub fn code(&self) -> &'static str {
        match self {
            Denial::MissingHost => "missing-host",
            Denial::ForeignHost(_) => "foreign-host",
            Denial::BrowserOrigin(_) => "browser-origin",
            Denial::BrowserFetch(_) => "browser-fetch",
            Denial::MissingToken => "missing-token",
            Denial::BadToken => "bad-token",
            Denial::NotJson(_) => "not-json",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Denial::MissingHost => "request has no Host header".to_string(),
            Denial::ForeignHost(host) => format!(
                "Host '{host}' does not name this listener (DNS rebinding is refused)"
            ),
            Denial::BrowserOrigin(origin) => format!(
                "browser request from origin '{origin}' refused: Ferric services do not serve web pages"
            ),
            Denial::BrowserFetch(site) => format!(
                "browser request (Sec-Fetch-Site: {site}) refused: Ferric services do not serve web pages"
            ),
            Denial::MissingToken => {
                "missing bearer token: send `Authorization: Bearer <token>` from the service's token file"
                    .to_string()
            }
            Denial::BadToken => "bearer token does not match".to_string(),
            Denial::NotJson(found) => {
                format!("request body must be application/json, not '{found}'")
            }
        }
    }
}

/// The policy for one listener.
#[derive(Clone)]
pub struct Gate {
    digest: [u8; 32],
    hosts: Vec<String>,
    open_paths: Vec<String>,
}

impl std::fmt::Debug for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gate")
            .field("hosts", &self.hosts)
            .field("open_paths", &self.open_paths)
            .finish_non_exhaustive()
    }
}

impl Gate {
    /// A gate for a loopback listener on `port`.
    pub fn new(token: &Token, port: u16) -> Self {
        Gate {
            digest: token.digest(),
            hosts: vec![
                format!("127.0.0.1:{port}"),
                format!("localhost:{port}"),
                format!("[::1]:{port}"),
            ],
            open_paths: Vec::new(),
        }
    }

    /// Also accept this `host:port` authority (for example a tunnel address).
    pub fn allow_host(mut self, authority: &str) -> Self {
        self.hosts.push(authority.to_ascii_lowercase());
        self
    }

    /// Let `GET` on this exact path skip the token (Host and browser checks
    /// still apply). Meant for a content-free health check.
    pub fn open_path(mut self, path: &str) -> Self {
        self.open_paths.push(path.to_string());
        self
    }

    pub fn allowed_hosts(&self) -> &[String] {
        &self.hosts
    }

    pub fn check(&self, method: &Method, path: &str, headers: &HeaderMap) -> Result<(), Denial> {
        // 1. Host.
        let host = headers
            .get(header::HOST)
            .ok_or(Denial::MissingHost)?
            .to_str()
            .map_err(|_| Denial::ForeignHost("<non-ascii>".to_string()))?
            .trim()
            .to_ascii_lowercase();
        if !self.hosts.contains(&host) {
            return Err(Denial::ForeignHost(host));
        }

        // 2. No browsers.
        if let Some(origin) = headers.get(header::ORIGIN) {
            return Err(Denial::BrowserOrigin(lossy(origin)));
        }
        for name in ["sec-fetch-site", "sec-fetch-mode", "sec-fetch-dest"] {
            if let Some(value) = headers.get(name) {
                return Err(Denial::BrowserFetch(lossy(value)));
            }
        }

        // 3. Token.
        let open = method == Method::GET && self.open_paths.iter().any(|p| p == path);
        if !open {
            let presented = headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| {
                    value
                        .strip_prefix("Bearer ")
                        .or_else(|| value.strip_prefix("bearer "))
                })
                .map(str::trim)
                .ok_or(Denial::MissingToken)?;
            let presented: [u8; 32] = sha2::Digest::finalize(
                <sha2::Sha256 as sha2::Digest>::new_with_prefix(presented.as_bytes()),
            )
            .into();
            if !constant_time_eq(&presented, &self.digest) {
                return Err(Denial::BadToken);
            }
        }

        // 4. JSON bodies.
        if matches!(*method, Method::POST | Method::PUT | Method::PATCH) {
            let content_type = headers
                .get(header::CONTENT_TYPE)
                .map(lossy)
                .unwrap_or_default();
            let essence = content_type
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if essence != "application/json" {
                return Err(Denial::NotJson(if content_type.is_empty() {
                    "<none>".to_string()
                } else {
                    content_type
                }));
            }
        }
        Ok(())
    }
}

fn lossy(value: &axum::http::HeaderValue) -> String {
    String::from_utf8_lossy(value.as_bytes()).into_owned()
}

/// Compare two digests without an early exit.
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn gate() -> (Gate, Token) {
        let token = Token::generate().unwrap();
        (Gate::new(&token, 3581).open_path("/health"), token)
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    fn bearer(token: &Token) -> String {
        format!("Bearer {}", token.as_str())
    }

    #[test]
    fn a_legitimate_local_client_passes() {
        let (gate, token) = gate();
        let h = headers(&[
            ("host", "127.0.0.1:3581"),
            ("authorization", &bearer(&token)),
            ("content-type", "application/json; charset=utf-8"),
        ]);
        assert_eq!(gate.check(&Method::POST, "/v1/query", &h), Ok(()));
        let h = headers(&[
            ("host", "localhost:3581"),
            ("authorization", &bearer(&token)),
        ]);
        assert_eq!(gate.check(&Method::GET, "/v1/models", &h), Ok(()));
    }

    #[test]
    fn dns_rebinding_is_refused_even_with_the_token() {
        let (gate, token) = gate();
        let h = headers(&[
            ("host", "attacker.example:3581"),
            ("authorization", &bearer(&token)),
            ("content-type", "application/json"),
        ]);
        assert_eq!(
            gate.check(&Method::POST, "/v1/query", &h),
            Err(Denial::ForeignHost("attacker.example:3581".into()))
        );
        // Wrong port on a loopback name is also foreign.
        let h = headers(&[
            ("host", "127.0.0.1:9999"),
            ("authorization", &bearer(&token)),
        ]);
        assert!(matches!(
            gate.check(&Method::GET, "/health", &h),
            Err(Denial::ForeignHost(_))
        ));
        assert_eq!(
            gate.check(&Method::GET, "/health", &HeaderMap::new()),
            Err(Denial::MissingHost)
        );
    }

    #[test]
    fn browsers_are_refused_even_with_the_token() {
        let (gate, token) = gate();
        let h = headers(&[
            ("host", "127.0.0.1:3581"),
            ("origin", "https://attacker.example"),
            ("authorization", &bearer(&token)),
            ("content-type", "application/json"),
        ]);
        assert_eq!(
            gate.check(&Method::POST, "/v1/query", &h)
                .unwrap_err()
                .code(),
            "browser-origin"
        );
        let h = headers(&[("host", "127.0.0.1:3581"), ("sec-fetch-site", "cross-site")]);
        assert_eq!(
            gate.check(&Method::GET, "/health", &h).unwrap_err().code(),
            "browser-fetch"
        );
        // `Origin: null` (sandboxed iframes, file://) is a browser too.
        let h = headers(&[("host", "127.0.0.1:3581"), ("origin", "null")]);
        assert!(gate.check(&Method::GET, "/health", &h).is_err());
    }

    #[test]
    fn the_token_is_required_and_must_match() {
        let (gate, _token) = gate();
        let h = headers(&[
            ("host", "127.0.0.1:3581"),
            ("content-type", "application/json"),
        ]);
        assert_eq!(
            gate.check(&Method::POST, "/v1/query", &h),
            Err(Denial::MissingToken)
        );
        let other = Token::generate().unwrap();
        let h = headers(&[
            ("host", "127.0.0.1:3581"),
            ("authorization", &bearer(&other)),
            ("content-type", "application/json"),
        ]);
        assert_eq!(
            gate.check(&Method::POST, "/v1/query", &h),
            Err(Denial::BadToken)
        );
        let h = headers(&[
            ("host", "127.0.0.1:3581"),
            ("authorization", "Basic Zm9vOmJhcg=="),
        ]);
        assert_eq!(
            gate.check(&Method::GET, "/x", &h),
            Err(Denial::MissingToken)
        );
    }

    #[test]
    fn open_paths_skip_only_the_token_and_only_for_get() {
        let (gate, _token) = gate();
        let h = headers(&[("host", "127.0.0.1:3581")]);
        assert_eq!(gate.check(&Method::GET, "/health", &h), Ok(()));
        assert_eq!(
            gate.check(&Method::POST, "/health", &h),
            Err(Denial::MissingToken)
        );
        assert_eq!(
            gate.check(&Method::GET, "/health/x", &h),
            Err(Denial::MissingToken)
        );
    }

    #[test]
    fn preflight_free_body_types_are_refused() {
        let (gate, token) = gate();
        for content_type in [
            "text/plain",
            "application/x-www-form-urlencoded",
            "multipart/form-data; boundary=x",
        ] {
            let h = headers(&[
                ("host", "127.0.0.1:3581"),
                ("authorization", &bearer(&token)),
                ("content-type", content_type),
            ]);
            assert_eq!(
                gate.check(&Method::POST, "/v1/query", &h)
                    .unwrap_err()
                    .code(),
                "not-json",
                "{content_type}"
            );
        }
        let h = headers(&[
            ("host", "127.0.0.1:3581"),
            ("authorization", &bearer(&token)),
        ]);
        assert_eq!(
            gate.check(&Method::POST, "/v1/query", &h),
            Err(Denial::NotJson("<none>".into()))
        );
    }

    #[test]
    fn allow_host_admits_a_tunnel_authority() {
        let token = Token::generate().unwrap();
        let gate = Gate::new(&token, 3581).allow_host("10.77.0.1:3581");
        let h = headers(&[
            ("host", "10.77.0.1:3581"),
            ("authorization", &bearer(&token)),
        ]);
        assert_eq!(gate.check(&Method::GET, "/x", &h), Ok(()));
    }
}
