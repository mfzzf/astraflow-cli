use crate::config::ModelSelection;
use crate::harness::Harness;
use crate::ucloud::SquareModel;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

pub const PREFERRED_CHAT_MODEL: &str = "deepseek-v4-flash-0731";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableModel {
    pub id: String,
    pub created: u64,
    pub pricing: Vec<ModelRate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRate {
    pub condition: String,
    pub charge_item: String,
    pub price: String,
    pub currency: String,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceColumns {
    pub input: String,
    pub cache_read: String,
    pub cache_create: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceTier {
    pub condition: String,
    pub input: String,
    pub cache_read: String,
    pub cache_create_5m: String,
    pub cache_create_1h: String,
    pub cache_storage: String,
    pub output: String,
}

type DisplayRate = (String, String);
type TierRates = BTreeMap<&'static str, DisplayRate>;

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
) -> Result<Vec<AvailableModel>> {
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
    let mut models: Vec<AvailableModel> = payload
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Value::as_str)?.to_owned();
            looks_like_text_model(&id).then_some(AvailableModel {
                id,
                created: entry.get("created").and_then(Value::as_u64).unwrap_or(0),
                pricing: parse_pricing(entry),
            })
        })
        .collect();
    models.sort_by(|left, right| {
        right
            .created
            .cmp(&left.created)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut seen = HashSet::new();
    models.retain(|model| seen.insert(model.id.to_ascii_lowercase()));
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
    select_models(&models, &[])
        .chat_completions
        .ok_or_else(|| anyhow!("this ModelVerse key has no text model available"))
}

pub fn model_ids(available: &[AvailableModel]) -> Vec<String> {
    available.iter().map(|model| model.id.clone()).collect()
}

pub fn compatible_models(available: &[AvailableModel], _harness: Harness) -> Vec<AvailableModel> {
    // Until ModelVerse exposes maintained per-model protocol capability data, treat every
    // conversational text model as compatible with every harness. `list_models` already removes
    // image, video, audio, embedding, rerank, OCR, batch, and moderation-only models.
    available.to_vec()
}

pub fn price_summary(model: &AvailableModel) -> String {
    if model.pricing.is_empty() {
        return "price unavailable".to_owned();
    }
    model
        .pricing
        .iter()
        .map(|rate| {
            let item = normalized_text_charge_item(&rate.charge_item).unwrap_or("tokens");
            let currency = if rate.currency.eq_ignore_ascii_case("CNY") {
                "¥"
            } else {
                rate.currency.as_str()
            };
            format!("{item} {currency}{}/{}", rate.price, rate.unit)
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn compact_price_summary(model: &AvailableModel) -> String {
    if model.pricing.is_empty() {
        return "Price unavailable".to_owned();
    }
    let groups = grouped_prices(model);
    [
        ("input", "Input"),
        ("cache_read", "Cache read"),
        ("cache_create", "Cache create"),
        ("output", "Output"),
    ]
    .into_iter()
    .filter_map(|(key, label)| {
        groups
            .get(key)
            .map(|values| format!("{label} {}", starting_price(values, true)))
    })
    .collect::<Vec<_>>()
    .join(" · ")
}

pub fn price_columns(model: &AvailableModel) -> PriceColumns {
    let groups = grouped_prices(model);
    let column = |key| {
        groups
            .get(key)
            .map(|values| starting_price(values, false))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "—".to_owned())
    };
    PriceColumns {
        input: column("input"),
        cache_read: column("cache_read"),
        cache_create: column("cache_create"),
        output: column("output"),
    }
}

pub fn price_tiers(model: &AvailableModel) -> Vec<PriceTier> {
    let mut tiers: Vec<(String, TierRates)> = Vec::new();
    for rate in &model.pricing {
        let Some(item) = normalized_text_charge_item(&rate.charge_item) else {
            continue;
        };
        let condition = if rate.condition.trim().is_empty() {
            "Default".to_owned()
        } else {
            rate.condition.clone()
        };
        let position = tiers
            .iter()
            .position(|(candidate, _)| candidate == &condition)
            .unwrap_or_else(|| {
                tiers.push((condition.clone(), BTreeMap::new()));
                tiers.len() - 1
            });
        tiers[position]
            .1
            .entry(item)
            .or_insert_with(|| display_price(rate));
    }
    tiers
        .into_iter()
        .map(|(condition, rates)| {
            let value = |key| {
                rates
                    .get(key)
                    .map(|value| format_price_group(std::slice::from_ref(value), "", false))
                    .unwrap_or_else(|| "—".to_owned())
            };
            PriceTier {
                condition,
                input: value("input"),
                cache_read: value("cache_read"),
                cache_create_5m: rates
                    .get("cache_create_5m")
                    .or_else(|| rates.get("cache_create"))
                    .map(|value| format_price_group(std::slice::from_ref(value), "", false))
                    .unwrap_or_else(|| "—".to_owned()),
                cache_create_1h: value("cache_create_1h"),
                cache_storage: value("cache_storage"),
                output: value("output"),
            }
        })
        .collect()
}

fn grouped_prices(model: &AvailableModel) -> BTreeMap<&'static str, Vec<(String, String)>> {
    let mut groups: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for rate in &model.pricing {
        let Some(item) = normalized_text_charge_item(&rate.charge_item) else {
            continue;
        };
        let group = if item.starts_with("cache_create") {
            "cache_create"
        } else {
            item
        };
        let value = display_price(rate);
        let values = groups.entry(group).or_default();
        if !values.contains(&value) {
            values.push(value);
        }
    }
    groups
}

fn display_price(rate: &ModelRate) -> (String, String) {
    let currency = if rate.currency.eq_ignore_ascii_case("CNY") {
        "¥"
    } else {
        rate.currency.as_str()
    };
    let unit = if rate.charge_item.to_ascii_lowercase().contains("storage")
        && (rate.unit.trim().is_empty() || rate.unit == "unit")
    {
        "token-hour".to_owned()
    } else if rate
        .unit
        .to_ascii_lowercase()
        .replace(' ', "")
        .contains("milliontokens")
    {
        "1M".to_owned()
    } else {
        rate.unit.clone()
    };
    (format!("{currency}{}", rate.price), unit)
}

fn starting_price(values: &[(String, String)], include_unit: bool) -> String {
    let minimum = values.iter().min_by(|left, right| {
        numeric_price(&left.0)
            .partial_cmp(&numeric_price(&right.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    minimum
        .map(|value| {
            let price = format_price_group(std::slice::from_ref(value), "", include_unit);
            format!("{price} 起")
        })
        .unwrap_or_else(|| "—".to_owned())
}

fn numeric_price(value: &str) -> f64 {
    value
        .trim_start_matches(|character: char| !character.is_ascii_digit() && character != '.')
        .parse()
        .unwrap_or(f64::INFINITY)
}

fn format_price_group(values: &[(String, String)], separator: &str, include_unit: bool) -> String {
    let shared_unit = values
        .first()
        .map(|(_, unit)| unit)
        .filter(|unit| values.iter().all(|(_, candidate)| candidate == *unit));
    let rates = values
        .iter()
        .map(|(value, unit)| {
            if shared_unit.is_some() {
                value.clone()
            } else {
                format!("{value}/{unit}")
            }
        })
        .collect::<Vec<_>>()
        .join(separator);
    match shared_unit {
        Some(unit) if include_unit || unit != "1M" => format!("{rates}/{unit}"),
        _ => rates,
    }
}

fn parse_pricing(entry: &Value) -> Vec<ModelRate> {
    entry
        .get("pricing")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|condition| {
            let context = condition
                .get("DescriptionEn")
                .or_else(|| condition.get("Condition"))
                .or_else(|| condition.get("Description"))
                .and_then(Value::as_str)
                .unwrap_or("Default")
                .to_owned();
            condition
                .get("Rates")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(move |rate| (context.clone(), rate))
        })
        .filter_map(|(condition, rate)| {
            let charge_item = rate.get("ChargeItem")?.as_str()?.to_owned();
            normalized_text_charge_item(&charge_item)?;
            let price = rate.get("Price").map(format_price_value)?;
            let currency = rate
                .get("Currency")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let unit = rate
                .get("UnitEn")
                .and_then(Value::as_str)
                .filter(|unit| !unit.trim().is_empty())
                .or_else(|| {
                    rate.get("Unit")
                        .and_then(Value::as_str)
                        .filter(|unit| !unit.trim().is_empty())
                })
                .unwrap_or("unit")
                .to_owned();
            Some(ModelRate {
                condition,
                charge_item,
                price,
                currency,
                unit,
            })
        })
        .collect()
}

fn format_price_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.as_f64().map_or_else(
            || value.to_string(),
            |value| {
                let fixed = format!("{value:.12}");
                fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
            },
        ),
        value => value.to_string(),
    }
}

fn normalized_text_charge_item(item: &str) -> Option<&'static str> {
    let item = item.to_ascii_lowercase();
    if item.contains("image") || item.contains("video") || item.contains("audio") {
        return None;
    }
    if item.contains("cache") && item.contains("storage") {
        Some("cache_storage")
    } else if item.contains("cache") && (item.contains("write") || item.contains("create")) {
        if item.contains("1h") || item.contains("1_hour") {
            Some("cache_create_1h")
        } else if item.contains("5m") || item.contains("5_min") {
            Some("cache_create_5m")
        } else {
            Some("cache_create")
        }
    } else if item.contains("cache") {
        Some("cache_read")
    } else if item == "input" || item.contains("input_text") || item.contains("input_token") {
        Some("input")
    } else if item == "output" || item.contains("output_text") || item.contains("output_token") {
        Some("output")
    } else {
        None
    }
}

pub fn select_models(available: &[AvailableModel], catalog: &[SquareModel]) -> ModelSelection {
    let text_candidates: Vec<&AvailableModel> = available
        .iter()
        .filter(|model| model_labels(&model.id, catalog).all(looks_like_coding_text_model))
        .collect();
    let selected = select_preferred(&text_candidates, PREFERRED_CHAT_MODEL);

    ModelSelection {
        chat_completions: selected.clone(),
        responses: selected.clone(),
        anthropic: selected,
    }
}

fn select_preferred(models: &[&AvailableModel], preferred: &str) -> Option<String> {
    models
        .iter()
        .find(|model| model.id.eq_ignore_ascii_case(preferred))
        .map(|model| model.id.clone())
        .or_else(|| select_latest(models))
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

fn select_latest(models: &[&AvailableModel]) -> Option<String> {
    models
        .iter()
        .copied()
        .max_by(|left, right| {
            left.created
                .cmp(&right.created)
                .then_with(|| left.id.cmp(&right.id))
        })
        .map(|model| model.id.clone())
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

fn model_leaf(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory(entries: &[(&str, u64)]) -> Vec<AvailableModel> {
        entries
            .iter()
            .map(|(id, created)| AvailableModel {
                id: (*id).to_owned(),
                created: *created,
                pricing: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn pricing_is_rendered_from_the_model_list_payload() {
        let value = json!({
            "pricing": [{
                "Rates": [
                    {"ChargeItem":"input","Price":3,"Currency":"CNY","UnitEn":"Million Tokens"},
                    {"ChargeItem":"output_text_tokens","Price":6,"Currency":"CNY","UnitEn":"Million Tokens"}
                ]
            }]
        });
        let model = AvailableModel {
            id: "deepseek-v4-pro-0813".to_owned(),
            created: 1,
            pricing: parse_pricing(&value),
        };
        assert_eq!(
            price_summary(&model),
            "input ¥3/Million Tokens · output ¥6/Million Tokens"
        );
    }

    #[test]
    fn picker_pricing_hides_image_video_and_audio_charges() {
        let value = json!({
            "pricing": [{
                "Rates": [
                    {"ChargeItem":"input","Price":1,"Currency":"CNY","UnitEn":"Million Tokens"},
                    {"ChargeItem":"input_image_count","Price":0.2,"Currency":"CNY","UnitEn":"Image"},
                    {"ChargeItem":"input_video_duration","Price":0.8,"Currency":"CNY","UnitEn":"Second"}
                ]
            }]
        });
        let model = AvailableModel {
            id: "multimodal-chat".to_owned(),
            created: 1,
            pricing: parse_pricing(&value),
        };
        assert_eq!(price_summary(&model), "input ¥1/Million Tokens");
    }

    #[test]
    fn picker_pricing_compacts_repeated_cache_tiers() {
        let value = json!({
            "pricing": [
                {
                    "DescriptionEn": "Input length (0, 200K]",
                    "Rates": [
                        {"ChargeItem":"input","Price":36,"Currency":"CNY","UnitEn":"Million Tokens"},
                        {"ChargeItem":"cache_read_tokens","Price":3.6,"Currency":"CNY","UnitEn":"Million Tokens"},
                        {"ChargeItem":"cache_write_5m_tokens","Price":45,"Currency":"CNY","UnitEn":"Million Tokens"},
                        {"ChargeItem":"cache_write_1h_tokens","Price":72,"Currency":"CNY","UnitEn":"Million Tokens"},
                        {"ChargeItem":"cache_storage_1h_tokens","Price":0.0000072,"Currency":"CNY","UnitEn":""},
                        {"ChargeItem":"output","Price":180,"Currency":"CNY","UnitEn":"Million Tokens"}
                    ]
                },
                {
                    "DescriptionEn": "Input length (200K, 1M]",
                    "Rates": [
                        {"ChargeItem":"input","Price":72,"Currency":"CNY","UnitEn":"Million Tokens"},
                        {"ChargeItem":"cache_read_tokens","Price":7.2,"Currency":"CNY","UnitEn":"Million Tokens"},
                        {"ChargeItem":"cache_write_5m_tokens","Price":90,"Currency":"CNY","UnitEn":"Million Tokens"},
                        {"ChargeItem":"output","Price":324,"Currency":"CNY","UnitEn":"Million Tokens"}
                    ]
                }
            ]
        });
        let model = AvailableModel {
            id: "claude-opus-5".to_owned(),
            created: 1,
            pricing: parse_pricing(&value),
        };
        assert_eq!(
            compact_price_summary(&model),
            "Input ¥36/1M 起 · Cache read ¥3.6/1M 起 · Cache create ¥45/1M 起 · Output ¥180/1M 起"
        );
        assert_eq!(
            price_columns(&model),
            PriceColumns {
                input: "¥36 起".to_owned(),
                cache_read: "¥3.6 起".to_owned(),
                cache_create: "¥45 起".to_owned(),
                output: "¥180 起".to_owned(),
            }
        );
        assert_eq!(
            price_tiers(&model),
            vec![
                PriceTier {
                    condition: "Input length (0, 200K]".to_owned(),
                    input: "¥36".to_owned(),
                    cache_read: "¥3.6".to_owned(),
                    cache_create_5m: "¥45".to_owned(),
                    cache_create_1h: "¥72".to_owned(),
                    cache_storage: "¥0.0000072/token-hour".to_owned(),
                    output: "¥180".to_owned(),
                },
                PriceTier {
                    condition: "Input length (200K, 1M]".to_owned(),
                    input: "¥72".to_owned(),
                    cache_read: "¥7.2".to_owned(),
                    cache_create_5m: "¥90".to_owned(),
                    cache_create_1h: "—".to_owned(),
                    cache_storage: "—".to_owned(),
                    output: "¥324".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn interactive_inventory_is_shared_by_every_harness() {
        let models = inventory(&[
            ("claude-opus-5", 3),
            ("gpt-5.6-luna", 2),
            ("deepseek-v4-pro-0813", 1),
        ]);
        let expected = ["claude-opus-5", "gpt-5.6-luna", "deepseek-v4-pro-0813"];
        for harness in [Harness::Claude, Harness::Codex, Harness::Grok] {
            assert_eq!(model_ids(&compatible_models(&models, harness)), expected);
        }
    }

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
    fn protocol_defaults_share_the_same_text_inventory() {
        let models = inventory(&[
            ("chat-only", 10),
            ("gpt-4.1-mini", 20),
            ("opaque-claude-id", 30),
        ]);
        let catalog = vec![SquareModel {
            id: "opaque-claude-id".into(),
            name: "Claude Sonnet 4.5".into(),
            aliases: vec!["anthropic/claude-sonnet-4-5".into()],
        }];
        let selected = select_models(&models, &catalog);
        assert_eq!(
            selected.chat_completions.as_deref(),
            Some("opaque-claude-id")
        );
        assert_eq!(selected.responses.as_deref(), Some("opaque-claude-id"));
        assert_eq!(selected.anthropic.as_deref(), Some("opaque-claude-id"));
    }

    #[test]
    fn catalog_names_can_filter_opaque_media_ids() {
        let models = inventory(&[("umodel-image", 30), ("deepseek-ai/DeepSeek-V3.2", 20)]);
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
    fn every_protocol_can_default_to_any_text_model() {
        let models = inventory(&[
            ("claude-haiku-4-5-20251001", 10),
            ("claude-sonnet-5", 30),
            ("claude-opus-5", 40),
            ("deepseek-ai/DeepSeek-V3.2", 50),
        ]);
        let selected = select_models(&models, &[]);
        assert_eq!(
            selected.chat_completions.as_deref(),
            Some("deepseek-ai/DeepSeek-V3.2")
        );
        assert_eq!(
            selected.responses.as_deref(),
            Some("deepseek-ai/DeepSeek-V3.2")
        );
        assert_eq!(
            selected.anthropic.as_deref(),
            Some("deepseek-ai/DeepSeek-V3.2")
        );
    }

    #[test]
    fn responses_uses_created_time_instead_of_a_hard_coded_family() {
        let models = inventory(&[
            ("gpt-5.6-luna", 50),
            ("gpt-5.6-sol", 40),
            ("gpt-5.3-codex", 30),
            ("codex-mini-latest", 20),
        ]);
        let selected = select_models(&models, &[]);
        assert_eq!(selected.responses.as_deref(), Some("gpt-5.6-luna"));
    }

    #[test]
    fn every_protocol_prefers_the_astraflow_default() {
        let models = inventory(&[
            ("gemini-3.7-flash", 400),
            ("claude-opus-5", 300),
            ("gpt-5.6-luna", 200),
            ("deepseek-v4-flash-0731", 100),
        ]);
        let selected = select_models(&models, &[]);
        assert_eq!(
            selected.chat_completions.as_deref(),
            Some("deepseek-v4-flash-0731")
        );
        assert_eq!(
            selected.responses.as_deref(),
            Some("deepseek-v4-flash-0731")
        );
        assert_eq!(
            selected.anthropic.as_deref(),
            Some("deepseek-v4-flash-0731")
        );
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
                    {"id": "deepseek-ai/DeepSeek-V3.2", "created": 100},
                    {"id": "claude-opus-5", "created": 200},
                    {"id": "pixverse-v6", "created": 400},
                    {"id": "text-to-sound-v2", "created": 300}
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
                AvailableModel {
                    id: "claude-opus-5".to_owned(),
                    created: 200,
                    pricing: Vec::new()
                },
                AvailableModel {
                    id: "deepseek-ai/DeepSeek-V3.2".to_owned(),
                    created: 100,
                    pricing: Vec::new()
                }
            ]
        );
        server.await.unwrap();
    }
}
