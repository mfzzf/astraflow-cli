use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use tempfile::Builder;
use tokio::process::Command;

const RELEASE_REPOSITORY: &str = "mfzzf/astraflow-cli";
const MAX_BINARY_BYTES: usize = 256 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    Replaced,
    PendingProcessExit,
}

pub async fn install_release(
    client: &Client,
    version: &str,
    destination: &Path,
) -> Result<InstallState> {
    let base_url = env::var("ASTRAFLOW_RELEASE_BASE_URL").unwrap_or_else(|_| {
        format!("https://github.com/{RELEASE_REPOSITORY}/releases/download/v{version}")
    });
    install_release_from(client, version, destination, &base_url).await
}

#[doc(hidden)]
pub async fn install_release_from(
    client: &Client,
    version: &str,
    destination: &Path,
    base_url: &str,
) -> Result<InstallState> {
    let version = semver::Version::parse(version.trim_start_matches('v'))
        .with_context(|| format!("invalid release version `{version}`"))?;
    let asset = asset_name(env::consts::OS, env::consts::ARCH)?;
    let base_url = base_url.trim_end_matches('/');
    let checksum_bytes = download(
        client,
        &format!("{base_url}/SHA256SUMS"),
        MAX_CHECKSUM_BYTES,
    )
    .await
    .context("download release checksums")?;
    let checksum_text =
        std::str::from_utf8(&checksum_bytes).context("release checksum file is not valid UTF-8")?;
    let expected = checksum_for(checksum_text, &asset)?;
    let binary = download(client, &format!("{base_url}/{asset}"), MAX_BINARY_BYTES)
        .await
        .with_context(|| format!("download release asset {asset}"))?;
    let actual = format!("{:x}", Sha256::digest(&binary));
    if actual != expected {
        bail!("release checksum verification failed for {asset}");
    }

    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("the AstraFlow executable has no parent directory"))?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let mut staged = Builder::new()
        .prefix(".astraflow-update-")
        .suffix(suffix)
        .tempfile_in(parent)
        .with_context(|| format!("create update file in {}", parent.display()))?;
    staged.write_all(&binary)?;
    staged.as_file().sync_all()?;
    set_executable(staged.path())?;
    let (_, staged_path) = staged.keep().context("keep staged update binary")?;

    if let Err(error) = validate_binary(&staged_path, &version.to_string()).await {
        let _ = fs::remove_file(&staged_path);
        return Err(error);
    }

    replace_executable(&staged_path, destination)
}

async fn download(client: &Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        bail!("download from {url} exceeds the {limit}-byte safety limit");
    }
    let mut body = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(limit as u64) as usize,
    );
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > limit {
            bail!("download from {url} exceeds the {limit}-byte safety limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn checksum_for(contents: &str, asset: &str) -> Result<String> {
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let Some(digest) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        if fields.next().is_some() || name.trim_start_matches('*') != asset {
            continue;
        }
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("release checksum for {asset} is not a SHA256 digest");
        }
        return Ok(digest.to_ascii_lowercase());
    }
    bail!("release asset {asset} is missing from SHA256SUMS")
}

fn asset_name(os: &str, arch: &str) -> Result<String> {
    let arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => bail!("automatic updates do not support architecture {other}"),
    };
    let (platform, extension) = match os {
        "linux" => ("unknown-linux-gnu", ""),
        "macos" => ("apple-darwin", ""),
        "windows" => ("pc-windows-msvc", ".exe"),
        other => bail!("automatic updates do not support operating system {other}"),
    };
    Ok(format!("astraflow-{arch}-{platform}{extension}"))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(windows)]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

async fn validate_binary(path: &Path, expected_version: &str) -> Result<()> {
    let output = Command::new(path)
        .args(["--json", "version"])
        .env("ASTRAFLOW_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::null())
        .output()
        .await
        .context("execute staged AstraFlow binary")?;
    if !output.status.success() {
        bail!("staged AstraFlow binary failed its version check");
    }
    let payload: Value = serde_json::from_slice(&output.stdout)
        .context("staged AstraFlow binary returned invalid version JSON")?;
    if payload.get("version").and_then(Value::as_str) != Some(expected_version) {
        bail!("staged AstraFlow binary version does not match v{expected_version}");
    }
    Ok(())
}

#[cfg(unix)]
fn replace_executable(staged: &Path, destination: &Path) -> Result<InstallState> {
    fs::rename(staged, destination).with_context(|| {
        format!(
            "atomically replace {} (check directory permissions)",
            destination.display()
        )
    })?;
    if let Some(parent) = destination.parent() {
        fs::OpenOptions::new()
            .read(true)
            .open(parent)?
            .sync_all()
            .context("sync updated executable directory")?;
    }
    Ok(InstallState::Replaced)
}

#[cfg(windows)]
fn replace_executable(staged: &Path, destination: &Path) -> Result<InstallState> {
    // Windows locks the running executable. A fixed local command waits for this process to
    // exit and then moves the already-verified binary into place. No downloaded script runs.
    const REPLACE: &str = "$ErrorActionPreference='Stop'; Wait-Process -Id $env:ASTRAFLOW_UPDATE_PARENT_PID -Timeout 30 -ErrorAction SilentlyContinue; Move-Item -LiteralPath $env:ASTRAFLOW_UPDATE_SOURCE -Destination $env:ASTRAFLOW_UPDATE_DESTINATION -Force";
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", REPLACE])
        .env(
            "ASTRAFLOW_UPDATE_PARENT_PID",
            std::process::id().to_string(),
        )
        .env("ASTRAFLOW_UPDATE_SOURCE", staged)
        .env("ASTRAFLOW_UPDATE_DESTINATION", destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("schedule verified AstraFlow executable replacement")?;
    Ok(InstallState::PendingProcessExit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_assets_are_platform_specific_raw_binaries() {
        assert_eq!(
            asset_name("linux", "x86_64").unwrap(),
            "astraflow-x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            asset_name("macos", "aarch64").unwrap(),
            "astraflow-aarch64-apple-darwin"
        );
        assert_eq!(
            asset_name("windows", "x86_64").unwrap(),
            "astraflow-x86_64-pc-windows-msvc.exe"
        );
    }

    #[test]
    fn checksum_parser_requires_an_exact_sha256_asset_entry() {
        let digest = "a".repeat(64);
        let sums = format!("{digest}  astraflow-x86_64-unknown-linux-gnu\n");
        assert_eq!(
            checksum_for(&sums, "astraflow-x86_64-unknown-linux-gnu").unwrap(),
            digest
        );
        assert!(checksum_for(&sums, "astraflow-aarch64-unknown-linux-gnu").is_err());
        assert!(
            checksum_for(
                "not-a-hash  astraflow-x86_64-unknown-linux-gnu",
                "astraflow-x86_64-unknown-linux-gnu"
            )
            .is_err()
        );
    }
}
