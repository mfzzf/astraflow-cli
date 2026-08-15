use anyhow::{Context, Result, anyhow, bail};
use astraflow::cli::{
    Cli, Command as CliCommand, CompletionShell, HarnessCommand, HarnessLaunchArgs, Language,
    ModelVerseRegion,
};
use astraflow::config::{self, Credential, OAuthProvider, ResolvedCredential};
use astraflow::harness::{self, Harness};
use astraflow::i18n::{BANNER, Messages};
use astraflow::output::OutputMode;
use astraflow::{modelverse, oauth, proxy, ucloud};
use clap::{CommandFactory, Parser, error::ErrorKind};
use clap_complete::generate;
use dialoguer::{Input, Select, theme::ColorfulTheme};
use is_terminal::IsTerminal;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[tokio::main]
async fn main() {
    let raw_args: Vec<_> = env::args_os().collect();
    let force_json = raw_args
        .iter()
        .any(|arg| arg == "--json" || arg == "--agent");
    let cli = match Cli::try_parse_from(&raw_args) {
        Ok(cli) => cli,
        Err(error) if force_json => {
            let informational = matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let mut payload = json!({"ok": informational});
            payload[if informational { "output" } else { "error" }] =
                Value::String(error.to_string());
            println!("{payload}");
            std::process::exit(if informational { 0 } else { 2 });
        }
        Err(error) => error.exit(),
    };
    let mode = OutputMode::resolve(cli.json, cli.human);
    init_tracing(cli.log_level);
    let exit = match run(cli, mode).await {
        Ok(code) => code,
        Err(error) => {
            match mode {
                OutputMode::Json => {
                    println!("{}", json!({"ok": false, "error": error.to_string()}))
                }
                OutputMode::Human => eprintln!("error: {error:#}"),
            }
            1
        }
    };
    std::process::exit(exit);
}

async fn run(cli: Cli, mode: OutputMode) -> Result<i32> {
    if let Some(shell) = cli.completions {
        print_completions(shell);
        return Ok(0);
    }

    let command = if cli.command.is_none() && cli.wizard {
        Some(CliCommand::Login(Default::default()))
    } else {
        cli.command
    };
    let explicit_language = cli.lang;
    let mut language = explicit_language
        .or(config::load_language()?)
        .unwrap_or(Language::En);
    if matches!(&command, Some(CliCommand::Login(_)))
        && explicit_language.is_none()
        && config::load_language()?.is_none()
        && mode == OutputMode::Human
        && io::stdin().is_terminal()
    {
        let choice = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Language / 语言")
            .items(&["English", "中文"])
            .default(0)
            .interact()?;
        language = if choice == 1 {
            Language::Zh
        } else {
            Language::En
        };
    }
    if explicit_language.is_some() || matches!(&command, Some(CliCommand::Login(_))) {
        config::save_language(language)?;
    }
    let messages = Messages::new(language);
    let cwd = env::current_dir()?;

    let Some(command) = command else {
        print_help(None, mode)?;
        return Ok(0);
    };
    match command {
        CliCommand::Help { command } => print_help(command.as_deref(), mode).map(|_| 0),
        CliCommand::Login(args) => login(args, mode, messages, &cwd).await.map(|_| 0),
        CliCommand::Auth => auth(mode, &cwd),
        CliCommand::Claude(args) => launch(Harness::Claude, args, &cwd).await,
        CliCommand::Codex(args) => launch(Harness::Codex, args, &cwd).await,
        CliCommand::Grok(args) => launch(Harness::Grok, args, &cwd).await,
        CliCommand::Opencode(args) => launch(Harness::Opencode, args, &cwd).await,
        CliCommand::Hermes(args) => launch(Harness::Hermes, args, &cwd).await,
        CliCommand::Pi(args) => launch(Harness::Pi, args, &cwd).await,
        CliCommand::Dsh(args) => launch(Harness::Dsh, args, &cwd).await,
        CliCommand::PrimeAgent(args) => launch(Harness::PrimeAgent, args, &cwd).await,
        CliCommand::HarnessDoctor => harness_doctor(mode, &cwd),
        CliCommand::Workspace(args) => workspace(mode, &cwd, args.repair),
        CliCommand::VaultTunnel(args) => vault_tunnel(mode, &cwd, args.listen, args.exec).await,
        CliCommand::Harness(args) => harness_command(mode, &cwd, args.command).await,
        CliCommand::Eval(args) => eval(mode, &cwd, args).await,
        CliCommand::Changelog { query } => changelog(mode, query.as_deref()),
        CliCommand::Update(args) => update(mode, args).await,
        CliCommand::Version => version(mode),
        CliCommand::Probe(args) => probe(args.live, args.model.as_deref()).await,
    }
}

