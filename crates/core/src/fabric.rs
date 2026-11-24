//! Fabric mod-loader resolution and server launcher download via Fabric Meta v2 API.
//! Fabric's server launcher downloads Mojang's official server jar on first run (never redistributed)
//! and loads `.jar` mods placed in `<server_dir>/mods/`.

use crate::{net, paper};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_FABRIC_META: &str = "https://meta.fabricmc.net/v2";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedFabric {
    pub mc_version: String,
    pub loader_version: String,
    pub installer_version: String,
    pub url: String,
    pub java_feature: u32,
}

#[derive(Deserialize)]
struct MetaVersion {
    version: String,
    #[serde(default)]
    stable: bool,
}

pub fn resolve(wanted_mc: Option<&str>) -> Result<ResolvedFabric> {
    resolve_from(DEFAULT_FABRIC_META, wanted_mc)
}

pub fn resolve_from(base: &str, wanted_mc: Option<&str>) -> Result<ResolvedFabric> {
    let base = base.trim_end_matches('/');
    let mc_version = match wanted_mc.filter(|s| !s.trim().is_empty()) {
        Some(v) => v.trim().to_string(),
        None => {
            let games: Vec<MetaVersion> = net::get_json(&format!("{base}/versions/game"))
                .context("could not load Fabric game versions")?;
            games
                .into_iter()
                .find(|g| g.stable && !g.version.contains('-'))
                .map(|g| g.version)
                .context("no stable Minecraft version found on Fabric Meta")?
        }
    };

    let loaders: Vec<MetaVersion> =
        net::get_json(&format!("{base}/versions/loader")).context("could not load Fabric loader versions")?;
    let loader_version = loaders
        .into_iter()
        .find(|l| l.stable)
        .map(|l| l.version)
        .context("no stable Fabric loader version found")?;

    let installers: Vec<MetaVersion> = net::get_json(&format!("{base}/versions/installer"))
        .context("could not load Fabric installer versions")?;
    let installer_version = installers
        .into_iter()
        .find(|i| i.stable)
        .map(|i| i.version)
        .context("no stable Fabric installer version found")?;

    let url = format!("{base}/versions/loader/{mc_version}/{loader_version}/{installer_version}/server/jar");
    let java_feature = paper::java_for_mc(&mc_version);

    Ok(ResolvedFabric {
        mc_version,
        loader_version,
        installer_version,
        url,
        java_feature,
    })
}

/// Download the Fabric server launcher jar and verify it is a valid JAR archive with a manifest.
pub fn download_server_jar(
    resolved: &ResolvedFabric,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<()> {
    let allowed = resolved.url.starts_with("https://meta.fabricmc.net/")
        || resolved.url.starts_with("http://127.0.0.1:");
    if !allowed {
        bail!("Fabric server download URL is not on meta.fabricmc.net");
    }
    net::download(&resolved.url, dest, None, progress)?;
    let file = std::fs::File::open(dest)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| {
        let _ = std::fs::remove_file(dest);
        anyhow::anyhow!("Downloaded Fabric server jar was corrupted. Try again.")
    })?;
    if archive.by_name("META-INF/MANIFEST.MF").is_err() {
        drop(archive);
        let _ = std::fs::remove_file(dest);
        bail!("Downloaded Fabric server jar is missing its manifest.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_fabric_download_urls() {
        let r = ResolvedFabric {
            mc_version: "1.21.4".into(),
            loader_version: "0.16.10".into(),
            installer_version: "1.0.1".into(),
            url: "https://evil.example/server.jar".into(),
            java_feature: 21,
        };
        let dest = std::env::temp_dir().join("bh-fabric-reject.jar");
        assert!(download_server_jar(&r, &dest, &mut |_, _| {}).is_err());
    }
}
