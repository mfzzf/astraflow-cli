use serde::Deserialize;
use std::sync::OnceLock;

pub const DEFAULT_MAX_CONTEXT_TOKENS: u64 = 1_000_000;

#[derive(Debug, Deserialize)]
struct ModelCatalog {
    schema_version: u8,
    default_max_context_tokens: u64,
    models: Vec<ModelContextEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ModelContextEntry {
    pub ids: Vec<String>,
    pub max_context_tokens: u64,
    pub source: String,
}

fn catalog() -> &'static ModelCatalog {
    static CATALOG: OnceLock<ModelCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let catalog: ModelCatalog = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/catalog/model-context-windows.json"
        )))
        .expect("embedded model context catalog must be valid JSON");
        assert_eq!(
            catalog.schema_version, 1,
            "unsupported model catalog schema"
        );
        assert_eq!(
            catalog.default_max_context_tokens, DEFAULT_MAX_CONTEXT_TOKENS,
            "model catalog and code defaults must agree"
        );
        catalog
    })
}

pub fn context_entry(model: &str) -> Option<&'static ModelContextEntry> {
    let model = model.trim();
    catalog().models.iter().find(|entry| {
        entry
            .ids
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(model))
    })
}

pub fn max_context_tokens(model: &str) -> u64 {
    context_entry(model)
        .map(|entry| entry.max_context_tokens)
        .unwrap_or(DEFAULT_MAX_CONTEXT_TOKENS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn known_models_use_catalog_values() {
        assert_eq!(max_context_tokens("claude-haiku-4-5-20251001"), 200_000);
        assert_eq!(max_context_tokens("gpt-5.6-sol"), 1_050_000);
        assert_eq!(max_context_tokens("gemini-2.5-pro"), 1_048_576);
        assert_eq!(max_context_tokens("MiniMax-M2.5"), 204_800);
    }

    #[test]
    fn lookup_is_case_insensitive_and_trims_input() {
        assert_eq!(max_context_tokens("  minimax-m2.5  "), 204_800);
    }

    #[test]
    fn unknown_models_default_to_one_million_tokens() {
        assert_eq!(max_context_tokens("future-custom-model"), 1_000_000);
    }

    #[test]
    fn catalog_entries_are_unique_and_sourced() {
        let mut ids = HashSet::new();
        for entry in &catalog().models {
            assert!(entry.max_context_tokens > 0);
            assert!(entry.source.starts_with("https://"));
            assert!(!entry.ids.is_empty());
            for id in &entry.ids {
                assert!(
                    ids.insert(id.to_ascii_lowercase()),
                    "duplicate model id: {id}"
                );
            }
        }
    }
}
