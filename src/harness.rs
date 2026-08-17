use crate::config::{Credential, HarnessModelSettings};
use crate::model_picker::ModelSlot;
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
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL_AWS_REGION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION",
    "ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES",
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

pub const DEFAULT_MODEL_SLOT: &str = "default";

pub fn model_slots(harness: Harness) -> Vec<ModelSlot> {
    let pairs: &[(&str, &str)] = match harness {
        Harness::Claude => &[
            ("default", "Default"),
            ("fable", "Fable"),
            ("opus", "Opus"),
            ("sonnet", "Sonnet"),
            ("haiku", "Haiku"),
            ("agents", "Agents"),
        ],
        Harness::Codex => &[
            ("default", "Default"),
            ("review", "Review"),
            ("agents", "Agents"),
        ],
        Harness::Grok => &[
            ("default", "Default"),
            ("summary", "Summary"),
            ("prompt", "Prompts"),
            ("general", "General Agent"),
            ("explore", "Explore Agent"),
            ("plan", "Plan Agent"),
        ],
        Harness::Opencode => &[
            ("default", "Default"),
            ("small", "Small"),
            ("build", "Build"),
            ("plan", "Plan"),
            ("general", "General"),
            ("explore", "Explore"),
            ("compaction", "Compaction"),
            ("title", "Title"),
            ("summary", "Summary"),
        ],
        Harness::Pi | Harness::PrimeAgent => &[("default", "Default"), ("cycle", "Cycle Pool")],
        Harness::Dsh => &[
            ("default", "Default"),
            ("title", "Title"),
            ("compaction", "Compaction"),
            ("spawn", "Spawn Agent"),
            ("fork", "Fork Agent"),
        ],
        Harness::Hermes => &[("default", "Default")],
    };
    pairs
        .iter()
        .map(|(key, label)| ModelSlot {
            key,
            label,
            multiple: *key == "cycle",
        })
        .collect()
}

fn model_for_slot<'a>(
    models: &'a BTreeMap<String, String>,
    slot: &str,
    default: &'a str,
) -> &'a str {
    models
        .get(slot)
        .filter(|model| !model.trim().is_empty())
        .map(String::as_str)
        .unwrap_or(default)
}

fn unique_models<'a>(default: &'a str, models: &'a BTreeMap<String, String>) -> Vec<&'a str> {
    let mut values = vec![default];
    for model in models.values().map(String::as_str) {
        if !values.iter().any(|value| value.eq_ignore_ascii_case(model)) {
            values.push(model);
        }
    }
    values
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
    }
    .or(credential.models.chat_completions.as_ref());
    if let Some(model) = selected.cloned().or_else(|| {
        env::var("ASTRAFLOW_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
    }) {
        return Ok(model);
    }
    Ok(crate::modelverse::PREFERRED_CHAT_MODEL.to_owned())
}

pub fn environment(
    harness: Harness,
    key: &SecretString,
    endpoint: &str,
    model: &str,
) -> Environment {
    environment_with_models(harness, key, endpoint, model, &BTreeMap::new())
}

pub fn environment_with_models(
    harness: Harness,
    key: &SecretString,
    endpoint: &str,
    model: &str,
    models: &BTreeMap<String, String>,
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
            values.insert("ANTHROPIC_API_KEY".into(), key.clone());
            values.insert("ANTHROPIC_AUTH_TOKEN".into(), key);
            values.insert("ANTHROPIC_BASE_URL".into(), root.to_owned());
            values.insert("ANTHROPIC_MODEL".into(), model.to_owned());
            for (name, slot) in [
                ("ANTHROPIC_DEFAULT_FABLE_MODEL", "fable"),
                ("ANTHROPIC_DEFAULT_OPUS_MODEL", "opus"),
                ("ANTHROPIC_DEFAULT_SONNET_MODEL", "sonnet"),
                ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "haiku"),
                ("CLAUDE_CODE_SUBAGENT_MODEL", "agents"),
            ] {
                values.insert(name.into(), model_for_slot(models, slot, model).to_owned());
            }
            for name in [
                "CLAUDE_CODE_USE_BEDROCK",
                "CLAUDE_CODE_USE_VERTEX",
                "CLAUDE_CODE_USE_FOUNDRY",
                "CLAUDE_CODE_USE_ANTHROPIC_AWS",
                "CLAUDE_CODE_USE_MANTLE",
                "CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST",
            ] {
                values.insert(name.into(), "0".into());
            }
            values.insert("NO_BROWSER".into(), "1".into());
        }
        Harness::Opencode => {
            values.insert("OPENAI_API_KEY".into(), key);
            values.insert("OPENAI_BASE_URL".into(), openai_base.clone());
            let user_content = env::var("OPENCODE_CONFIG_CONTENT").ok();
            values.insert(
                "OPENCODE_CONFIG_CONTENT".into(),
                opencode_config_content(&openai_base, model, models, user_content.as_deref())
                    .to_string(),
            );
        }
        Harness::Hermes => {
            values.insert("HERMES_INFERENCE_MODEL".into(), model.to_owned());
            values.insert("HERMES_INFERENCE_PROVIDER".into(), "astraflow".into());
        }
        Harness::Dsh => {}
        Harness::Grok => {
            // Grok 1.0.4 uses these values for wire-level main and auxiliary
            // requests even when --model selects a named custom backend.
            values.insert("GROK_DEFAULT_MODEL".into(), model.to_owned());
            values.insert(
                "GROK_WEB_SEARCH_MODEL".into(),
                model_for_slot(models, "web", model).to_owned(),
            );
            values.insert(
                "GROK_SESSION_SUMMARY_MODEL".into(),
                model_for_slot(models, "summary", model).to_owned(),
            );
            values.insert("GROK_IMAGE_DESCRIPTION_MODEL".into(), model.to_owned());
            values.insert(
                "GROK_PROMPT_SUGGESTIONS_MODEL".into(),
                model_for_slot(models, "prompt", model).to_owned(),
            );
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
        removed: SCRUBBED_ENV
            .iter()
            .filter(|name| !preserve_user_environment(harness, name))
            .map(|name| (*name).to_owned())
            .collect(),
    }
}

