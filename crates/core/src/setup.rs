//! Create a ready-to-run server (Paper for plugins or Fabric for mods):
//! pick a hardware profile, fetch Java + server jar, write safe defaults,
//! and optionally add the owner's Minecraft username as initial admin.

use crate::{config, fabric, hardware, java, net, paper, paths, players};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub name: String,
    #[serde(default)]
    pub software: config::ServerSoftware,
    pub version: Option<String>,
    pub port: u16,
    pub eula: bool,
    pub heap_mb: Option<u64>,
    pub tunnel: bool,
    pub owner_username: Option<String>,
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 32
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `step` gets human-readable status lines; `progress(label, done, total)` gets download progress.
pub fn create_server(
    o: &Options,
    step: &mut dyn FnMut(&str),
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<config::ServerConfig> {
    if !valid_name(&o.name) {
        bail!("server name may only contain letters, numbers, - and _");
    }
    if !o.eula {
        bail!("you need to accept the Minecraft EULA (https://aka.ms/MinecraftEULA)");
    }
    if let Some(owner) = o
        .owner_username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !config::valid_username(owner) {
            bail!("'{owner}' is not a valid Minecraft username (3-16 letters, numbers, or _).");
        }
    }

    let dir = paths::server_dir(&o.name);
    if dir.join("blockhost.json").exists() {
        bail!("a server called {} already exists", o.name);
    }
    let hw = hardware::detect();
    let profile = hardware::recommend(&hw);
    let max_heap = hw.total_ram_mb.saturating_sub(1024).max(1024);
    let heap_mb = o.heap_mb.unwrap_or(profile.heap_mb).clamp(1024, max_heap);

    let cfg = match o.software {
        config::ServerSoftware::Paper => {
            step("Finding the newest stable Paper build...");
            let r = paper::resolve(o.version.as_deref())?;
            step(&format!(
                "Paper {} (build {}) needs Java {}",
                r.mc_version, r.build, r.java_feature
            ));

            step("Getting Java (one-time download)...");
            java::ensure_java(r.java_feature, &mut |d, t| progress("java", d, t))?;

            std::fs::create_dir_all(&dir)?;
            std::fs::create_dir_all(dir.join("plugins"))?;
            step("Downloading Paper server...");
            net::download(&r.url, &dir.join("server.jar"), Some(&r.sha256), &mut |d, t| {
                progress("paper", d, t)
            })?;

            let mut c = config::ServerConfig::new(
                o.name.clone(),
                r.mc_version,
                r.build,
                r.java_feature,
                heap_mb,
                o.port,
                profile.max_players,
            );
            c.software = config::ServerSoftware::Paper;
            c.tunnel = o.tunnel;
            c
        }
        config::ServerSoftware::Fabric => {
            step("Finding the newest stable Fabric mod loader...");
            let r = fabric::resolve(o.version.as_deref())?;
            step(&format!(
                "Fabric {} (loader {}) needs Java {}",
                r.mc_version, r.loader_version, r.java_feature
            ));

            step("Getting Java (one-time download)...");
            java::ensure_java(r.java_feature, &mut |d, t| progress("java", d, t))?;

            std::fs::create_dir_all(&dir)?;
            std::fs::create_dir_all(dir.join("mods"))?;
            step("Downloading Fabric server launcher...");
            fabric::download_server_jar(&r, &dir.join("server.jar"), &mut |d, t| progress("fabric", d, t))?;

            let mut c = config::ServerConfig::new(
                o.name.clone(),
                r.mc_version,
                0,
                r.java_feature,
                heap_mb,
                o.port,
                profile.max_players,
            );
            c.software = config::ServerSoftware::Fabric;
            c.loader_version = Some(r.loader_version);
            c.tunnel = o.tunnel;
            c
        }
    };

    config::write_eula(&dir, o.eula)?;
    config::apply_defaults(&dir, &cfg, profile.view_distance, profile.simulation_distance)?;
    std::fs::write(dir.join("whitelist.json"), "[]\n")?;
    std::fs::write(dir.join("ops.json"), "[]\n")?;
    cfg.save(&dir)?;

    if let Some(owner) = o
        .owner_username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        step(&format!("Adding {owner} as server admin..."));
        if let Err(e) = players::add_player(&dir, owner, true, None, players::DEFAULT_MOJANG_BASE) {
            step(&format!(
                "Note: could not add {owner} automatically ({e:#}); you can add them in Players."
            ));
        }
    }

    step("Server created. Whitelist is on, so only allowed players can join.");
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_validated() {
        assert!(valid_name("my-smp_2"));
        assert!(!valid_name(""));
        assert!(!valid_name("../etc"));
        assert!(!valid_name("a b"));
    }

    #[test]
    fn eula_is_required() {
        let o = Options {
            name: "x".into(),
            software: config::ServerSoftware::Paper,
            version: None,
            port: 25565,
            eula: false,
            heap_mb: None,
            tunnel: false,
            owner_username: None,
        };
        assert!(create_server(&o, &mut |_| {}, &mut |_, _, _| {}).is_err());
    }
}
