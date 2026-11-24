#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use blockhost_core::{
    autostart, backup,
    config::{self, ServerConfig, ServerSoftware},
    diagnostics,
    events::{LiveMetrics, StatusSnapshot},
    fabric, hardware,
    manager::Manager,
    network::{self, ActionResult, CgnatReport, LinkInfo, PingStats},
    notifications::{self, AppUpdateInfo},
    paper::{self, Resolved, UpdateStatus},
    paths,
    players::{self, BannedEntry, PlayerSummary},
    plugins::{self, InstalledPlugin, PluginHit, ResolvedPluginVersion},
    power, setup,
    world::{self, GameRules},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager as TauriManager, RunEvent, State, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};

struct AppState {
    core: Manager,
}

fn friendly_err(e: anyhow::Error) -> String {
    format!("{e}")
}

#[derive(Deserialize)]
struct CreateServerArgs {
    name: String,
    software: ServerSoftware,
    version: Option<String>,
    heap_mb: Option<u64>,
    tunnel: bool,
    owner_username: Option<String>,
    eula: bool,
}

#[derive(Deserialize)]
struct UpdateSettingsArgs {
    server_name: String,
    heap_mb: Option<u64>,
    max_players: Option<u32>,
    motd: Option<String>,
    tunnel: Option<bool>,
    playit_address: Option<String>,
    backup_hours: Option<u32>,
    restart_hours: Option<u32>,
    perf_mode: Option<bool>,
    aikar_flags: Option<bool>,
}

#[derive(Serialize)]
struct LogsPayload {
    next: usize,
    lines: Vec<String>,
}

#[derive(Serialize)]
struct PlayersState {
    allowed: Vec<PlayerSummary>,
    banned: Vec<BannedEntry>,
    whitelist_enabled: bool,
}

#[derive(Serialize)]
struct NetworkOverview {
    lan_ip: Option<String>,
    public_ip: Option<String>,
    tailscale_ip: Option<String>,
    link: Option<LinkInfo>,
    cgnat: CgnatReport,
}

// ---------- Available Minecraft Versions ----------

#[tauri::command]
async fn list_mc_versions(software: ServerSoftware) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || match software {
        ServerSoftware::Paper => paper::list_versions(paper::DEFAULT_BASE).map_err(friendly_err),
        ServerSoftware::Fabric => fabric::list_versions(fabric::DEFAULT_FABRIC_META).map_err(friendly_err),
    })
    .await
    .map_err(|e| format!("Could not load Minecraft versions: {e}"))?
}

// ---------- Core Status & Lifecycle ----------

#[tauri::command]
fn get_status(state: State<'_, AppState>, selected: Option<String>) -> StatusSnapshot {
    state.core.status(selected.as_deref())
}

#[tauri::command]
async fn get_live_metrics(state: State<'_, AppState>, server_name: String) -> Result<LiveMetrics, String> {
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.live_metrics(&server_name))
        .await
        .map_err(|e| format!("Could not check live server metrics: {e}"))
}

#[tauri::command]
fn get_logs(state: State<'_, AppState>, since: usize) -> LogsPayload {
    let (next, lines) = state.core.logs_since(since);
    LogsPayload { next, lines }
}

#[tauri::command]
fn create_server(state: State<'_, AppState>, args: CreateServerArgs) -> Result<(), String> {
    state
        .core
        .begin_setup(setup::Options {
            name: args.name.trim().to_string(),
            software: args.software,
            version: args.version.filter(|v| !v.trim().is_empty()),
            port: 25565,
            eula: args.eula,
            heap_mb: args.heap_mb,
            tunnel: args.tunnel,
            owner_username: args.owner_username,
        })
        .map_err(friendly_err)
}

#[tauri::command]
fn start_server(
    state: State<'_, AppState>,
    server_name: String,
    use_tunnel: Option<bool>,
) -> Result<(), String> {
    state.core.start(&server_name, use_tunnel).map_err(friendly_err)
}

