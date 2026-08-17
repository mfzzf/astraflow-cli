use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;

const DEFAULT_ENDPOINT: &str = "https://api.ucloud.cn/";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDetail {
    pub request_id: String,
    pub api_key_id: Option<String>,
    pub model_name: Option<String>,
    pub is_success: Option<bool>,
    pub http_status_code: Option<i64>,
    pub start_time: Option<i64>,
    pub usage: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SquareModel {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthControlPlane {
    client: Client,
    endpoint: String,
    token_type: String,
    access_token: SecretString,
}

impl OAuthControlPlane {
    pub fn new(
        client: Client,
        endpoint: impl Into<String>,
        token_type: impl Into<String>,
        access_token: SecretString,
    ) -> Self {
        Self {
            client,
            endpoint: endpoint.into(),
            token_type: token_type.into(),
            access_token,
        }
    }

    pub async fn projects(&self) -> Result<Vec<Project>> {
        let payload = self.call(json!({ "Action": "GetProjectList" })).await?;
        let entries = values(payload.get("ProjectSet"));
        Ok(entries
            .into_iter()
            .filter_map(|entry| {
                let id = text(entry.get("ProjectId"))?;
                Some(Project {
                    name: text(entry.get("ProjectName")).unwrap_or_else(|| id.clone()),
                    is_default: truthy(entry.get("IsDefault")),
                    id,
                })
            })
            .collect())
    }

    pub async fn api_keys(&self, project_id: &str) -> Result<Vec<ApiKey>> {
        let payload = self
            .call(json!({
                "Action": "ListUMInferAPIKey",
                "ProjectId": project_id,
                "Offset": 0,
                "Limit": 100
            }))
            .await?;
        let entries = values(payload.get("Data"));
        Ok(entries
            .into_iter()
            .filter_map(normalize_key)
            .filter(|key| key.enabled)
            .collect())
    }

    pub async fn create_astraflow_api_key(&self, project_id: &str, name: &str) -> Result<ApiKey> {
        let payload = self
            .call(json!({
                "Action": "CreateUMInferAPIKey",
                "ProjectId": project_id,
                "Name": name,
                "ModelverseDisabled": 0,
                "SandBoxDisabled": 1,
                "GrantAllModels": true
            }))
            .await?;
        normalize_key(payload.get("Data").unwrap_or(&Value::Null))
            .ok_or_else(|| anyhow!("CreateUMInferAPIKey returned no usable API key"))
    }

    /// Read catalog names and aliases only for models exposed by the selected
    /// inference key. Protocol selection is deliberately performed locally.
    pub async fn square_models(
        &self,
        project_id: &str,
        available: &[String],
    ) -> Result<Vec<SquareModel>> {
        let payload = self
            .call(json!({
                "Action": "ListUFSquareModel",
                "ProjectId": project_id,
                "Offset": 0,
                "Limit": 1000
            }))
            .await?;
        let entries = values(
            payload
                .get("SquareModels")
                .or_else(|| payload.get("Data"))
                .or_else(|| payload.get("ModelSet")),
        );
        Ok(entries
            .into_iter()
            .filter_map(|entry| {
                let id = text(entry.get("Id").or_else(|| entry.get("ModelId")))?;
                let model = normalize_square_model(entry, &id);
                matches_available(&model, available).then_some(model)
            })
            .collect())
    }

    async fn call(&self, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(&self.endpoint)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("{} {}", self.token_type, self.access_token.expose_secret()),
            )
            .json(&body)
            .send()
            .await
            .context("unable to reach UCloud")?;
        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .context("UCloud returned an invalid JSON response")?;
        validate_response(status.as_u16(), &payload)?;
        Ok(payload)
    }
}

fn matches_available(model: &SquareModel, available: &[String]) -> bool {
    available.iter().any(|candidate| {
        model.id.eq_ignore_ascii_case(candidate)
            || model.name.eq_ignore_ascii_case(candidate)
            || model
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(candidate))
    })
}