async fn login(
    args: astraflow::cli::LoginArgs,
    mode: OutputMode,
    messages: Messages,
    cwd: &Path,
) -> Result<()> {
    let client = http_client()?;
    if mode == OutputMode::Human {
        println!("{BANNER}");
    }
    let path = config::credential_path(args.local, cwd)?;
    let region = select_region(args.region, mode)?;

    if let Some(input) = args.with_key {
        let raw = if input == "-" {
            let mut value = String::new();
            io::stdin().read_to_string(&mut value)?;
            value
        } else {
            input
        };
        if raw.trim().is_empty() {
            bail!("the ModelVerse API key is empty");
        }
        let mut credential = config::imported(raw);
        credential.project_id = args.project_id;
        credential.region = region;
        credential.endpoint = modelverse_endpoint(region);
        let available = modelverse::list_models(&client, &credential.endpoint, &credential.api_key)
            .await
            .context("the imported key could not be validated")?;
        credential.models = modelverse::select_models(&available, &[]);
        let model = credential
            .models
            .chat_completions
            .clone()
            .ok_or_else(|| anyhow!("the imported key has no Chat Completions model"))?;
        config::save_credential(&path, &credential)?;
        return mode.print(
            format!("{} Validated with {model}.", messages.ready()),
            &json!({
                "ok": true,
                "source": "imported",
                "path": path,
                "region": credential.region,
                "endpoint": credential.endpoint,
                "models": credential.models,
                "validated_model": model
            }),
        );
    }

    let provider = if args.global {
        OAuthProvider::UcloudGlobal
    } else {
        OAuthProvider::Ucloud
    };
    let flow = oauth::begin(provider).await?;
    if mode == OutputMode::Human {
        eprintln!("{}", messages.opening_browser());
        eprintln!("{}\n{}", messages.browser_url(), flow.authorization_url);
    } else {
        eprintln!("oauth_url={}", flow.authorization_url);
    }
    if !args.no_open {
        let _ = webbrowser::open(&flow.authorization_url);
    }
    let callback_url = if let Some(url) = args.callback_url {
        Some(url)
    } else if args.no_open && io::stdin().is_terminal() {
        Some(
            Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt(messages.paste_callback())
                .interact_text()?,
        )
    } else {
        None
    };
    let tokens = oauth::finish(&client, flow, callback_url.as_deref()).await?;
    let endpoint = env::var("ASTRAFLOW_UCLOUD_API_ENDPOINT")
        .unwrap_or_else(|_| provider.api_endpoint().to_owned());
    let control = ucloud::OAuthControlPlane::new(
        client.clone(),
        endpoint,
        tokens.token_type.clone(),
        tokens.access_token.clone(),
    );
    let projects = control.projects().await?;
    let project = ucloud::default_project(&projects)
        .ok_or_else(|| anyhow!("no UCloud project is available"))?
        .clone();
    let mut keys = control.api_keys(&project.id).await?;
    if let Some(name) = args.create_key.as_deref() {
        keys.push(control.create_astraflow_api_key(&project.id, name).await?);
    } else if keys.is_empty() {
        keys.push(
            control
                .create_astraflow_api_key(&project.id, "AstraFlow Agent")
                .await?,
        );
    }
    let selected = select_api_key(&keys, args.key_id.as_deref(), mode, messages)?;
    if selected.key.trim().is_empty() {
        bail!("the selected API key did not include key material");
    }
    let credential = Credential {
        api_key: SecretString::from(selected.key.clone()),
        key_id: Some(selected.id.clone()),
        key_name: Some(selected.name.clone()),
        project_id: Some(project.id.clone()),
        endpoint: modelverse_endpoint(region),
        region,
        models: Default::default(),
        oauth: Some(tokens.clone()),
    };
    let available = modelverse::list_models(&client, &credential.endpoint, &credential.api_key)
        .await
        .context("the selected key could not list ModelVerse models")?;
    let available_ids = modelverse::model_ids(&available);
    let catalog = match control.square_models(&project.id, &available_ids).await {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!(
                "warning: ModelVerse catalog unavailable; using model-name protocol fallback: {error}"
            );
            Vec::new()
        }
    };
    let mut credential = credential;
    credential.models = modelverse::select_models(&available, &catalog);
    config::save_credential(&path, &credential)?;
    mode.print(
        format!(
            "{} {} · {} ({})",
            messages.ready(),
            project.name,
            selected.name,
            selected.id
        ),
        &json!({
            "ok": true,
            "project": {"id": project.id, "name": project.name},
            "api_key": {"id": selected.id, "name": selected.name},
            "email": tokens.email,
            "region": credential.region,
            "endpoint": credential.endpoint,
            "models": credential.models,
            "path": path
        }),
    )
}