#[tauri::command]
fn stop_server(state: State<'_, AppState>, warn_seconds: u64) {
    state.core.stop_with_warning(warn_seconds);
}

#[tauri::command]
fn force_stop_server(state: State<'_, AppState>) {
    state.core.force_stop();
}

#[tauri::command]
async fn restart_server(state: State<'_, AppState>, server_name: String) -> Result<(), String> {
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.shutdown_and_wait(Duration::from_secs(60));
        std::thread::sleep(Duration::from_secs(2));
        core.start(&server_name, None).map_err(friendly_err)
    })
    .await
    .map_err(|e| format!("Restart failed: {e}"))?
}

#[tauri::command]
async fn send_command(state: State<'_, AppState>, command: String) -> Result<String, String> {
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.command(&command).map_err(friendly_err))
        .await
        .map_err(|e| format!("Command failed: {e}"))?
}

// ---------- Players & Admin ----------

#[tauri::command]
fn list_players(server_name: String) -> PlayersState {
    let dir = paths::server_dir(&server_name);
    let whitelist_enabled =
        config::get_property(&dir.join("server.properties"), "white-list").as_deref() != Some("false");
    PlayersState {
        allowed: players::list_players(&dir),
        banned: players::read_banned(&dir),
        whitelist_enabled,
    }
}

#[tauri::command]
async fn add_player(
    state: State<'_, AppState>,
    server_name: String,
    username: String,
    admin: bool,
) -> Result<String, String> {
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.add_player(&server_name, username.trim(), admin)
            .map_err(friendly_err)
    })
    .await
    .map_err(|e| format!("Could not add player: {e}"))?
}

#[tauri::command]
async fn remove_player(
    state: State<'_, AppState>,
    server_name: String,
    username: String,
    admin_only: bool,
) -> Result<String, String> {
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.remove_player(&server_name, username.trim(), admin_only)
            .map_err(friendly_err)
    })
    .await
    .map_err(|e| format!("Could not remove player: {e}"))?
}

#[tauri::command]
async fn player_admin_action(
    state: State<'_, AppState>,
    action: String,
    username: String,
    arg: Option<String>,
) -> Result<String, String> {
    let username = username.trim().to_string();
    if !config::valid_username(&username) {
        return Err(format!("'{username}' is not a valid Minecraft username."));
    }
    let cmd = match action.as_str() {
        "kick" => match arg.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(reason) => format!("kick {username} {reason}"),
            None => format!("kick {username}"),
        },
        "ban" => match arg.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(reason) => format!("ban {username} {reason}"),
            None => format!("ban {username}"),
        },
        "pardon" => format!("pardon {username}"),
        "gamemode" => {
            let mode = arg.unwrap_or_else(|| "survival".into());
            if !matches!(mode.as_str(), "survival" | "creative" | "adventure" | "spectator") {
                return Err("Invalid game mode.".into());
            }
            format!("gamemode {mode} {username}")
        }
        "tp" => {
            let target = arg.unwrap_or_default().trim().to_string();
            if !config::valid_username(&target) {
                return Err("Select a valid target player to teleport to.".into());
            }
            format!("tp {username} {target}")
        }
        "heal" => format!("effect give {username} minecraft:instant_health 1 10 true"),
        "feed" => format!("effect give {username} minecraft:saturation 1 10 true"),
        "msg" => {
            let text = arg.unwrap_or_default().trim().to_string();
            if text.is_empty() {
                return Err("Type a message first.".into());
            }
            format!("tell {username} {text}")
        }
        _ => return Err("Unknown admin action.".into()),
    };
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.command(&cmd).map_err(friendly_err))
        .await
        .map_err(|e| format!("Action failed: {e}"))?
}

// ---------- Backups ----------

#[tauri::command]
fn backup_now(state: State<'_, AppState>, server_name: String) -> Result<(), String> {
    state.core.backup_now(&server_name).map_err(friendly_err)
}