fn normalize_square_model(value: &Value, fallback_id: &str) -> SquareModel {
    let id = text(value.get("Id").or_else(|| value.get("ModelId")))
        .unwrap_or_else(|| fallback_id.to_owned());
    let name = text(
        value
            .get("Name")
            .or_else(|| value.get("ModelName"))
            .or_else(|| value.get("Model")),
    )
    .unwrap_or_else(|| id.clone());
    let aliases = value
        .get("Aliases")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| text(Some(entry)))
        .collect();
    SquareModel { id, name, aliases }
}

pub fn default_project(projects: &[Project]) -> Option<&Project> {
    projects
        .iter()
        .find(|project| project.is_default)
        .or_else(|| {
            projects
                .iter()
                .find(|project| !project.id.trim().is_empty())
        })
}

/// The only signature-authenticated operation exposed by this CLI is a read of
/// one ModelVerse request-log detail. This keeps the supplied verification keys
/// out of every resource-mutation path.
pub async fn get_request_log_detail(
    client: &Client,
    endpoint: Option<&str>,
    public_key: &str,
    private_key: &str,
    project_id: &str,
    request_id: &str,
) -> Result<UsageDetail> {
    let mut params = BTreeMap::from([
        ("Action".to_owned(), "GetUMInferRequestLogDetail".to_owned()),
        ("ProjectId".to_owned(), project_id.to_owned()),
        ("PublicKey".to_owned(), public_key.to_owned()),
        ("Region".to_owned(), "cn-wlcb".to_owned()),
        ("RequestId".to_owned(), request_id.to_owned()),
        ("Zone".to_owned(), "cn-wlcb-01".to_owned()),
    ]);
    let signature = create_signature(&params, private_key);
    params.insert("Signature".to_owned(), signature);
    let response = client
        .post(endpoint.unwrap_or(DEFAULT_ENDPOINT))
        .json(&params)
        .send()
        .await
        .context("unable to query the UCloud request log")?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .context("UCloud returned an invalid request-log response")?;
    validate_response(status.as_u16(), &payload)?;
    let data = payload
        .get("Data")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("request-log response has no Data object"))?;
    Ok(UsageDetail {
        request_id: text(data.get("RequestId")).unwrap_or_else(|| request_id.to_owned()),
        api_key_id: text(data.get("ApiKeyId")),
        model_name: text(data.get("ModelName")),
        is_success: data.get("IsSuccess").and_then(Value::as_bool),
        http_status_code: data.get("HttpStatusCode").and_then(Value::as_i64),
        start_time: data.get("StartTime").and_then(Value::as_i64),
        usage: normalize_usage(data.get("Usage")),
    })
}

pub fn create_signature(params: &BTreeMap<String, String>, private_key: &str) -> String {
    let mut canonical = String::new();
    for (key, value) in params {
        canonical.push_str(key);
        canonical.push_str(value);
    }
    canonical.push_str(private_key);
    // UCloud OpenAPI V1 mandates SHA-1 over sorted parameter pairs plus PrivateKey.
    // This compatibility boundary cannot use another algorithm until the server protocol changes.
    format!("{:x}", Sha1::digest(canonical.as_bytes())) // nosemgrep: rust.lang.security.weak-crypto
}

fn validate_response(status: u16, payload: &Value) -> Result<()> {
    let ret_code = payload.get("RetCode").and_then(Value::as_i64);
    if !(200..300).contains(&status) || ret_code != Some(0) {
        let message =
            text(payload.get("Message")).unwrap_or_else(|| "UCloud request failed".into());
        bail!(
            "{message} (HTTP {status}, RetCode {})",
            ret_code.unwrap_or(-1)
        );
    }
    Ok(())
}

fn normalize_key(value: &Value) -> Option<ApiKey> {
    let id = text(value.get("KeyId"))?;
    let status = value.get("Status").and_then(Value::as_i64);
    let modelverse_disabled = value.get("ModelverseDisabled").and_then(Value::as_i64);
    Some(ApiKey {
        name: text(value.get("Name")).unwrap_or_else(|| id.clone()),
        key: text(value.get("Key")).unwrap_or_default(),
        enabled: (status.is_none() || status == Some(1)) && modelverse_disabled != Some(1),
        id,
    })
}

fn normalize_usage(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(raw)) => {
            serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()))
        }
        Some(value) => value.clone(),
        None => Value::Null,
    }
}

