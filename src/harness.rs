use crate::config::Credential;
use anyhow::{Context, Result, anyhow, bail};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use tokio::process::Command;

const SCRUBBED_ENV: &[&str] = &[
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ASTRAFLOW_MODELVERSE_API_KEY",
    "CODEX_API_KEY",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_BASE_URL",
    "OPENCODE_CONFIG_CONTENT",
];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Harness {
    Claude,
    Codex,
    Grok,
    Opencode,
    Hermes,
    Pi,
    Dsh,
    PrimeAgent,
}

impl Harness {
    pub const ALL: [Self; 8] = [
        Self::Claude,
        Self::Codex,
        Self::Grok,
        Self::Opencode,
        Self::Hermes,
        Self::Pi,
        Self::Dsh,
        Self::PrimeAgent,
    ];

    pub fn parse(name: &str) -> Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "grok" => Ok(Self::Grok),
            "opencode" => Ok(Self::Opencode),
            "hermes" => Ok(Self::Hermes),
            "pi" => Ok(Self::Pi),
            "dsh" | "deepseek" => Ok(Self::Dsh),
            "prime" | "prime-agent" => Ok(Self::PrimeAgent),
            _ => bail!("unsupported harness: {name}"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Grok => "grok",
            Self::Opencode => "opencode",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::Dsh => "dsh",
            Self::PrimeAgent => "prime-agent",
        }
    }

    pub fn executable(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Grok => "grok",
            Self::Opencode => "opencode",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
            Self::Dsh => "dsh",
            Self::PrimeAgent => "prime-agent",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub values: BTreeMap<String, String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct HarnessInfo {
    pub name: Harness,
    pub executable: String,
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub injected_variables: Vec<String>,
    pub scrubbed_variables: Vec<String>,
}

pub fn environment(harness: Harness, key: &SecretString, endpoint: &str) -> Environment {
    let key = key.expose_secret().to_owned();
    let root = endpoint.trim_end_matches('/');
    let openai_base = format!("{root}/v1");
    let mut values = BTreeMap::from([("ASTRAFLOW_MODELVERSE_API_KEY".to_owned(), key.clone())]);
    match harness {
        Harness::Codex => {
            values.insert("CODEX_API_KEY".into(), key.clone());
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base.clone());
            values.insert("MODEL_PROVIDER".into(), "modelverse".into());
            values.insert("NO_BROWSER".into(), "1".into());
            values.insert(
                "CODEX_CONFIG".into(),
                json!({
                    "model_provider": "modelverse",
                    "model_providers": {
                        "modelverse": {
                            "name": "AstraFlow ModelVerse",
                            "base_url": openai_base,
                            "env_key": "ASTRAFLOW_MODELVERSE_API_KEY",
                            "wire_api": "responses"
                        }
                    }
                })
                .to_string(),
            );
            values.insert(
                "DEFAULT_AUTH_REQUEST".into(),
                json!({"methodId": "api-key"}).to_string(),
            );
        }
        Harness::Claude => {
            values.insert("ANTHROPIC_AUTH_TOKEN".into(), key);
            values.insert("ANTHROPIC_BASE_URL".into(), root.to_owned());
            values.insert("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB".into(), "1".into());
            values.insert("NO_BROWSER".into(), "1".into());
        }
        Harness::Opencode => {
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base.clone());
            values.insert(
                "OPENCODE_CONFIG_CONTENT".into(),
                json!({
                    "provider": {
                        "astraflow": {
                            "name": "AstraFlow ModelVerse",
                            "npm": "@ai-sdk/openai-compatible",
                            "options": {
                                "baseURL": openai_base,
                                "apiKey": "{env:ASTRAFLOW_MODELVERSE_API_KEY}"
                            }
                        }
                    }
                })
                .to_string(),
            );
        }
        Harness::Dsh => {
            values.insert("DEEPSEEK_API_KEY".into(), key.clone());
            values.insert("DEEPSEEK_BASE_URL".into(), openai_base.clone());
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base);
        }
        Harness::Grok | Harness::Hermes | Harness::Pi | Harness::PrimeAgent => {
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base);
        }
    }
    Environment {
        values,
        removed: SCRUBBED_ENV.iter().map(|name| (*name).to_owned()).collect(),
    }
}

