use crate::config::ModelSelection;
use crate::ucloud::SquareModel;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatProbe {
    pub model: String,
    pub request_id: Option<String>,
    pub content: String,
    pub usage: Value,
}

pub async fn list_models(
    client: &Client,
    endpoint: &str,
    api_key: &SecretString,
) -> Result<Vec<String>> {
    let response = client
        .get(format!("{}/v1/models", endpoint.trim_end_matches('/')))
        .bearer_auth(api_key.expose_secret())
        .send()
        .await
        .context("unable to list ModelVerse models")?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .context("invalid ModelVerse model list")?;
    if !status.is_success() {
        bail!("ModelVerse model list failed with HTTP {status}");
    }
    let mut models: Vec<String> = payload
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str).map(str::to_owned))
        .filter(|id| looks_like_text_model(id))
        .collect();
    models.sort();
    models.dedup();
    if models.is_empty() {
        bail!("this ModelVerse key has no text model available");
    }
    Ok(models)
}

pub async fn choose_text_model(
    client: &Client,
    endpoint: &str,
    api_key: &SecretString,
) -> Result<String> {
    let models = list_models(client, endpoint, api_key).await?;
    select_preferred(&models)
        .ok_or_else(|| anyhow!("this ModelVerse key has no text model available"))
}

pub fn select_models(available: &[String], catalog: &[SquareModel]) -> ModelSelection {
    let matching = |model: &str| {
        catalog.iter().find(|entry| {
            entry.name.eq_ignore_ascii_case(model)
                || entry.id.eq_ignore_ascii_case(model)
                || entry
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(model))
        })
    };

    let chat_candidates: Vec<String> = available
        .iter()
        .filter(|model| matching(model).is_none_or(|entry| entry.api_protocols.chat_completions))
        .cloned()
        .collect();
    let response_candidates: Vec<String> = available
        .iter()
        .filter(|model| {
            matching(model)
                .map(|entry| entry.api_protocols.responses)
                .unwrap_or_else(|| looks_like_responses_model(model))
        })
        .cloned()
        .collect();
    let anthropic_candidates: Vec<String> = available
        .iter()
        .filter(|model| {
            matching(model)
                .map(|entry| entry.api_protocols.anthropic)
                .unwrap_or_else(|| model.to_ascii_lowercase().contains("claude"))
        })
        .cloned()
        .collect();

    ModelSelection {
        chat_completions: select_preferred(&chat_candidates),
        responses: select_preferred(&response_candidates),
        anthropic: select_preferred(&anthropic_candidates),
    }
}

fn select_preferred(models: &[String]) -> Option<String> {
    let preferred = ["gpt-5-mini", "gpt-4.1-mini", "deepseek-ai/DeepSeek-V3.2"];
    for candidate in preferred {
        if let Some(found) = models
            .iter()
            .find(|model| model.eq_ignore_ascii_case(candidate))
        {
            return Some(found.clone());
        }
    }
    models.iter().min().cloned()
}

pub async fn minimal_chat(
    client: &Client,
    endpoint: &str,
    api_key: &SecretString,
    model: &str,
) -> Result<ChatProbe> {
    let response = client
        .post(format!(
            "{}/v1/chat/completions",
            endpoint.trim_end_matches('/')
        ))
        .bearer_auth(api_key.expose_secret())
        .json(&json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with exactly: ASTRAFLOW_OK"}],
            "max_tokens": 12,
            "temperature": 0
        }))
        .send()
        .await
        .context("unable to send the ModelVerse probe")?;
    let status = response.status();
    let header_request_id = ["x-request-id", "x-um-request-id", "request-id"]
        .iter()
        .find_map(|name| response.headers().get(*name))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let payload: Value = response
        .json()
        .await
        .context("ModelVerse returned an invalid chat response")?;
    if !status.is_success() {
        let message = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("ModelVerse chat request failed");
        bail!("{message} (HTTP {status})");
    }
    let content = payload
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(ChatProbe {
        model: model.to_owned(),
        request_id: header_request_id
            .or_else(|| payload.get("id").and_then(Value::as_str).map(str::to_owned)),
        content,
        usage: payload.get("usage").cloned().unwrap_or(Value::Null),
    })
}

fn looks_like_text_model(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    ![
        "image",
        "flux",
        "video",
        "sora",
        "veo-",
        "kling",
        "wan2",
        "tts",
        "suno",
        "embedding",
        "rerank",
        "ocr",
        "speech",
        "batch",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn looks_like_responses_model(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    lower.starts_with("gpt-")
        || lower.contains("codex")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_models_are_not_used_for_chat_probe() {
        assert!(!looks_like_text_model("gpt-image-2"));
        assert!(!looks_like_text_model("Wan-AI/Wan2.7-Video"));
        assert!(!looks_like_text_model("gpt-5.4-mini-batch"));
        assert!(looks_like_text_model("deepseek-ai/DeepSeek-V3.2"));
    }

    #[test]
    fn catalog_protocols_split_harness_models() {
        let models = vec![
            "chat-only".into(),
            "gpt-4.1-mini".into(),
            "claude-sonnet".into(),
        ];
        let catalog = vec![
            SquareModel::test("chat-only", true, false, false),
            SquareModel::test("gpt-4.1-mini", true, true, false),
            SquareModel::test("claude-sonnet", false, false, true),
        ];
        let selected = select_models(&models, &catalog);
        assert_eq!(selected.responses.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(selected.anthropic.as_deref(), Some("claude-sonnet"));
    }
}
