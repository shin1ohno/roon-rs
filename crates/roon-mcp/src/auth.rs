//! OAuth 2.1 Resource Server: Bearer JWT verification + RFC 9728 metadata.
//!
//! The MCP server validates RS256 access tokens issued by an external
//! authorization server (Hydra). The signing keys are fetched from the AS's
//! JWKS endpoint and cached in-process; mismatched-kid signatures trigger a
//! single forced refresh before failure. Unauthorized responses follow the
//! cognee `auth-proxy` precedent (JSON-RPC 2.0 envelope, RFC 9728
//! `resource_metadata` parameter on `WWW-Authenticate`).

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use http::{HeaderValue, Response, StatusCode, header};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use parking_lot::RwLock;
use serde::Deserialize;
use std::convert::Infallible;
use tokio::sync::Mutex;

const JWKS_TTL: Duration = Duration::from_secs(300);
const JSONRPC_UNAUTHORIZED: i64 = -32001;

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub issuer: String,
    pub audience: String,
    pub jwks_url: String,
    pub require_auth: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Claims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub aud: Option<serde_json::Value>,
    pub exp: Option<u64>,
    pub iat: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing Authorization header")]
    Missing,
    #[error("malformed Authorization header")]
    Malformed,
    #[error("token signature invalid")]
    InvalidSignature,
    #[error("token expired")]
    Expired,
    #[error("issuer mismatch")]
    WrongIssuer,
    #[error("audience mismatch")]
    WrongAudience,
    #[error("JWKS fetch failed: {0}")]
    JwksFetch(String),
    #[error("token verification failed: {0}")]
    Other(String),
}

#[derive(Default)]
struct JwksState {
    keys: std::collections::HashMap<String, DecodingKey>,
    fetched_at: Option<Instant>,
}

pub struct JwksCache {
    state: RwLock<JwksState>,
    fetch_lock: Mutex<()>,
    jwks_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct Jwks {
    keys: Vec<serde_json::Value>,
}

impl JwksCache {
    pub fn new(jwks_url: String) -> Self {
        Self {
            state: RwLock::new(JwksState::default()),
            fetch_lock: Mutex::new(()),
            jwks_url,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("reqwest client build"),
        }
    }

    fn is_fresh(&self) -> bool {
        let s = self.state.read();
        match s.fetched_at {
            Some(at) => at.elapsed() < JWKS_TTL && !s.keys.is_empty(),
            None => false,
        }
    }

    pub async fn ensure(&self, force: bool) -> Result<(), AuthError> {
        if !force && self.is_fresh() {
            return Ok(());
        }
        let _guard = self.fetch_lock.lock().await;
        // Re-check inside the lock to coalesce concurrent fetches.
        if !force && self.is_fresh() {
            return Ok(());
        }
        let body: Jwks = self
            .http
            .get(&self.jwks_url)
            .send()
            .await
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?
            .error_for_status()
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?
            .json()
            .await
            .map_err(|e| AuthError::JwksFetch(e.to_string()))?;
        let mut keys = std::collections::HashMap::new();
        for jwk in &body.keys {
            let kid = jwk
                .get("kid")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let n = jwk.get("n").and_then(|v| v.as_str());
            let e_b64 = jwk.get("e").and_then(|v| v.as_str());
            match (n, e_b64) {
                (Some(n), Some(e_b64)) => match DecodingKey::from_rsa_components(n, e_b64) {
                    Ok(k) => {
                        keys.insert(kid, k);
                    }
                    Err(err) => {
                        tracing::warn!("skipping unparseable JWK kid={}: {}", kid, err);
                    }
                },
                _ => {
                    tracing::warn!("skipping JWK kid={} (missing n/e)", kid);
                }
            }
        }
        if keys.is_empty() {
            return Err(AuthError::JwksFetch("no usable keys".into()));
        }
        let mut s = self.state.write();
        s.keys = keys;
        s.fetched_at = Some(Instant::now());
        tracing::info!("JWKS cache populated with {} key(s)", s.keys.len());
        Ok(())
    }

    fn pick(&self, kid: Option<&str>) -> Option<DecodingKey> {
        let s = self.state.read();
        if let Some(k) = kid
            && let Some(key) = s.keys.get(k)
        {
            return Some(key.clone());
        }
        s.keys.values().next().cloned()
    }
}

/// Pre-warm the cache at startup with a short retry loop so a transient
/// network blip at boot does not kill the container.
pub async fn warm_cache(cache: &Arc<JwksCache>) {
    let mut delay = Duration::from_secs(2);
    for attempt in 1..=5 {
        match cache.ensure(false).await {
            Ok(()) => {
                tracing::info!("JWKS warm-up succeeded on attempt {}", attempt);
                return;
            }
            Err(e) => {
                tracing::warn!("JWKS warm-up attempt {} failed: {}", attempt, e);
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(60));
            }
        }
    }
    tracing::warn!("JWKS warm-up gave up after 5 attempts; will lazy-load on first request");
}