#[tauri::command]
async fn restore_backup(
    state: State<'_, AppState>,
    server_name: String,
    filename: String,
) -> Result<String, String> {
    if state.core.is_running(&server_name) {
        return Err("Stop the server first before restoring a backup.".into());
    }
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err("Invalid backup filename.".into());
    }
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let zip = backup::backups_dir(&server_name).join(&filename);
        let aside = backup::restore(&dir, &zip).map_err(friendly_err)?;
        core.emit_status(Some(&server_name));
        Ok(format!(
            "Restored backup. Previous world kept safely in {}",
            aside.display()
        ))
    })
    .await
    .map_err(|e| format!("Restore failed: {e}"))?
}

// ---------- Settings & Updates ----------

#[tauri::command]
fn update_settings(state: State<'_, AppState>, args: UpdateSettingsArgs) -> Result<StatusSnapshot, String> {
    let dir = paths::server_dir(&args.server_name);
    let mut cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
    let hw = hardware::detect();
    let max_heap = hw.total_ram_mb.saturating_sub(1024).max(1024);

    if let Some(mb) = args.heap_mb {
        cfg.heap_mb = mb.clamp(1024, max_heap);
    }
    if let Some(mp) = args.max_players {
        world::set_max_players(&dir, mp).map_err(friendly_err)?;
        cfg.max_players = mp;
    }
    if let Some(motd) = args.motd {
        world::set_motd(&dir, motd.trim()).map_err(friendly_err)?;
    }
    if let Some(t) = args.tunnel {
        cfg.tunnel = t;
    }
    if let Some(addr) = args.playit_address {
        let trimmed = addr.trim();
        if trimmed.is_empty() {
            cfg.playit_address = None;
        } else if config::valid_address(trimmed) {
            cfg.playit_address = Some(trimmed.to_string());
        } else {
            return Err("That address does not look valid (letters, numbers, dots, and dashes only).".into());
        }
    }
    if let Some(bh) = args.backup_hours {
        cfg.backup_hours = bh.min(168);
    }
    if let Some(rh) = args.restart_hours {
        cfg.restart_hours = rh.min(168);
    }
    if let Some(pm) = args.perf_mode {
        cfg.perf_mode = pm;
    }
    if let Some(af) = args.aikar_flags {
        cfg.aikar_flags = af;
    }

    cfg.save(&dir).map_err(friendly_err)?;
    state.core.emit_status(Some(&args.server_name));
    Ok(state.core.status(Some(&args.server_name)))
}

#[tauri::command]
fn set_whitelist_enabled(
    state: State<'_, AppState>,
    server_name: String,
    enabled: bool,
) -> Result<(), String> {
    let dir = paths::server_dir(&server_name);
    let live = state.core.live_rcon(&server_name);
    let live_ref = live.as_ref().map(|(p, pw)| (*p, pw.as_str()));
    world::set_whitelist_enabled(&dir, enabled, live_ref).map_err(friendly_err)?;
    state.core.emit_status(Some(&server_name));
    Ok(())
}

#[tauri::command]
fn set_autostart(state: State<'_, AppState>, enabled: bool) -> Result<bool, String> {
    autostart::set_enabled(enabled).map_err(friendly_err)?;
    state.core.emit_status(None);
    Ok(autostart::is_enabled())
}

#[tauri::command]
async fn check_server_updates(server_name: String) -> Result<Option<UpdateStatus>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        if cfg.software != ServerSoftware::Paper {
            return Ok(None);
        }
        paper::check_updates(&cfg.mc_version, cfg.paper_build)
            .map(Some)
            .map_err(friendly_err)
    })
    .await
    .map_err(|e| format!("Update check failed: {e}"))?
}

