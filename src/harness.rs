use crate::config::Credential;
use anyhow::{Context, Result, anyhow, bail};
use directories::BaseDirs;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use tempfile::{NamedTempFile, TempDir};
use tokio::process::Command;
use toml_edit::{DocumentMut, Item, Table, value};

const SCRUBBED_ENV: &[&str] = &[
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "OPENROUTER_API_KEY",
    "OPENROUTER_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "HERMES_INFERENCE_MODEL",
    "HERMES_INFERENCE_PROVIDER",
    "HERMES_HOME",
    "ASTRAFLOW_MODELVERSE_API_KEY",
    "CODEX_API_KEY",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_BASE_URL",
    "OPENCODE_CONFIG_CONTENT",
    "XAI_API_KEY",
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
        self.name()
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

pub fn selected_model(
    harness: Harness,
    credential: &Credential,
    override_model: Option<&str>,
) -> Result<String> {
    if let Some(model) = override_model.filter(|model| !model.trim().is_empty()) {
        return Ok(model.trim().to_owned());
    }
    let selected = match harness {
        Harness::Codex => credential.models.responses.as_ref(),
        Harness::Claude => credential.models.anthropic.as_ref(),
        _ => credential.models.chat_completions.as_ref(),
    };
    if let Some(model) = selected.cloned().or_else(|| {
        env::var("ASTRAFLOW_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
    }) {
        return Ok(model);
    }
    let discovery_was_saved = credential.models.chat_completions.is_some()
        || credential.models.responses.is_some()
        || credential.models.anthropic.is_some();
    if discovery_was_saved {
        let protocol = match harness {
            Harness::Codex => "Responses",
            Harness::Claude => "Anthropic Messages",
            _ => "Chat Completions",
        };
        bail!(
            "the selected ModelVerse key has no {protocol}-capable model for {}; use --model to override or log in with another key",
            harness.name()
        );
    }
    Ok(match harness {
        Harness::Claude => "claude-sonnet-4-6".to_owned(),
        _ => "gpt-4.1-mini".to_owned(),
    })
}

pub fn environment(
    harness: Harness,
    key: &SecretString,
    endpoint: &str,
    model: &str,
) -> Environment {
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
                    "model": model,
                    "model_provider": "modelverse",
                    "model_providers": {
                        "modelverse": {
                            "name": "AstraFlow ModelVerse",
                            "base_url": openai_base,
                            "env_key": "ASTRAFLOW_MODELVERSE_API_KEY",
                            "wire_api": "responses",
                            "requires_openai_auth": false
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
            for name in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "CLAUDE_CODE_SUBAGENT_MODEL",
            ] {
                values.insert(name.into(), model.to_owned());
            }
            values.insert("NO_BROWSER".into(), "1".into());
        }
        Harness::Opencode => {
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base.clone());
            values.insert(
                "OPENCODE_CONFIG_CONTENT".into(),
                json!({
                    "model": format!("astraflow/{model}"),
                    "provider": {
                        "astraflow": {
                            "name": "AstraFlow ModelVerse",
                            "npm": "@ai-sdk/openai-compatible",
                            "env": ["ASTRAFLOW_MODELVERSE_API_KEY"],
                            "options": {
                                "baseURL": openai_base,
                                "apiKey": "{env:ASTRAFLOW_MODELVERSE_API_KEY}"
                            },
                            "models": {
                                model: {"name": model}
                            }
                        }
                    }
                })
                .to_string(),
            );
        }
        Harness::Hermes => {
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("HERMES_INFERENCE_MODEL".into(), model.to_owned());
            values.insert("HERMES_INFERENCE_PROVIDER".into(), "astraflow".into());
        }
        Harness::Dsh => {
            values.insert("DEEPSEEK_API_KEY".into(), key.clone());
            values.insert("DEEPSEEK_BASE_URL".into(), openai_base.clone());
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base);
        }
        Harness::Grok => {
            values.insert("XAI_API_KEY".into(), key.clone());
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base);
        }
        Harness::Pi | Harness::PrimeAgent => {
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
            selected_model(harness, credential, None)
                .ok()
                .map(|model| {
                    environment(harness, &credential.api_key, &credential.endpoint, &model)
                        .values
                        .keys()
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
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
    override_model: Option<&str>,
) -> Result<ExitStatus> {
    let executable = binary
        .map(PathBuf::from)
        .or_else(|| which::which(harness.executable()).ok())
        .ok_or_else(|| anyhow!("{} is not installed or not on PATH", harness.executable()))?;
    let model = selected_model(harness, credential, override_model)?;
    let mut overlay = environment(harness, &credential.api_key, &credential.endpoint, &model);
    let mut artifacts = Vec::new();
    let mut artifact_dirs = Vec::new();
    prepare_configuration(
        harness,
        &credential.endpoint,
        &model,
        &mut overlay,
        &mut artifacts,
        &mut artifact_dirs,
    )?;
    let args = command_arguments(
        harness,
        &credential.endpoint,
        &model,
        args,
        artifacts.first().map(|file| file.path()),
    );
    run_with_environment(&executable, &args, &overlay).await
}

pub async fn launch_capture(
    harness: Harness,
    credential: &Credential,
    binary: Option<&Path>,
    args: &[String],
    override_model: Option<&str>,
) -> Result<std::process::Output> {
    let executable = binary
        .map(PathBuf::from)
        .or_else(|| which::which(harness.executable()).ok())
        .ok_or_else(|| anyhow!("{} is not installed or not on PATH", harness.executable()))?;
    let model = selected_model(harness, credential, override_model)?;
    let mut overlay = environment(harness, &credential.api_key, &credential.endpoint, &model);
    let mut artifacts = Vec::new();
    let mut artifact_dirs = Vec::new();
    prepare_configuration(
        harness,
        &credential.endpoint,
        &model,
        &mut overlay,
        &mut artifacts,
        &mut artifact_dirs,
    )?;
    let args = command_arguments(
        harness,
        &credential.endpoint,
        &model,
        args,
        artifacts.first().map(|file| file.path()),
    );
    let mut command = Command::new(&executable);
    command.args(args).stdin(Stdio::null());
    for key in &overlay.removed {
        command.env_remove(key);
    }
    command.envs(&overlay.values);
    command
        .output()
        .await
        .with_context(|| format!("launch {}", executable.display()))
}

fn prepare_configuration(
    harness: Harness,
    endpoint: &str,
    model: &str,
    overlay: &mut Environment,
    artifacts: &mut Vec<NamedTempFile>,
    artifact_dirs: &mut Vec<TempDir>,
) -> Result<()> {
    match harness {
        Harness::Grok => configure_grok(endpoint, model)?,
        Harness::Pi => configure_pi_like(false, endpoint, model)?,
        Harness::PrimeAgent => configure_pi_like(true, endpoint, model)?,
        Harness::Dsh => artifacts.push(dsh_patch(endpoint, model)?),
        Harness::Hermes => {
            let dir = tempfile::tempdir().context("create temporary Hermes home")?;
            let config = json!({
                "_config_version": 12,
                "model": {
                    "default": model,
                    "provider": "astraflow"
                },
                "providers": {
                    "astraflow": {
                        "name": "AstraFlow ModelVerse",
                        "base_url": format!("{}/v1", endpoint.trim_end_matches('/')),
                        "key_env": "ASTRAFLOW_MODELVERSE_API_KEY",
                        "default_model": model,
                        "transport": "chat_completions"
                    }
                }
            });
            fs::write(
                dir.path().join("config.yaml"),
                serde_json::to_vec_pretty(&config)?,
            )
            .context("write temporary Hermes config")?;
            overlay.values.insert(
                "HERMES_HOME".into(),
                dir.path().to_string_lossy().into_owned(),
            );
            artifact_dirs.push(dir);
        }
        _ => {}
    }
    Ok(())
}

pub fn command_arguments(
    harness: Harness,
    endpoint: &str,
    model: &str,
    args: &[String],
    patch_path: Option<&Path>,
) -> Vec<String> {
    let base_url = format!("{}/v1", endpoint.trim_end_matches('/'));
    let mut configured = match harness {
        Harness::Codex => vec![
            "-c".into(),
            format!("model={}", toml_string(model)),
            "-c".into(),
            "model_provider=\"modelverse\"".into(),
            "-c".into(),
            "model_providers.modelverse.name=\"AstraFlow ModelVerse\"".into(),
            "-c".into(),
            format!(
                "model_providers.modelverse.base_url={}",
                toml_string(&base_url)
            ),
            "-c".into(),
            "model_providers.modelverse.env_key=\"ASTRAFLOW_MODELVERSE_API_KEY\"".into(),
            "-c".into(),
            "model_providers.modelverse.wire_api=\"responses\"".into(),
            "-c".into(),
            "model_providers.modelverse.requires_openai_auth=false".into(),
        ],
        Harness::Claude => vec!["--model".into(), model.into()],
        Harness::Grok => vec!["--model".into(), "astraflow".into()],
        Harness::Opencode => Vec::new(),
        Harness::Hermes => vec![
            "--model".into(),
            model.into(),
            "--provider".into(),
            "astraflow".into(),
        ],
        Harness::Pi | Harness::PrimeAgent => vec![
            "--provider".into(),
            "astraflow".into(),
            "--model".into(),
            model.into(),
        ],
        Harness::Dsh => patch_path
            .map(|path| vec!["--patch".into(), path.display().to_string()])
            .unwrap_or_default(),
    };
    configured.extend_from_slice(args);
    configured
}

fn configure_grok(endpoint: &str, model: &str) -> Result<()> {
    let dir = env::var_os("GROK_HOME")
        .map(PathBuf::from)
        .or_else(|| BaseDirs::new().map(|dirs| dirs.home_dir().join(".grok")))
        .ok_or_else(|| anyhow!("unable to locate Grok config directory"))?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("config.toml");
    let mut document = if path.is_file() {
        fs::read_to_string(&path)?
            .parse::<DocumentMut>()
            .context("parse Grok config.toml")?
    } else {
        DocumentMut::new()
    };
    if !document.get("model").is_some_and(Item::is_table) {
        document["model"] = Item::Table(Table::new());
    }
    if !document["model"]
        .get("astraflow")
        .is_some_and(Item::is_table)
    {
        document["model"]["astraflow"] = Item::Table(Table::new());
    }
    document["model"]["astraflow"]["model"] = value(model);
    document["model"]["astraflow"]["base_url"] =
        value(format!("{}/v1", endpoint.trim_end_matches('/')));
    document["model"]["astraflow"]["env_key"] = value("ASTRAFLOW_MODELVERSE_API_KEY");
    document["model"]["astraflow"]["api_backend"] = value("chat_completions");
    write_config(&path, document.to_string().as_bytes())
}

fn configure_pi_like(prime: bool, endpoint: &str, model: &str) -> Result<()> {
    let env_name = if prime {
        "PRIME_AGENT_CODING_AGENT_DIR"
    } else {
        "PI_CODING_AGENT_DIR"
    };
    let default_dir = if prime { ".prime/agent" } else { ".pi/agent" };
    let dir = env::var_os(env_name)
        .map(PathBuf::from)
        .or_else(|| BaseDirs::new().map(|dirs| dirs.home_dir().join(default_dir)))
        .ok_or_else(|| {
            anyhow!(
                "unable to locate {} config directory",
                if prime { "Prime Agent" } else { "Pi" }
            )
        })?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("models.json");
    let mut root: Value = if path.is_file() {
        serde_json::from_slice(&fs::read(&path)?)
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        json!({})
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} must contain a JSON object", path.display()))?;
    let providers = object
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()));
    let providers = providers
        .as_object_mut()
        .ok_or_else(|| anyhow!("providers in {} must be an object", path.display()))?;
    let api_key_reference = if prime {
        "ASTRAFLOW_MODELVERSE_API_KEY"
    } else {
        "$ASTRAFLOW_MODELVERSE_API_KEY"
    };
    providers.insert(
        "astraflow".into(),
        json!({
            "baseUrl": format!("{}/v1", endpoint.trim_end_matches('/')),
            "api": "openai-completions",
            "apiKey": api_key_reference,
            "authHeader": true,
            "models": [{
                "id": model,
                "name": model,
                "input": ["text"],
                "contextWindow": 128000,
                "maxTokens": 16384
            }]
        }),
    );
    write_config(&path, &serde_json::to_vec_pretty(&root)?)
}

