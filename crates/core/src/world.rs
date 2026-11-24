//! World settings, game rules, low-lag presets, safe new-world creation (never deletes),
//! and disk usage inspection.

use crate::{backup, config, hardware::Profile, paths, rcon};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

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
    let old_root = paths::root().join("old-worlds");
    let mut dest = old_root.join(format!("{server_name}-{stamp}"));
    let mut suffix = 2u32;
    while dest.exists() {
        dest = old_root.join(format!("{server_name}-{stamp}-{suffix}"));
        suffix += 1;
    }
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

/// Export the active world folders (`world`, `world_nether`, `world_the_end`, etc.) to a user-chosen `.zip`.
pub fn export_world_zip(server_dir: &Path, dest_zip: &Path) -> Result<PathBuf> {
    let level = config::get_property(&server_dir.join("server.properties"), "level-name")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "world".to_string());
    let worlds: Vec<String> = std::fs::read_dir(server_dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    (e.path().is_dir() && n.starts_with(&level)).then_some(n)
                })
                .collect()
        })
        .unwrap_or_default();
    if worlds.is_empty() {
        bail!("No world exists yet. Start the server once to generate a world first.");
    }
    if let Some(parent) = dest_zip.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest_zip.with_extension("zip.part");
    let mut zip = zip::ZipWriter::new(File::create(&part)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    for w in &worlds {
        add_dir_to_zip(&mut zip, server_dir, Path::new(w), opts)?;
    }
    zip.finish()?.flush()?;
    std::fs::rename(&part, dest_zip)?;
    Ok(dest_zip.to_path_buf())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<File>,
    base: &Path,
    rel: &Path,
    opts: zip::write::SimpleFileOptions,
) -> Result<()> {
    let full = base.join(rel);
    let name = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if full.is_dir() {
        for e in std::fs::read_dir(&full)? {
            add_dir_to_zip(zip, base, &rel.join(e?.file_name()), opts)?;
        }
    } else if rel.file_name().is_some_and(|n| n != "session.lock") {
        zip.start_file(name, opts)?;
        std::io::copy(&mut File::open(&full)?, zip)?;
    }
    Ok(())
}

/// Safely import a world `.zip` (singleplayer save zip, custom map, or BlockHost backup) onto a stopped server:
/// 1. Validates the zip contains a `level.dat` (at root, inside `world/`, or inside a single top-level folder).
/// 2. Backs up existing worlds and moves them aside into `old-worlds/` (never deletes!).
/// 3. Extracts cleanly into `server/world/` (and `world_nether` / `world_the_end` if present).
pub fn import_world_zip(
    server_dir: &Path,
    server_name: &str,
    zip_path: &Path,
    backup_keep: usize,
) -> Result<PathBuf> {
    let file = File::open(zip_path).context("Could not open the selected world zip file.")?;
    let mut archive = zip::ZipArchive::new(file).context("That file is not a valid .zip archive.")?;

    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().replace('\\', "/")))
        .collect();

    // Find `level.dat` to determine zip layout.
    let level_dat = names
        .iter()
        .find(|n| {
            let trimmed = n.trim_start_matches("./");
            trimmed == "level.dat" || trimmed.ends_with("/level.dat")
        })
        .cloned()
        .context("This zip does not look like a Minecraft world (no level.dat found inside).")?;

    let level_parts: Vec<&str> = level_dat
        .trim_start_matches("./")
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();

    // Determine whether the zip already contains `world/`, `world_nether/` layout OR a single world folder/root.
    let multi_world_layout = names
        .iter()
        .any(|n| n.starts_with("world_nether/") || n.starts_with("world_the_end/"))
        || (level_parts.len() == 2 && level_parts[0] == "world");

    let staging = server_dir.join(format!("import-staging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;

    // Safely extract all entries (ZipArchive::extract rejects path traversal `..`).
    let extract_res = archive.extract(&staging);
    if let Err(e) = extract_res {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e).context("Failed to unpack the world zip.");
    }

    // Back up and move current worlds aside (Section 6: never delete worlds!).
    let moved_aside = new_world(server_dir, server_name, "", backup_keep)?;

    if multi_world_layout {
        for entry in std::fs::read_dir(&staging)? {
            let e = entry?;
            let fname = e.file_name();
            let s = fname.to_string_lossy();
            if e.path().is_dir() && s.starts_with("world") {
                let target = server_dir.join(&fname);
                if target.exists() {
                    let _ = std::fs::remove_dir_all(&target);
                }
                std::fs::rename(e.path(), target)?;
            }
        }
    } else if level_parts.len() == 1 {
        // `level.dat` was at root of the zip -> staging itself IS the world folder.
        std::fs::rename(&staging, server_dir.join("world"))?;
        return Ok(moved_aside);
    } else {
        // `SomeMapName/level.dat` (or nested) -> move the folder containing `level.dat` to `server/world`.
        let mut src_world = staging.clone();
        for seg in &level_parts[..level_parts.len() - 1] {
            src_world = src_world.join(seg);
        }
        std::fs::rename(src_world, server_dir.join("world"))?;
    }

    let _ = std::fs::remove_dir_all(&staging);
    // Remove stale session.lock if the uploaded zip came from a running singleplayer save.
    let _ = std::fs::remove_file(server_dir.join("world").join("session.lock"));
    Ok(moved_aside)
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