#[tauri::command]
async fn install_paper_update(
    state: State<'_, AppState>,
    server_name: String,
    pick: Resolved,
) -> Result<String, String> {
    if state.core.is_running(&server_name) {
        return Err("Stop the server before installing an update.".into());
    }
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let mut cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        paper::install_update(
            &dir,
            &mut cfg,
            &pick,
            &mut |jv| paper::default_ensure_java(jv, &mut |_, _| {}),
            &mut |_, _| {},
        )
        .map_err(friendly_err)?;
        core.emit_status(Some(&server_name));
        Ok(format!(
            "Updated to Minecraft {} (Paper build {}).",
            cfg.mc_version, cfg.paper_build
        ))
    })
    .await
    .map_err(|e| format!("Update failed: {e}"))?
}

// ---------- World & Rules ----------

#[tauri::command]
fn get_game_rules(server_name: String) -> GameRules {
    world::read_game_rules(&paths::server_dir(&server_name))
}

#[tauri::command]
fn update_game_rule(
    state: State<'_, AppState>,
    server_name: String,
    rule: String,
    value: String,
) -> Result<GameRules, String> {
    let dir = paths::server_dir(&server_name);
    let live = state.core.live_rcon(&server_name);
    let live_ref = live.as_ref().map(|(p, pw)| (*p, pw.as_str()));
    match rule.as_str() {
        "difficulty" => world::set_difficulty(&dir, &value, live_ref),
        "gamemode" => world::set_gamemode(&dir, &value),
        "pvp" => world::set_pvp(&dir, value == "true"),
        "command_blocks" => world::set_command_blocks(&dir, value == "true"),
        "view_distance" => {
            let n: u8 = value
                .parse()
                .map_err(|_| "View distance must be a number.".to_string())?;
            world::set_view_distance(&dir, n)
        }
        "low_lag" => world::apply_low_lag(&dir),
        "reset_recommended" => {
            let profile = hardware::recommend(&hardware::detect());
            world::reset_recommended(&dir, &profile)
        }
        _ => return Err("Unknown rule.".into()),
    }
    .map_err(friendly_err)?;
    Ok(world::read_game_rules(&dir))
}

#[tauri::command]
async fn create_new_world(
    state: State<'_, AppState>,
    server_name: String,
    seed: String,
) -> Result<String, String> {
    if state.core.is_running(&server_name) {
        return Err("Stop the server first before creating a new world.".into());
    }
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let keep = ServerConfig::load(&dir).map(|c| c.backup_keep).unwrap_or(10);
        let moved = world::new_world(&dir, &server_name, &seed, keep).map_err(friendly_err)?;
        core.emit_status(Some(&server_name));
        Ok(format!(
            "Fresh world ready on next start. Your old world was kept safely in {}",
            moved.display()
        ))
    })
    .await
    .map_err(|e| format!("Could not create new world: {e}"))?
}

#[tauri::command]
async fn import_world_zip(state: State<'_, AppState>, server_name: String) -> Result<Option<String>, String> {
    if state.core.is_running(&server_name) {
        return Err("Stop the server first before uploading a world.".into());
    }
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Choose a Minecraft world .zip file")
            .add_filter("World archive", &["zip"])
            .pick_file();
        let Some(zip_path) = picked else {
            return Ok(None);
        };
        let dir = paths::server_dir(&server_name);
        let keep = ServerConfig::load(&dir).map(|c| c.backup_keep).unwrap_or(10);
        let moved = world::import_world_zip(&dir, &server_name, &zip_path, keep).map_err(friendly_err)?;
        core.emit_status(Some(&server_name));
        Ok(Some(format!(
            "World uploaded and ready! Your previous world was kept safely in {}",
            moved.display()
        )))
    })
    .await
    .map_err(|e| format!("World upload failed: {e}"))?
}

#[tauri::command]
async fn export_world_zip(server_name: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Export world as .zip")
            .set_file_name(format!("{server_name}-world.zip"))
            .add_filter("World archive", &["zip"])
            .save_file();
        let Some(dest) = picked else {
            return Ok(None);
        };
        let dir = paths::server_dir(&server_name);
        let saved = world::export_world_zip(&dir, &dest).map_err(friendly_err)?;
        Ok(Some(format!("Exported world to {}", saved.display())))
    })
    .await
    .map_err(|e| format!("World export failed: {e}"))?
}

