//! World settings, game rules, low-lag presets, safe new-world creation (never deletes),
//! and disk usage inspection.

use crate::{backup, config, hardware::Profile, paths, rcon};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameRules {
    pub difficulty: String,
    pub gamemode: String,
    pub pvp: bool,
    pub enable_command_block: bool,
    pub view_distance: u8,
    pub simulation_distance: u8,
    pub sync_chunk_writes: bool,
    pub max_players: u32,
    pub motd: String,
    pub white_list: bool,
    pub level_seed: String,
}

pub fn read_game_rules(server_dir: &Path) -> GameRules {
    let props = server_dir.join("server.properties");
    let get = |k: &str, d: &str| config::get_property(&props, k).unwrap_or_else(|| d.to_string());
    GameRules {
        difficulty: get("difficulty", "easy"),
        gamemode: get("gamemode", "survival"),
        pvp: get("pvp", "true") == "true",
        enable_command_block: get("enable-command-block", "false") == "true",
        view_distance: get("view-distance", "8").parse().unwrap_or(8),
        simulation_distance: get("simulation-distance", "6").parse().unwrap_or(6),
        sync_chunk_writes: get("sync-chunk-writes", "true") == "true",
        max_players: get("max-players", "8").parse().unwrap_or(8),
        motd: get("motd", "My Minecraft server"),
        white_list: get("white-list", "true") == "true",
        level_seed: get("level-seed", ""),
    }
}

pub fn set_difficulty(server_dir: &Path, difficulty: &str, live_rcon: Option<(u16, &str)>) -> Result<()> {
    if !matches!(difficulty, "peaceful" | "easy" | "normal" | "hard") {
        bail!("invalid difficulty '{difficulty}'");
    }
    config::set_properties(
        &server_dir.join("server.properties"),
        &[("difficulty", difficulty.to_string())],
    )?;
    if let Some((port, password)) = live_rcon {
        let _ = rcon::send_command(port, password, &format!("difficulty {difficulty}"));
    }
    Ok(())
}

pub fn set_gamemode(server_dir: &Path, gamemode: &str) -> Result<()> {
    if !matches!(gamemode, "survival" | "creative" | "adventure" | "spectator") {
        bail!("invalid gamemode '{gamemode}'");
    }
    config::set_properties(
        &server_dir.join("server.properties"),
        &[("gamemode", gamemode.to_string())],
    )
}

pub fn set_pvp(server_dir: &Path, enabled: bool) -> Result<()> {
    config::set_properties(
        &server_dir.join("server.properties"),
        &[("pvp", enabled.to_string())],
    )
}

pub fn set_command_blocks(server_dir: &Path, enabled: bool) -> Result<()> {
    config::set_properties(
        &server_dir.join("server.properties"),
        &[("enable-command-block", enabled.to_string())],
    )
}

pub fn set_view_distance(server_dir: &Path, chunks: u8) -> Result<()> {
    if !(3..=32).contains(&chunks) {
        bail!("view distance must be between 3 and 32 chunks");
    }
    config::set_properties(
        &server_dir.join("server.properties"),
        &[("view-distance", chunks.to_string())],
    )
}

pub fn set_max_players(server_dir: &Path, max_players: u32) -> Result<()> {
    if !(1..=100).contains(&max_players) {
        bail!("max players must be between 1 and 100");
    }
    config::set_properties(
        &server_dir.join("server.properties"),
        &[("max-players", max_players.to_string())],
    )
}

pub fn set_motd(server_dir: &Path, motd: &str) -> Result<()> {
    if !config::valid_motd(motd) {
        bail!("server name must be 1-40 plain letters, numbers, spaces, _, ., !, or -");
    }
    config::set_properties(
        &server_dir.join("server.properties"),
        &[("motd", motd.to_string())],
    )
}

pub fn set_whitelist_enabled(server_dir: &Path, enabled: bool, live_rcon: Option<(u16, &str)>) -> Result<()> {
    let val = enabled.to_string();
    config::set_properties(
        &server_dir.join("server.properties"),
        &[("white-list", val.clone()), ("enforce-whitelist", val)],
    )?;
    if let Some((port, password)) = live_rcon {
        let cmd = if enabled { "whitelist on" } else { "whitelist off" };
        let _ = rcon::send_command(port, password, cmd);
    }
    Ok(())
}

