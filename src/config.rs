use crate::cli::{Language, ModelVerseRegion};
use anyhow::{Context, Result, anyhow, bail};
use directories::ProjectDirs;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CREDENTIALS_FILE: &str = "credentials.json";
const SETTINGS_FILE: &str = "config.json";

#[derive(Debug, Clone)]
pub struct Credential {
    pub api_key: SecretString,
    pub key_id: Option<String>,
    pub key_name: Option<String>,
    pub project_id: Option<String>,
    pub endpoint: String,
    pub region: ModelVerseRegion,
    pub models: ModelSelection,
    pub oauth: Option<OAuthTokens>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelSelection {
    #[serde(default)]
    pub chat_completions: Option<String>,
    #[serde(default)]
    pub responses: Option<String>,
    #[serde(default)]
    pub anthropic: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthTokens {
    pub provider: OAuthProvider,
    pub access_token: SecretString,
    pub refresh_token: Option<SecretString>,
    pub token_type: String,
    pub expires_at: Option<u64>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OAuthProvider {
    Ucloud,
    UcloudGlobal,
}

impl OAuthProvider {
    pub fn api_endpoint(self) -> &'static str {
        match self {
            Self::Ucloud => "https://api.ucloud.cn/",
            Self::UcloudGlobal => "https://api.ucloud-global.com/",
        }
    }

    pub fn oauth_base_url(self) -> &'static str {
        match self {
            Self::Ucloud => "https://oauth2.ucloud.cn",
            Self::UcloudGlobal => "https://oauth2.ucloud-global.com",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    AstraFlowEnvironment,
    ModelVerseEnvironment,
    WorkspaceFile(PathBuf),
    GlobalFile(PathBuf),
}

#[derive(Debug, Clone)]
pub struct ResolvedCredential {
    pub credential: Credential,
    pub source: CredentialSource,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredCredential {
    version: u8,
    api_key: String,
    #[serde(default)]
    key_id: Option<String>,
    #[serde(default)]
    key_name: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default = "default_endpoint")]
    endpoint: String,
    #[serde(default = "default_region")]
    region: ModelVerseRegion,
    #[serde(default)]
    models: ModelSelection,
    #[serde(default)]
    oauth: Option<StoredOAuthTokens>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredOAuthTokens {
    provider: OAuthProvider,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default = "default_token_type")]
    token_type: String,
    #[serde(default)]
    expires_at: Option<u64>,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    language: Option<Language>,
}

fn default_endpoint() -> String {
    env::var("ASTRAFLOW_MODELVERSE_ENDPOINT")
        .unwrap_or_else(|_| "https://api.modelverse.cn".to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn default_region() -> ModelVerseRegion {
    ModelVerseRegion::China
}

fn default_token_type() -> String {
    "Bearer".to_owned()
}

pub fn global_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("ASTRAFLOW_HOME") {
        return Ok(PathBuf::from(path));
    }
    ProjectDirs::from("cn", "UCloud", "AstraFlow")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .ok_or_else(|| anyhow!("unable to locate the user configuration directory"))
}

pub fn global_credentials_path() -> Result<PathBuf> {
    Ok(global_dir()?.join(CREDENTIALS_FILE))
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(global_dir()?.join(SETTINGS_FILE))
}

pub fn workspace_credentials_path(start: &Path) -> PathBuf {
    start.join(".astraflow").join(CREDENTIALS_FILE)
}

fn find_workspace_credentials(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        let candidate = workspace_credentials_path(dir);
        if candidate.is_file() {
            return Some(candidate);
        }
        current = dir.parent();
    }
    None
}

pub fn credential_path(local: bool, cwd: &Path) -> Result<PathBuf> {
    if local {
        Ok(workspace_credentials_path(cwd))
    } else {
        global_credentials_path()
    }
}

pub fn resolve(cwd: &Path) -> Result<Option<ResolvedCredential>> {
    if let Ok(value) = env::var("ASTRAFLOW_API_KEY")
        && !value.trim().is_empty()
    {
        return Ok(Some(ResolvedCredential {
            credential: imported(value),
            source: CredentialSource::AstraFlowEnvironment,
        }));
    }
    if let Ok(value) = env::var("MODELVERSE_API_KEY")
        && !value.trim().is_empty()
    {
        return Ok(Some(ResolvedCredential {
            credential: imported(value),
            source: CredentialSource::ModelVerseEnvironment,
        }));
    }
    if let Some(path) = find_workspace_credentials(cwd) {
        return Ok(Some(ResolvedCredential {
            credential: read_credential(&path)?,
            source: CredentialSource::WorkspaceFile(path),
        }));
    }
    let path = global_credentials_path()?;
    if path.is_file() {
        return Ok(Some(ResolvedCredential {
            credential: read_credential(&path)?,
            source: CredentialSource::GlobalFile(path),
        }));
    }
    Ok(None)
}

pub fn imported(value: String) -> Credential {
    Credential {
        api_key: SecretString::from(value.trim().to_owned()),
        key_id: None,
        key_name: None,
        project_id: None,
        endpoint: default_endpoint(),
        region: default_region(),
        models: ModelSelection::default(),
        oauth: None,
    }
}

fn read_credential(path: &Path) -> Result<Credential> {
    ensure_safe_file(path)?;
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let stored: StoredCredential =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    if stored.version != 1 || stored.api_key.trim().is_empty() {
        bail!(
            "{} is not a valid AstraFlow credential file",
            path.display()
        );
    }
    Ok(Credential {
        api_key: SecretString::from(stored.api_key),
        key_id: stored.key_id,
        key_name: stored.key_name,
        project_id: stored.project_id,
        endpoint: stored.endpoint.trim_end_matches('/').to_owned(),
        region: stored.region,
        models: stored.models,
        oauth: stored.oauth.map(|oauth| OAuthTokens {
            provider: oauth.provider,
            access_token: SecretString::from(oauth.access_token),
            refresh_token: oauth.refresh_token.map(SecretString::from),
            token_type: oauth.token_type,
            expires_at: oauth.expires_at,
            email: oauth.email,
        }),
    })
}

pub fn save_credential(path: &Path, credential: &Credential) -> Result<()> {
    let stored = StoredCredential {
        version: 1,
        api_key: credential.api_key.expose_secret().to_owned(),
        key_id: credential.key_id.clone(),
        key_name: credential.key_name.clone(),
        project_id: credential.project_id.clone(),
        endpoint: credential.endpoint.clone(),
        region: credential.region,
        models: credential.models.clone(),
        oauth: credential.oauth.as_ref().map(|oauth| StoredOAuthTokens {
            provider: oauth.provider,
            access_token: oauth.access_token.expose_secret().to_owned(),
            refresh_token: oauth
                .refresh_token
                .as_ref()
                .map(|token| token.expose_secret().to_owned()),
            token_type: oauth.token_type.clone(),
            expires_at: oauth.expires_at,
            email: oauth.email.clone(),
        }),
    };
    write_private_json(path, &stored)
}

pub fn load_language() -> Result<Option<Language>> {
    let path = settings_path()?;
    if !path.is_file() {
        return Ok(None);
    }
    let settings: Settings = serde_json::from_slice(&fs::read(&path)?)?;
    Ok(settings.language)
}

pub fn save_language(language: Language) -> Result<()> {
    write_private_json(
        &settings_path()?,
        &Settings {
            language: Some(language),
        },
    )
}

pub fn repair(cwd: &Path) -> Result<Vec<PathBuf>> {
    let mut repaired = Vec::new();
    let global = global_dir()?;
    create_private_dir(&global)?;
    repaired.push(global.clone());
    for path in [global.join(CREDENTIALS_FILE), global.join(SETTINGS_FILE)] {
        if path.exists() {
            set_private_file_permissions(&path)?;
            repaired.push(path);
        }
    }
    if let Some(path) = find_workspace_credentials(cwd) {
        if let Some(parent) = path.parent() {
            create_private_dir(parent)?;
        }
        set_private_file_permissions(&path)?;
        repaired.push(path);
    }
    Ok(repaired)
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("credential path has no parent"))?;
    create_private_dir(parent)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let tmp = parent.join(format!(".astraflow-{nonce}.tmp"));
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    set_private_file_permissions(path)?;
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn ensure_safe_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            bail!("refusing symlinked credential file: {}", path.display());
        }
        if metadata.mode() & 0o077 != 0 {
            bail!(
                "credential file permissions are too broad: {}; run `astf workspace --repair`",
                path.display()
            );
        }
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            bail!("refusing symlinked credential file: {}", path.display());
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_path_is_scoped() {
        assert_eq!(
            workspace_credentials_path(Path::new("/tmp/example")),
            PathBuf::from("/tmp/example/.astraflow/credentials.json")
        );
    }

    #[test]
    fn private_file_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("credentials.json");
        let credential = imported("secret-test-key".to_owned());
        save_credential(&path, &credential).unwrap();
        let loaded = read_credential(&path).unwrap();
        assert_eq!(loaded.api_key.expose_secret(), "secret-test-key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
