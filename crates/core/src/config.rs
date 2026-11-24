use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const CURRENT_SCHEMA_VERSION: u32 = 3;

/// Server software type: Paper (fast, plugins) or Fabric (lightweight, mods).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ServerSoftware {
    #[default]
    Paper,
    Fabric,
}

impl ServerSoftware {
    pub fn addons_folder_name(self) -> &'static str {
        match self {
            ServerSoftware::Paper => "plugins",
            ServerSoftware::Fabric => "mods",
        }
    }

    pub fn modrinth_loader(self) -> &'static str {
        match self {
            ServerSoftware::Paper => "paper",
            ServerSoftware::Fabric => "fabric",
        }
    }
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

fn default_rcon_port() -> u16 {
    25575
}

fn default_backup_hours() -> u32 {
    6
}

fn default_backup_keep() -> usize {
    10
}

fn default_perf_mode() -> bool {
    true
}

/// Generate a cryptographically strong random alphanumeric password (24 chars by default)
/// using the OS CSPRNG via `getrandom`.
pub fn generate_password(len: usize) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let max_usable = (256 / ALPHABET.len()) * ALPHABET.len();
    let mut out = String::with_capacity(len);
    let mut buf = [0u8; 64];
    while out.len() < len {
        getrandom::getrandom(&mut buf).expect("OS CSPRNG unavailable");
        for &b in &buf {
            if (b as usize) < max_usable {
                out.push(ALPHABET[(b as usize) % ALPHABET.len()] as char);
                if out.len() == len {
                    break;
                }
            }
        }
    }
    out
}

/// Saved per-server settings (`servers/<name>/blockhost.json`).
/// Migrates seamlessly from schema v1 (which lacked RCON/backup/restart/tunnel/perf fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub software: ServerSoftware,
    #[serde(default)]
    pub loader_version: Option<String>,
    pub mc_version: String,
    pub paper_build: u32,
    pub java_feature: u32,
    pub heap_mb: u64,
    pub port: u16,
    pub max_players: u32,
    #[serde(default = "default_rcon_port")]
    pub rcon_port: u16,
    #[serde(default)]
    pub rcon_password: String,
    #[serde(default)]
    pub tunnel: bool,
    #[serde(default)]
    pub playit_address: Option<String>,
    #[serde(default = "default_backup_hours")]
    pub backup_hours: u32,
    #[serde(default = "default_backup_keep")]
    pub backup_keep: usize,
    #[serde(default)]
    pub restart_hours: u32,
    #[serde(default = "default_perf_mode")]
    pub perf_mode: bool,
}

impl ServerConfig {
    pub fn new(
        name: String,
        mc_version: String,
        paper_build: u32,
        java_feature: u32,
        heap_mb: u64,
        port: u16,
        max_players: u32,
    ) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            name,
            software: ServerSoftware::Paper,
            loader_version: None,
            mc_version,
            paper_build,
            java_feature,
            heap_mb,
            port,
            max_players,
            rcon_port: default_rcon_port(),
            rcon_password: generate_password(24),
            tunnel: false,
            playit_address: None,
            backup_hours: default_backup_hours(),
            backup_keep: default_backup_keep(),
            restart_hours: 0,
            perf_mode: true,
        }
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let mut copy = self.clone();
        copy.ensure_defaults();
        std::fs::write(dir.join("blockhost.json"), serde_json::to_vec_pretty(&copy)?)?;
        Ok(())
    }

    /// Load `blockhost.json`, running any schema migrations and saving if defaults were filled in.
    pub fn load(dir: &Path) -> Result<Self> {
        let raw = std::fs::read(dir.join("blockhost.json"))?;
        let mut cfg: Self = serde_json::from_slice(&raw)?;
        if cfg.ensure_defaults() {
            let _ = cfg.save(dir);
        }
        Ok(cfg)
    }

    /// Ensure required fields (such as a strong RCON password) are populated and schema is current.
    /// Returns `true` if any field was migrated/filled in.
    pub fn ensure_defaults(&mut self) -> bool {
        let mut changed = false;
        if self.schema_version < CURRENT_SCHEMA_VERSION {
            self.schema_version = CURRENT_SCHEMA_VERSION;
            changed = true;
        }
        if self.rcon_port == 0 {
            self.rcon_port = default_rcon_port();
            changed = true;
        }
        if self.rcon_password.trim().is_empty() {
            self.rcon_password = generate_password(24);
            changed = true;
        }
        if self.backup_keep == 0 {
            self.backup_keep = default_backup_keep();
            changed = true;
        }
        changed
    }
}

/// Legacy `panel.ps1` `settings.json` format for seamless migration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyPanelSettings {
    pub java_version: Option<u32>,
    pub minecraft: Option<String>,
    pub paper_build: Option<u32>,
    pub tunnel: Option<bool>,
    pub heap_mb: Option<u64>,
    pub rcon_password: Option<String>,
    pub backup_hours: Option<u32>,
    pub restart_hours: Option<u32>,
    pub playit_address: Option<String>,
    pub perf_mode: Option<bool>,
}