fn select_region(
    requested: Option<ModelVerseRegion>,
    mode: OutputMode,
) -> Result<ModelVerseRegion> {
    if let Some(region) = requested {
        return Ok(region);
    }
    if mode == OutputMode::Json || !io::stdin().is_terminal() {
        return Ok(ModelVerseRegion::China);
    }
    let labels: Vec<_> = ModelVerseRegion::ALL
        .iter()
        .map(|region| region.label())
        .collect();
    let selected = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("ModelVerse region / 地域")
        .items(&labels)
        .default(0)
        .interact()?;
    Ok(ModelVerseRegion::ALL[selected])
}

fn modelverse_endpoint(region: ModelVerseRegion) -> String {
    env::var("ASTRAFLOW_MODELVERSE_ENDPOINT")
        .unwrap_or_else(|_| region.endpoint().to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn select_api_key<'a>(
    keys: &'a [ucloud::ApiKey],
    requested: Option<&str>,
    mode: OutputMode,
    messages: Messages,
) -> Result<&'a ucloud::ApiKey> {
    if let Some(id) = requested {
        return keys
            .iter()
            .find(|key| key.id == id)
            .ok_or_else(|| anyhow!("ModelVerse API key {id} was not found"));
    }
    if keys.len() == 1 || mode == OutputMode::Json || !io::stdin().is_terminal() {
        return keys
            .first()
            .ok_or_else(|| anyhow!("no ModelVerse API key is available"));
    }
    let items: Vec<_> = keys
        .iter()
        .map(|key| format!("{} ({})", key.name, key.id))
        .collect();
    let selected = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(messages.choose_key())
        .items(&items)
        .default(0)
        .interact()?;
    Ok(&keys[selected])
}

fn auth(mode: OutputMode, cwd: &Path) -> Result<i32> {
    let resolved = config::resolve(cwd)?;
    match resolved {
        Some(ResolvedCredential { credential, source }) => {
            mode.print(
                format!("Authenticated via {source:?}."),
                &json!({
                    "ok": true,
                    "authenticated": true,
                    "source": source,
                    "project_id": credential.project_id,
                    "key_id": credential.key_id,
                    "key_name": credential.key_name,
                    "endpoint": credential.endpoint,
                    "region": credential.region,
                    "models": credential.models
                }),
            )?;
            Ok(0)
        }
        None => {
            mode.print(
                "Not authenticated. Run `astraflow login`.",
                &json!({"ok": false, "authenticated": false}),
            )?;
            Ok(1)
        }
    }
}

async fn launch(harness: Harness, args: HarnessLaunchArgs, cwd: &Path) -> Result<i32> {
    let credential = require_credential(cwd)?;
    let status = harness::launch(
        harness,
        &credential,
        args.binary.as_deref(),
        &args.args,
        args.model.as_deref(),
    )
    .await?;
    Ok(status.code().unwrap_or(1))
}

