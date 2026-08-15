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
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_MANTLE",
    "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
    "HERMES_INFERENCE_MODEL",
    "HERMES_INFERENCE_PROVIDER",
    "HERMES_HOME",
    "HERMES_SAFE_MODE",
    "HERMES_MANAGED_DIR",
    "HERMES_ENABLE_PROJECT_PLUGINS",
    "CODEX_HOME",
    "CODEX_CONFIG",
    "DEFAULT_AUTH_REQUEST",
    "MODEL_PROVIDER",
    "ASTRAFLOW_MODELVERSE_API_KEY",
    "$ASTRAFLOW_MODELVERSE_API_KEY",
    "CODEX_API_KEY",
    "DEEPSEEK_API_KEY",
    "DEEPSEEK_BASE_URL",
    "OPENCODE_CONFIG",
    "OPENCODE_CONFIG_DIR",
    "OPENCODE_CONFIG_CONTENT",
    "OPENCODE_DISABLE_PROJECT_CONFIG",
    "OPENCODE_DISABLE_DEFAULT_PLUGINS",
    "OPENCODE_DISABLE_EXTERNAL_SKILLS",
    "OPENCODE_DISABLE_CLAUDE_CODE",
    "OPENCODE_DISABLE_CLAUDE_CODE_PROMPT",
    "OPENCODE_DISABLE_CLAUDE_CODE_SKILLS",
    "OPENCODE_PURE",
    "OPENCODE_CONSOLE_TOKEN",
    "PI_CODING_AGENT_DIR",
    "PI_CODING_AGENT_SESSION_DIR",
    "PRIME_AGENT_CODING_AGENT_DIR",
    "PRIME_AGENT_CODING_AGENT_SESSION_DIR",
    "PRIME_AGENT_SESSION_DIR",
    "DSH_HOME",
    "GROK_HOME",
    "GROK_CODE_XAI_API_KEY",
    "GROK_MODELS_BASE_URL",
    "GROK_MODELS_LIST_URL",
    "GROK_CLI_CHAT_PROXY_BASE_URL",
    "GROK_XAI_API_BASE_URL",
    "GROK_DEFAULT_MODEL",
    "GROK_WEB_SEARCH_MODEL",
    "GROK_SESSION_SUMMARY_MODEL",
    "GROK_IMAGE_DESCRIPTION_MODEL",
    "GROK_PROMPT_SUGGESTIONS_MODEL",
    "GROK_AUTH_PROVIDER_COMMAND",
    "GROK_DEPLOYMENT_KEY",
    "GROK_AUTH",
    "GROK_AUTH_PATH",
    "GROK_EXTRA_AUTH_KEY",
    "GROK_MANAGED_CONFIG_URL",
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
            values.insert("NO_BROWSER".into(), "1".into());
        }
        Harness::Claude => {
            values.insert("ANTHROPIC_AUTH_TOKEN".into(), key);
            values.insert("ANTHROPIC_BASE_URL".into(), root.to_owned());
            for name in [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_FABLE_MODEL",
                "CLAUDE_CODE_SUBAGENT_MODEL",
            ] {
                values.insert(name.into(), model.to_owned());
            }
            values.insert("NO_BROWSER".into(), "1".into());
        }
        Harness::Opencode => {
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base.clone());
            values.insert("OPENCODE_DISABLE_PROJECT_CONFIG".into(), "1".into());
            values.insert("OPENCODE_DISABLE_DEFAULT_PLUGINS".into(), "1".into());
            values.insert("OPENCODE_DISABLE_EXTERNAL_SKILLS".into(), "1".into());
            values.insert("OPENCODE_DISABLE_CLAUDE_CODE".into(), "1".into());
            values.insert("OPENCODE_DISABLE_CLAUDE_CODE_PROMPT".into(), "1".into());
            values.insert("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS".into(), "1".into());
            values.insert("OPENCODE_PURE".into(), "1".into());
            values.insert(
                "OPENCODE_CONFIG_CONTENT".into(),
                json!({
                    "plugin": [],
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
            values.insert("HERMES_SAFE_MODE".into(), "1".into());
            values.insert("HERMES_ENABLE_PROJECT_PLUGINS".into(), "0".into());
        }
        Harness::Dsh => {}
        Harness::Grok => {
            // Grok 1.0.4 uses these values for wire-level main and auxiliary
            // requests even when --model selects a named custom backend.
            for name in [
                "GROK_DEFAULT_MODEL",
                "GROK_WEB_SEARCH_MODEL",
                "GROK_SESSION_SUMMARY_MODEL",
                "GROK_IMAGE_DESCRIPTION_MODEL",
                "GROK_PROMPT_SUGGESTIONS_MODEL",
            ] {
                values.insert(name.into(), model.to_owned());
            }
        }
        Harness::Pi => {
            // Pi <= 0.73 resolves the entire models.json apiKey string as an
            // environment-variable name, while newer Pi expands the leading
            // `$`. Supplying both names keeps the key out of the config file
            // and supports both resolution rules.
            values.insert("$ASTRAFLOW_MODELVERSE_API_KEY".into(), key.clone());
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base);
        }
        Harness::PrimeAgent => {
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
    validate_passthrough_args(harness, args)?;
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
    validate_passthrough_args(harness, args)?;
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

fn validate_passthrough_args(harness: Harness, args: &[String]) -> Result<()> {
    if harness == Harness::Dsh
        && args
            .first()
            .is_some_and(|arg| matches!(arg.as_str(), "web" | "plugin"))
    {
        bail!("DSH web and plugin modes are disabled for AstraFlow-managed routing");
    }
    let conflicts = match harness {
        Harness::Codex => [
            "-m",
            "--model",
            "-c",
            "--config",
            "--oss",
            "--local-provider",
        ]
        .as_slice(),
        Harness::Grok => ["-m", "--model"].as_slice(),
        Harness::Hermes => ["--profile", "-p", "--provider", "--model", "-m"].as_slice(),
        Harness::Dsh => [
            "--patch",
            "--profile",
            "--dump-config",
            "--dump-default-config",
        ]
        .as_slice(),
        Harness::Pi | Harness::PrimeAgent => {
            ["--provider", "--model", "--api-key", "-e", "--extension"].as_slice()
        }
        _ => return Ok(()),
    };
    for arg in args {
        let exact = conflicts.contains(&arg.as_str());
        let assigned = conflicts
            .iter()
            .filter(|flag| flag.starts_with("--"))
            .any(|flag| arg.starts_with(&format!("{flag}=")));
        let attached_short_extension = matches!(harness, Harness::Pi | Harness::PrimeAgent)
            && arg.starts_with("-e")
            && arg != "-e";
        let attached_codex_short = harness == Harness::Codex
            && ["-c", "-m"]
                .iter()
                .any(|flag| arg.starts_with(flag) && arg != flag);
        let attached_hermes_short = harness == Harness::Hermes
            && ["-p", "-m"]
                .iter()
                .any(|flag| arg.starts_with(flag) && arg != flag);
        let attached_grok_short = harness == Harness::Grok && arg.starts_with("-m") && arg != "-m";
        if exact
            || assigned
            || attached_short_extension
            || attached_codex_short
            || attached_hermes_short
            || attached_grok_short
        {
            bail!(
                "{} argument `{arg}` conflicts with AstraFlow routing; use the outer `astraflow {} --model ...` option and remove inner provider, model, key, config, or extension overrides",
                harness.name(),
                harness.name()
            );
        }
    }
    Ok(())
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
        Harness::Codex => {
            let base = BaseDirs::new()
                .map(|dirs| dirs.cache_dir().join("astraflow").join("runtime"))
                .ok_or_else(|| anyhow!("unable to locate cache directory for Codex isolation"))?;
            fs::create_dir_all(&base).context("create AstraFlow runtime cache")?;
            let dir = tempfile::Builder::new()
                .prefix("codex-home-")
                .tempdir_in(base)
                .context("create temporary Codex home")?;
            overlay.values.insert(
                "CODEX_HOME".into(),
                dir.path().to_string_lossy().into_owned(),
            );
            if let Some(catalog) = codex_catalog(model)? {
                artifacts.push(catalog);
            }
            artifact_dirs.push(dir);
        }
        Harness::Grok => {
            let dir = tempfile::tempdir().context("create temporary Grok home")?;
            configure_grok(endpoint, model, Some(dir.path()))?;
            overlay.values.insert(
                "GROK_HOME".into(),
                dir.path().to_string_lossy().into_owned(),
            );
            artifact_dirs.push(dir);
        }
        Harness::Pi | Harness::PrimeAgent => {
            let prime = harness == Harness::PrimeAgent;
            let session_envs: &[&str] = if prime {
                &[
                    "PRIME_AGENT_SESSION_DIR",
                    "PRIME_AGENT_CODING_AGENT_SESSION_DIR",
                ]
            } else {
                &["PI_CODING_AGENT_SESSION_DIR"]
            };
            let agent_env = if prime {
                "PRIME_AGENT_CODING_AGENT_DIR"
            } else {
                "PI_CODING_AGENT_DIR"
            };
            let default_agent_dir = if prime { ".prime/agent" } else { ".pi/agent" };
            let session_dir = session_envs
                .iter()
                .find_map(|name| env::var_os(name).map(PathBuf::from))
                .or_else(|| {
                    env::var_os(agent_env)
                        .map(PathBuf::from)
                        .or_else(|| {
                            BaseDirs::new().map(|dirs| dirs.home_dir().join(default_agent_dir))
                        })
                        .map(|dir| dir.join("sessions"))
                });
            let dir = tempfile::tempdir().context("create temporary Pi-compatible agent home")?;
            configure_pi_like(prime, endpoint, model, Some(dir.path()))?;
            overlay
                .values
                .insert(agent_env.into(), dir.path().to_string_lossy().into_owned());
            if let Some(session_dir) = session_dir {
                for session_env in session_envs {
                    overlay.values.insert(
                        (*session_env).into(),
                        session_dir.to_string_lossy().into_owned(),
                    );
                }
            }
            artifact_dirs.push(dir);
        }
        Harness::Dsh => {
            let dir = tempfile::tempdir().context("create temporary DSH home")?;
            fs::write(dir.path().join("settings.yaml"), b"{}\n")
                .context("write isolated DSH settings")?;
            fs::write(dir.path().join("cordis.patch.yml"), b"[]\n")
                .context("write isolated DSH home patch")?;
            overlay
                .values
                .insert("DSH_HOME".into(), dir.path().to_string_lossy().into_owned());
            artifacts.push(dsh_patch(endpoint, model)?);
            artifact_dirs.push(dir);
        }
        Harness::Hermes => {
            let dir = tempfile::tempdir().context("create temporary Hermes home")?;
            let profile_dir = dir.path().join("profiles").join("astraflow");
            let managed_dir = dir.path().join("managed");
            fs::create_dir_all(&profile_dir).context("create isolated Hermes profile")?;
            fs::create_dir_all(&managed_dir).context("create isolated Hermes managed scope")?;
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
                profile_dir.join("config.yaml"),
                serde_json::to_vec_pretty(&config)?,
            )
            .context("write temporary Hermes config")?;
            overlay.values.insert(
                "HERMES_HOME".into(),
                profile_dir.to_string_lossy().into_owned(),
            );
            overlay.values.insert(
                "HERMES_MANAGED_DIR".into(),
                managed_dir.to_string_lossy().into_owned(),
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
        Harness::Codex => {
            let mut args = vec![
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
            ];
            if let Some(path) = patch_path {
                args.push("-c".into());
                args.push(format!(
                    "model_catalog_json={}",
                    toml_string(&path.display().to_string())
                ));
            }
            args
        }
        Harness::Claude => vec![
            "--setting-sources".into(),
            String::new(),
            "--model".into(),
            model.into(),
        ],
        Harness::Grok => vec![
            "--disable-web-search".into(),
            "--model".into(),
            "astraflow".into(),
        ],
        Harness::Opencode => vec![
            "--pure".into(),
            "--model".into(),
            format!("astraflow/{model}"),
        ],
        Harness::Hermes => vec![
            "--model".into(),
            model.into(),
            "--provider".into(),
            "astraflow".into(),
        ],
        Harness::Pi | Harness::PrimeAgent => vec![
            "--no-extensions".into(),
            "--provider".into(),
            "astraflow".into(),
            "--model".into(),
            model.into(),
        ],
        Harness::Dsh => patch_path
            .map(|path| {
                vec![
                    "--profile".into(),
                    "headless".into(),
                    "--patch".into(),
                    path.display().to_string(),
                ]
            })
            .unwrap_or_default(),
    };
    configured.extend_from_slice(args);
    configured
}

fn configure_grok(endpoint: &str, model: &str, isolated_dir: Option<&Path>) -> Result<()> {
    let dir = isolated_dir
        .map(PathBuf::from)
        .or_else(|| env::var_os("GROK_HOME").map(PathBuf::from))
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
    let mut astraflow = Table::new();
    astraflow["model"] = value(model);
    astraflow["base_url"] = value(format!("{}/v1", endpoint.trim_end_matches('/')));
    astraflow["env_key"] = value("ASTRAFLOW_MODELVERSE_API_KEY");
    astraflow["api_backend"] = value("chat_completions");
    document["model"]["astraflow"] = Item::Table(astraflow);
    write_config(&path, document.to_string().as_bytes())
}

fn configure_pi_like(
    prime: bool,
    endpoint: &str,
    model: &str,
    isolated_dir: Option<&Path>,
) -> Result<()> {
    let env_name = if prime {
        "PRIME_AGENT_CODING_AGENT_DIR"
    } else {
        "PI_CODING_AGENT_DIR"
    };
    let default_dir = if prime { ".prime/agent" } else { ".pi/agent" };
    let dir = isolated_dir.map(PathBuf::from).or_else(|| {
        env::var_os(env_name)
            .map(PathBuf::from)
            .or_else(|| BaseDirs::new().map(|dirs| dirs.home_dir().join(default_dir)))
    });
    let dir = dir.ok_or_else(|| {
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

fn codex_catalog(model: &str) -> Result<Option<NamedTempFile>> {
    const BUNDLED_MODELS_0_147: &[&str] = &[
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.2",
        "codex-auto-review",
    ];
    if BUNDLED_MODELS_0_147.contains(&model) {
        return Ok(None);
    }
    let mut file = NamedTempFile::new().context("create temporary Codex model catalog")?;
    let catalog = json!({
        "models": [{
            "slug": model,
            "display_name": model,
            "description": "AstraFlow ModelVerse Responses model",
            "default_reasoning_level": null,
            "supported_reasoning_levels": [],
            "shell_type": "default",
            "visibility": "none",
            "supported_in_api": true,
            "priority": 1,
            "availability_nux": null,
            "upgrade": null,
            "include_skills_usage_instructions": false,
            "include_plugin_usage_instructions": false,
            "include_apps_usage_instructions": false,
            "supports_reasoning_summary_parameter": false,
            "default_reasoning_summary": "none",
            "support_verbosity": false,
            "default_verbosity": null,
            "apply_patch_tool_type": null,
            "web_search_tool_type": "text",
            "truncation_policy": {"mode": "tokens", "limit": 10000},
            "supports_parallel_tool_calls": false,
            "supports_image_detail_original": false,
            "context_window": 128000,
            "max_context_window": 128000,
            "experimental_supported_tools": [],
            "input_modalities": ["text"],
            "supports_search_tool": false,
            "use_responses_lite": false,
            "base_instructions": "You are Codex, a coding agent. Work carefully in the user's repository and follow the user's instructions."
        }]
    });
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&catalog)?)?;
    file.flush()?;
    Ok(Some(file))
}

fn dsh_patch(endpoint: &str, model: &str) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new()?;
    let payload = format!(
        "- id: settings\n  disabled: true\n\n- id: agent-default-model\n  config:\n    provider: astraflow\n    model: {}\n\n- id: llm-pi-ai\n  config:\n    providers:\n      astraflow:\n        displayName: AstraFlow ModelVerse\n        apiKeyEnv: ASTRAFLOW_MODELVERSE_API_KEY\n        api: openai-completions\n        baseURL: {}\n        models:\n          - id: {}\n",
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
        let args = command_arguments(
            Harness::Claude,
            &cred.endpoint,
            &model,
            &["--print".into(), "hello".into()],
            None,
        );
        assert_eq!(
            &args[..4],
            ["--setting-sources", "", "--model", "claude-model"]
        );
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
        assert_eq!(config["plugin"], json!([]));
        assert_eq!(env.values["OPENCODE_DISABLE_PROJECT_CONFIG"], "1");
        assert_eq!(env.values["OPENCODE_DISABLE_DEFAULT_PLUGINS"], "1");
        assert_eq!(env.values["OPENCODE_DISABLE_EXTERNAL_SKILLS"], "1");
        assert_eq!(env.values["OPENCODE_PURE"], "1");
        let args = command_arguments(
            Harness::Opencode,
            &cred.endpoint,
            "chat-model",
            &["run".into(), "hello".into()],
            None,
        );
        assert_eq!(&args[..3], ["--pure", "--model", "astraflow/chat-model"]);
    }

    #[test]
    fn pi_supports_legacy_and_current_environment_resolution() {
        let cred = credential();
        let env = environment(Harness::Pi, &cred.api_key, &cred.endpoint, "chat-model");
        assert_eq!(env.values["ASTRAFLOW_MODELVERSE_API_KEY"], "test-key");
        assert_eq!(env.values["$ASTRAFLOW_MODELVERSE_API_KEY"], "test-key");
    }

    #[test]
    fn routing_conflicts_are_rejected_before_launch() {
        assert!(
            validate_passthrough_args(
                Harness::Codex,
                &["exec".into(), "--model=hostile".into(), "hello".into()]
            )
            .is_err()
        );
        assert!(
            validate_passthrough_args(Harness::Pi, &["--extension".into(), "hostile.ts".into()])
                .is_err()
        );
        assert!(
            validate_passthrough_args(Harness::Pi, &["--print".into(), "hello".into()]).is_ok()
        );
    }

    #[test]
    fn unknown_codex_model_gets_a_text_only_catalog() {
        assert!(codex_catalog("gpt-5.6-sol").unwrap().is_none());
        let catalog = codex_catalog("future-responses-model").unwrap().unwrap();
        let value: Value = serde_json::from_slice(&fs::read(catalog.path()).unwrap()).unwrap();
        assert_eq!(value["models"][0]["slug"], "future-responses-model");
        assert_eq!(value["models"][0]["input_modalities"], json!(["text"]));
        assert_eq!(value["models"][0]["use_responses_lite"], false);
    }

    #[test]
    fn grok_replaces_stale_same_name_credentials() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            "[model.astraflow]\napi_key='hostile'\n[model.astraflow.extra_headers]\nAuthorization='Bearer hostile'\n",
        )
        .unwrap();
        configure_grok("https://api.modelverse.cn", "chat-model", Some(dir.path())).unwrap();
        let document = fs::read_to_string(dir.path().join("config.toml"))
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        let provider = document["model"]["astraflow"].as_table().unwrap();
        assert_eq!(provider["model"].as_str(), Some("chat-model"));
        assert!(provider.get("api_key").is_none());
        assert!(provider.get("extra_headers").is_none());

        let env = environment(
            Harness::Grok,
            &credential().api_key,
            "https://api.modelverse.cn",
            "chat-model",
        );
        assert!(!env.values.contains_key("XAI_API_KEY"));
        assert!(!env.values.contains_key("OPENAI_API_KEY"));
        for name in [
            "GROK_DEFAULT_MODEL",
            "GROK_WEB_SEARCH_MODEL",
            "GROK_SESSION_SUMMARY_MODEL",
            "GROK_IMAGE_DESCRIPTION_MODEL",
            "GROK_PROMPT_SUGGESTIONS_MODEL",
        ] {
            assert_eq!(env.values[name], "chat-model");
        }
    }

    #[test]
    fn dsh_uses_managed_headless_patch_and_rejects_user_layers() {
        let patch = dsh_patch("https://api.modelverse.cn", "chat-model").unwrap();
        let content = fs::read_to_string(patch.path()).unwrap();
        assert!(content.contains("- id: settings\n  disabled: true"));
        let args = command_arguments(
            Harness::Dsh,
            "https://api.modelverse.cn",
            "chat-model",
            &["hello".into()],
            Some(patch.path()),
        );
        assert_eq!(&args[..3], ["--profile", "headless", "--patch"]);
        assert!(
            validate_passthrough_args(
                Harness::Dsh,
                &["--patch=hostile.yml".into(), "hello".into()]
            )
            .is_err()
        );
        assert!(validate_passthrough_args(Harness::Dsh, &["plugin".into()]).is_err());

        let env = environment(
            Harness::Dsh,
            &credential().api_key,
            "https://api.modelverse.cn",
            "chat-model",
        );
        assert!(!env.values.contains_key("DEEPSEEK_API_KEY"));
        assert!(!env.values.contains_key("OPENAI_API_KEY"));
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
