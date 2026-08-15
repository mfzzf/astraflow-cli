use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Zh,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LogLevel {
    All,
    Trace,
    Debug,
    Info,
    Warn,
    Warning,
    Error,
    Fatal,
    None,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Sh,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelVerseRegion {
    /// Mainland China
    China,
    /// Singapore
    Singapore,
    /// Los Angeles
    LosAngeles,
    /// Frankfurt
    Frankfurt,
}

impl ModelVerseRegion {
    pub const ALL: [Self; 4] = [
        Self::China,
        Self::Singapore,
        Self::LosAngeles,
        Self::Frankfurt,
    ];

    pub fn endpoint(self) -> &'static str {
        match self {
            Self::China => "https://api.modelverse.cn",
            Self::Singapore => "https://api-sg.umodelverse.ai",
            Self::LosAngeles => "https://api-us-ca.umodelverse.ai",
            Self::Frankfurt => "https://api-ge-fra.umodelverse.ai",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::China => "China / 中国大陆",
            Self::Singapore => "Singapore / 新加坡",
            Self::LosAngeles => "Los Angeles / 洛杉矶",
            Self::Frankfurt => "Frankfurt / 法兰克福",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "astraflow",
    version,
    about = "The easiest way to use AstraFlow locally.",
    long_about = "AstraFlow signs you in, selects a ModelVerse API key, and launches local coding agents with the correct provider environment.",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Start wizard mode for a command
    #[arg(long, global = true)]
    pub wizard: bool,

    /// Print shell completion script
    #[arg(long, value_enum, exclusive = true)]
    pub completions: Option<CompletionShell>,

    /// Sets the minimum log level
    #[arg(long, value_enum, global = true, default_value = "warn")]
    pub log_level: LogLevel,

    /// Force machine JSON output
    #[arg(long, alias = "agent", global = true, conflicts_with = "human")]
    pub json: bool,

    /// Force human-readable output
    #[arg(long, alias = "tty", global = true)]
    pub human: bool,

    /// Interface language
    #[arg(long, value_enum, global = true)]
    pub lang: Option<Language>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show help for AstraFlow or another command
    Help {
        /// Command whose help should be shown
        command: Option<String>,
    },

    /// Sign in through the browser and select a ModelVerse API key
    Login(LoginArgs),

    /// Report whether an AstraFlow credential resolves and where it comes from
    Auth,

    /// Launch Claude Code with AstraFlow's ModelVerse environment
    Claude(HarnessLaunchArgs),

    /// Launch Codex CLI with AstraFlow's ModelVerse environment
    Codex(HarnessLaunchArgs),

    /// Launch Grok Build with AstraFlow's ModelVerse environment
    Grok(HarnessLaunchArgs),

    /// Launch OpenCode with AstraFlow's ModelVerse environment
    Opencode(HarnessLaunchArgs),

    /// Launch Hermes Agent with AstraFlow's ModelVerse environment
    Hermes(HarnessLaunchArgs),

    /// Launch Pi Agent with AstraFlow's ModelVerse environment
    Pi(HarnessLaunchArgs),

    /// Set up or launch DeepSeek Harness
    #[command(alias = "deepseek")]
    Dsh(HarnessLaunchArgs),

    /// Launch Prime Agent with AstraFlow's ModelVerse environment
    #[command(alias = "prime")]
    PrimeAgent(HarnessLaunchArgs),

    /// Troubleshoot conflicts that break harness launches
    HarnessDoctor,

    /// Inspect and repair the AstraFlow workspace
    Workspace(WorkspaceArgs),

    /// Keep the real ModelVerse key in a localhost reverse proxy
    VaultTunnel(VaultTunnelArgs),

    /// Inspect and test agent harnesses
    Harness(HarnessArgs),

    /// Run agent evaluation files through Bun
    Eval(EvalArgs),

    /// Browse the curated project changelog
    Changelog {
        /// Show entries containing this text
        query: Option<String>,
    },

    /// Check for or install an AstraFlow CLI update
    Update(UpdateArgs),

    /// Show detailed version information
    Version,

    /// Internal child-process injection probe
    #[command(hide = true, name = "_probe")]
    Probe(ProbeArgs),
}

#[derive(Debug, Default, Args)]
pub struct LoginArgs {
    /// Read a ModelVerse API key from the argument, or stdin when omitted
    #[arg(long, num_args = 0..=1, default_missing_value = "-", value_name = "KEY")]
    pub with_key: Option<String>,

    /// Save credentials in .astraflow for this workspace
    #[arg(long)]
    pub local: bool,

    /// Do not attempt to open a browser
    #[arg(long)]
    pub no_open: bool,

    /// Complete an SSH login using the redirected localhost URL
    #[arg(long)]
    pub callback_url: Option<String>,

    /// Select the global UCloud OAuth service
    #[arg(long)]
    pub global: bool,

    /// Select this key ID without prompting
    #[arg(long)]
    pub key_id: Option<String>,

    /// Create an AstraFlow key with this name
    #[arg(long, value_name = "NAME")]
    pub create_key: Option<String>,

    /// Associate an imported API key with a project
    #[arg(long)]
    pub project_id: Option<String>,

    /// ModelVerse access region
    #[arg(long, value_enum)]
    pub region: Option<ModelVerseRegion>,
}

#[derive(Debug, Args)]
pub struct HarnessLaunchArgs {
    /// Override the harness executable
    #[arg(long)]
    pub binary: Option<PathBuf>,

    /// Override the AstraFlow-selected model for this launch
    #[arg(long)]
    pub model: Option<String>,

    /// Arguments passed through to the harness
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Args)]
pub struct WorkspaceArgs {
    /// Repair directories and secret-file permissions
    #[arg(long)]
    pub repair: bool,
}

#[derive(Debug, Args)]
pub struct VaultTunnelArgs {
    /// Local listen address
    #[arg(long, default_value = "127.0.0.1:0")]
    pub listen: String,

    /// Start a harness behind the tunnel, then stop the tunnel on exit
    #[arg(long, num_args = 1.., allow_hyphen_values = true)]
    pub exec: Vec<String>,
}

#[derive(Debug, Args)]
pub struct HarnessArgs {
    #[command(subcommand)]
    pub command: HarnessCommand,
}

#[derive(Debug, Subcommand)]
pub enum HarnessCommand {
    /// List supported harnesses and installation state
    List,
    /// Show executable and environment details without revealing secrets
    Inspect { name: String },
    /// Run the real installed harness with AstraFlow injection
    Test(HarnessTestArgs),
}

#[derive(Debug, Args)]
pub struct HarnessTestArgs {
    /// Harness environment to test
    #[arg(default_value = "codex")]
    pub name: String,

    /// Send one minimal live message
    #[arg(long)]
    pub live: bool,

    /// Model to use for a live test; auto-selects when omitted
    #[arg(long)]
    pub model: Option<String>,

    /// Query UCloud for the request detail after the message
    #[arg(long, requires = "live")]
    pub verify_usage: bool,
}

#[derive(Debug, Args)]
pub struct EvalArgs {
    /// Eval files or directories; defaults to the current directory
    pub paths: Vec<PathBuf>,

    /// List matching evals without running them
    #[arg(long)]
    pub list: bool,

    /// Print the Bun command without running it
    #[arg(long)]
    pub dry_run: bool,

    /// Permit listing/dry-runs without a credential
    #[arg(long)]
    pub allow_no_key: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Only check whether an update exists
    #[arg(long)]
    pub check: bool,

    /// Override the GitHub-compatible release-manifest URL
    #[arg(long, env = "ASTRAFLOW_UPDATE_URL")]
    pub manifest_url: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProbeArgs {
    #[arg(long)]
    pub live: bool,

    #[arg(long)]
    pub model: Option<String>,
}