fn harness_doctor(mode: OutputMode, cwd: &Path) -> Result<i32> {
    let credential = config::resolve(cwd)?.map(|resolved| resolved.credential);
    let results: Vec<_> = Harness::ALL
        .into_iter()
        .map(|item| harness::inspect(item, credential.as_ref()))
        .collect();
    let installed = results.iter().filter(|item| item.installed).count();
    let human = format!(
        "Credential: {}\nInstalled harnesses: {installed}/{}\n{}",
        if credential.is_some() {
            "ready"
        } else {
            "missing"
        },
        results.len(),
        results
            .iter()
            .map(|item| format!(
                "  {:12} {}",
                item.executable,
                item.path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "not installed".into())
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    mode.print(
        human,
        &json!({"ok": credential.is_some(), "credential": credential.is_some(), "harnesses": results}),
    )?;
    Ok(if credential.is_some() { 0 } else { 1 })
}

fn workspace(mode: OutputMode, cwd: &Path, repair: bool) -> Result<i32> {
    let repaired = if repair {
        config::repair(cwd)?
    } else {
        Vec::new()
    };
    let global_dir = config::global_dir()?;
    let local_path = config::workspace_credentials_path(cwd);
    mode.print(
        format!(
            "Global workspace: {}\nLocal credential: {}\n{}",
            global_dir.display(),
            local_path.display(),
            if repair {
                "Permissions repaired."
            } else {
                "Use --repair to repair permissions."
            }
        ),
        &json!({
            "ok": true,
            "global_dir": global_dir,
            "local_credentials": local_path,
            "repaired": repaired
        }),
    )?;
    Ok(0)
}

async fn vault_tunnel(
    mode: OutputMode,
    cwd: &Path,
    listen: String,
    exec: Vec<String>,
) -> Result<i32> {
    let credential = require_credential(cwd)?;
    let tunnel = proxy::start(
        http_client()?,
        &listen,
        &credential.endpoint,
        credential.api_key.clone(),
    )
    .await?;
    let info = tunnel.info();
    mode.print(
        format!(
            "Vault tunnel: {}\nEphemeral token: {}\n{}",
            info.base_url, info.local_token, info.note
        ),
        &info,
    )?;
    if exec.is_empty() {
        tokio::signal::ctrl_c().await?;
        tunnel.stop().await?;
        return Ok(0);
    }
    let executable = which::which(&exec[0])
        .with_context(|| format!("{} is not installed or not on PATH", exec[0]))?;
    let harness = Harness::parse(
        executable
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("prime-agent"),
    )
    .unwrap_or(Harness::PrimeAgent);
    let tunnel_credential = Credential {
        api_key: tunnel.token.clone(),
        endpoint: format!("http://{}", tunnel.address),
        ..credential
    };
    let status = harness::launch(
        harness,
        &tunnel_credential,
        Some(&executable),
        &exec[1..],
        None,
    )
    .await?;
    tunnel.stop().await?;
    Ok(status.code().unwrap_or(1))
}

async fn harness_command(mode: OutputMode, cwd: &Path, command: HarnessCommand) -> Result<i32> {
    let credential = config::resolve(cwd)?.map(|resolved| resolved.credential);
    match command {
        HarnessCommand::List => {
            let items: Vec<_> = Harness::ALL
                .into_iter()
                .map(|item| harness::inspect(item, credential.as_ref()))
                .collect();
            let human = items
                .iter()
                .map(|item| {
                    format!(
                        "{:<12} {}",
                        item.executable,
                        if item.installed {
                            "installed"
                        } else {
                            "missing"
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            mode.print(human, &json!({"ok": true, "harnesses": items}))?;
            Ok(0)
        }
        HarnessCommand::Inspect { name } => {
            let item = harness::inspect(Harness::parse(&name)?, credential.as_ref());
            mode.print(format!("{item:#?}"), &json!({"ok": true, "harness": item}))?;
            Ok(0)
        }
        HarnessCommand::Test(args) => {
            let credential = credential.ok_or_else(|| anyhow!("run `astraflow login` first"))?;
            harness_test(
                mode,
                credential,
                Harness::parse(&args.name)?,
                args.live,
                args.model,
                args.verify_usage,
            )
            .await
        }
    }
}

async fn harness_test(
    mode: OutputMode,
    credential: Credential,
    harness_name: Harness,
    live: bool,
    model: Option<String>,
    verify_usage: bool,
) -> Result<i32> {
    let selected_model = harness::selected_model(harness_name, &credential, model.as_deref())?;
    let args = if live {
        harness_live_arguments(harness_name)
    } else {
        vec!["--version".to_owned()]
    };
    let output = harness::launch_capture(
        harness_name,
        &credential,
        None,
        &args,
        Some(&selected_model),
    )
    .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{} exited with {}: {}",
            harness_name.executable(),
            output.status,
            stderr.trim()
        );
    }
    let usage_detail = if verify_usage {
        let probe = modelverse::minimal_chat(
            &http_client()?,
            &credential.endpoint,
            &credential.api_key,
            &selected_model,
        )
        .await?;
        let request_id = probe
            .request_id
            .ok_or_else(|| anyhow!("the usage verification probe exposed no request ID"))?;
        Some(verify_usage_detail(&credential, &request_id).await?)
    } else {
        None
    };
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    mode.print(
        if live {
            "The real harness sent a live ModelVerse message successfully."
        } else {
            "The real installed harness loaded with AstraFlow configuration."
        },
        &json!({
            "ok": true,
            "harness": harness_name,
            "model": selected_model,
            "live": live,
            "stdout": stdout,
            "usage_detail": usage_detail
        }),
    )?;
    Ok(0)
}

fn harness_live_arguments(harness: Harness) -> Vec<String> {
    let prompt = "Reply with exactly: ASTRAFLOW_OK".to_owned();
    match harness {
        Harness::Claude => vec![
            "--print".into(),
            prompt,
            "--output-format".into(),
            "json".into(),
        ],
        Harness::Codex => vec![
            "exec".into(),
            "--skip-git-repo-check".into(),
            "--sandbox".into(),
            "read-only".into(),
            prompt,
        ],
        Harness::Grok => vec!["--single".into(), prompt],
        Harness::Opencode => vec!["run".into(), prompt, "--format".into(), "json".into()],
        Harness::Hermes => vec!["--oneshot".into(), prompt],
        Harness::Pi | Harness::PrimeAgent => vec!["--print".into(), prompt],
        Harness::Dsh => vec!["--profile".into(), "headless".into(), prompt],
    }
}

async fn verify_usage_detail(
    credential: &Credential,
    request_id: &str,
) -> Result<ucloud::UsageDetail> {
    let public_key = env::var("UCLOUD_PUBLIC_KEY")
        .context("UCLOUD_PUBLIC_KEY is required for --verify-usage")?;
    let private_key = env::var("UCLOUD_PRIVATE_KEY")
        .context("UCLOUD_PRIVATE_KEY is required for --verify-usage")?;
    let project_id = credential
        .project_id
        .clone()
        .or_else(|| env::var("UCLOUD_PROJECT_ID").ok())
        .ok_or_else(|| anyhow!("the selected credential has no UCloud project ID"))?;
    let endpoint = env::var("ASTRAFLOW_UCLOUD_API_ENDPOINT").ok();
    let client = http_client()?;
    let mut last_error = None;
    for attempt in 0..3 {
        match ucloud::get_request_log_detail(
            &client,
            endpoint.as_deref(),
            &public_key,
            &private_key,
            &project_id,
            request_id,
        )
        .await
        {
            Ok(detail) => return Ok(detail),
            Err(error) => last_error = Some(error),
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("request-log detail was unavailable")))
}

async fn probe(live: bool, model: Option<&str>) -> Result<i32> {
    let raw_key = env::var("ASTRAFLOW_MODELVERSE_API_KEY")
        .or_else(|_| env::var("OPENAI_API_KEY"))
        .or_else(|_| env::var("ANTHROPIC_AUTH_TOKEN"))
        .context("no injected AstraFlow credential was found")?;
    let endpoint = env::var("OPENAI_BASE_URL")
        .ok()
        .map(|value| {
            value
                .trim_end_matches('/')
                .trim_end_matches("/v1")
                .to_owned()
        })
        .or_else(|| env::var("ANTHROPIC_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.modelverse.cn".into());
    let key = SecretString::from(raw_key);
    let payload = if live {
        let client = http_client()?;
        let selected = match model {
            Some(model) => model.to_owned(),
            None => modelverse::choose_text_model(&client, &endpoint, &key).await?,
        };
        serde_json::to_value(modelverse::minimal_chat(&client, &endpoint, &key, &selected).await?)?
    } else {
        json!({
            "injected": true,
            "endpoint": endpoint,
            "credential_length": key.expose_secret().len()
        })
    };
    println!("{}", json!({"ok": true, "probe": payload}));
    Ok(0)
}

async fn eval(mode: OutputMode, cwd: &Path, args: astraflow::cli::EvalArgs) -> Result<i32> {
    let roots = if args.paths.is_empty() {
        vec![cwd.to_path_buf()]
    } else {
        args.paths
    };
    let mut files = Vec::new();
    for root in roots {
        discover_evals(&root, &mut files)?;
    }
    files.sort();
    files.dedup();
    if args.list || args.dry_run {
        return mode
            .print(
                files.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join("\n"),
                &json!({"ok": true, "dry_run": args.dry_run, "files": files, "command": ["bun", "test"]}),
            )
            .map(|_| 0);
    }
    if files.is_empty() {
        bail!("no *.eval.ts, *.eval.js, or *.eval.mts files found");
    }
    let credential = config::resolve(cwd)?.map(|resolved| resolved.credential);
    if credential.is_none() && !args.allow_no_key {
        bail!("no AstraFlow credential resolves; run `astraflow login`");
    }
    let bun = which::which("bun").context("bun is required to run eval files")?;
    let mut command = Command::new(bun);
    command.arg("test").args(&files);
    if let Some(credential) = credential {
        let selected_model = harness::selected_model(Harness::PrimeAgent, &credential, None)?;
        let overlay = harness::environment(
            Harness::PrimeAgent,
            &credential.api_key,
            &credential.endpoint,
            &selected_model,
        );
        for key in overlay.removed {
            command.env_remove(key);
        }
        command.envs(overlay.values);
    }
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;
    Ok(status.code().unwrap_or(1))
}

fn discover_evals(path: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if [".eval.ts", ".eval.js", ".eval.mts", ".eval.mjs"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
        {
            output.push(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        }
        return Ok(());
    }
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let child = entry.path();
        let name = entry.file_name();
        if entry.file_type()?.is_dir()
            && matches!(name.to_str(), Some(".git" | "node_modules" | "target"))
        {
            continue;
        }
        discover_evals(&child, output)?;
    }
    Ok(())
}

fn changelog(mode: OutputMode, query: Option<&str>) -> Result<i32> {
    let content = include_str!("../CHANGELOG.md");
    let filtered = match query {
        None => content.to_owned(),
        Some(query) => content
            .lines()
            .filter(|line| {
                line.to_ascii_lowercase()
                    .contains(&query.to_ascii_lowercase())
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    mode.print(
        filtered.clone(),
        &json!({"ok": true, "query": query, "content": filtered}),
    )?;
    Ok(0)
}

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/mfzzf/astraflow-cli/releases/latest";
const INSTALL_SH_URL: &str =
    "https://raw.githubusercontent.com/mfzzf/astraflow-cli/main/install.sh";
const INSTALL_PS1_URL: &str =
    "https://raw.githubusercontent.com/mfzzf/astraflow-cli/main/install.ps1";

async fn update(mode: OutputMode, args: astraflow::cli::UpdateArgs) -> Result<i32> {
    let url = args
        .manifest_url
        .unwrap_or_else(|| LATEST_RELEASE_URL.into());
    let payload: Value = http_client()?
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let latest = payload
        .get("tag_name")
        .and_then(Value::as_str)
        .map(|version| version.strip_prefix('v').unwrap_or(version))
        .ok_or_else(|| anyhow!("update service returned no release version"))?;
    let current = env!("CARGO_PKG_VERSION");
    let available = semver::Version::parse(latest)
        .with_context(|| format!("invalid release version `{latest}`"))?
        > semver::Version::parse(current).context("invalid current package version")?;
    if !args.check && available {
        install_release(latest, mode).await?;
    }
    mode.print(
        if available {
            format!("AstraFlow {latest} is available.")
        } else {
            format!("AstraFlow {current} is current.")
        },
        &json!({
            "ok": true,
            "current": current,
            "latest": latest,
            "update_available": available,
            "installed": !args.check && available,
            "source": url,
        }),
    )?;
    Ok(0)
}

async fn install_release(version: &str, mode: OutputMode) -> Result<()> {
    let script_url = if cfg!(windows) {
        INSTALL_PS1_URL
    } else {
        INSTALL_SH_URL
    };
    let script = http_client()?
        .get(script_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let mut file = tempfile::NamedTempFile::new().context("create temporary installer")?;
    file.write_all(&script)?;
    file.flush()?;

    let install_dir = env::current_exe()
        .context("locate the running astraflow executable")?
        .parent()
        .ok_or_else(|| anyhow!("the running astraflow executable has no parent directory"))?
        .to_owned();
    let mut command = if cfg!(windows) {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        command
    } else {
        Command::new("sh")
    };
    command
        .arg(file.path())
        .env("ASTRAFLOW_VERSION", version)
        .env("ASTRAFLOW_INSTALL_DIR", install_dir)
        .stdin(Stdio::inherit())
        .stdout(if mode == OutputMode::Json {
            Stdio::null()
        } else {
            Stdio::inherit()
        })
        .stderr(Stdio::inherit());
    let status = command.status().await.context("run release installer")?;
    if !status.success() {
        bail!("release installer exited with status {status}");
    }
    Ok(())
}

fn version(mode: OutputMode) -> Result<i32> {
    mode.print(
        format!(
            "astraflow {} ({}-{})",
            env!("CARGO_PKG_VERSION"),
            env::consts::OS,
            env::consts::ARCH
        ),
        &json!({
            "ok": true,
            "name": "astraflow",
            "version": env!("CARGO_PKG_VERSION"),
            "target": format!("{}-{}", env::consts::OS, env::consts::ARCH)
        }),
    )?;
    Ok(0)
}

fn print_help(subcommand: Option<&str>, mode: OutputMode) -> Result<()> {
    let mut command = Cli::command();
    let mut buffer = Vec::new();
    if let Some(name) = subcommand {
        let child = command
            .find_subcommand_mut(name)
            .ok_or_else(|| anyhow!("unknown command: {name}"))?;
        child.write_long_help(&mut buffer)?;
    } else {
        command.write_long_help(&mut buffer)?;
    }
    let help = String::from_utf8(buffer)?;
    mode.print(&help, &json!({"ok": true, "help": help}))
}

fn print_completions(shell: CompletionShell) {
    let mut command = Cli::command();
    match shell {
        CompletionShell::Bash => generate(
            clap_complete::shells::Bash,
            &mut command,
            "astraflow",
            &mut io::stdout(),
        ),
        CompletionShell::Zsh => generate(
            clap_complete::shells::Zsh,
            &mut command,
            "astraflow",
            &mut io::stdout(),
        ),
        CompletionShell::Fish => generate(
            clap_complete::shells::Fish,
            &mut command,
            "astraflow",
            &mut io::stdout(),
        ),
        CompletionShell::Sh => generate(
            clap_complete::shells::Bash,
            &mut command,
            "astraflow",
            &mut io::stdout(),
        ),
    }
}

fn require_credential(cwd: &Path) -> Result<Credential> {
    config::resolve(cwd)?
        .map(|resolved| resolved.credential)
        .ok_or_else(|| anyhow!("no AstraFlow credential resolves; run `astraflow login`"))
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .user_agent(concat!("astraflow/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

fn init_tracing(level: astraflow::cli::LogLevel) {
    let filter = match level {
        astraflow::cli::LogLevel::All | astraflow::cli::LogLevel::Trace => "trace",
        astraflow::cli::LogLevel::Debug => "debug",
        astraflow::cli::LogLevel::Info => "info",
        astraflow::cli::LogLevel::Warn | astraflow::cli::LogLevel::Warning => "warn",
        astraflow::cli::LogLevel::Error | astraflow::cli::LogLevel::Fatal => "error",
        astraflow::cli::LogLevel::None => "off",
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .try_init();
}
