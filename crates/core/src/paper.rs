use crate::net;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

const BASE: &str = "https://fill.papermc.io/v3/projects/paper";

#[derive(Debug, Clone)]
pub struct Resolved {
    pub mc_version: String,
    pub build: u32,
    pub url: String,
    pub sha256: String,
    pub java_feature: u32,
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
fn version_key(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
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
    resolve_from(BASE, wanted)
}

pub fn resolve_from(base: &str, wanted: Option<&str>) -> Result<Resolved> {
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