#[tauri::command]
async fn import_local_addon(server_name: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Choose a Minecraft mod or plugin .jar file")
            .add_filter("Minecraft JAR", &["jar"])
            .pick_files();
        let Some(files) = picked else {
            return Ok(None);
        };
        if files.is_empty() {
            return Ok(None);
        }
        let dir = paths::server_dir(&server_name);
        let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        let mut added = Vec::new();
        for f in files {
            let name = plugins::import_local_jar(&dir, cfg.software, &f).map_err(friendly_err)?;
            added.push(name);
        }
        Ok(Some(format!(
            "Added {} file(s): {}. Restart the server to load them.",
            added.len(),
            added.join(", ")
        )))
    })
    .await
    .map_err(|e| format!("Could not add .jar file: {e}"))?
}

// ---------- Mods & Plugins (Paper Plugins / Fabric Mods) ----------

#[tauri::command]
fn list_addons(server_name: String) -> Result<Vec<InstalledPlugin>, String> {
    let dir = paths::server_dir(&server_name);
    let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
    Ok(plugins::list_addons(&dir, cfg.software))
}

#[tauri::command]
async fn search_addons(server_name: String, query: String) -> Result<Vec<PluginHit>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        plugins::search_for_software(
            plugins::DEFAULT_MODRINTH_BASE,
            cfg.software,
            &query,
            &cfg.mc_version,
            10,
        )
        .map_err(friendly_err)
    })
    .await
    .map_err(|e| format!("Search failed: {e}"))?
}

#[tauri::command]
async fn resolve_addon(server_name: String, project_id: String) -> Result<ResolvedPluginVersion, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        plugins::resolve_version_for_software(
            plugins::DEFAULT_MODRINTH_BASE,
            cfg.software,
            &project_id,
            &cfg.mc_version,
        )
        .map_err(friendly_err)
    })
    .await
    .map_err(|e| format!("Could not find compatible download: {e}"))?
}

#[tauri::command]
async fn install_addon(server_name: String, resolved: ResolvedPluginVersion) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        plugins::install_addon(&dir, cfg.software, &resolved, &mut |_, _| {}).map_err(friendly_err)?;
        Ok(format!(
            "Installed {}. Restart the server to load it.",
            resolved.filename
        ))
    })
    .await
    .map_err(|e| format!("Install failed: {e}"))?
}

#[tauri::command]
fn remove_addon(state: State<'_, AppState>, server_name: String, filename: String) -> Result<(), String> {
    if state.core.is_running(&server_name) {
        return Err("Stop the server first before removing a mod or plugin.".into());
    }
    let dir = paths::server_dir(&server_name);
    let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
    plugins::remove_addon(&dir, cfg.software, &filename).map_err(friendly_err)
}

// ---------- Network ----------

#[tauri::command]
async fn get_network_overview() -> Result<NetworkOverview, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let lan_ip = network::detect_lan_ip().map(|ip| ip.to_string());
        let public_ip = network::detect_public_ip();
        let tailscale_ip = network::detect_tailscale_ip();
        let link = network::get_link_info();
        let wan_ip = network::upnp_external_ip().ok();
        let cgnat = network::evaluate_cgnat(wan_ip.as_deref(), public_ip.as_deref());
        NetworkOverview {
            lan_ip,
            public_ip,
            tailscale_ip,
            link,
            cgnat,
        }
    })
    .await
    .map_err(|e| format!("Network check failed: {e}"))
}