fn values(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(Value::Object(items)) => items.values().collect(),
        _ => Vec::new(),
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn truthy(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
        || matches!(value, Some(Value::Number(number)) if number.as_i64() == Some(1))
        || matches!(value, Some(Value::String(text)) if text.eq_ignore_ascii_case("true") || text == "1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn published_ucloud_signature_sample_matches() {
        let params = BTreeMap::from([
            ("Action".to_owned(), "DescribeUHostInstance".to_owned()),
            ("Limit".to_owned(), "10".to_owned()),
            (
                "PublicKey".to_owned(),
                "ucloudsomeone@example.com1296235120854146120".to_owned(),
            ),
            ("Region".to_owned(), "cn-bj2".to_owned()),
        ]);
        assert_eq!(
            create_signature(&params, "46f09bb9fab4f12dfc160dae12273d5332b5debe"),
            "cba5cf5ec4d4233d206b1b54951e3787350a642f"
        );
    }

    #[test]
    fn default_project_prefers_explicit_default() {
        let projects = vec![
            Project {
                id: "first".into(),
                name: "First".into(),
                is_default: false,
            },
            Project {
                id: "default".into(),
                name: "Default".into(),
                is_default: true,
            },
        ];
        assert_eq!(default_project(&projects).unwrap().id, "default");
    }

    #[test]
    fn usage_string_is_decoded() {
        assert_eq!(
            normalize_usage(Some(&Value::String("{\"total_tokens\":7}".into()))),
            json!({"total_tokens": 7})
        );
    }

    #[test]
    fn catalog_matching_accepts_public_name_and_alias() {
        let model = SquareModel {
            id: "umodel-1".into(),
            name: "gpt-4.1-mini".into(),
            aliases: vec!["openai/gpt-4.1-mini".into()],
        };
        assert!(matches_available(&model, &["gpt-4.1-mini".into()]));
        assert!(matches_available(&model, &["openai/gpt-4.1-mini".into()]));
        assert!(!matches_available(&model, &["claude-sonnet".into()]));
    }

    #[tokio::test]
    async fn oauth_control_plane_uses_only_expected_actions() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for expected in [
                "GetProjectList",
                "ListUMInferAPIKey",
                "CreateUMInferAPIKey",
                "ListUFSquareModel",
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = vec![0_u8; 16 * 1024];
                let size = stream.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..size]);
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: bearer oauth-token")
                );
                let body_start = request.find("\r\n\r\n").unwrap() + 4;
                let body: Value = serde_json::from_str(&request[body_start..]).unwrap();
                assert_eq!(body["Action"], expected);
                let response_body = match expected {
                    "GetProjectList" => json!({
                        "RetCode": 0,
                        "ProjectSet": [{"ProjectId":"org-default","ProjectName":"Default","IsDefault":true}]
                    }),
                    "ListUMInferAPIKey" => json!({
                        "RetCode": 0,
                        "Data": [{"KeyId":"key-1","Name":"Existing","Key":"secret-1","Status":1}]
                    }),
                    "CreateUMInferAPIKey" => json!({
                        "RetCode": 0,
                        "Data": {"KeyId":"key-2","Name":"AstraFlow Agent","Key":"secret-2","Status":1}
                    }),
                    _ => json!({
                        "RetCode": 0,
                        "Data": [{
                            "Id": "umodel-1",
                            "Name": "gpt-4.1-mini",
                            "Aliases": ["openai/gpt-4.1-mini"]
                        }]
                    }),
                }
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let control = OAuthControlPlane::new(
            Client::new(),
            format!("http://{address}"),
            "Bearer",
            SecretString::from("oauth-token".to_owned()),
        );
        let projects = control.projects().await.unwrap();
        assert_eq!(default_project(&projects).unwrap().id, "org-default");
        let keys = control.api_keys("org-default").await.unwrap();
        assert_eq!(keys[0].id, "key-1");
        let created = control
            .create_astraflow_api_key("org-default", "AstraFlow Agent")
            .await
            .unwrap();
        assert_eq!(created.id, "key-2");
        let catalog = control
            .square_models("org-default", &["gpt-4.1-mini".into()])
            .await
            .unwrap();
        assert_eq!(catalog[0].name, "gpt-4.1-mini");
        server.await.unwrap();
    }
}