fn opencode_config_content(
    openai_base: &str,
    model: &str,
    models: &BTreeMap<String, String>,
    user_content: Option<&str>,
) -> Value {
    let mut content = user_content
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let root = content.as_object_mut().expect("content is an object");
    root.insert("model".into(), json!(format!("astraflow-managed/{model}")));
    root.insert(
        "small_model".into(),
        json!(format!(
            "astraflow-managed/{}",
            model_for_slot(models, "small", model)
        )),
    );

    let agents = root.entry("agent").or_insert_with(|| json!({}));
    if !agents.is_object() {
        *agents = json!({});
    }
    let agents = agents.as_object_mut().expect("agents is an object");
    for slot in [
        "build",
        "plan",
        "general",
        "explore",
        "compaction",
        "title",
        "summary",
    ] {
        let agent = agents.entry(slot).or_insert_with(|| json!({}));
        if !agent.is_object() {
            *agent = json!({});
        }
        agent.as_object_mut().expect("agent is an object").insert(
            "model".into(),
            json!(format!(
                "astraflow-managed/{}",
                model_for_slot(models, slot, model)
            )),
        );
    }

    let providers = root.entry("provider").or_insert_with(|| json!({}));
    if !providers.is_object() {
        *providers = json!({});
    }
    let registered: Map<String, Value> = unique_models(model, models)
        .into_iter()
        .map(|id| (id.to_owned(), json!({"name": id})))
        .collect();
    providers
        .as_object_mut()
        .expect("providers is an object")
        .insert(
            "astraflow-managed".into(),
            json!({
                "name": "AstraFlow ModelVerse",
                "npm": "@ai-sdk/openai-compatible",
                "env": ["ASTRAFLOW_MODELVERSE_API_KEY"],
                "options": {
                    "baseURL": openai_base,
                    "apiKey": "{env:ASTRAFLOW_MODELVERSE_API_KEY}"
                },
                "models": registered
            }),
        );
    content
}