pub async fn verify_bearer(
    auth_header: Option<&str>,
    cache: &Arc<JwksCache>,
    cfg: &AuthConfig,
) -> Result<Claims, AuthError> {
    let header_val = auth_header.ok_or(AuthError::Missing)?;
    let token = header_val
        .strip_prefix("Bearer ")
        .ok_or(AuthError::Malformed)?
        .trim();
    if token.is_empty() {
        return Err(AuthError::Malformed);
    }

    let header =
        decode_header(token).map_err(|e| AuthError::Other(format!("bad header: {}", e)))?;
    let kid = header.kid;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(std::slice::from_ref(&cfg.issuer));
    validation.set_audience(std::slice::from_ref(&cfg.audience));

    async fn attempt(
        cache: &Arc<JwksCache>,
        token: &str,
        kid: Option<&str>,
        validation: &Validation,
        force_refresh: bool,
    ) -> Result<Claims, AuthError> {
        cache.ensure(force_refresh).await?;
        let key = cache.pick(kid).ok_or(AuthError::InvalidSignature)?;
        decode::<Claims>(token, &key, validation)
            .map(|d| d.claims)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::Expired,
                jsonwebtoken::errors::ErrorKind::InvalidIssuer => AuthError::WrongIssuer,
                jsonwebtoken::errors::ErrorKind::InvalidAudience => AuthError::WrongAudience,
                jsonwebtoken::errors::ErrorKind::InvalidSignature => AuthError::InvalidSignature,
                _ => AuthError::Other(e.to_string()),
            })
    }

    match attempt(cache, token, kid.as_deref(), &validation, false).await {
        Ok(c) => Ok(c),
        Err(AuthError::InvalidSignature) | Err(AuthError::JwksFetch(_)) => {
            attempt(cache, token, kid.as_deref(), &validation, true).await
        }
        Err(other) => Err(other),
    }
}

fn body_from_json(value: &serde_json::Value) -> BoxBody<Bytes, Infallible> {
    let bytes = Bytes::from(serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec()));
    Full::new(bytes).map_err(|never| match never {}).boxed()
}

const CORS_EXPOSE: &str = "WWW-Authenticate";
const CORS_ALLOW_ORIGIN: &str = "*";

fn add_common_headers(builder: http::response::Builder) -> http::response::Builder {
    builder
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, CORS_ALLOW_ORIGIN)
        .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, CORS_EXPOSE)
}

/// RFC 9728 §3 protected-resource metadata.
pub fn metadata_response(cfg: &AuthConfig) -> Response<BoxBody<Bytes, Infallible>> {
    let body = serde_json::json!({
        "resource": cfg.audience,
        "authorization_servers": [cfg.issuer],
        "scopes_supported": ["mcp:read", "mcp:write"],
        "bearer_methods_supported": ["header"],
    });
    add_common_headers(Response::builder().status(StatusCode::OK))
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(body_from_json(&body))
        .expect("valid response")
}

/// JSON-RPC 2.0 envelope per the cognee precedent — Claude clients reject
/// plain `{"error":"..."}` 401 bodies before reading WWW-Authenticate.
pub fn unauthorized_response(
    cfg: &AuthConfig,
    invalid_token: bool,
) -> Response<BoxBody<Bytes, Infallible>> {
    let resource_metadata = format!(
        "{}/.well-known/oauth-protected-resource",
        cfg.audience.trim_end_matches('/')
    );
    let www_auth = if invalid_token {
        format!(
            "Bearer error=\"invalid_token\", resource_metadata=\"{}\"",
            resource_metadata
        )
    } else {
        format!("Bearer resource_metadata=\"{}\"", resource_metadata)
    };
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": JSONRPC_UNAUTHORIZED,
            "message": if invalid_token { "invalid_token" } else { "unauthorized" },
        },
    });
    add_common_headers(Response::builder().status(StatusCode::UNAUTHORIZED))
        .header(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_str(&www_auth).unwrap_or(HeaderValue::from_static("Bearer")),
        )
        .body(body_from_json(&body))
        .expect("valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(jwks_url: &str) -> AuthConfig {
        AuthConfig {
            issuer: "https://example.com".into(),
            audience: "https://example.com/roon".into(),
            jwks_url: jwks_url.into(),
            require_auth: true,
        }
    }

    #[test]
    fn metadata_shape_matches_rfc9728() {
        let c = cfg("https://example.com/.well-known/jwks.json");
        let resp = metadata_response(&c);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn unauthorized_missing_token_has_bare_www_authenticate() {
        let c = cfg("https://example.com/.well-known/jwks.json");
        let resp = unauthorized_response(&c, false);
        let h = resp
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(h.starts_with("Bearer resource_metadata="));
        assert!(!h.contains("error="));
    }

    #[test]
    fn unauthorized_invalid_token_includes_error_param() {
        let c = cfg("https://example.com/.well-known/jwks.json");
        let resp = unauthorized_response(&c, true);
        let h = resp
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(h.contains("error=\"invalid_token\""));
        assert!(h.contains("resource_metadata="));
    }
}
