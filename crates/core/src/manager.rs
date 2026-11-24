//! Shared state behind BlockHost UI shells (Tauri desktop app, CLI, service).
//! Owns the running server, log ring buffer, setup progress, tunnel info, RCON actions, and live event stream.

use crate::{
    autostart, backup, config,
    crash::{self, CrashDiagnosis},
    events::{
        BackupEntry, CoreEvent, HardwareSummary, LiveMetrics, SetupSummary, StatusSnapshot, TunnelSummary,
    },
    hardware, java, network, paths, players, power, rcon, server, setup, supervisor, tunnel, world,
};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{channel, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::sync::broadcast;

const MAX_LOG: usize = 3000;
const EVENT_CHANNEL_CAPACITY: usize = 256;

#[derive(Default)]
struct Logs {
    base: usize,
    lines: VecDeque<String>,
}

#[derive(Default, Clone)]
struct SetupState {
    state: String,
    message: String,
    pct: u64,
}

#[derive(Default, Clone)]
struct TunnelInfo {
    claim: Option<String>,
    address: Option<String>,
    error: Option<String>,
}

struct Running {
    name: String,
    rcon_port: u16,
    rcon_password: String,
    started_at: Instant,
    child_pid: Arc<AtomicU32>,
    shutdown: Arc<AtomicBool>,
    force_kill: Arc<AtomicBool>,
    tx: Sender<String>,
}

struct Inner {
    hw: hardware::Hardware,
    logs: Mutex<Logs>,
    running: Mutex<Option<Running>>,
    setup: Mutex<SetupState>,
    tunnel: Mutex<TunnelInfo>,
    last_crash: Mutex<Option<CrashDiagnosis>>,
    events: broadcast::Sender<CoreEvent>,
}

#[derive(Clone)]
pub struct Manager(Arc<Inner>);

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

/// Acquire a PID-aware server lockfile. If an old lockfile belongs to a dead PID (e.g. after power loss),
/// automatically reclaim it so non-technical users are never stranded.
fn acquire_pid_lock(dir: &Path) -> Result<()> {
    let lock = dir.join("blockhost.lock");
    if lock.exists() {
        if let Ok(raw) = std::fs::read_to_string(&lock) {
            if let Ok(pid_num) = raw.trim().parse::<u32>() {
                let mut sys = System::new();
                let pid = Pid::from_u32(pid_num);
                sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                if sys.process(pid).is_some() {
                    bail!("this server is already running (PID {pid_num})");
                }
            }
        }
        // Stale lock from a previous crash/power cut -> remove and reclaim.
        let _ = std::fs::remove_file(&lock);
    }
    std::fs::write(&lock, std::process::id().to_string())?;
    Ok(())
}

impl Manager {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Manager(Arc::new(Inner {
            hw: hardware::detect(),
            logs: Mutex::new(Logs::default()),
            running: Mutex::new(None),
            setup: Mutex::new(SetupState::default()),
            tunnel: Mutex::new(TunnelInfo::default()),
            last_crash: Mutex::new(None),
            events,
        }))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.0.events.subscribe()
    }

    pub fn emit_status(&self, selected: Option<&str>) {
        let snap = self.status(selected);
        let _ = self.0.events.send(CoreEvent::StatusChanged(Box::new(snap)));
    }

    pub fn log(&self, line: &str) {
        let index = {
            let mut l = self.0.logs.lock().unwrap();
            l.lines.push_back(line.to_string());
            if l.lines.len() > MAX_LOG {
                l.lines.pop_front();
                l.base += 1;
            }
            l.base + l.lines.len() - 1
        };
        let _ = self.0.events.send(CoreEvent::LogLine {
            index,
            line: line.to_string(),
        });
    }

    pub fn logs_since(&self, since: usize) -> (usize, Vec<String>) {
        let l = self.0.logs.lock().unwrap();
        let skip = since.max(l.base) - l.base;
        (
            l.base + l.lines.len(),
            l.lines.iter().skip(skip).cloned().collect(),
        )
    }

    pub fn servers(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(paths::root().join("servers"))
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().join("blockhost.json").exists())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.0
            .running
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|r| r.name == name)
    }

    /// Return `(rcon_port, rcon_password)` if `name` is currently running.
    pub fn live_rcon(&self, name: &str) -> Option<(u16, String)> {
        self.0
            .running
            .lock()
            .unwrap()
            .as_ref()
            .filter(|r| r.name == name)
            .map(|r| (r.rcon_port, r.rcon_password.clone()))
    }

    /// Query live players, TPS, and process RAM/CPU if the server is running.
    pub fn live_metrics(&self, name: &str) -> LiveMetrics {
        let info = self
            .0
            .running
            .lock()
            .unwrap()
            .as_ref()
            .filter(|r| r.name == name)
            .map(|r| {
                (
                    r.rcon_port,
                    r.rcon_password.clone(),
                    r.started_at.elapsed().as_secs(),
                    r.child_pid.load(Ordering::SeqCst),
                )
            });
        let Some((port, pw, uptime, pid_num)) = info else {
            return LiveMetrics {
                reachable: false,
                players_text: None,
                online_players: Vec::new(),
                tps_text: None,
                uptime_secs: None,
                process_ram_mb: None,
                process_cpu_pct: None,
            };
        };
        let (process_ram_mb, process_cpu_pct) = if pid_num > 0 {
            let mut sys = System::new();
            let pid = Pid::from_u32(pid_num);
            sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
            if let Some(p) = sys.process(pid) {
                (Some(p.memory() / (1024 * 1024)), Some(p.cpu_usage()))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        let list = rcon::send_command(port, &pw, "list").ok();
        let reachable = list.is_some();
        let players_text = list.map(|r| r.text);
        let online_players = players_text
            .as_deref()
            .map(parse_online_players)
            .unwrap_or_default();
        let tps_text = if reachable {
            rcon::send_command(port, &pw, "tps")
                .ok()
                .map(|r| r.text)
                .filter(|t| !t.is_empty())
        } else {
            None
        };
        LiveMetrics {
            reachable,
            players_text,
            online_players,
            tps_text,
            uptime_secs: Some(uptime),
            process_ram_mb,
            process_cpu_pct,
        }
    }

    pub fn status(&self, selected: Option<&str>) -> StatusSnapshot {
        let servers = self.servers();
        let running = self.0.running.lock().unwrap();
        let running_name = running.as_ref().map(|r| r.name.clone());
        let stopping = running
            .as_ref()
            .is_some_and(|r| r.shutdown.load(Ordering::SeqCst));
        let uptime_secs = running.as_ref().map(|r| r.started_at.elapsed().as_secs());
        drop(running);
        let current = selected
            .map(String::from)
            .filter(|s| servers.contains(s))
            .or_else(|| running_name.clone())
            .or_else(|| servers.first().cloned());
        let profile = hardware::recommend(&self.0.hw);
        let s = self.0.setup.lock().unwrap().clone();
        let t = self.0.tunnel.lock().unwrap().clone();
        let cfg = current
            .as_ref()
            .and_then(|n| config::ServerConfig::load(&paths::server_dir(n)).ok());
        let world_size_bytes = current
            .as_ref()
            .map(|n| world::world_size_bytes(&paths::server_dir(n)))
            .unwrap_or(0);
        let backups: Vec<BackupEntry> = current
            .as_ref()
            .map(|n| {
                backup::list(n)
                    .iter()
                    .filter_map(|p| {
                        Some(BackupEntry {
                            filename: p.file_name()?.to_string_lossy().to_string(),
                            size_bytes: p.metadata().map(|m| m.len()).unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        StatusSnapshot {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            data_dir: paths::root().display().to_string(),
            hardware: HardwareSummary {
                ram_mb: self.0.hw.total_ram_mb,
                cores: self.0.hw.cpu_cores,
                tier: profile.tier.to_string(),
                recommended_heap_mb: profile.heap_mb,
                recommended_max_players: profile.max_players,
                recommended_view_distance: profile.view_distance,
                recommended_simulation_distance: profile.simulation_distance,
                warning: profile.warning.map(String::from),
            },
            servers,
            current,
            software: cfg.as_ref().map(|c| c.software),
            loader_version: cfg.as_ref().and_then(|c| c.loader_version.clone()),
            port: cfg.as_ref().map(|c| c.port),
            heap_mb: cfg.as_ref().map(|c| c.heap_mb),
            max_players: cfg.as_ref().map(|c| c.max_players),
            mc_version: cfg.as_ref().map(|c| c.mc_version.clone()),
            paper_build: cfg.as_ref().map(|c| c.paper_build),
            backup_hours: cfg.as_ref().map(|c| c.backup_hours),
            restart_hours: cfg.as_ref().map(|c| c.restart_hours),
            perf_mode: cfg.as_ref().map(|c| c.perf_mode),
            aikar_flags: cfg.as_ref().is_some_and(|c| c.aikar_flags),
            duckdns_domain: cfg.as_ref().and_then(|c| c.duckdns_domain.clone()),
            tailscale_ip: network::detect_tailscale_ip(),
            autostart_enabled: autostart::is_enabled(),
            lan_ip: network::detect_lan_ip().map(|ip| ip.to_string()),
            world_size_bytes,
            running: running_name,
            stopping,
            uptime_secs,
            last_crash: self.0.last_crash.lock().unwrap().clone(),
            setup: SetupSummary {
                state: s.state,
                message: s.message,
                pct: s.pct,
            },
            tunnel: TunnelSummary {
                enabled: cfg.as_ref().is_some_and(|c| c.tunnel),
                claim_url: t.claim,
                address: t.address,
                saved_address: cfg.as_ref().and_then(|c| c.playit_address.clone()),
                error: t.error,
            },
            backups,
        }
    }

    pub fn begin_setup(&self, opts: setup::Options) -> Result<()> {
        {
            let mut s = self.0.setup.lock().unwrap();
            if s.state == "working" {
                bail!("a setup is already in progress");
            }
            *s = SetupState {
                state: "working".into(),
                message: "Starting...".into(),
                pct: 0,
            };
        }
        let _ = self.0.events.send(CoreEvent::SetupProgress {
            state: "working".into(),
            message: "Starting...".into(),
            pct: 0,
        });
        let me = self.clone();
        thread::spawn(move || {
            let m1 = me.clone();
            let m2 = me.clone();
            let mut last = u64::MAX;
            let res = setup::create_server(
                &opts,
                &mut |msg| {
                    m1.log(&format!("[setup] {msg}"));
                    {
                        let mut s = m1.0.setup.lock().unwrap();
                        s.message = msg.to_string();
                        s.pct = 0;
                    }
                    let _ = m1.0.events.send(CoreEvent::SetupProgress {
                        state: "working".into(),
                        message: msg.to_string(),
                        pct: 0,
                    });
                },
                &mut |label, d, t| {
                    if let Some(t) = t.filter(|t| *t > 0) {
                        let pct = d * 100 / t;
                        if pct != last {
                            last = pct;
                            let message = format!("Downloading {label}... {pct}%");
                            {
                                let mut s = m2.0.setup.lock().unwrap();
                                s.pct = pct;
                                s.message = message.clone();
                            }
                            let _ = m2.0.events.send(CoreEvent::SetupProgress {
                                state: "working".into(),
                                message,
                                pct,
                            });
                        }
                    }
                },
            );
            let (state, message, pct) = match res {
                Ok(_) => ("done".to_string(), "Server ready.".to_string(), 100),
                Err(e) => {
                    me.log(&format!("[setup] failed: {e:#}"));
                    ("error".to_string(), format!("{e:#}"), 0)
                }
            };
            *me.0.setup.lock().unwrap() = SetupState {
                state: state.clone(),
                message: message.clone(),
                pct,
            };
            let _ = me.0.events.send(CoreEvent::SetupProgress { state, message, pct });
            me.emit_status(None);
        });
        Ok(())
    }

    pub fn start(&self, name: &str, use_tunnel_override: Option<bool>) -> Result<()> {
        if !setup::valid_name(name) {
            bail!("bad server name");
        }
        let mut running = self.0.running.lock().unwrap();
        if running.is_some() {
            bail!("a server is already running");
        }
        let dir = paths::server_dir(name);
        let cfg = config::ServerConfig::load(&dir).context("server not found")?;
        let java_bin = java::find_java(&paths::java_dir(cfg.java_feature))
            .context("Java is missing - create the server again")?;

        // Ensure RCON is configured in server.properties before booting Paper.
        config::apply_rcon_properties(&dir, &cfg)?;
        acquire_pid_lock(&dir)?;

        let use_tunnel = use_tunnel_override.unwrap_or(cfg.tunnel);
        let child_pid = Arc::new(AtomicU32::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let force_kill = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        *running = Some(Running {
            name: name.to_string(),
            rcon_port: cfg.rcon_port,
            rcon_password: cfg.rcon_password.clone(),
            started_at: Instant::now(),
            child_pid: child_pid.clone(),
            shutdown: shutdown.clone(),
            force_kill: force_kill.clone(),
            tx: tx.clone(),
        });
        drop(running);
        *self.0.tunnel.lock().unwrap() = TunnelInfo::default();
        *self.0.last_crash.lock().unwrap() = None;
        self.emit_status(Some(name));

        let me = self.clone();
        let name = name.to_string();
        thread::spawn(move || {
            let _awake = power::KeepAwake::start();
            let _perf = power::PerfModeGuard::enter(cfg.perf_mode);
            me.log(&format!(
                "[blockhost] starting {name} ({} MB, port {})",
                cfg.heap_mb, cfg.port
            ));

            // Keep DuckDNS hostname synced on start if configured (zero-playit NZ direct ping).
            if let (Some(dom), Some(tok)) = (cfg.duckdns_domain.as_deref(), cfg.duckdns_token.as_deref()) {
                match network::update_duckdns(dom, tok) {
                    Ok(host) => me.log(&format!("[blockhost] DuckDNS updated: {host}")),
                    Err(e) => me.log(&format!("[blockhost] DuckDNS update skipped: {e:#}")),
                }
            }

            // Pre-start world backup (matching panel.ps1 Run-Server).
            if let Ok(z) = backup::create(&dir, &name, cfg.backup_keep) {
                me.log(&format!("[blockhost] pre-start backup saved: {}", z.display()));
            }

            if cfg.backup_hours > 0 {
                backup::scheduled(
                    tx.clone(),
                    dir.clone(),
                    name.clone(),
                    Duration::from_secs(u64::from(cfg.backup_hours) * 3600),
                    cfg.backup_keep,
                    shutdown.clone(),
                );
            }

            let tunnel_thread = use_tunnel.then(|| {
                let (m, sd, srv_dir) = (me.clone(), shutdown.clone(), dir.clone());
                thread::spawn(move || {
                    if let Err(e) = tunnel::ensure_agent(&mut |_, _| {}) {
                        m.0.tunnel.lock().unwrap().error = Some(format!("{e:#}"));
                        m.log(&format!("[blockhost] tunnel unavailable: {e:#}"));
                        m.emit_status(None);
                        return;
                    }
                    let (m1, m2) = (m.clone(), m.clone());
                    let _ = tunnel::run(
                        &sd,
                        &mut |ev| {
                            {
                                let mut t = m1.0.tunnel.lock().unwrap();
                                match ev {
                                    tunnel::Event::ClaimUrl(u) => t.claim = Some(u),
                                    tunnel::Event::Address(a) => {
                                        t.address = Some(a.clone());
                                        t.claim = None;
                                        if let Ok(mut c) = config::ServerConfig::load(&srv_dir) {
                                            c.playit_address = Some(a);
                                            let _ = c.save(&srv_dir);
                                        }
                                    }
                                }
                            }
                            m1.emit_status(None);
                        },
                        &mut |l| m2.log(&format!("[playit] {l}")),
                    );
                })
            });

            let policy = supervisor::Policy {
                max_uptime: (cfg.restart_hours > 0)
                    .then(|| Duration::from_secs(u64::from(cfg.restart_hours) * 3600)),
                ..supervisor::Policy::default()
            };
            let res = supervisor::supervise_full(
                || {
                    Ok(server::java_command_with_flags(
                        &java_bin,
                        &dir,
                        cfg.heap_mb,
                        cfg.aikar_flags,
                    ))
                },
                &policy,
                &shutdown,
                &force_kill,
                &child_pid,
                &rx,
                &mut |l| me.log(l),
            );
            match res {
                Ok(supervisor::Outcome::GaveUp { crashes }) => {
                    me.log(&format!(
                        "[blockhost] stopped after {crashes} crashes in 10 minutes"
                    ));
                    let (_, recent) = me.logs_since(0);
                    *me.0.last_crash.lock().unwrap() = crash::analyze_logs(&recent).or_else(|| {
                        Some(CrashDiagnosis {
                            title: "Server kept crashing".into(),
                            explanation: "The server crashed repeatedly so BlockHost paused restarts.".into(),
                            suggestion: "Check the Console tab for the last error message.".into(),
                        })
                    });
                }
                Ok(o) => me.log(&format!("[blockhost] stopped: {o:?}")),
                Err(e) => {
                    me.log(&format!("[blockhost] error: {e:#}"));
                    let (_, recent) = me.logs_since(0);
                    *me.0.last_crash.lock().unwrap() = crash::analyze_logs(&recent);
                }
            }
            shutdown.store(true, Ordering::SeqCst);
            if let Some(t) = tunnel_thread {
                let _ = t.join();
            }
            let _ = std::fs::remove_file(dir.join("blockhost.lock"));
            *me.0.running.lock().unwrap() = None;
            me.emit_status(None);
        });
        Ok(())
    }

    /// Request graceful stop, optionally warning online players for `warn_seconds` first.
    pub fn stop_with_warning(&self, warn_seconds: u64) {
        let info = self.0.running.lock().unwrap().as_ref().map(|r| {
            (
                r.rcon_port,
                r.rcon_password.clone(),
                r.shutdown.clone(),
                r.tx.clone(),
            )
        });
        let Some((port, pw, shutdown, tx)) = info else {
            return;
        };
        if warn_seconds == 0 {
            let _ = rcon::send_command(port, &pw, "save-all");
            shutdown.store(true, Ordering::SeqCst);
            self.emit_status(None);
            return;
        }
        let me = self.clone();
        thread::spawn(move || {
            let msg = format!("say Server stopping in {warn_seconds} seconds!");
            if rcon::send_command(port, &pw, &msg).is_err() {
                let _ = tx.send(msg);
            }
            me.log(&format!(
                "[blockhost] warned players; stopping in {warn_seconds} seconds..."
            ));
            thread::sleep(Duration::from_secs(warn_seconds));
            let _ = rcon::send_command(port, &pw, "save-all");
            shutdown.store(true, Ordering::SeqCst);
            me.emit_status(None);
        });
    }

    pub fn stop(&self) {
        self.stop_with_warning(0);
    }

    /// Force-kill a hung server process without waiting for save (caller MUST confirm with user first).
    pub fn force_stop(&self) {
        if let Some(r) = self.0.running.lock().unwrap().as_ref() {
            r.shutdown.store(true, Ordering::SeqCst);
            r.force_kill.store(true, Ordering::SeqCst);
        }
        self.emit_status(None);
    }

    /// Stop and wait (used on Ctrl-C / app quit so the world is saved).
    pub fn shutdown_and_wait(&self, timeout: Duration) {
        self.stop();
        let end = Instant::now() + timeout;
        while Instant::now() < end && self.0.running.lock().unwrap().is_some() {
            thread::sleep(Duration::from_millis(200));
        }
    }

    /// Execute a command on the live server (prefers RCON for structured reply; falls back to stdin).
    pub fn command(&self, cmd: &str) -> Result<String> {
        let cmd = cmd.trim().trim_start_matches('/');
        if cmd.is_empty() {
            bail!("empty command");
        }
        if cmd.contains('\n') || cmd.contains('\r') {
            bail!("one command at a time");
        }
        let (port, pw, tx) = match self.0.running.lock().unwrap().as_ref() {
            Some(r) => (r.rcon_port, r.rcon_password.clone(), r.tx.clone()),
            None => bail!("start the server first"),
        };
        if let Ok(resp) = rcon::send_command(port, &pw, cmd) {
            return Ok(resp.text);
        }
        tx.send(cmd.to_string())
            .map_err(|_| anyhow!("server is not accepting commands"))?;
        Ok(String::new())
    }

    pub fn add_player(&self, server_name: &str, username: &str, admin: bool) -> Result<String> {
        let dir = paths::server_dir(server_name);
        let live = self.live_rcon(server_name);
        let live_ref = live.as_ref().map(|(p, pw)| (*p, pw.as_str()));
        let res = players::add_player(&dir, username, admin, live_ref, players::DEFAULT_MOJANG_BASE)?;
        self.emit_status(Some(server_name));
        Ok(res)
    }

    pub fn remove_player(&self, server_name: &str, username: &str, admin_only: bool) -> Result<String> {
        let dir = paths::server_dir(server_name);
        let live = self.live_rcon(server_name);
        let live_ref = live.as_ref().map(|(p, pw)| (*p, pw.as_str()));
        let res = players::remove_player(&dir, username, admin_only, live_ref)?;
        self.emit_status(Some(server_name));
        Ok(res)
    }

    pub fn backup_now(&self, name: &str) -> Result<()> {
        if !setup::valid_name(name) {
            bail!("bad server name");
        }
        let dir = paths::server_dir(name);
        let keep = config::ServerConfig::load(&dir)
            .map(|c| c.backup_keep)
            .unwrap_or(10);
        let live = self
            .0
            .running
            .lock()
            .unwrap()
            .as_ref()
            .filter(|r| r.name == name)
            .map(|r| (r.rcon_port, r.rcon_password.clone(), r.tx.clone()));
        let (me, name) = (self.clone(), name.to_string());
        thread::spawn(move || {
            if let Some((port, pw, tx)) = &live {
                if rcon::send_command(*port, pw, "save-off").is_ok() {
                    let _ = rcon::send_command(*port, pw, "save-all flush");
                } else {
                    let _ = tx.send("save-off".into());
                    let _ = tx.send("save-all flush".into());
                }
                thread::sleep(Duration::from_secs(8));
            }
            match backup::create(&dir, &name, keep) {
                Ok(p) => me.log(&format!("[blockhost] backup saved: {}", p.display())),
                Err(e) => me.log(&format!("[blockhost] backup failed: {e:#}")),
            }
            if let Some((port, pw, tx)) = &live {
                if rcon::send_command(*port, pw, "save-on").is_err() {
                    let _ = tx.send("save-on".into());
                }
            }
            me.emit_status(Some(&name));
        });
        Ok(())
    }
}

/// Parse usernames from Minecraft's `list` reply, e.g.
/// "There are 2 of a max of 8 players online: Alice, Bob".
pub fn parse_online_players(list_reply: &str) -> Vec<String> {
    let Some((_, after)) = list_reply.split_once(':') else {
        return Vec::new();
    };
    after
        .split(',')
        .map(str::trim)
        .filter(|name| config::valid_username(name))
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_live_status_and_log_events() {
        let mgr = Manager::new();
        let mut rx = mgr.subscribe();
        mgr.log("hello core");
        mgr.emit_status(None);

        let ev1 = rx.try_recv().expect("expected log event");
        match ev1 {
            CoreEvent::LogLine { index, line } => {
                assert_eq!(index, 0);
                assert_eq!(line, "hello core");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let ev2 = rx.try_recv().expect("expected status event");
        match ev2 {
            CoreEvent::StatusChanged(snap) => {
                assert!(snap.hardware.recommended_heap_mb >= 1024);
                assert_eq!(snap.app_version, env!("CARGO_PKG_VERSION"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn reclaims_stale_pid_lock() {
        let dir = std::env::temp_dir().join(format!("bh-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Write a bogus dead PID that cannot exist
        std::fs::write(dir.join("blockhost.lock"), "99999999").unwrap();
        acquire_pid_lock(&dir).expect("should reclaim stale lock");
        assert_eq!(
            std::fs::read_to_string(dir.join("blockhost.lock")).unwrap(),
            std::process::id().to_string()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
