//! Whitelist, ops, and ban management.
//! Uses RCON when the server is live; edits `whitelist.json` / `ops.json` via Mojang UUID lookup when stopped.

use crate::{config, net, rcon};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_MOJANG_BASE: &str = "https://api.mojang.com";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WhitelistEntry {
    pub uuid: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpEntry {
    pub uuid: String,
    pub name: String,
    #[serde(default = "default_op_level")]
    pub level: u8,
    #[serde(default)]
    pub bypasses_player_limit: bool,
}

fn default_op_level() -> u8 {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BannedEntry {
    pub uuid: Option<String>,
    pub name: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSummary {
    pub name: String,
    pub uuid: String,
    pub is_admin: bool,
}

#[derive(Deserialize)]
struct MojangProfile {
    id: String,
    #[allow(dead_code)]
    name: Option<String>,
}

/// Format a 32-char Mojang hex UUID into dashed canonical form.
pub fn format_uuid(raw: &str) -> Result<String> {
    let clean: String = raw.chars().filter(|c| *c != '-').collect();
    if clean.len() != 32 || !clean.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid UUID from Mojang: {raw}");
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &clean[0..8],
        &clean[8..12],
        &clean[12..16],
        &clean[16..20],
        &clean[20..32]
    ))
}

pub fn lookup_uuid(mojang_base: &str, username: &str) -> Result<String> {
    if !config::valid_username(username) {
        bail!("'{username}' is not a valid Minecraft username (3-16 letters, numbers, or _).");
    }
    let url = format!(
        "{}/users/profiles/minecraft/{username}",
        mojang_base.trim_end_matches('/')
    );
    let prof: MojangProfile = net::get_json(&url).with_context(|| {
        format!(
            "Could not find a Minecraft account called '{username}' (check the spelling and your internet)."
        )
    })?;
    format_uuid(&prof.id)
}

pub fn read_whitelist(server_dir: &Path) -> Vec<WhitelistEntry> {
    read_json_list(&server_dir.join("whitelist.json"))
}

pub fn read_ops(server_dir: &Path) -> Vec<OpEntry> {
    read_json_list(&server_dir.join("ops.json"))
}

pub fn read_banned(server_dir: &Path) -> Vec<BannedEntry> {
    read_json_list(&server_dir.join("banned-players.json"))
}

pub fn list_players(server_dir: &Path) -> Vec<PlayerSummary> {
    let ops = read_ops(server_dir);
    read_whitelist(server_dir)
        .into_iter()
        .map(|w| {
            let is_admin = ops.iter().any(|o| o.name.eq_ignore_ascii_case(&w.name));
            PlayerSummary {
                name: w.name,
                uuid: w.uuid,
                is_admin,
            }
        })
        .collect()
}

fn read_json_list<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    std::fs::read_to_string(path)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_json_list<T: Serialize>(path: &Path, items: &[T]) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(items)?)?;
    Ok(())
}

/// Add a player to the whitelist (and optionally ops).
/// When `live_rcon` is `Some((port, password))`, uses RCON; otherwise resolves UUID via Mojang and edits JSON.
pub fn add_player(
    server_dir: &Path,
    username: &str,
    admin: bool,
    live_rcon: Option<(u16, &str)>,
    mojang_base: &str,
) -> Result<String> {
    if !config::valid_username(username) {
        bail!("'{username}' is not a valid Minecraft username (3-16 letters, numbers, or _).");
    }
    if let Some((port, password)) = live_rcon {
        let mut messages = Vec::new();
        let wl = rcon::send_command(port, password, &format!("whitelist add {username}"))?;
        if !wl.text.is_empty() {
            messages.push(wl.text);
        }
        if admin {
            let op = rcon::send_command(port, password, &format!("op {username}"))?;
            if !op.text.is_empty() {
                messages.push(op.text);
            }
        }
        return Ok(if messages.is_empty() {
            format!("Added {username}{}.", if admin { " as an admin" } else { "" })
        } else {
            messages.join(" | ")
        });
    }

    let uuid = lookup_uuid(mojang_base, username)?;
    let mut wl: Vec<WhitelistEntry> = read_whitelist(server_dir)
        .into_iter()
        .filter(|e| !e.name.eq_ignore_ascii_case(username))
        .collect();
    wl.push(WhitelistEntry {
        uuid: uuid.clone(),
        name: username.to_string(),
    });
    write_json_list(&server_dir.join("whitelist.json"), &wl)?;

    if admin {
        let mut ops: Vec<OpEntry> = read_ops(server_dir)
            .into_iter()
            .filter(|e| !e.name.eq_ignore_ascii_case(username))
            .collect();
        ops.push(OpEntry {
            uuid,
            name: username.to_string(),
            level: 4,
            bypasses_player_limit: false,
        });
        write_json_list(&server_dir.join("ops.json"), &ops)?;
    }

    Ok(format!(
        "Added {username}{}.",
        if admin { " as an admin" } else { "" }
    ))
}

/// Remove a player from whitelist + ops (or only revoke admin rights if `admin_only` is true).
pub fn remove_player(
    server_dir: &Path,
    username: &str,
    admin_only: bool,
    live_rcon: Option<(u16, &str)>,
) -> Result<String> {
    if !config::valid_username(username) {
        bail!("'{username}' is not a valid Minecraft username.");
    }
    if let Some((port, password)) = live_rcon {
        let mut messages = Vec::new();
        if !admin_only {
            let wl = rcon::send_command(port, password, &format!("whitelist remove {username}"))?;
            if !wl.text.is_empty() {
                messages.push(wl.text);
            }
        }
        let deop = rcon::send_command(port, password, &format!("deop {username}"))?;
        if !deop.text.is_empty() {
            messages.push(deop.text);
        }
        return Ok(if messages.is_empty() {
            "Done.".to_string()
        } else {
            messages.join(" | ")
        });
    }

    if !admin_only {
        let wl: Vec<WhitelistEntry> = read_whitelist(server_dir)
            .into_iter()
            .filter(|e| !e.name.eq_ignore_ascii_case(username))
            .collect();
        write_json_list(&server_dir.join("whitelist.json"), &wl)?;
    }
    let ops: Vec<OpEntry> = read_ops(server_dir)
        .into_iter()
        .filter(|e| !e.name.eq_ignore_ascii_case(username))
        .collect();
    write_json_list(&server_dir.join("ops.json"), &ops)?;
    Ok("Done.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_mojang_uuid() {
        assert_eq!(
            format_uuid("11111111222233334444555555555555").unwrap(),
            "11111111-2222-3333-4444-555555555555"
        );
        assert!(format_uuid("short").is_err());
    }
}
