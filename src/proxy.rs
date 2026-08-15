use anyhow::{Context, Result, bail};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use futures_util::TryStreamExt;
use rand::distr::{Alphanumeric, SampleString};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Clone)]
struct ProxyState {
    client: Client,
    upstream: String,
    upstream_key: SecretString,
    local_token: SecretString,
}

pub struct VaultTunnel {
    pub address: SocketAddr,
    pub token: SecretString,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), std::io::Error>>,
}

#[derive(Debug, Serialize)]
pub struct VaultTunnelInfo {
    pub base_url: String,
    pub local_token: String,
    pub note: &'static str,
}

impl VaultTunnel {
    pub fn info(&self) -> VaultTunnelInfo {
        VaultTunnelInfo {
            base_url: format!("http://{}", self.address),
            local_token: self.token.expose_secret().to_owned(),
            note: "This is an ephemeral localhost token, not your ModelVerse API key.",
        }
    }

    pub async fn stop(mut self) -> Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.await.context("vault tunnel task failed")??;
        Ok(())
    }
}

pub async fn start(
    client: Client,
    listen: &str,
    endpoint: &str,
    api_key: SecretString,
) -> Result<VaultTunnel> {
    let requested: SocketAddr = listen
        .parse()
        .context("invalid vault-tunnel listen address")?;
    if !requested.ip().is_loopback() {
        bail!("vault-tunnel may only bind to a loopback address");
    }
    let listener = TcpListener::bind(requested).await?;
    let address = listener.local_addr()?;
    let local_token = SecretString::from(Alphanumeric.sample_string(&mut rand::rng(), 48));
    let state = ProxyState {
        client,
        upstream: endpoint.trim_end_matches('/').to_owned(),
        upstream_key: api_key,
        local_token: local_token.clone(),
    };
    let app = Router::new().fallback(forward).with_state(state);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
    });
    Ok(VaultTunnel {
        address,
        token: local_token,
        shutdown: Some(shutdown_tx),
        task,
    })
}

async fn forward(
    State(state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match forward_inner(state, method, uri, headers, body).await {
        Ok(response) => response,
        Err(error) => Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"error":{"message": error.to_string()}}).to_string(),
            ))
            .expect("static proxy error response"),
    }
}

async fn forward_inner(
    state: ProxyState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let authorization = headers
        .get(reqwest::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let expected = format!("Bearer {}", state.local_token.expose_secret());
    if authorization != expected {
        return Ok(Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"error":{"message":"invalid vault token"}}"#))?);
    }
    if !uri.path().starts_with("/v1/") && uri.path() != "/v1" {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("only ModelVerse /v1 routes are allowed"))?);
    }
    let target = format!(
        "{}{}{}",
        state.upstream,
        uri.path(),
        uri.query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
    );
    let mut request = state
        .client
        .request(method, target)
        .bearer_auth(state.upstream_key.expose_secret())
        .body(body);
    for (name, value) in &headers {
        if !matches!(
            name.as_str().to_ascii_lowercase().as_str(),
            "authorization" | "host" | "content-length" | "connection" | "transfer-encoding"
        ) {
            request = request.header(name, value);
        }
    }
    let upstream = request.send().await?;
    let status = upstream.status();
    let mut builder = Response::builder().status(status);
    for (name, value) in upstream.headers() {
        if !matches!(
            name.as_str().to_ascii_lowercase().as_str(),
            "content-length" | "connection" | "transfer-encoding"
        ) {
            builder = builder.header(name, value);
        }
    }
    let stream = upstream
        .bytes_stream()
        .map_err(|error| std::io::Error::other(error.to_string()));
    Ok(builder.body(Body::from_stream(stream))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refuses_non_loopback_bind() {
        let error = start(
            Client::new(),
            "0.0.0.0:0",
            "https://api.modelverse.cn",
            SecretString::from("key".to_owned()),
        )
        .await
        .err()
        .unwrap();
        assert!(error.to_string().contains("loopback"));
    }
}
