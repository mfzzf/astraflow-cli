#![cfg(unix)]

use astraflow::updater::{InstallState, install_release_from};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::thread;

fn platform_asset() -> String {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => panic!("unsupported test architecture {other}"),
    };
    let platform = match std::env::consts::OS {
        "linux" => "unknown-linux-gnu",
        "macos" => "apple-darwin",
        other => panic!("unsupported test operating system {other}"),
    };
    format!("astraflow-{arch}-{platform}")
}

fn serve_release(binary: Vec<u8>, checksum: String) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let size = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            let path = request.split_whitespace().nth(1).unwrap();
            let (status, content_type, body): (&str, &str, &[u8]) = if path == "/SHA256SUMS" {
                ("200 OK", "text/plain", checksum.as_bytes())
            } else if path.ends_with(&platform_asset()) {
                ("200 OK", "application/octet-stream", &binary)
            } else {
                ("404 Not Found", "text/plain", b"not found")
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        }
    });
    (format!("http://{address}"), handle)
}

fn version_of(binary: &Path) -> String {
    let output = Command::new(binary)
        .args(["--json", "version"])
        .env("ASTRAFLOW_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["version"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn installs_a_checksum_verified_versioned_binary_atomically() {
    let source = Path::new(env!("CARGO_BIN_EXE_astraflow"));
    let binary = fs::read(source).unwrap();
    let asset = platform_asset();
    let checksum = format!("{:x}  {asset}\n", Sha256::digest(&binary));
    let (base_url, server) = serve_release(binary, checksum);
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("astraflow");
    fs::copy(source, &destination).unwrap();

    let state = install_release_from(
        &reqwest::Client::new(),
        env!("CARGO_PKG_VERSION"),
        &destination,
        &base_url,
    )
    .await
    .unwrap();

    assert_eq!(state, InstallState::Replaced);
    assert_eq!(version_of(&destination), env!("CARGO_PKG_VERSION"));
    server.join().unwrap();
}

#[tokio::test]
async fn rejects_a_tampered_release_without_replacing_the_destination() {
    let source = Path::new(env!("CARGO_BIN_EXE_astraflow"));
    let binary = fs::read(source).unwrap();
    let checksum = format!("{}  {}\n", "0".repeat(64), platform_asset());
    let (base_url, server) = serve_release(binary, checksum);
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("astraflow");
    let original = b"existing executable";
    fs::write(&destination, original).unwrap();

    let error = install_release_from(
        &reqwest::Client::new(),
        env!("CARGO_PKG_VERSION"),
        &destination,
        &base_url,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("checksum verification failed"));
    assert_eq!(fs::read(&destination).unwrap(), original);
    server.join().unwrap();
}