fn preserve_user_environment(harness: Harness, name: &str) -> bool {
    match harness {
        Harness::Codex => matches!(name, "CODEX_HOME" | "CODEX_CONFIG"),
        Harness::Grok => name == "GROK_HOME",
        Harness::Opencode => name.starts_with("OPENCODE_") && name != "OPENCODE_CONFIG_CONTENT",
        Harness::Hermes => matches!(
            name,
            "HERMES_HOME"
                | "HERMES_SAFE_MODE"
                | "HERMES_MANAGED_DIR"
                | "HERMES_ENABLE_PROJECT_PLUGINS"
        ),
        Harness::Pi => matches!(name, "PI_CODING_AGENT_DIR" | "PI_CODING_AGENT_SESSION_DIR"),
        Harness::PrimeAgent => matches!(
            name,
            "PRIME_AGENT_CODING_AGENT_DIR"
                | "PRIME_AGENT_CODING_AGENT_SESSION_DIR"
                | "PRIME_AGENT_SESSION_DIR"
        ),
        Harness::Dsh => name == "DSH_HOME",
        Harness::Claude => false,
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
    let mut models = HarnessModelSettings::default();
    if let Some(model) = override_model.filter(|model| !model.trim().is_empty()) {
        models
            .slots
            .insert(DEFAULT_MODEL_SLOT.into(), model.trim().to_owned());
    }
    launch_with_models(harness, credential, binary, args, &models).await
}

pub async fn launch_with_models(
    harness: Harness,
    credential: &Credential,
    binary: Option<&Path>,
    args: &[String],
    models: &HarnessModelSettings,
) -> Result<ExitStatus> {
    let executable = binary
        .map(PathBuf::from)
        .or_else(|| which::which(harness.executable()).ok())
        .ok_or_else(|| anyhow!("{} is not installed or not on PATH", harness.executable()))?;
    validate_passthrough_args(harness, args)?;
    let model = selected_model(
        harness,
        credential,
        models.slots.get(DEFAULT_MODEL_SLOT).map(String::as_str),
    )?;
    let mut overlay = environment_with_models(
        harness,
        &credential.api_key,
        &credential.endpoint,
        &model,
        &models.slots,
    );
    let mut artifacts = Vec::new();
    let mut artifact_dirs = Vec::new();
    prepare_configuration(
        harness,
        &credential.endpoint,
        &model,
        models,
        &mut overlay,
        &mut artifacts,
        &mut artifact_dirs,
    )?;
    let args = command_arguments_with_models(
        harness,
        &credential.endpoint,
        &model,
        args,
        artifacts.first().map(|file| file.path()),
        models,
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
        &HarnessModelSettings::default(),
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
        bail!(
            "DSH positional web and plugin modes are not accepted; start the managed Web UI with `astraflow dsh --profile web`"
        );
    }
    if harness == Harness::Dsh {
        let mut profiles = 0;
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            let profile = if arg == "--profile" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("dsh argument `--profile` requires web or headless"))?;
                index += 1;
                Some(value.as_str())
            } else {
                arg.strip_prefix("--profile=")
            };
            if let Some(profile) = profile {
                profiles += 1;
                if profiles > 1 {
                    bail!("dsh argument `--profile` may only be specified once");
                }
                if !matches!(profile, "web" | "headless") {
                    bail!(
                        "dsh profile `{profile}` is not supported; use `--profile web` or `--profile headless`"
                    );
                }
            }
            index += 1;
        }
    }
    let conflicts = match harness {
        Harness::Claude => [
            "--model",
            "--settings",
            "--setting-sources",
            "--fallback-model",
        ]
        .as_slice(),
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
        Harness::Opencode => ["-m", "--model"].as_slice(),
        Harness::Hermes => ["--profile", "-p", "--provider", "--model", "-m"].as_slice(),
        Harness::Dsh => ["--patch", "--dump-config", "--dump-default-config"].as_slice(),
        Harness::Pi | Harness::PrimeAgent => {
            ["--provider", "--model", "--models", "--api-key"].as_slice()
        }
    };
    for arg in args {
        let exact = conflicts.contains(&arg.as_str());
        let assigned = conflicts
            .iter()
            .filter(|flag| flag.starts_with("--"))
            .any(|flag| arg.starts_with(&format!("{flag}=")));
        let attached_codex_short = harness == Harness::Codex
            && ["-c", "-m"]
                .iter()
                .any(|flag| arg.starts_with(flag) && arg != flag);
        let attached_hermes_short = harness == Harness::Hermes
            && ["-p", "-m"]
                .iter()
                .any(|flag| arg.starts_with(flag) && arg != flag);
        let attached_grok_short = harness == Harness::Grok && arg.starts_with("-m") && arg != "-m";
        let attached_opencode_short =
            harness == Harness::Opencode && arg.starts_with("-m") && arg != "-m";
        if exact
            || assigned
            || attached_codex_short
            || attached_hermes_short
            || attached_grok_short
            || attached_opencode_short
        {
            bail!(
                "{} argument `{arg}` conflicts with AstraFlow routing; use the outer `astraflow {} --model ...` option and remove inner provider, model, key, or routing-config overrides",
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
    models: &HarnessModelSettings,
    overlay: &mut Environment,
    artifacts: &mut Vec<NamedTempFile>,
    artifact_dirs: &mut Vec<TempDir>,
) -> Result<()> {
    match harness {
        Harness::Codex => {
            if let Some(catalog) = codex_catalog(&unique_models(model, &models.slots))? {
                artifacts.push(catalog);
            }
        }
        Harness::Grok => {
            configure_grok(endpoint, model, &models.slots, None)?;
        }
        Harness::Pi | Harness::PrimeAgent => {
            let prime = harness == Harness::PrimeAgent;
            configure_pi_like(prime, endpoint, model, &models.cycle, None)?;
        }
        Harness::Dsh => {
            let settings_dir = crate::config::global_dir()?.join("dsh");
            fs::create_dir_all(&settings_dir)
                .context("create AstraFlow-managed DSH settings directory")?;
            artifacts.push(dsh_patch(
                endpoint,
                model,
                &models.slots,
                &settings_dir.join("settings.yaml"),
            )?);
        }
        Harness::Claude => {
            artifacts.push(claude_settings_overlay(overlay)?);
        }
        Harness::Hermes => {
            let dir = tempfile::tempdir().context("create temporary Hermes managed overlay")?;
            if let Some(source) = env::var_os("HERMES_MANAGED_DIR").map(PathBuf::from)
                && source.is_dir()
            {
                copy_directory_contents(&source, dir.path())
                    .context("copy existing Hermes managed configuration")?;
            }
            let key_env = format!("ASTRAFLOW_HERMES_KEY_{:016X}", rand::random::<u64>());
            let key = overlay
                .values
                .get("ASTRAFLOW_MODELVERSE_API_KEY")
                .cloned()
                .ok_or_else(|| anyhow!("AstraFlow key is missing from Hermes environment"))?;
            overlay.values.insert(key_env.clone(), key);
            let path = dir.path().join("config.yaml");
            let mut config: Value = if path.is_file() {
                serde_yaml::from_slice(&fs::read(&path)?)
                    .context("parse existing Hermes managed config.yaml")?
            } else {
                json!({})
            };
            let root = config
                .as_object_mut()
                .ok_or_else(|| anyhow!("Hermes managed config.yaml must contain a mapping"))?;
            root.entry("_config_version").or_insert(json!(12));
            let model_config = root.entry("model").or_insert_with(|| json!({}));
            if !model_config.is_object() {
                *model_config = json!({});
            }
            let model_config = model_config.as_object_mut().expect("model is an object");
            model_config.insert("default".into(), json!(model));
            model_config.insert("provider".into(), json!("astraflow"));
            let providers = root.entry("providers").or_insert_with(|| json!({}));
            if !providers.is_object() {
                *providers = json!({});
            }
            providers
                .as_object_mut()
                .expect("providers is an object")
                .insert(
                    "astraflow".into(),
                    json!({
                        "name": "AstraFlow ModelVerse",
                        "base_url": format!("{}/v1", endpoint.trim_end_matches('/')),
                        "key_env": key_env,
                        "default_model": model,
                        "transport": "chat_completions"
                    }),
                );
            fs::write(&path, serde_json::to_vec_pretty(&config)?)
                .context("write temporary Hermes managed routing overlay")?;
            overlay.values.insert(
                "HERMES_MANAGED_DIR".into(),
                dir.path().to_string_lossy().into_owned(),
            );
            artifact_dirs.push(dir);
        }
        _ => {}
    }
    Ok(())
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_directory_contents(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn claude_settings_overlay(overlay: &Environment) -> Result<NamedTempFile> {
    let env: Map<String, Value> = overlay
        .values
        .iter()
        .filter(|(name, _)| {
            name.starts_with("ANTHROPIC_")
                || name.starts_with("CLAUDE_CODE_")
                || name.as_str() == "ASTRAFLOW_MODELVERSE_API_KEY"
        })
        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
        .collect();
    let model = overlay
        .values
        .get("ANTHROPIC_MODEL")
        .cloned()
        .ok_or_else(|| anyhow!("Claude model is missing from AstraFlow environment"))?;
    let mut file = NamedTempFile::new().context("create temporary Claude routing settings")?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&json!({
        "env": env,
        "model": model,
        "advisorModel": model,
        "teammateDefaultModel": model,
        "fallbackModel": [model]
    }))?)?;
    file.flush()?;
    Ok(file)
}

pub fn command_arguments(
    harness: Harness,
    endpoint: &str,
    model: &str,
    args: &[String],
    patch_path: Option<&Path>,
) -> Vec<String> {
    command_arguments_with_models(
        harness,
        endpoint,
        model,
        args,
        patch_path,
        &HarnessModelSettings::default(),
    )
}

pub fn command_arguments_with_models(
    harness: Harness,
    endpoint: &str,
    model: &str,
    args: &[String],
    patch_path: Option<&Path>,
    models: &HarnessModelSettings,
) -> Vec<String> {
    let base_url = format!("{}/v1", endpoint.trim_end_matches('/'));
    let mut configured = match harness {
        Harness::Codex => {
            let mut args = vec![
                "-c".into(),
                format!("model={}", toml_string(model)),
                "-c".into(),
                "model_provider=\"astraflow_managed\"".into(),
                "-c".into(),
                "model_providers.astraflow_managed.name=\"AstraFlow ModelVerse\"".into(),
                "-c".into(),
                format!(
                    "model_providers.astraflow_managed.base_url={}",
                    toml_string(&base_url)
                ),
                "-c".into(),
                "model_providers.astraflow_managed.env_key=\"ASTRAFLOW_MODELVERSE_API_KEY\"".into(),
                "-c".into(),
                "model_providers.astraflow_managed.wire_api=\"responses\"".into(),
                "-c".into(),
                "model_providers.astraflow_managed.requires_openai_auth=false".into(),
            ];
            if let Some(path) = patch_path {
                args.push("-c".into());
                args.push(format!(
                    "model_catalog_json={}",
                    toml_string(&path.display().to_string())
                ));
            }
            for (key, slot) in [
                ("review_model", "review"),
                ("agents.default_subagent_model", "agents"),
            ] {
                args.push("-c".into());
                args.push(format!(
                    "{key}={}",
                    toml_string(model_for_slot(&models.slots, slot, model))
                ));
            }
            args
        }
        Harness::Claude => {
            let mut configured = vec!["--setting-sources".into(), "user,project,local".into()];
            if let Some(path) = patch_path {
                configured.push("--settings".into());
                configured.push(path.display().to_string());
            }
            configured.push("--model".into());
            configured.push(model.into());
            configured
        }
        Harness::Grok => vec!["--model".into(), "astraflow".into()],
        Harness::Opencode => Vec::new(),
        Harness::Hermes => vec![
            "--model".into(),
            model.into(),
            "--provider".into(),
            "astraflow".into(),
        ],
        Harness::Pi | Harness::PrimeAgent => {
            let mut configured = vec![
                "--provider".into(),
                "astraflow-managed".into(),
                "--model".into(),
                model.into(),
            ];
            if !models.cycle.is_empty() {
                let mut cycle = vec![format!("astraflow-managed/{model}")];
                for item in &models.cycle {
                    let qualified = format!("astraflow-managed/{item}");
                    if !cycle
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(&qualified))
                    {
                        cycle.push(qualified);
                    }
                }
                configured.push("--models".into());
                configured.push(cycle.join(","));
            }
            configured
        }
        Harness::Dsh => patch_path
            .map(|path| {
                let mut profile = "headless";
                let mut passthrough = Vec::new();
                let mut index = 0;
                while index < args.len() {
                    if args[index] == "--profile" {
                        profile = args[index + 1].as_str();
                        index += 2;
                        continue;
                    }
                    if let Some(value) = args[index].strip_prefix("--profile=") {
                        profile = value;
                        index += 1;
                        continue;
                    }
                    passthrough.push(args[index].clone());
                    index += 1;
                }
                let mut configured = vec![
                    "--profile".into(),
                    profile.into(),
                    "--patch".into(),
                    path.display().to_string(),
                ];
                configured.extend(passthrough);
                configured
            })
            .unwrap_or_default(),
    };
    if harness != Harness::Dsh {
        configured.extend_from_slice(args);
    }
    configured
}

fn configure_grok(
    endpoint: &str,
    model: &str,
    models: &BTreeMap<String, String>,
    isolated_dir: Option<&Path>,
) -> Result<()> {
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
    let model_table = document["model"]
        .as_table_mut()
        .expect("model was initialized as a table");
    model_table.retain(|name, _| !name.starts_with("astraflow"));
    for (index, selected) in unique_models(model, models).into_iter().enumerate() {
        let mut astraflow = Table::new();
        astraflow["model"] = value(selected);
        astraflow["base_url"] = value(format!("{}/v1", endpoint.trim_end_matches('/')));
        astraflow["env_key"] = value("ASTRAFLOW_MODELVERSE_API_KEY");
        astraflow["api_backend"] = value("chat_completions");
        let alias = if index == 0 {
            "astraflow".to_owned()
        } else {
            format!("astraflow_slot_{index}")
        };
        model_table.insert(&alias, Item::Table(astraflow));
    }
    if !document.get("subagents").is_some_and(Item::is_table) {
        document["subagents"] = Item::Table(Table::new());
    }
    if !document["subagents"]
        .get("models")
        .is_some_and(Item::is_table)
    {
        document["subagents"]["models"] = Item::Table(Table::new());
    }
    for (agent, slot) in [
        ("general-purpose", "general"),
        ("explore", "explore"),
        ("plan", "plan"),
    ] {
        document["subagents"]["models"][agent] = value(model_for_slot(models, slot, model));
    }
    write_config(&path, document.to_string().as_bytes())
}

fn configure_pi_like(
    prime: bool,
    endpoint: &str,
    model: &str,
    cycle: &[String],
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
    let mut configured_models = vec![model.to_owned()];
    for item in cycle {
        if !configured_models
            .iter()
            .any(|value| value.eq_ignore_ascii_case(item))
        {
            configured_models.push(item.clone());
        }
    }
    let configured_models: Vec<Value> = configured_models
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "name": id,
                "input": ["text"],
                "contextWindow": 128000,
                "maxTokens": 16384
            })
        })
        .collect();
    providers.insert(
        "astraflow-managed".into(),
        json!({
            "baseUrl": format!("{}/v1", endpoint.trim_end_matches('/')),
            "api": "openai-completions",
            "apiKey": api_key_reference,
            "authHeader": true,
            "models": configured_models
        }),
    );
    write_config(&path, &serde_json::to_vec_pretty(&root)?)
}

