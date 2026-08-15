use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn cli() -> Command {
    Command::cargo_bin("astraflow").unwrap()
}

#[test]
fn help_exposes_the_ori_compatible_surface() {
    cli().arg("--help").assert().success().stdout(
        predicate::str::contains("login")
            .and(predicate::str::contains("claude"))
            .and(predicate::str::contains("codex"))
            .and(predicate::str::contains("grok"))
            .and(predicate::str::contains("opencode"))
            .and(predicate::str::contains("hermes"))
            .and(predicate::str::contains("pi"))
            .and(predicate::str::contains("dsh"))
            .and(predicate::str::contains("prime-agent"))
            .and(predicate::str::contains("harness-doctor"))
            .and(predicate::str::contains("workspace"))
            .and(predicate::str::contains("vault-tunnel"))
            .and(predicate::str::contains("harness"))
            .and(predicate::str::contains("eval"))
            .and(predicate::str::contains("changelog"))
            .and(predicate::str::contains("update"))
            .and(predicate::str::contains("version")),
    );
}

#[test]
fn dsh_help_exposes_managed_profiles() {
    cli().args(["dsh", "--help"]).assert().success().stdout(
        predicate::str::contains("--profile <PROFILE>")
            .and(predicate::str::contains("headless"))
            .and(predicate::str::contains("web")),
    );
}

#[test]
fn model_without_value_enters_selector_instead_of_failing_clap_parsing() {
    cli()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("ASTRAFLOW_API_KEY", "offline-test-key")
        .args(["grok", "--model"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "--model without a value requires an interactive terminal",
        ));
}

#[test]
fn json_version_is_exactly_one_document() {
    let output = cli().args(["--json", "version"]).output().unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["name"], "astraflow");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
}

#[test]
fn json_parse_errors_are_exactly_one_document() {
    let output = cli()
        .args(["--json", "--definitely-invalid"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert!(
        value["error"]
            .as_str()
            .unwrap()
            .contains("unexpected argument")
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn auth_reports_missing_credential_without_leaking_host_state() {
    let home = tempfile::tempdir().unwrap();
    let output = cli()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("ASTRAFLOW_HOME", home.path())
        .args(["--json", "auth"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["authenticated"], false);
}

#[test]
fn imported_key_is_validated_and_saved_privately() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let size = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..size]);
        assert!(request.starts_with("GET /v1/models "));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer imported-test-key")
        );
        let body = r#"{"data":[{"id":"gpt-5-mini"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        )
        .unwrap();
    });
    let home = tempfile::tempdir().unwrap();
    let output = cli()
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("ASTRAFLOW_HOME", home.path())
        .env("ASTRAFLOW_MODELVERSE_ENDPOINT", format!("http://{address}"))
        .args(["--json", "login", "--with-key", "imported-test-key"])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["validated_model"], "gpt-5-mini");
    assert_eq!(value["endpoint"], format!("http://{address}"));
    assert_eq!(value["region"], "china");
    assert_eq!(value["models"]["chat_completions"], "gpt-5-mini");
    assert_eq!(value["models"]["responses"], "gpt-5-mini");
    assert!(value["models"]["anthropic"].is_null());
    let path = home.path().join("credentials.json");
    let stored = fs::read_to_string(&path).unwrap();
    assert!(stored.contains("imported-test-key"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn injected_probe_never_prints_the_key() {
    let output = cli()
        .env("ASTRAFLOW_MODELVERSE_API_KEY", "do-not-print-this-key")
        .args(["--json", "_probe"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("do-not-print-this-key"));
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["probe"]["injected"], true);
}

#[test]
fn completion_script_is_generated() {
    cli()
        .args(["--completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_astraflow"));
}

#[test]
fn update_check_reads_a_github_release_manifest() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let size = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..size]);
        assert!(request.starts_with("GET /release/latest "));
        let body = r#"{"tag_name":"v9.8.7"}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        )
        .unwrap();
    });
    let output = cli()
        .env(
            "ASTRAFLOW_UPDATE_URL",
            format!("http://{address}/release/latest"),
        )
        .args(["--json", "update", "--check"])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["latest"], "9.8.7");
    assert_eq!(value["update_available"], true);
    assert_eq!(value["installed"], false);
}
