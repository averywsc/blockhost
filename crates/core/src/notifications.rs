//! Optional Discord webhook notifications (strictly opt-in, disabled by default)
//! and GitHub Releases app update checks.

use crate::net;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const GITHUB_LATEST_RELEASE: &str = "https://api.github.com/repos/averywsc/blockhost/releases/latest";

/// Validate a Discord webhook URL (`https://discord.com/api/webhooks/...` or `discordapp.com`).
pub fn valid_discord_webhook(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("https://discord.com/api/webhooks/")
        || u.starts_with("https://discordapp.com/api/webhooks/")
}

/// Send a short plain-language message to an opt-in Discord webhook.
pub fn send_discord_webhook(webhook_url: &str, message: &str) -> Result<()> {
    let url = webhook_url.trim();
    if !valid_discord_webhook(url) {
        bail!("Discord webhook URL must start with https://discord.com/api/webhooks/");
    }
    let payload = serde_json::json!({
        "username": "BlockHost",
        "content": message,
    });
    net::agent()
        .post(url)
        .timeout(Duration::from_secs(8))
        .send_json(payload)
        .context("Could not send Discord notification")?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: String,
    pub notes: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: String,
}

fn semver_tuple(v: &str) -> (u32, u32, u32) {
    let clean = v.trim().trim_start_matches('v');
    let mut parts = clean.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

/// Check GitHub Releases for a newer version of BlockHost itself.
pub fn check_app_update() -> Result<AppUpdateInfo> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let rel: GithubRelease =
        net::get_json(GITHUB_LATEST_RELEASE).context("No published BlockHost release found yet.")?;
    let latest = rel.tag_name.trim_start_matches('v').to_string();
    let update_available = semver_tuple(&latest) > semver_tuple(&current);
    Ok(AppUpdateInfo {
        current_version: current,
        latest_version: latest,
        update_available,
        release_url: rel.html_url,
        notes: rel.body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_discord_webhook_urls() {
        assert!(valid_discord_webhook(
            "https://discord.com/api/webhooks/12345/abcdef"
        ));
        assert!(valid_discord_webhook(
            "https://discordapp.com/api/webhooks/12345/abcdef"
        ));
        assert!(!valid_discord_webhook(
            "http://discord.com/api/webhooks/12345/abcdef"
        ));
        assert!(!valid_discord_webhook("https://evil.example/webhook"));
    }

    #[test]
    fn compares_semver_versions() {
        assert!(semver_tuple("v0.2.0") > semver_tuple("0.1.0"));
        assert!(semver_tuple("1.0.0") > semver_tuple("0.9.9"));
        assert_eq!(semver_tuple("v0.1.0"), semver_tuple("0.1.0"));
    }
}