fn codex_catalog(models: &[&str]) -> Result<Option<NamedTempFile>> {
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
    let custom: Vec<_> = models
        .iter()
        .copied()
        .filter(|model| !BUNDLED_MODELS_0_147.contains(model))
        .collect();
    if custom.is_empty() {
        return Ok(None);
    }
    let mut file = NamedTempFile::new().context("create temporary Codex model catalog")?;
    let catalog = json!({
        "models": custom.into_iter().map(|model| json!({
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
            "include_skills_usage_instructions": true,
            "include_plugin_usage_instructions": true,
            "include_apps_usage_instructions": true,
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
        })).collect::<Vec<_>>()
    });
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&catalog)?)?;
    file.flush()?;
    Ok(Some(file))
}

fn dsh_patch(
    endpoint: &str,
    model: &str,
    models: &BTreeMap<String, String>,
    settings_path: &Path,
) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new()?;
    let catalog = unique_models(model, models)
        .into_iter()
        .map(|id| format!("          - id: {}", yaml_string(id)))
        .collect::<Vec<_>>()
        .join("\n");
    let payload = format!(
        "- id: settings\n  config:\n    path: {}\n\n- id: agent-default-model\n  config:\n    provider: astraflow\n    model: {}\n\n- id: session-title-llm\n  config:\n    targetWords: 5\n    targetCjkCharacters: 10\n    maxInputBytes: 4096\n    maxOutputTokens: 64\n    timeoutMs: 60000\n    provider: astraflow\n    model: {}\n\n- id: compaction-basic\n  config:\n    summarizationProvider: astraflow\n    summarizationModel: {}\n\n- id: tool-subagent\n  config:\n    provider: spawn\n    toolName: subagent\n    backgroundMode: continuable\n    agentOptions:\n      provider: astraflow\n      model: {}\n\n- id: tool-subagent-fork\n  config:\n    provider: fork\n    toolName: subagent_fork\n    backgroundMode: one-shot\n    agentOptions:\n      provider: astraflow\n      model: {}\n\n- id: llm-pi-ai\n  config:\n    providers:\n      astraflow:\n        displayName: AstraFlow ModelVerse\n        apiKeyEnv: ASTRAFLOW_MODELVERSE_API_KEY\n        api: openai-completions\n        baseURL: {}\n        models:\n{}\n",
        yaml_string(&settings_path.to_string_lossy()),
        yaml_string(model),
        yaml_string(model_for_slot(models, "title", model)),
        yaml_string(model_for_slot(models, "compaction", model)),
        yaml_string(model_for_slot(models, "spawn", model)),
        yaml_string(model_for_slot(models, "fork", model)),
        yaml_string(&format!("{}/v1", endpoint.trim_end_matches('/'))),
        catalog,
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
            protocol: crate::cli::ProviderProtocol::All,
            kind: crate::config::ConfigKind::AstraflowKey,
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
                .any(|arg| arg == "model_provider=\"astraflow_managed\"")
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
            [
                "--setting-sources",
                "user,project,local",
                "--model",
                "claude-model"
            ]
        );

        let overlay = claude_settings_overlay(&env).unwrap();
        let settings: Value = serde_json::from_slice(&fs::read(overlay.path()).unwrap()).unwrap();
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "test-key");
        assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], cred.endpoint);
        assert_eq!(settings["env"]["CLAUDE_CODE_USE_BEDROCK"], "0");
        assert_eq!(settings["advisorModel"], "claude-model");
        assert_eq!(settings["fallbackModel"], json!(["claude-model"]));
    }

    #[test]
    fn claude_routes_every_picker_slot_independently() {
        let cred = credential();
        let models = BTreeMap::from([
            ("fable".into(), "claude-fable-slot".into()),
            ("opus".into(), "claude-opus-slot".into()),
            ("sonnet".into(), "claude-sonnet-slot".into()),
            ("haiku".into(), "claude-haiku-slot".into()),
            ("agents".into(), "claude-agent-slot".into()),
        ]);
        let env = environment_with_models(
            Harness::Claude,
            &cred.api_key,
            &cred.endpoint,
            "claude-main-slot",
            &models,
        );
        assert_eq!(env.values["ANTHROPIC_MODEL"], "claude-main-slot");
        assert_eq!(
            env.values["ANTHROPIC_DEFAULT_FABLE_MODEL"],
            "claude-fable-slot"
        );
        assert_eq!(
            env.values["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "claude-opus-slot"
        );
        assert_eq!(
            env.values["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "claude-sonnet-slot"
        );
        assert_eq!(
            env.values["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
            "claude-haiku-slot"
        );
        assert_eq!(
            env.values["CLAUDE_CODE_SUBAGENT_MODEL"],
            "claude-agent-slot"
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
        assert_eq!(config["model"], "astraflow-managed/chat-model");
        assert_eq!(
            config["provider"]["astraflow-managed"]["models"]["chat-model"]["name"],
            "chat-model"
        );
        assert!(config.get("plugin").is_none());
        assert!(!env.values.contains_key("OPENCODE_DISABLE_PROJECT_CONFIG"));
        assert!(!env.values.contains_key("OPENCODE_DISABLE_DEFAULT_PLUGINS"));
        assert!(!env.values.contains_key("OPENCODE_DISABLE_EXTERNAL_SKILLS"));
        assert!(!env.values.contains_key("OPENCODE_PURE"));
        let args = command_arguments(
            Harness::Opencode,
            &cred.endpoint,
            "chat-model",
            &["run".into(), "hello".into()],
            None,
        );
        assert_eq!(&args, &["run", "hello"]);

        let merged = opencode_config_content(
            "https://api.modelverse.cn/v1",
            "chat-model",
            &BTreeMap::new(),
            Some(
                r#"{"plugin":["user-plugin"],"theme":"user-theme","agent":{"build":{"tools":{"bash":false},"model":"hostile/model"}},"provider":{"user-provider":{"name":"User"},"astraflow":{"options":{"baseURL":"http://127.0.0.1:9/v1"}}}}"#,
            ),
        );
        assert_eq!(merged["plugin"], json!(["user-plugin"]));
        assert_eq!(merged["theme"], "user-theme");
        assert_eq!(merged["agent"]["build"]["tools"]["bash"], false);
        assert_eq!(
            merged["agent"]["build"]["model"],
            "astraflow-managed/chat-model"
        );
        assert_eq!(merged["provider"]["user-provider"]["name"], "User");
        assert_eq!(
            merged["provider"]["astraflow-managed"]["options"]["baseURL"],
            "https://api.modelverse.cn/v1"
        );
    }

    #[test]
    fn opencode_registers_and_routes_all_agent_models() {
        let cred = credential();
        let models = BTreeMap::from([
            ("small".into(), "small-slot".into()),
            ("build".into(), "build-slot".into()),
            ("plan".into(), "plan-slot".into()),
            ("general".into(), "general-slot".into()),
            ("explore".into(), "explore-slot".into()),
            ("compaction".into(), "compact-slot".into()),
            ("title".into(), "title-slot".into()),
            ("summary".into(), "summary-slot".into()),
        ]);
        let env = environment_with_models(
            Harness::Opencode,
            &cred.api_key,
            &cred.endpoint,
            "main-slot",
            &models,
        );
        let config: Value = serde_json::from_str(&env.values["OPENCODE_CONFIG_CONTENT"]).unwrap();
        assert_eq!(config["small_model"], "astraflow-managed/small-slot");
        assert_eq!(
            config["agent"]["compaction"]["model"],
            "astraflow-managed/compact-slot"
        );
        for id in ["main-slot", "small-slot", "build-slot", "summary-slot"] {
            assert_eq!(
                config["provider"]["astraflow-managed"]["models"][id]["name"],
                id
            );
        }
    }

    #[test]
    fn pi_supports_legacy_and_current_environment_resolution() {
        let cred = credential();
        let env = environment(Harness::Pi, &cred.api_key, &cred.endpoint, "chat-model");
        assert_eq!(env.values["ASTRAFLOW_MODELVERSE_API_KEY"], "test-key");
        assert_eq!(env.values["$ASTRAFLOW_MODELVERSE_API_KEY"], "test-key");
    }

    #[test]
    fn pi_cycle_pool_is_registered_and_passed_to_the_real_cli_contract() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("models.json"),
            br#"{"theme":"user-theme","providers":{"user-provider":{"models":[]}}}"#,
        )
        .unwrap();
        configure_pi_like(
            false,
            "https://api.modelverse.cn",
            "main-slot",
            &["cycle-a".into(), "cycle-b".into()],
            Some(dir.path()),
        )
        .unwrap();
        let config: Value =
            serde_json::from_slice(&fs::read(dir.path().join("models.json")).unwrap()).unwrap();
        assert_eq!(
            config["providers"]["astraflow-managed"]["models"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(config["theme"], "user-theme");
        assert!(config["providers"]["user-provider"].is_object());
        let models = HarnessModelSettings {
            slots: BTreeMap::from([("default".into(), "main-slot".into())]),
            cycle: vec!["cycle-a".into(), "cycle-b".into()],
        };
        let args = command_arguments_with_models(
            Harness::Pi,
            "https://api.modelverse.cn",
            "main-slot",
            &[],
            None,
            &models,
        );
        assert!(args.windows(2).any(|pair| {
            pair == [
                "--models",
                "astraflow-managed/main-slot,astraflow-managed/cycle-a,astraflow-managed/cycle-b",
            ]
        }));
    }

    #[test]
    fn user_customization_homes_are_preserved_while_route_values_are_managed() {
        let cred = credential();
        for (harness, home) in [
            (Harness::Codex, "CODEX_HOME"),
            (Harness::Grok, "GROK_HOME"),
            (Harness::Opencode, "OPENCODE_CONFIG_DIR"),
            (Harness::Hermes, "HERMES_HOME"),
            (Harness::Pi, "PI_CODING_AGENT_DIR"),
            (Harness::PrimeAgent, "PRIME_AGENT_CODING_AGENT_DIR"),
            (Harness::Dsh, "DSH_HOME"),
        ] {
            let env = environment(harness, &cred.api_key, &cred.endpoint, "chat-model");
            assert!(!env.removed.iter().any(|name| name == home), "{harness:?}");
        }

        let mut hermes = environment(Harness::Hermes, &cred.api_key, &cred.endpoint, "chat-model");
        assert_eq!(hermes.values["HERMES_INFERENCE_PROVIDER"], "astraflow");
        assert!(!hermes.values.contains_key("HERMES_SAFE_MODE"));
        let mut artifacts = Vec::new();
        let mut artifact_dirs = Vec::new();
        prepare_configuration(
            Harness::Hermes,
            &cred.endpoint,
            "chat-model",
            &HarnessModelSettings::default(),
            &mut hermes,
            &mut artifacts,
            &mut artifact_dirs,
        )
        .unwrap();
        let managed_dir = Path::new(&hermes.values["HERMES_MANAGED_DIR"]);
        let managed: Value =
            serde_json::from_slice(&fs::read(managed_dir.join("config.yaml")).unwrap()).unwrap();
        let key_env = managed["providers"]["astraflow"]["key_env"]
            .as_str()
            .unwrap();
        assert!(key_env.starts_with("ASTRAFLOW_HERMES_KEY_"));
        assert_eq!(hermes.values[key_env], "test-key");
        assert_eq!(
            managed["providers"]["astraflow"]["base_url"],
            "https://api.modelverse.cn/v1"
        );

        let opencode = environment(
            Harness::Opencode,
            &cred.api_key,
            &cred.endpoint,
            "chat-model",
        );
        assert!(
            !opencode
                .removed
                .iter()
                .any(|name| name == "OPENCODE_DISABLE_EXTERNAL_SKILLS")
        );
    }

    #[test]
    fn codex_picker_routes_review_and_subagent_models() {
        let models = HarnessModelSettings {
            slots: BTreeMap::from([
                ("review".into(), "review-slot".into()),
                ("agents".into(), "agent-slot".into()),
            ]),
            cycle: Vec::new(),
        };
        let args = command_arguments_with_models(
            Harness::Codex,
            "https://api.modelverse.cn",
            "main-slot",
            &[],
            None,
            &models,
        );
        assert!(args.iter().any(|arg| arg == "review_model=\"review-slot\""));
        assert!(
            args.iter()
                .any(|arg| arg == "agents.default_subagent_model=\"agent-slot\"")
        );
    }

    #[test]
    fn routing_conflicts_are_rejected_before_launch() {
        assert!(
            validate_passthrough_args(
                Harness::Claude,
                &["--model".into(), "hostile".into(), "--print".into()]
            )
            .is_err()
        );
        assert!(
            validate_passthrough_args(Harness::Claude, &["--settings=hostile.json".into()])
                .is_err()
        );
        assert!(
            validate_passthrough_args(
                Harness::Codex,
                &["exec".into(), "--model=hostile".into(), "hello".into()]
            )
            .is_err()
        );
        assert!(
            validate_passthrough_args(Harness::Pi, &["--extension".into(), "trusted.ts".into()])
                .is_ok()
        );
        assert!(
            validate_passthrough_args(Harness::Pi, &["--print".into(), "hello".into()]).is_ok()
        );
    }

    #[test]
    fn unknown_codex_model_gets_a_text_only_catalog() {
        assert!(codex_catalog(&["gpt-5.6-sol"]).unwrap().is_none());
        let catalog = codex_catalog(&["future-responses-model"]).unwrap().unwrap();
        let value: Value = serde_json::from_slice(&fs::read(catalog.path()).unwrap()).unwrap();
        assert_eq!(value["models"][0]["slug"], "future-responses-model");
        assert_eq!(value["models"][0]["input_modalities"], json!(["text"]));
        assert_eq!(value["models"][0]["use_responses_lite"], false);
        assert_eq!(
            value["models"][0]["include_skills_usage_instructions"],
            true
        );
        assert_eq!(
            value["models"][0]["include_plugin_usage_instructions"],
            true
        );
    }

    #[test]
    fn grok_replaces_stale_same_name_credentials() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            "theme='user-theme'\n[model.user]\nmodel='user-model'\n[model.astraflow]\napi_key='hostile'\n[model.astraflow.extra_headers]\nAuthorization='Bearer hostile'\n",
        )
        .unwrap();
        configure_grok(
            "https://api.modelverse.cn",
            "chat-model",
            &BTreeMap::new(),
            Some(dir.path()),
        )
        .unwrap();
        let document = fs::read_to_string(dir.path().join("config.toml"))
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        let provider = document["model"]["astraflow"].as_table().unwrap();
        assert_eq!(provider["model"].as_str(), Some("chat-model"));
        assert!(provider.get("api_key").is_none());
        assert!(provider.get("extra_headers").is_none());
        assert_eq!(document["theme"].as_str(), Some("user-theme"));
        assert_eq!(
            document["model"]["user"]["model"].as_str(),
            Some("user-model")
        );

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
    fn grok_registers_and_routes_auxiliary_and_subagent_models() {
        let dir = tempfile::tempdir().unwrap();
        let models = BTreeMap::from([
            ("summary".into(), "summary-slot".into()),
            ("prompt".into(), "prompt-slot".into()),
            ("general".into(), "general-slot".into()),
            ("explore".into(), "explore-slot".into()),
            ("plan".into(), "plan-slot".into()),
        ]);
        configure_grok(
            "https://api.modelverse.cn",
            "main-slot",
            &models,
            Some(dir.path()),
        )
        .unwrap();
        let document = fs::read_to_string(dir.path().join("config.toml"))
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(
            document["subagents"]["models"]["general-purpose"].as_str(),
            Some("general-slot")
        );
        assert!(
            document["model"]
                .as_table()
                .unwrap()
                .iter()
                .any(|(_, item)| item["model"].as_str() == Some("summary-slot"))
        );
        let cred = credential();
        let env = environment_with_models(
            Harness::Grok,
            &cred.api_key,
            &cred.endpoint,
            "main-slot",
            &models,
        );
        assert_eq!(env.values["GROK_SESSION_SUMMARY_MODEL"], "summary-slot");
        assert_eq!(env.values["GROK_PROMPT_SUGGESTIONS_MODEL"], "prompt-slot");
    }

    #[test]
    fn dsh_preserves_home_enables_settings_and_rejects_route_overrides() {
        let models = BTreeMap::from([
            ("title".into(), "title-slot".into()),
            ("compaction".into(), "compact-slot".into()),
            ("spawn".into(), "spawn-slot".into()),
            ("fork".into(), "fork-slot".into()),
        ]);
        let patch = dsh_patch(
            "https://api.modelverse.cn",
            "chat-model",
            &models,
            Path::new("/tmp/astraflow/dsh/settings.yaml"),
        )
        .unwrap();
        let content = fs::read_to_string(patch.path()).unwrap();
        assert!(content.contains("- id: settings\n  config:\n    path:"));
        assert!(!content.contains("disabled: true"));
        assert!(content.contains("/tmp/astraflow/dsh/settings.yaml"));
        assert!(content.contains("model: \"title-slot\""));
        assert!(content.contains("summarizationModel: \"compact-slot\""));
        assert!(content.contains("model: \"spawn-slot\""));
        assert!(content.contains("model: \"fork-slot\""));
        let args = command_arguments(
            Harness::Dsh,
            "https://api.modelverse.cn",
            "chat-model",
            &["hello".into()],
            Some(patch.path()),
        );
        assert_eq!(&args[..3], ["--profile", "headless", "--patch"]);
        let web_args = command_arguments(
            Harness::Dsh,
            "https://api.modelverse.cn",
            "chat-model",
            &["--profile".into(), "web".into()],
            Some(patch.path()),
        );
        assert_eq!(&web_args[..3], ["--profile", "web", "--patch"]);
        assert!(validate_passthrough_args(Harness::Dsh, &["--profile=web".into()]).is_ok());
        assert!(
            validate_passthrough_args(
                Harness::Dsh,
                &["--profile".into(), "headless".into(), "run tests".into()]
            )
            .is_ok()
        );
        assert!(
            validate_passthrough_args(Harness::Dsh, &["--profile".into(), "custom".into()])
                .is_err()
        );
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
        assert!(!env.values.contains_key("DSH_HOME"));
        assert!(!env.removed.iter().any(|name| name == "DSH_HOME"));
    }

    #[test]
    fn codex_falls_back_to_the_shared_chat_model() {
        let mut cred = credential();
        cred.models.responses = None;
        assert_eq!(
            selected_model(Harness::Codex, &cred, None).unwrap(),
            "chat-model"
        );
    }

    #[test]
    fn unknown_harness_is_rejected() {
        assert!(Harness::parse("unsafe-custom-command").is_err());
    }
}
