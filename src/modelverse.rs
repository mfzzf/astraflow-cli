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
    let chat_candidates: Vec<String> = available
        .iter()
        .filter(|model| model_labels(model, catalog).all(looks_like_coding_text_model))
        .filter(|model| !model_labels(model, catalog).any(looks_like_anthropic_model))
        .cloned()
        .collect();
    let response_candidates: Vec<String> = available
        .iter()
        .filter(|model| model_labels(model, catalog).all(looks_like_coding_text_model))
        .filter(|model| model_labels(model, catalog).any(looks_like_responses_model))
        .cloned()
        .collect();
    let anthropic_candidates: Vec<String> = available
        .iter()
        .filter(|model| model_labels(model, catalog).all(looks_like_coding_text_model))
        .filter(|model| model_labels(model, catalog).any(looks_like_anthropic_model))
        .cloned()
        .collect();

    ModelSelection {
        chat_completions: select_preferred(&chat_candidates),
        responses: select_responses(&response_candidates),
        anthropic: select_anthropic(&anthropic_candidates),
    }
}

fn model_labels<'a>(model: &'a str, catalog: &'a [SquareModel]) -> impl Iterator<Item = &'a str> {
    let matching = catalog.iter().find(|entry| {
        entry.name.eq_ignore_ascii_case(model)
            || entry.id.eq_ignore_ascii_case(model)
            || entry
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(model))
    });
    std::iter::once(model).chain(matching.into_iter().flat_map(|entry| {
        std::iter::once(entry.id.as_str())
            .chain(std::iter::once(entry.name.as_str()))
            .chain(entry.aliases.iter().map(String::as_str))
    }))
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

fn select_responses(models: &[String]) -> Option<String> {
    let preferred = [
        "gpt-5.3-codex",
        "gpt-5.2-codex",
        "gpt-5.1-codex-max",
        "gpt-5.1-codex",
        "gpt-5-codex",
        "codex-mini-latest",
    ];
    for candidate in preferred {
        if let Some(found) = models
            .iter()
            .find(|model| model_leaf(model).eq_ignore_ascii_case(candidate))
        {
            return Some(found.clone());
        }
    }
    models
        .iter()
        .max_by(|left, right| response_rank(left).cmp(&response_rank(right)))
        .cloned()
}

fn response_rank(model: &str) -> (u8, String) {
    let lower = model.to_ascii_lowercase();
    let family = if lower.contains("codex") {
        3
    } else if model_leaf(&lower).starts_with("gpt-") {
        2
    } else {
        1
    };
    (family, lower)
}

fn select_anthropic(models: &[String]) -> Option<String> {
    models
        .iter()
        .max_by(|left, right| anthropic_rank(left).cmp(&anthropic_rank(right)))
        .cloned()
}

fn anthropic_rank(model: &str) -> (u8, String) {
    let lower = normalize_model_name(model);
    let family = if lower.contains("sonnet") {
        3
    } else if lower.contains("opus") {
        2
    } else if lower.contains("haiku") {
        1
    } else {
        0
    };
    (family, lower)
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
    looks_like_coding_text_model(id)
}

fn looks_like_coding_text_model(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    let leaf = model_leaf(&lower);
    let non_text_markers = [
        // Image generation and editing.
        "image",
        "seedream",
        "flux",
        "midjourney",
        "stable-diffusion",
        "sdxl",
        // Video generation and editing.
        "video",
        "sora",
        "kling",
        "wan2",
        "hailuo",
        "happyhorse",
        "pixverse",
        "vidu",
        "seedance",
        "i2v",
        "t2v",
        "r2v",
        "lip-sync",
        // Audio generation, transcription, and speech.
        "audio",
        "tts",
        "speech",
        "suno",
        "music",
        "sound",
        "whisper",
        "transcrib",
        // Retrieval, document processing, and offline jobs.
        "embedding",
        "rerank",
        "ocr",
        "easydoc",
        "batch",
        "moderation",
    ];
    !non_text_markers.iter().any(|marker| lower.contains(marker))
        && !leaf.starts_with("veo-")
        && !leaf.starts_with("bge-")
        && !leaf.starts_with("gte-")
        && !leaf.starts_with("e5-")
}

fn looks_like_responses_model(id: &str) -> bool {
    let lower = normalize_model_name(id);
    let leaf = model_leaf(&lower);
    leaf.starts_with("gpt-")
        || leaf.contains("codex")
        || matches!(leaf.split('-').next(), Some("o1" | "o3" | "o4"))
}

fn looks_like_anthropic_model(id: &str) -> bool {
    let normalized = normalize_model_name(id);
    let leaf = model_leaf(&normalized);
    leaf == "claude" || leaf.starts_with("claude-")
}