/// Parse a legacy `panel.ps1` `settings.json` into a `ServerConfig` with safe defaults.
pub fn migrate_legacy_panel_settings(
    name: &str,
    legacy: &LegacyPanelSettings,
    fallback_heap_mb: u64,
    fallback_max_players: u32,
) -> ServerConfig {
    let mut cfg = ServerConfig::new(
        name.to_string(),
        legacy.minecraft.clone().unwrap_or_else(|| "1.21.4".to_string()),
        legacy.paper_build.unwrap_or(0),
        legacy.java_version.unwrap_or(21),
        legacy.heap_mb.unwrap_or(fallback_heap_mb),
        25565,
        fallback_max_players,
    );
    if let Some(pw) = legacy.rcon_password.as_deref().filter(|s| !s.trim().is_empty()) {
        cfg.rcon_password = pw.to_string();
    }
    cfg.tunnel = legacy.tunnel.unwrap_or(false);
    cfg.backup_hours = legacy.backup_hours.unwrap_or(6);
    cfg.restart_hours = legacy.restart_hours.unwrap_or(0);
    cfg.playit_address = legacy.playit_address.clone().filter(|a| valid_address(a));
    cfg.perf_mode = legacy.perf_mode.unwrap_or(true);
    cfg
}

/// Validate a Minecraft Java Edition username (`^[A-Za-z0-9_]{3,16}$`).
pub fn valid_username(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Validate server MOTD shown in the Minecraft server list (`^[A-Za-z0-9 _.!-]{1,40}$`).
pub fn valid_motd(motd: &str) -> bool {
    (1..=40).contains(&motd.len())
        && motd
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'.' | b'!' | b'-'))
}

/// Validate world seed (`^[A-Za-z0-9 _-]{1,40}$`). Empty string means random seed.
pub fn valid_seed(seed: &str) -> bool {
    seed.is_empty()
        || (seed.len() <= 40
            && seed
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'-')))
}

/// Validate a host or host:port address (`^[A-Za-z0-9.\-]+(:\d{1,5})?$`).
pub fn valid_address(addr: &str) -> bool {
    let trimmed = addr.trim();
    if trimmed.is_empty() || trimmed.len() > 253 {
        return false;
    }
    let (host, port) = match trimmed.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (trimmed, None),
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return false;
    }
    if let Some(p) = port {
        if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        if p.parse::<u16>().is_err() {
            return false;
        }
    }
    true
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

/// Ensure local RCON settings are written to `server.properties`.
pub fn apply_rcon_properties(dir: &Path, cfg: &ServerConfig) -> Result<()> {
    set_properties(
        &dir.join("server.properties"),
        &[
            ("enable-rcon", "true".into()),
            ("rcon.port", cfg.rcon_port.to_string()),
            ("rcon.password", cfg.rcon_password.clone()),
            ("broadcast-rcon-to-ops", "false".into()),
        ],
    )
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
    )?;
    apply_rcon_properties(dir, cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
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

    #[test]
    fn migrates_v1_config_with_strong_rcon_password() {
        let d = tmp("v1-migrate");
        let v1 = r#"{
            "name": "smp",
            "mc_version": "1.21.10",
            "paper_build": 100,
            "java_feature": 21,
            "heap_mb": 3072,
            "port": 25565,
            "max_players": 8
        }"#;
        std::fs::write(d.join("blockhost.json"), v1).unwrap();
        let cfg = ServerConfig::load(&d).unwrap();
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.rcon_port, 25575);
        assert_eq!(cfg.rcon_password.len(), 24);
        assert_eq!(cfg.backup_hours, 6);
        assert_eq!(cfg.backup_keep, 10);
        assert_eq!(cfg.restart_hours, 0);
        assert!(cfg.perf_mode);
        assert!(!cfg.tunnel);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn migrates_legacy_panel_settings() {
        let json = r#"{"javaVersion":21,"minecraft":"1.21.10","paperBuild":100,"rconPassword":"secret123","playitAddress":"cool.joinmc.link"}"#;
        let legacy: LegacyPanelSettings = serde_json::from_str(json).unwrap();
        let cfg = migrate_legacy_panel_settings("home", &legacy, 4915, 16);
        assert_eq!(cfg.heap_mb, 4915);
        assert_eq!(cfg.rcon_password, "secret123");
        assert_eq!(cfg.backup_hours, 6);
        assert!(!cfg.tunnel);
        assert_eq!(cfg.playit_address.as_deref(), Some("cool.joinmc.link"));
    }

    #[test]
    fn validators_enforce_rules() {
        assert!(valid_username("Steve_1"));
        assert!(!valid_username("ab"));
        assert!(!valid_username("bad name!"));
        assert!(!valid_username("way_too_long_username_123"));

        assert!(valid_motd("My Minecraft Server!"));
        assert!(!valid_motd(""));
        assert!(!valid_motd("bad\nline"));

        assert!(valid_seed(""));
        assert!(valid_seed("my seed_1-2"));
        assert!(!valid_seed("bad;seed!"));

        assert!(valid_address("myserver.joinmc.link"));
        assert!(valid_address("192.168.1.5:25565"));
        assert!(!valid_address("not an addr!!"));
        assert!(!valid_address("host:99999"));
    }
}
