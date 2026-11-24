//! Create a ready-to-run server: pick a profile, fetch Java + Paper, write config.

use crate::{config, hardware, java, net, paper, paths};
use anyhow::{bail, Result};

pub struct Options {
    pub name: String,
    pub version: Option<String>,
    pub port: u16,
    pub eula: bool,
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
    let dir = paths::server_dir(&o.name);
    if dir.join("blockhost.json").exists() {
        bail!("a server called {} already exists", o.name);
    }
    let profile = hardware::recommend(&hardware::detect());

    step("Finding the newest stable Paper build...");
    let r = paper::resolve(o.version.as_deref())?;
    step(&format!(
        "Paper {} (build {}) needs Java {}",
        r.mc_version, r.build, r.java_feature
    ));

    step("Getting Java (one-time download)...");
    java::ensure_java(r.java_feature, &mut |d, t| progress("java", d, t))?;

    std::fs::create_dir_all(&dir)?;
    step("Downloading the server...");
    net::download(&r.url, &dir.join("server.jar"), Some(&r.sha256), &mut |d, t| {
        progress("paper", d, t)
    })?;

    let cfg = config::ServerConfig {
        name: o.name.clone(),
        mc_version: r.mc_version,
        paper_build: r.build,
        java_feature: r.java_feature,
        heap_mb: profile.heap_mb,
        port: o.port,
        max_players: profile.max_players,
    };
    config::write_eula(&dir, o.eula)?;
    config::apply_defaults(&dir, &cfg, profile.view_distance, profile.simulation_distance)?;
    cfg.save(&dir)?;
    step("Server created. Whitelist is on, so add your players before they join.");
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
            version: None,
            port: 25565,
            eula: false,
        };
        assert!(create_server(&o, &mut |_| {}, &mut |_, _, _| {}).is_err());
    }
}