fn normalize_model_name(id: &str) -> String {
    let mut normalized = String::with_capacity(id.len());
    let mut last_was_separator = false;
    for character in id.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() || character == '/' {
            normalized.push(character);
            last_was_separator = false;
        } else if !last_was_separator {
            normalized.push('-');
            last_was_separator = true;
        }
    }
    normalized.trim_matches('-').to_owned()
}

fn model_leaf(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_models_are_not_used_for_chat_probe() {
        for model in [
            "gpt-image-2",
            "Qwen/Qwen-Image-Edit",
            "doubao-seedream-5-0-pro",
            "Wan-AI/Wan2.7-Video",
            "happyhorse-1.1-i2v",
            "kling-v3",
            "MiniMax-Hailuo-2.3",
            "viduq3-pro",
            "pixverse-v6",
            "qwen3-tts-flash",
            "speech-2.8-turbo",
            "text-to-sound-v2",
            "BAAI/bge-m3",
            "qwen3-reranker-8b",
            "deepseek-ai/DeepSeek-OCR-2",
            "gpt-5.4-mini-batch",
        ] {
            assert!(!looks_like_text_model(model), "{model} must be filtered");
        }
        assert!(looks_like_text_model("deepseek-ai/DeepSeek-V3.2"));
        assert!(looks_like_text_model("Qwen/Qwen3-VL-235B-A22B-Instruct"));
        assert!(looks_like_text_model("zai-org/glm-4.6v"));
    }

    #[test]
    fn local_rules_split_harness_models_without_protocol_metadata() {
        let models = vec![
            "chat-only".into(),
            "gpt-4.1-mini".into(),
            "opaque-claude-id".into(),
        ];
        let catalog = vec![SquareModel {
            id: "opaque-claude-id".into(),
            name: "Claude Sonnet 4.5".into(),
            aliases: vec!["anthropic/claude-sonnet-4-5".into()],
        }];
        let selected = select_models(&models, &catalog);
        assert_eq!(selected.chat_completions.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(selected.responses.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(selected.anthropic.as_deref(), Some("opaque-claude-id"));
    }

    #[test]
    fn catalog_names_can_filter_opaque_media_ids() {
        let models = vec!["umodel-image".into(), "deepseek-ai/DeepSeek-V3.2".into()];
        let catalog = vec![SquareModel {
            id: "umodel-image".into(),
            name: "Qwen Image Edit".into(),
            aliases: Vec::new(),
        }];
        let selected = select_models(&models, &catalog);
        assert_eq!(
            selected.chat_completions.as_deref(),
            Some("deepseek-ai/DeepSeek-V3.2")
        );
    }

    #[test]
    fn claude_models_are_anthropic_only() {
        let models = vec![
            "claude-haiku-4-5-20251001".into(),
            "claude-opus-4-7".into(),
            "claude-sonnet-4-6".into(),
            "deepseek-ai/DeepSeek-V3.2".into(),
        ];
        let selected = select_models(&models, &[]);
        assert_eq!(
            selected.chat_completions.as_deref(),
            Some("deepseek-ai/DeepSeek-V3.2")
        );
        assert_eq!(selected.responses, None);
        assert_eq!(selected.anthropic.as_deref(), Some("claude-sonnet-4-6"));
    }

    #[test]
    fn responses_prefers_a_coding_model_from_authenticated_catalog() {
        let models = vec![
            "gpt-5.6-sol".into(),
            "openai/gpt-5.1-codex-mini".into(),
            "gpt-5.3-codex".into(),
            "codex-mini-latest".into(),
        ];
        let selected = select_models(&models, &[]);
        assert_eq!(selected.responses.as_deref(), Some("gpt-5.3-codex"));
    }

    #[tokio::test]
    async fn model_listing_requires_bearer_auth_and_filters_media() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0_u8; 16 * 1024];
            let size = stream.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..size]).to_ascii_lowercase();
            assert!(request.contains("authorization: bearer test-model-key"));
            let body = json!({
                "data": [
                    {"id": "deepseek-ai/DeepSeek-V3.2"},
                    {"id": "claude-sonnet-5"},
                    {"id": "pixverse-v6"},
                    {"id": "text-to-sound-v2"}
                ]
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let models = list_models(
            &Client::new(),
            &format!("http://{address}"),
            &SecretString::from("test-model-key".to_owned()),
        )
        .await
        .unwrap();
        assert_eq!(
            models,
            vec![
                "claude-sonnet-5".to_owned(),
                "deepseek-ai/DeepSeek-V3.2".to_owned()
            ]
        );
        server.await.unwrap();
    }
}
