//! Integration tests for the OAuth Resource Server middleware.
//!
//! Each test mints a real RS256 JWT in-process, serves a JWKS over a local
//! `wiremock` instance, and exercises `verify_bearer` against it.

#![cfg(test)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// 2048-bit RSA test keypair (generated for tests only — never used in prod).
// PEM-encoded; loaded into jsonwebtoken at runtime.
const RSA_PRIV_PEM: &str = include_str!("./fixtures/rsa_priv.pem");
const RSA_JWK_N: &str = include_str!("./fixtures/rsa_n.txt");
const RSA_JWK_E: &str = include_str!("./fixtures/rsa_e.txt");

#[derive(Serialize)]
struct TestClaims {
    iss: String,
    aud: String,
    sub: String,
    exp: u64,
    iat: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn jwks_json(kid: &str) -> serde_json::Value {
    serde_json::json!({
        "keys": [{
            "kty": "RSA",
            "kid": kid,
            "use": "sig",
            "alg": "RS256",
            "n": RSA_JWK_N.trim(),
            "e": RSA_JWK_E.trim(),
        }]
    })
}

async fn mock_jwks(kid: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jwks_json(kid)))
        .mount(&server)
        .await;
    server
}

fn mint_token(claims: &TestClaims, kid: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_string());
    let key = EncodingKey::from_rsa_pem(RSA_PRIV_PEM.as_bytes()).expect("private key");
    encode(&header, claims, &key).expect("encode")
}

fn cfg_for(server: &MockServer) -> roon_mcp::auth::AuthConfig {
    roon_mcp::auth::AuthConfig {
        issuer: "https://issuer.test".into(),
        audience: "https://issuer.test/roon".into(),
        jwks_url: format!("{}/jwks.json", server.uri()),
        require_auth: true,
    }
}

#[tokio::test]
async fn rejects_missing_header() {
    let server = mock_jwks("k1").await;
    let cfg = cfg_for(&server);
    let cache = Arc::new(roon_mcp::auth::JwksCache::new(cfg.jwks_url.clone()));
    let err = roon_mcp::auth::verify_bearer(None, &cache, &cfg)
        .await
        .unwrap_err();
    assert!(matches!(err, roon_mcp::auth::AuthError::Missing));
}

#[tokio::test]
async fn rejects_malformed_header() {
    let server = mock_jwks("k1").await;
    let cfg = cfg_for(&server);
    let cache = Arc::new(roon_mcp::auth::JwksCache::new(cfg.jwks_url.clone()));
    let err = roon_mcp::auth::verify_bearer(Some("Basic xyz"), &cache, &cfg)
        .await
        .unwrap_err();
    assert!(matches!(err, roon_mcp::auth::AuthError::Malformed));
}

#[tokio::test]
async fn rejects_expired_token() {
    let server = mock_jwks("k1").await;
    let cfg = cfg_for(&server);
    let cache = Arc::new(roon_mcp::auth::JwksCache::new(cfg.jwks_url.clone()));
    // jsonwebtoken default leeway is 60s — set exp well past leeway window
    let claims = TestClaims {
        iss: cfg.issuer.clone(),
        aud: cfg.audience.clone(),
        sub: "user-1".into(),
        exp: now_secs() - 3600,
        iat: now_secs() - 7200,
    };
    let token = mint_token(&claims, "k1");
    let header = format!("Bearer {}", token);
    let err = roon_mcp::auth::verify_bearer(Some(&header), &cache, &cfg)
        .await
        .unwrap_err();
    assert!(matches!(err, roon_mcp::auth::AuthError::Expired));
}

#[tokio::test]
async fn rejects_wrong_issuer() {
    let server = mock_jwks("k1").await;
    let cfg = cfg_for(&server);
    let cache = Arc::new(roon_mcp::auth::JwksCache::new(cfg.jwks_url.clone()));
    let claims = TestClaims {
        iss: "https://wrong-issuer.test".into(),
        aud: cfg.audience.clone(),
        sub: "user-1".into(),
        exp: now_secs() + 3600,
        iat: now_secs(),
    };
    let token = mint_token(&claims, "k1");
    let header = format!("Bearer {}", token);
    let err = roon_mcp::auth::verify_bearer(Some(&header), &cache, &cfg)
        .await
        .unwrap_err();
    assert!(matches!(err, roon_mcp::auth::AuthError::WrongIssuer));
}

#[tokio::test]
async fn accepts_any_audience_because_aud_check_disabled() {
    // Audience verification is intentionally disabled (cognee/openmemory
    // precedent) — Hydra issues tokens without `aud` unless the client
    // passes RFC 8707 `resource`, which Claude clients do not. Issuer +
    // RS256 signature verification + ALLOWED_EMAILS at the consent screen
    // is the effective authorization perimeter.
    let server = mock_jwks("k1").await;
    let cfg = cfg_for(&server);
    let cache = Arc::new(roon_mcp::auth::JwksCache::new(cfg.jwks_url.clone()));
    let claims = TestClaims {
        iss: cfg.issuer.clone(),
        aud: "https://other-resource.test/api".into(),
        sub: "user-1".into(),
        exp: now_secs() + 3600,
        iat: now_secs(),
    };
    let token = mint_token(&claims, "k1");
    let header = format!("Bearer {}", token);
    let result = roon_mcp::auth::verify_bearer(Some(&header), &cache, &cfg).await;
    assert!(
        result.is_ok(),
        "expected Ok despite audience mismatch, got {:?}",
        result
    );
}

#[tokio::test]
async fn accepts_valid_token() {
    let server = mock_jwks("k1").await;
    let cfg = cfg_for(&server);
    let cache = Arc::new(roon_mcp::auth::JwksCache::new(cfg.jwks_url.clone()));
    let claims = TestClaims {
        iss: cfg.issuer.clone(),
        aud: cfg.audience.clone(),
        sub: "user-1".into(),
        exp: now_secs() + 3600,
        iat: now_secs(),
    };
    let token = mint_token(&claims, "k1");
    let header = format!("Bearer {}", token);
    let result = roon_mcp::auth::verify_bearer(Some(&header), &cache, &cfg).await;
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    assert_eq!(result.unwrap().sub.as_deref(), Some("user-1"));
}
