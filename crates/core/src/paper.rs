use crate::{backup, config::ServerConfig, java, net};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

pub const DEFAULT_BASE: &str = "https://fill.papermc.io/v3/projects/paper";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Resolved {
    pub mc_version: String,
    pub build: u32,
    pub url: String,
    pub sha256: String,
    pub java_feature: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub current_version: String,
    pub current_build: u32,
    /// Newer build for the same Minecraft version (safe: clients don't need to update).
    pub same_version: Option<Resolved>,
    /// Newer Minecraft major/minor version (requires client update + backup first).
    pub latest_major: Option<Resolved>,
}

#[derive(Deserialize)]
struct Build {
    id: u32,
    channel: String,
    downloads: HashMap<String, Download>,
}

#[derive(Deserialize)]
struct Download {
    url: String,
    checksums: Checksums,
}

#[derive(Deserialize)]
struct Checksums {
    sha256: String,
}

/// Newest-first sort key: "1.21.10" -> [1,21,10]. Pre-releases ("-rc1") are excluded by callers.
pub fn version_key(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

pub fn list_versions(base: &str) -> Result<Vec<String>> {
    all_versions(base.trim_end_matches('/'))
}

fn all_versions(base: &str) -> Result<Vec<String>> {
    let v: serde_json::Value = net::get_json(base)?;
    let groups = v
        .get("versions")
        .and_then(|g| g.as_object())
        .context("unexpected Paper API shape")?;
    let mut out: Vec<String> = groups
        .values()
        .filter_map(|g| g.as_array())
        .flatten()
        .filter_map(|s| s.as_str().map(String::from))
        .filter(|s| !s.contains('-'))
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(version_key(a)));
    Ok(out)
}

fn latest_stable_build(base: &str, version: &str) -> Result<Option<Build>> {
    let builds: Vec<Build> = net::get_json(&format!("{base}/versions/{version}/builds"))?;
    Ok(builds
        .into_iter()
        .filter(|b| b.channel.eq_ignore_ascii_case("STABLE") && b.downloads.contains_key("server:default"))
        .max_by_key(|b| b.id))
}

/// Fallback only; the API's own Java requirement wins when present.
pub fn java_for_mc(version: &str) -> u32 {
    let k = version_key(version);
    match (
        k.first().copied().unwrap_or(1),
        k.get(1).copied().unwrap_or(0),
        k.get(2).copied().unwrap_or(0),
    ) {
        (major, _, _) if major >= 26 => 25,
        (1, minor, patch) if minor > 20 || (minor == 20 && patch >= 5) => 21,
        (1, minor, _) if minor >= 17 => 17,
        _ => 8,
    }
}

fn java_requirement(base: &str, version: &str) -> u32 {
    net::get_json::<serde_json::Value>(&format!("{base}/versions/{version}"))
        .ok()
        .and_then(|v| {
            v.pointer("/version/java/version/minimum")
                .and_then(|m| m.as_u64())
        })
        .map(|m| m as u32)
        .unwrap_or_else(|| java_for_mc(version))
}

/// Resolve the newest STABLE Paper build. With `wanted = None`, picks the newest
/// Minecraft version that actually has a stable build.
pub fn resolve(wanted: Option<&str>) -> Result<Resolved> {
    resolve_from(DEFAULT_BASE, wanted)
}

pub fn resolve_from(base: &str, wanted: Option<&str>) -> Result<Resolved> {
    let base = base.trim_end_matches('/');
    let candidates = match wanted {
        Some(v) => vec![v.to_string()],
        None => all_versions(base)?.into_iter().take(8).collect(),
    };
    for v in candidates {
        if let Some(b) = latest_stable_build(base, &v)? {
            let d = &b.downloads["server:default"];
            return Ok(Resolved {
                java_feature: java_requirement(base, &v),
                mc_version: v,
                build: b.id,
                url: d.url.clone(),
                sha256: d.checksums.sha256.clone(),
            });
        }
    }
    bail!(
        "no stable Paper build found{}",
        wanted.map(|v| format!(" for {v}")).unwrap_or_default()
    )
}

/// Check whether a newer same-version Paper build or newer Minecraft version is available.
pub fn check_updates_from(base: &str, current_version: &str, current_build: u32) -> Result<UpdateStatus> {
    let same = resolve_from(base, Some(current_version))
        .ok()
        .filter(|r| r.build > current_build);
    let latest = resolve_from(base, None)
        .ok()
        .filter(|r| version_key(&r.mc_version) > version_key(current_version));
    Ok(UpdateStatus {
        current_version: current_version.to_string(),
        current_build,
        same_version: same,
        latest_major: latest,
    })
}

pub fn check_updates(current_version: &str, current_build: u32) -> Result<UpdateStatus> {
    check_updates_from(DEFAULT_BASE, current_version, current_build)
}

/// Install a resolved Paper build onto an existing (stopped) server:
/// 1. Back up existing worlds first (if any exist).
/// 2. Download to `server.jar.new` and verify SHA-256.
/// 3. Ensure required Java major version is present (when `ensure_java_fn` is provided).
/// 4. Keep previous `server.jar` as `server.jar.old`, swap in new jar, and update `blockhost.json`.
pub fn install_update(
    server_dir: &Path,
    cfg: &mut ServerConfig,
    pick: &Resolved,
    ensure_java_fn: &mut dyn FnMut(u32) -> Result<()>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Option<std::path::PathBuf>> {
    // Take a backup first if a world exists (Section 6: backup before updates).
    let backup_path = backup::create(server_dir, &cfg.name, cfg.backup_keep).ok();

    let new_jar = server_dir.join("server.jar.new");
    net::download(&pick.url, &new_jar, Some(&pick.sha256), progress)?;

    ensure_java_fn(pick.java_feature)?;

    let jar = server_dir.join("server.jar");
    if jar.exists() {
        std::fs::copy(&jar, server_dir.join("server.jar.old"))?;
    }
    std::fs::rename(&new_jar, &jar)?;

    cfg.mc_version = pick.mc_version.clone();
    cfg.paper_build = pick.build;
    cfg.java_feature = pick.java_feature;
    cfg.save(server_dir)?;

    Ok(backup_path)
}

/// Default Java installer callback for production updates.
pub fn default_ensure_java(feature: u32, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<()> {
    java::ensure_java(feature, progress)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_sorting_is_numeric() {
        let mut v = vec!["1.9.4", "1.21.10", "26.1", "1.21.9"];
        v.sort_by_key(|a| std::cmp::Reverse(version_key(a)));
        assert_eq!(v, vec!["26.1", "1.21.10", "1.21.9", "1.9.4"]);
    }

    #[test]
    fn java_fallbacks() {
        assert_eq!(java_for_mc("26.1"), 25);
        assert_eq!(java_for_mc("1.21.4"), 21);
        assert_eq!(java_for_mc("1.20.4"), 17);
        assert_eq!(java_for_mc("1.16.5"), 8);
    }
}