#[tauri::command]
async fn save_duckdns(
    state: State<'_, AppState>,
    server_name: String,
    domain: String,
    token: String,
) -> Result<String, String> {
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let mut cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        if domain.trim().is_empty() {
            cfg.duckdns_domain = None;
            cfg.duckdns_token = None;
            cfg.save(&dir).map_err(friendly_err)?;
            core.emit_status(Some(&server_name));
            return Ok("Cleared DuckDNS domain.".into());
        }
        let host = network::update_duckdns(&domain, &token).map_err(friendly_err)?;
        cfg.duckdns_domain = Some(host.clone());
        cfg.duckdns_token = Some(token.trim().to_string());
        cfg.save(&dir).map_err(friendly_err)?;
        core.emit_status(Some(&server_name));
        Ok(format!("DuckDNS active: {host}"))
    })
    .await
    .map_err(|e| format!("DuckDNS setup failed: {e}"))?
}

#[tauri::command]
async fn export_diagnostics(
    state: State<'_, AppState>,
    server_name: String,
) -> Result<Option<String>, String> {
    let (_, recent_logs) = state.core.logs_since(0);
    tauri::async_runtime::spawn_blocking(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Save BlockHost diagnostics .zip")
            .set_file_name(format!("{server_name}-diagnostics.zip"))
            .add_filter("Diagnostics archive", &["zip"])
            .save_file();
        let Some(dest) = picked else {
            return Ok(None);
        };
        let dir = paths::server_dir(&server_name);
        let saved = diagnostics::export_diagnostics_zip(&dir, &dest, &recent_logs).map_err(friendly_err)?;
        Ok(Some(format!(
            "Saved diagnostics (passwords redacted) to {}",
            saved.display()
        )))
    })
    .await
    .map_err(|e| format!("Diagnostics export failed: {e}"))?
}

#[tauri::command]
async fn save_discord_webhook(
    state: State<'_, AppState>,
    server_name: String,
    webhook_url: String,
    send_test: bool,
) -> Result<String, String> {
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let mut cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        let trimmed = webhook_url.trim();
        if trimmed.is_empty() {
            cfg.discord_webhook = None;
            cfg.save(&dir).map_err(friendly_err)?;
            core.emit_status(Some(&server_name));
            return Ok("Discord notifications turned off.".into());
        }
        if !notifications::valid_discord_webhook(trimmed) {
            return Err("Webhook URL must start with https://discord.com/api/webhooks/".into());
        }
        if send_test {
            notifications::send_discord_webhook(
                trimmed,
                &format!("BlockHost connected to **{server_name}**!"),
            )
            .map_err(friendly_err)?;
        }
        cfg.discord_webhook = Some(trimmed.to_string());
        cfg.save(&dir).map_err(friendly_err)?;
        core.emit_status(Some(&server_name));
        Ok(if send_test {
            "Discord webhook saved and test message sent!".into()
        } else {
            "Discord webhook saved.".into()
        })
    })
    .await
    .map_err(|e| format!("Could not save Discord webhook: {e}"))?
}

#[tauri::command]
async fn check_app_update() -> Result<AppUpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(|| notifications::check_app_update().map_err(friendly_err))
        .await
        .map_err(|e| format!("App update check failed: {e}"))?
}

#[tauri::command]
async fn apply_laptop_lid_power() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        power::apply_windows_power().map_err(friendly_err)?;
        Ok("Windows AC sleep and lid-close sleep disabled while plugged in.".into())
    })
    .await
    .map_err(|e| format!("Power configuration failed: {e}"))?
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("Only http:// and https:// links can be opened.".into());
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", trimmed])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(trimmed).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(trimmed).spawn();
    }
    Ok(())
}

#[tauri::command]
async fn ping_target(target: String, count: usize) -> Result<PingStats, String> {
    tauri::async_runtime::spawn_blocking(move || network::ping_host(&target, count).map_err(friendly_err))
        .await
        .map_err(|e| format!("Ping failed: {e}"))?
}

#[tauri::command]
async fn setup_port_forward(server_name: String) -> Result<(ActionResult, ActionResult), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        let fw = network::add_firewall_rule(cfg.port, cfg.rcon_port);
        let upnp = network::add_upnp_forward(cfg.port);
        Ok((fw, upnp))
    })
    .await
    .map_err(|e| format!("Port forward failed: {e}"))?
}