/// Apply low-lag mode for old laptops (`panel.ps1` Apply-LowLag).
pub fn apply_low_lag(server_dir: &Path) -> Result<()> {
    config::set_properties(
        &server_dir.join("server.properties"),
        &[
            ("view-distance", "6".into()),
            ("simulation-distance", "4".into()),
            ("sync-chunk-writes", "false".into()),
        ],
    )
}

/// Reset performance properties to hardware-recommended defaults (`panel.ps1` Reset-Recommended).
pub fn reset_recommended(server_dir: &Path, profile: &Profile) -> Result<()> {
    config::set_properties(
        &server_dir.join("server.properties"),
        &[
            ("view-distance", profile.view_distance.to_string()),
            ("simulation-distance", profile.simulation_distance.to_string()),
            ("sync-chunk-writes", "true".into()),
            ("max-players", profile.max_players.to_string()),
        ],
    )
}

/// Start a fresh world safely (server MUST be stopped):
/// 1. Validates seed (`^[A-Za-z0-9 _-]{1,40}$` or empty).
/// 2. Takes a backup first (aborts if backup fails when a world exists).
/// 3. Moves existing `world*` directories aside into `<root>/old-worlds/<name>-<stamp>` (NEVER deletes).
/// 4. Writes `level-seed` in `server.properties`.
pub fn new_world(server_dir: &Path, server_name: &str, seed: &str, backup_keep: usize) -> Result<PathBuf> {
    let seed = seed.trim();
    if !config::valid_seed(seed) {
        bail!("a world seed can only use up to 40 letters, numbers, spaces, - and _");
    }

    let level = config::get_property(&server_dir.join("server.properties"), "level-name")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "world".to_string());

    let world_dirs: Vec<PathBuf> = std::fs::read_dir(server_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with(&level))
                })
                .collect()
        })
        .unwrap_or_default();

    if !world_dirs.is_empty() {
        // Backup must succeed before touching worlds (Section 6).
        backup::create(server_dir, server_name, backup_keep)?;
    }

    let stamp = backup::timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    let dest = paths::root()
        .join("old-worlds")
        .join(format!("{server_name}-{stamp}"));
    std::fs::create_dir_all(&dest)?;

    for w in world_dirs {
        if let Some(fname) = w.file_name() {
            std::fs::rename(&w, dest.join(fname))?;
        }
    }

    config::set_properties(
        &server_dir.join("server.properties"),
        &[("level-seed", seed.to_string())],
    )?;

    Ok(dest)
}

/// Recursively compute total bytes of all `world*` folders in `server_dir`.
pub fn world_size_bytes(server_dir: &Path) -> u64 {
    let level = config::get_property(&server_dir.join("server.properties"), "level-name")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "world".to_string());
    let Ok(rd) = std::fs::read_dir(server_dir) else {
        return 0;
    };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&level))
        })
        .map(|p| dir_size(&p))
        .sum()
}

pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(meta) = e.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{recommend, Hardware};

    #[test]
    fn low_lag_and_reset_recommended_update_properties() {
        let d = std::env::temp_dir().join(format!("bh-world-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let profile = recommend(&Hardware {
            total_ram_mb: 8192,
            cpu_cores: 8,
        });

        apply_low_lag(&d).unwrap();
        let r1 = read_game_rules(&d);
        assert_eq!(r1.view_distance, 6);
        assert_eq!(r1.simulation_distance, 4);
        assert!(!r1.sync_chunk_writes);

        reset_recommended(&d, &profile).unwrap();
        let r2 = read_game_rules(&d);
        assert_eq!(r2.view_distance, 10);
        assert_eq!(r2.simulation_distance, 8);
        assert!(r2.sync_chunk_writes);
        assert_eq!(r2.max_players, 16);

        std::fs::remove_dir_all(&d).unwrap();
    }
}
