//! OAuth 2.1 minimal et sans état, pour les connecteurs claude.ai.
//! Codes et tokens sont signés en HMAC avec le secret du serveur (bearer_token).
use crate::auth::tokens_match;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Form, Json};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

const TOKEN_TTL: u64 = 7 * 24 * 3600;

#[derive(Clone)]
pub struct OAuthState {
    pub secret: Arc<String>,
    pub base_url: Arc<String>,
}

#[derive(Serialize, Deserialize)]
struct ClientInfo {
    redirect_uris: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct CodeClaims {
    client_id: String,
    redirect_uri: String,
    challenge: String,
    exp: u64,
}

#[derive(Serialize, Deserialize)]
struct TokenClaims {
    exp: u64,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn sign<T: Serialize>(s: &OAuthState, kind: &str, payload: &T) -> String {
    let body = B64.encode(serde_json::to_vec(payload).unwrap());
    let mut mac = HmacSha256::new_from_slice(s.secret.as_bytes()).unwrap();
    mac.update(kind.as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    format!("{}.{}", body, B64.encode(mac.finalize().into_bytes()))
}

fn verify<T: for<'de> Deserialize<'de>>(s: &OAuthState, kind: &str, token: &str) -> Option<T> {
    let (body, sig) = token.split_once('.')?;
    let sig_bytes = B64.decode(sig).ok()?;
    let mut mac = HmacSha256::new_from_slice(s.secret.as_bytes()).ok()?;
    mac.update(kind.as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    mac.verify_slice(&sig_bytes).ok()?;
    serde_json::from_slice(&B64.decode(body).ok()?).ok()
}

pub fn is_valid_access_token(s: &OAuthState, token: &str) -> bool {
    verify::<TokenClaims>(s, "token", token).map_or(false, |c| c.exp > now())
}

fn redirect_allowed(u: &str) -> bool {
    if u.starts_with("https://claude.ai/") || u.starts_with("https://claude.com/") {
        return true;
    }
    for p in ["http://localhost", "http://127.0.0.1"] {
        if let Some(rest) = u.strip_prefix(p) {
            return rest.starts_with(':') || rest.starts_with('/');
        }
    }
    false
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

// ---------- Métadonnées de découverte ----------

pub async fn protected_resource(State(s): State<OAuthState>) -> Json<Value> {
    Json(json!({
        "resource": format!("{}/mcp", s.base_url),
        "authorization_servers": [s.base_url.as_str()],
        "bearer_methods_supported": ["header"],
    }))
}

pub async fn auth_server_metadata(State(s): State<OAuthState>) -> Json<Value> {
    let b = s.base_url.as_str();
    Json(json!({
        "issuer": b,
        "authorization_endpoint": format!("{b}/authorize"),
        "token_endpoint": format!("{b}/token"),
        "registration_endpoint": format!("{b}/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
    }))
}

// ---------- Enregistrement dynamique du client ----------

#[derive(Deserialize)]
pub struct RegisterReq {
    redirect_uris: Vec<String>,
}

pub async fn register(State(s): State<OAuthState>, Json(req): Json<RegisterReq>) -> Response {
    if req.redirect_uris.is_empty() || !req.redirect_uris.iter().all(|u| redirect_allowed(u)) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_redirect_uri"}))).into_response();
    }
    let client_id = sign(&s, "client", &ClientInfo { redirect_uris: req.redirect_uris.clone() });
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": client_id,
            "redirect_uris": req.redirect_uris,
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code"],
            "response_types": ["code"],
        })),
    )
        .into_response()
}

// ---------- Autorisation (page de saisie du secret) ----------

#[derive(Deserialize)]
pub struct AuthorizeParams {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
pub struct AuthorizeForm {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    #[serde(default)]
    state: String,
    secret: String,
}

fn check_request(
    s: &OAuthState,
    response_type: &str,
    method: Option<&str>,
    client_id: &str,
    redirect_uri: &str,
) -> Result<(), &'static str> {
    if response_type != "code" {
        return Err("response_type doit valoir code");
    }
    if method != Some("S256") {
        return Err("PKCE S256 requis");
    }
    let info = verify::<ClientInfo>(s, "client", client_id).ok_or("client_id invalide")?;
    if !info.redirect_uris.iter().any(|u| u == redirect_uri) {
        return Err("redirect_uri non enregistre");
    }
    Ok(())
}

fn form_page(client_id: &str, redirect_uri: &str, challenge: &str, state: &str, error: Option<&str>) -> Html<String> {
    let host = redirect_uri.split('/').nth(2).unwrap_or("?");
    let err = error.map(|e| format!("<p style=\"color:#b00\">{}</p>", esc(e))).unwrap_or_default();
    Html(format!(
        r#"<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<body style="font-family:sans-serif;max-width:28rem;margin:2rem auto;padding:0 1rem">
<h3>mcp-vps-server</h3>
<p>Autoriser l'accès pour <b>{host}</b> ?</p>
{err}
<form method="post" action="/authorize">
<input type="hidden" name="client_id" value="{cid}">
<input type="hidden" name="redirect_uri" value="{ruri}">
<input type="hidden" name="code_challenge" value="{chal}">
<input type="hidden" name="state" value="{st}">
<input type="password" name="secret" placeholder="Secret du serveur" autofocus style="width:100%;padding:.6rem;font-size:1rem">
<button style="margin-top:1rem;padding:.6rem 1rem;font-size:1rem">Autoriser</button>
</form></body>"#,
        host = esc(host),
        err = err,
        cid = esc(client_id),
        ruri = esc(redirect_uri),
        chal = esc(challenge),
        st = esc(state),
    ))
}

pub async fn authorize_get(State(s): State<OAuthState>, Query(p): Query<AuthorizeParams>) -> Response {
    if let Err(msg) = check_request(&s, &p.response_type, p.code_challenge_method.as_deref(), &p.client_id, &p.redirect_uri) {
        return (StatusCode::BAD_REQUEST, msg).into_response();
    }
    form_page(&p.client_id, &p.redirect_uri, &p.code_challenge, p.state.as_deref().unwrap_or(""), None).into_response()
}

pub async fn authorize_post(State(s): State<OAuthState>, Form(f): Form<AuthorizeForm>) -> Response {
    if let Err(msg) = check_request(&s, "code", Some("S256"), &f.client_id, &f.redirect_uri) {
        return (StatusCode::BAD_REQUEST, msg).into_response();
    }
    if !tokens_match(&f.secret, &s.secret) {
        return (
            StatusCode::UNAUTHORIZED,
            form_page(&f.client_id, &f.redirect_uri, &f.code_challenge, &f.state, Some("Secret incorrect")),
        )
            .into_response();
    }
    let code = sign(
        &s,
        "code",
        &CodeClaims {
            client_id: f.client_id.clone(),
            redirect_uri: f.redirect_uri.clone(),
            challenge: f.code_challenge.clone(),
            exp: now() + 300,
        },
    );
    let sep = if f.redirect_uri.contains('?') { '&' } else { '?' };
    let mut url = format!("{}{}code={}", f.redirect_uri, sep, code);
    if !f.state.is_empty() {
        url.push_str(&format!("&state={}", pct(&f.state)));
    }
    Redirect::to(&url).into_response()
}

// ---------- Échange code -> token ----------

#[derive(Deserialize)]
pub struct TokenForm {
    grant_type: String,
    code: String,
    redirect_uri: String,
    client_id: String,
    code_verifier: String,
}

pub async fn token(State(s): State<OAuthState>, Form(f): Form<TokenForm>) -> Response {
    let err = |e: &str| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response();
    if f.grant_type != "authorization_code" {
        return err("unsupported_grant_type");
    }
    let Some(claims) = verify::<CodeClaims>(&s, "code", &f.code) else {
        return err("invalid_grant");
    };
    if claims.exp < now() || claims.client_id != f.client_id || claims.redirect_uri != f.redirect_uri {
        return err("invalid_grant");
    }
    let computed = B64.encode(Sha256::digest(f.code_verifier.as_bytes()));
    if !tokens_match(&computed, &claims.challenge) {
        return err("invalid_grant");
    }
    let token = sign(&s, "token", &TokenClaims { exp: now() + TOKEN_TTL });
    Json(json!({"access_token": token, "token_type": "Bearer", "expires_in": TOKEN_TTL})).into_response()
}

pub fn router(state: OAuthState) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/.well-known/oauth-protected-resource", get(protected_resource))
        .route("/.well-known/oauth-protected-resource/mcp", get(protected_resource))
        .route("/.well-known/oauth-authorization-server", get(auth_server_metadata))
        .route("/register", post(register))
        .route("/authorize", get(authorize_get).post(authorize_post))
        .route("/token", post(token))
        .with_state(state)
}