pub fn inspect(harness: Harness, credential: Option<&Credential>) -> HarnessInfo {
    let path = which::which(harness.executable()).ok();
    let injected_variables = credential
        .map(|credential| {
            environment(harness, &credential.api_key, &credential.endpoint)
                .values
                .keys()
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    HarnessInfo {
        name: harness,
        executable: harness.executable().to_owned(),
        installed: path.is_some(),
        path,
        injected_variables,
        scrubbed_variables: SCRUBBED_ENV.iter().map(|name| (*name).to_owned()).collect(),
    }
}

pub async fn launch(
    harness: Harness,
    credential: &Credential,
    binary: Option<&Path>,
    args: &[String],
) -> Result<ExitStatus> {
    let executable = binary
        .map(PathBuf::from)
        .or_else(|| which::which(harness.executable()).ok())
        .ok_or_else(|| anyhow!("{} is not installed or not on PATH", harness.executable()))?;
    let overlay = environment(harness, &credential.api_key, &credential.endpoint);
    let args = command_arguments(harness, &credential.endpoint, args);
    run_with_environment(&executable, &args, &overlay).await
}

pub fn command_arguments(harness: Harness, endpoint: &str, args: &[String]) -> Vec<String> {
    if harness != Harness::Codex {
        return args.to_vec();
    }
    let base_url = format!("{}/v1", endpoint.trim_end_matches('/'));
    let mut configured = vec![
        "-c".to_owned(),
        "model_provider=\"modelverse\"".to_owned(),
        "-c".to_owned(),
        "model_providers.modelverse.name=\"AstraFlow ModelVerse\"".to_owned(),
        "-c".to_owned(),
        format!("model_providers.modelverse.base_url=\"{base_url}\""),
        "-c".to_owned(),
        "model_providers.modelverse.env_key=\"ASTRAFLOW_MODELVERSE_API_KEY\"".to_owned(),
        "-c".to_owned(),
        "model_providers.modelverse.wire_api=\"responses\"".to_owned(),
        "-c".to_owned(),
        "model_providers.modelverse.requires_openai_auth=false".to_owned(),
    ];
    configured.extend_from_slice(args);
    configured
}

pub async fn run_with_environment(
    executable: &Path,
    args: &[String],
    overlay: &Environment,
) -> Result<ExitStatus> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for key in &overlay.removed {
        command.env_remove(key);
    }
    command.envs(&overlay.values);
    command
        .status()
        .await
        .with_context(|| format!("launch {}", executable.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_environment_matches_modelverse_provider_contract() {
        let secret = SecretString::from("test-key".to_owned());
        let env = environment(Harness::Codex, &secret, "https://api.modelverse.cn/");
        assert_eq!(
            env.values["OPENAI_BASE_URL"],
            "https://api.modelverse.cn/v1"
        );
        assert_eq!(env.values["CODEX_API_KEY"], "test-key");
        assert!(env.values["CODEX_CONFIG"].contains("\"wire_api\":\"responses\""));
    }

    #[test]
    fn claude_environment_uses_messages_base_without_v1() {
        let secret = SecretString::from("test-key".to_owned());
        let env = environment(Harness::Claude, &secret, "https://api.modelverse.cn/");
        assert_eq!(
            env.values["ANTHROPIC_BASE_URL"],
            "https://api.modelverse.cn"
        );
        assert!(!env.values.contains_key("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn unknown_harness_is_rejected() {
        assert!(Harness::parse("unsafe-custom-command").is_err());
    }

    #[test]
    fn codex_uses_official_per_invocation_config_overrides() {
        let args = command_arguments(
            Harness::Codex,
            "https://api.modelverse.cn",
            &["exec".into(), "hello".into()],
        );
        assert_eq!(args[0], "-c");
        assert!(
            args.iter()
                .any(|arg| arg == "model_provider=\"modelverse\"")
        );
        assert!(
            args.iter()
                .any(|arg| arg == "model_providers.modelverse.wire_api=\"responses\"")
        );
        assert_eq!(&args[args.len() - 2..], ["exec", "hello"]);
    }
}
