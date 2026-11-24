use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Saved per-server settings (servers/<name>/blockhost.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub mc_version: String,
    pub paper_build: u32,
    pub java_feature: u32,
    pub heap_mb: u64,
    pub port: u16,
    pub max_players: u32,
}

impl ServerConfig {
    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::write(dir.join("blockhost.json"), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn load(dir: &Path) -> Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(
            dir.join("blockhost.json"),
        )?)?)
    }
}

/// Mojang's EULA must be accepted by the person running the server, so this
/// refuses to write eula.txt unless the caller says they explicitly accepted.
pub fn write_eula(dir: &Path, accepted: bool) -> Result<()> {
    if !accepted {
        bail!("EULA not accepted (https://aka.ms/MinecraftEULA)");
    }
    std::fs::write(dir.join("eula.txt"), "eula=true\n")?;
    Ok(())
}

/// Read a key from server.properties if present.
pub fn get_property(path: &Path, key: &str) -> Option<String> {
    let existing = std::fs::read_to_string(path).ok()?;
    let prefix = format!("{key}=");
    existing
        .lines()
        .find_map(|l| l.strip_prefix(&prefix).map(|v| v.to_string()))
}

/// Update keys in server.properties, keeping every other line (and comments) intact.
pub fn set_properties(path: &Path, updates: &[(&str, String)]) -> Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(String::from).collect();
    for (key, value) in updates {
        let prefix = format!("{key}=");
        match lines.iter_mut().find(|l| l.starts_with(&prefix)) {
            Some(l) => *l = format!("{key}={value}"),
            None => lines.push(format!("{key}={value}")),
        }
    }
    std::fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

/// Safe defaults for a server that will be reachable from the internet.
pub fn apply_defaults(dir: &Path, cfg: &ServerConfig, view: u8, sim: u8) -> Result<()> {
    set_properties(
        &dir.join("server.properties"),
        &[
            ("server-port", cfg.port.to_string()),
            ("max-players", cfg.max_players.to_string()),
            ("view-distance", view.to_string()),
            ("simulation-distance", sim.to_string()),
            ("white-list", "true".into()),
            ("enforce-whitelist", "true".into()),
            ("online-mode", "true".into()),
            ("motd", format!("{} - hosted with BlockHost", cfg.name)),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bh-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn properties_update_in_place_and_append() {
        let d = tmp("props");
        let f = d.join("server.properties");
        std::fs::write(&f, "#comment\nmotd=old\npvp=true\n").unwrap();
        set_properties(&f, &[("motd", "new".into()), ("white-list", "true".into())]).unwrap();
        let out = std::fs::read_to_string(&f).unwrap();
        assert_eq!(out, "#comment\nmotd=new\npvp=true\nwhite-list=true\n");
        assert_eq!(get_property(&f, "motd").as_deref(), Some("new"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn eula_requires_acceptance() {
        let d = tmp("eula");
        assert!(write_eula(&d, false).is_err());
        assert!(!d.join("eula.txt").exists());
        write_eula(&d, true).unwrap();
        assert!(d.join("eula.txt").exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}
