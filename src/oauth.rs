use crate::config::{OAuthProvider, OAuthTokens};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use rand::distr::{Alphanumeric, SampleString};
use reqwest::Client;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

const CLIENT_ID: &str = "kGsiLX8UJCf34BPOXtzn";
const CLIENT_SECRET: &str = "RJSTS3P6FkwQW5oCIP9QEtgetTWMrARYrESyAAZz";
const SCOPE: &str = "openid email offline_access full_access";
const CALLBACK_PATH: &str = "/authorization";

pub struct OAuthFlow {
    listener: TcpListener,
    pub state: String,
    pub redirect_uri: String,
    pub authorization_url: String,
    token_url: String,
    provider: OAuthProvider,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn begin(provider: OAuthProvider) -> Result<OAuthFlow> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("unable to allocate a local OAuth callback port")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://localhost:{port}{CALLBACK_PATH}");
    let state = Alphanumeric.sample_string(&mut rand::rng(), 48);
    let base = oauth_base_url(provider);
    let mut url = Url::parse(&format!("{base}/authorize"))?;
    url.query_pairs_mut()
        .append_pair("client_id", &client_id())
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("state", &state);
    Ok(OAuthFlow {
        listener,
        state,
        redirect_uri,
        authorization_url: url.to_string(),
        token_url: format!("{base}/token"),
        provider,
    })
}

pub async fn finish(
    client: &Client,
    flow: OAuthFlow,
    callback_url: Option<&str>,
) -> Result<OAuthTokens> {
    let callback = match callback_url {
        Some(url) => Url::parse(url.trim()).context("invalid OAuth callback URL")?,
        None => wait_for_callback(&flow).await?,
    };
    let code = validate_callback(&callback, &flow.state)?;
    let id = client_id();
    let secret = client_secret();
    let response = client
        .post(&flow.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", id.as_str()),
            ("client_secret", secret.as_str()),
            ("redirect_uri", flow.redirect_uri.as_str()),
        ])
        .send()
        .await
        .context("unable to reach the UCloud OAuth token endpoint")?;
    decode_token_response(response, flow.provider).await
}

pub async fn refresh(client: &Client, tokens: &OAuthTokens) -> Result<OAuthTokens> {
    let refresh_token = tokens
        .refresh_token
        .as_ref()
        .ok_or_else(|| anyhow!("the OAuth session cannot be refreshed; run `astf login`"))?;
    use secrecy::ExposeSecret;
    let token_url = format!("{}/token", oauth_base_url(tokens.provider));
    let id = client_id();
    let secret = client_secret();
    let response = client
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose_secret()),
            ("client_id", id.as_str()),
            ("client_secret", secret.as_str()),
        ])
        .send()
        .await?;
    let mut refreshed = decode_token_response(response, tokens.provider).await?;
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = tokens.refresh_token.clone();
    }
    if refreshed.email.is_none() {
        refreshed.email = tokens.email.clone();
    }
    Ok(refreshed)
}

pub fn needs_refresh(tokens: &OAuthTokens) -> bool {
    tokens
        .expires_at
        .is_some_and(|expires| expires <= unix_now().saturating_add(300))
}

async fn wait_for_callback(flow: &OAuthFlow) -> Result<Url> {
    let accept = async {
        let (mut stream, _) = flow.listener.accept().await?;
        let mut buffer = vec![0_u8; 16 * 1024];
        let read = stream.read(&mut buffer).await?;
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or_else(|| anyhow!("invalid OAuth callback request"))?;
        let url = Url::parse(&format!("http://localhost{target}"))?;
        let result = validate_callback(&url, &flow.state);
        let (status, title, message) = match &result {
            Ok(_) => (
                "200 OK",
                "Login successful",
                "You can close this tab and return to AstraFlow.",
            ),
            Err(_) => (
                "400 Bad Request",
                "Authorization failed",
                "Return to AstraFlow and start login again.",
            ),
        };
        let body = format!(
            "<!doctype html><meta charset=utf-8><title>{title}</title><style>body{{font:16px system-ui;display:grid;place-items:center;min-height:90vh;background:#111;color:#e9fbff}}main{{max-width:36rem;padding:2rem;border:1px solid #79dff2}}h1{{color:#79dff2}}</style><main><h1>{title}</h1><p>{message}</p></main>"
        );
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        result?;
        Ok::<_, anyhow::Error>(url)
    };
    tokio::time::timeout(Duration::from_secs(180), accept)
        .await
        .context("UCloud authorization timed out")?
}

fn validate_callback(url: &Url, expected_state: &str) -> Result<String> {
    let values: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    if let Some(error) = values.get("error") {
        bail!(
            "UCloud authorization failed: {}",
            values.get("error_description").unwrap_or(error)
        );
    }
    if values.get("state").map(String::as_str) != Some(expected_state) {
        bail!("OAuth state mismatch");
    }
    values
        .get("code")
        .filter(|code| !code.trim().is_empty())
        .cloned()
        .ok_or_else(|| anyhow!("the OAuth callback did not include an authorization code"))
}

async fn decode_token_response(
    response: reqwest::Response,
    provider: OAuthProvider,
) -> Result<OAuthTokens> {
    let status = response.status();
    let payload: TokenResponse = response
        .json()
        .await
        .context("UCloud OAuth returned an invalid response")?;
    let access_token = payload.access_token.ok_or_else(|| {
        anyhow!(
            "UCloud OAuth failed: {}",
            payload
                .error_description
                .or(payload.error)
                .unwrap_or_else(|| format!("HTTP {status}"))
        )
    })?;
    Ok(OAuthTokens {
        provider,
        access_token: SecretString::from(access_token),
        refresh_token: payload.refresh_token.map(SecretString::from),
        token_type: payload.token_type.unwrap_or_else(|| "Bearer".into()),
        expires_at: payload
            .expires_in
            .map(|seconds| unix_now().saturating_add(seconds)),
        email: payload.id_token.as_deref().and_then(id_token_email),
    })
}

fn id_token_email(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let json: Value = serde_json::from_slice(&bytes).ok()?;
    json.get("email")?.as_str().map(str::to_owned)
}

fn oauth_base_url(provider: OAuthProvider) -> String {
    env::var("ASTRAFLOW_OAUTH_BASE_URL")
        .unwrap_or_else(|_| provider.oauth_base_url().to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn client_id() -> String {
    env::var("UCLOUD_OAUTH_CLIENT_ID").unwrap_or_else(|_| CLIENT_ID.to_owned())
}

fn client_secret() -> String {
    env::var("UCLOUD_OAUTH_CLIENT_SECRET").unwrap_or_else(|_| CLIENT_SECRET.to_owned())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_requires_matching_state() {
        let ok = Url::parse("http://localhost:123/authorization?code=abc&state=expected").unwrap();
        assert_eq!(validate_callback(&ok, "expected").unwrap(), "abc");
        assert!(
            validate_callback(&ok, "wrong")
                .unwrap_err()
                .to_string()
                .contains("state")
        );
    }

    #[test]
    fn id_token_email_is_decoded() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"email":"agent@example.com"}"#);
        assert_eq!(
            id_token_email(&format!("header.{payload}.sig")).as_deref(),
            Some("agent@example.com")
        );
    }
}