fn dsh_patch(endpoint: &str, model: &str) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new()?;
    let payload = format!(
        "- id: agent-default-model\n  config:\n    provider: astraflow\n    model: {}\n\n- id: llm-pi-ai\n  config:\n    providers:\n      astraflow:\n        displayName: AstraFlow ModelVerse\n        apiKeyEnv: ASTRAFLOW_MODELVERSE_API_KEY\n        api: openai-completions\n        baseURL: {}\n        models:\n          - id: {}\n",
        yaml_string(model),
        yaml_string(&format!("{}/v1", endpoint.trim_end_matches('/'))),
        yaml_string(model)
    );
    use std::io::Write;
    file.write_all(payload.as_bytes())?;
    file.flush()?;
    Ok(file)
}

fn write_config(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("config path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    use std::io::Write;
    temp.write_all(bytes)?;
    temp.write_all(b"\n")?;
    temp.flush()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
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
    use crate::cli::ModelVerseRegion;
    use crate::config::ModelSelection;

    fn credential() -> Credential {
        Credential {
            api_key: SecretString::from("test-key".to_owned()),
            key_id: None,
            key_name: None,
            project_id: None,
            endpoint: "https://api.modelverse.cn".into(),
            region: ModelVerseRegion::China,
            models: ModelSelection {
                chat_completions: Some("chat-model".into()),
                responses: Some("response-model".into()),
                anthropic: Some("claude-model".into()),
            },
            oauth: None,
        }
    }

    #[test]
    fn codex_forces_provider_and_model_per_invocation() {
        let args = command_arguments(
            Harness::Codex,
            "https://api.modelverse.cn",
            "response-model",
            &["exec".into(), "hello".into()],
            None,
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "model=\"response-model\""])
        );
        assert!(
            args.iter()
                .any(|arg| arg == "model_provider=\"modelverse\"")
        );
    }

    #[test]
    fn claude_overrides_primary_and_auxiliary_models() {
        let cred = credential();
        let model = selected_model(Harness::Claude, &cred, None).unwrap();
        let env = environment(Harness::Claude, &cred.api_key, &cred.endpoint, &model);
        assert_eq!(env.values["ANTHROPIC_MODEL"], "claude-model");
        assert_eq!(env.values["CLAUDE_CODE_SUBAGENT_MODEL"], "claude-model");
    }

    #[test]
    fn opencode_content_is_the_last_layer_and_selects_model() {
        let cred = credential();
        let env = environment(
            Harness::Opencode,
            &cred.api_key,
            &cred.endpoint,
            "chat-model",
        );
        let config: Value = serde_json::from_str(&env.values["OPENCODE_CONFIG_CONTENT"]).unwrap();
        assert_eq!(config["model"], "astraflow/chat-model");
        assert_eq!(
            config["provider"]["astraflow"]["models"]["chat-model"]["name"],
            "chat-model"
        );
    }

    #[test]
    fn discovered_credentials_do_not_guess_an_incompatible_codex_model() {
        let mut cred = credential();
        cred.models.responses = None;
        let error = selected_model(Harness::Codex, &cred, None).unwrap_err();
        assert!(error.to_string().contains("no Responses-capable model"));
    }

    #[test]
    fn unknown_harness_is_rejected() {
        assert!(Harness::parse("unsafe-custom-command").is_err());
    }
}