#[tauri::command]
async fn remove_port_forward(server_name: String) -> Result<ActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = paths::server_dir(&server_name);
        let cfg = ServerConfig::load(&dir).map_err(friendly_err)?;
        Ok(network::remove_upnp_forward(cfg.port))
    })
    .await
    .map_err(|e| format!("Could not remove port forward: {e}"))?
}

#[tauri::command]
fn open_folder(server_name: Option<String>, sub: Option<String>) -> Result<(), String> {
    let target = match (server_name, sub.as_deref()) {
        (Some(name), Some("backups")) => backup::backups_dir(&name),
        (Some(name), Some("addons")) => {
            let dir = paths::server_dir(&name);
            let sw = ServerConfig::load(&dir).map(|c| c.software).unwrap_or_default();
            plugins::addons_dir(&dir, sw)
        }
        (Some(name), _) => paths::server_dir(&name),
        (None, _) => paths::root(),
    };
    std::fs::create_dir_all(&target).map_err(|e| format!("Could not open folder: {e}"))?;
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(&target).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&target).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&target).spawn();
    }
    Ok(())
}

/// Show the main window, recreating WebView2 if it was destroyed when closed to tray.
fn show_or_recreate_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("BlockHost")
        .inner_size(1060.0, 720.0)
        .min_inner_size(860.0, 580.0)
        .center()
        .build();
}

fn main() {
    let core = Manager::new();
    let core_for_events = core.clone();
    let core_for_tray = core.clone();
    let core_for_exit = core.clone();

    let app = tauri::Builder::default()
        .manage(AppState { core })
        .invoke_handler(tauri::generate_handler![
            list_mc_versions,
            get_status,
            get_live_metrics,
            get_logs,
            create_server,
            start_server,
            stop_server,
            force_stop_server,
            restart_server,
            send_command,
            list_players,
            add_player,
            remove_player,
            player_admin_action,
            backup_now,
            restore_backup,
            update_settings,
            set_whitelist_enabled,
            set_autostart,
            check_server_updates,
            install_paper_update,
            get_game_rules,
            update_game_rule,
            create_new_world,
            import_world_zip,
            export_world_zip,
            list_addons,
            search_addons,
            resolve_addon,
            install_addon,
            import_local_addon,
            remove_addon,
            get_network_overview,
            ping_target,
            setup_port_forward,
            remove_port_forward,
            save_duckdns,
            export_diagnostics,
            save_discord_webhook,
            check_app_update,
            apply_laptop_lid_power,
            open_folder,
            open_url,
        ])
        .setup(move |app| {
            let open_item = MenuItem::with_id(app, "open", "Open BlockHost", true, None::<&str>)?;
            let start_item = MenuItem::with_id(app, "start", "Start Server", true, None::<&str>)?;
            let stop_item = MenuItem::with_id(app, "stop", "Stop Server Safely", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit BlockHost", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &start_item, &stop_item, &quit_item])?;

            let mut tray = TrayIconBuilder::with_id("blockhost-tray")
                .menu(&menu)
                .tooltip("BlockHost Minecraft Server Host")
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "open" => show_or_recreate_main_window(app),
                    "start" => {
                        let snap = core_for_tray.status(None);
                        if snap.running.is_none() {
                            if let Some(name) = snap.current {
                                let _ = core_for_tray.start(&name, None);
                            }
                        }
                    }
                    "stop" => core_for_tray.stop(),
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_or_recreate_main_window(tray.app_handle());
                    }
                });

            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            let handle = app.handle().clone();
            let mut rx = core_for_events.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            let _ = handle.emit("core://event", &ev);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { .. } = event {
                    // Window closes naturally, freeing WebView2 memory while tray stays alive.
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building BlockHost");

    app.run(move |_app_handle, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => {
            core_for_exit.shutdown_and_wait(Duration::from_secs(60));
        }
        _ => {}
    });
}
