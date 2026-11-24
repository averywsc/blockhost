use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256, Sha512};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::Arc,
    time::Duration,
};

/// PaperMC's download API requires a real User-Agent with a contact URL or email.
pub const USER_AGENT: &str = concat!(
    "BlockHost/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/averywsc/blockhost; mailto:averybeuth4@gmail.com)"
);

pub fn agent() -> ureq::Agent {
    let tls = native_tls::TlsConnector::new().expect("failed to initialize native TLS connector");
    ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .user_agent(USER_AGENT)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
}

pub fn get_json<T: DeserializeOwned>(url: &str) -> Result<T> {
    agent()
        .get(url)
        .call()
        .with_context(|| format!("request failed: {url}"))?
        .into_json()
        .with_context(|| format!("bad JSON from {url}"))
}

/// Download to `dest` (via a .part file), optionally verifying SHA-256.
/// `progress(bytes_done, total_bytes)` is called as data arrives.
pub fn download(
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<()> {
    let resp = agent()
        .get(url)
        .call()
        .with_context(|| format!("download failed: {url}"))?;
    let total = resp.header("Content-Length").and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_reader();

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest.with_extension("part");
    let mut out = File::create(&part)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    out.flush()?;
    drop(out);

    if let Some(expected) = expected_sha256 {
        let got = hex(&hasher.finalize());
        if !got.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&part);
            bail!("checksum mismatch for {url}: expected {expected}, got {got}");
        }
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

/// Download to `dest` (via a .part file), optionally verifying SHA-512 (used by Modrinth).
pub fn download_sha512(
    url: &str,
    dest: &Path,
    expected_sha512: Option<&str>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<()> {
    let resp = agent()
        .get(url)
        .call()
        .with_context(|| format!("download failed: {url}"))?;
    let total = resp.header("Content-Length").and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.into_reader();

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest.with_extension("part");
    let mut out = File::create(&part)?;
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    out.flush()?;
    drop(out);

    if let Some(expected) = expected_sha512 {
        let got = hex(&hasher.finalize());
        if !got.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&part);
            bail!("checksum mismatch for {url}: expected {expected}, got {got}");
        }
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
