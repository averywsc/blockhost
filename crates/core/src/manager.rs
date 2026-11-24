//! Shared state behind BlockHost UI shells (Tauri desktop app, CLI, service).
//! Owns the running server, log ring buffer, setup progress, tunnel info, and live event stream.

use crate::{
    backup, config,
    events::{CoreEvent, HardwareSummary, SetupSummary, StatusSnapshot, TunnelSummary},
    hardware, java, paths, power, server, setup, supervisor, tunnel,
};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
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
    shutdown: Arc<AtomicBool>,
    tx: Sender<String>,
}

struct Inner {
    hw: hardware::Hardware,
    logs: Mutex<Logs>,
    running: Mutex<Option<Running>>,
    setup: Mutex<SetupState>,
    tunnel: Mutex<TunnelInfo>,
    events: broadcast::Sender<CoreEvent>,
}

#[derive(Clone)]
pub struct Manager(Arc<Inner>);

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
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

    pub fn status(&self, selected: Option<&str>) -> StatusSnapshot {
        let servers = self.servers();
        let running = self.0.running.lock().unwrap();
        let running_name = running.as_ref().map(|r| r.name.clone());
        let stopping = running
            .as_ref()
            .is_some_and(|r| r.shutdown.load(Ordering::SeqCst));
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
        let backups: Vec<String> = current
            .as_ref()
            .map(|n| {
                backup::list(n)
                    .iter()
                    .filter_map(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
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
            port: cfg.as_ref().map(|c| c.port),
            mc_version: cfg.as_ref().map(|c| c.mc_version.clone()),
            paper_build: cfg.as_ref().map(|c| c.paper_build),
            running: running_name,
            stopping,
            setup: SetupSummary {
                state: s.state,
                message: s.message,
                pct: s.pct,
            },
            tunnel: TunnelSummary {
                claim_url: t.claim,
                address: t.address,
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

    pub fn start(&self, name: &str, use_tunnel: bool) -> Result<()> {
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
        let lock = dir.join("blockhost.lock");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map_err(|_| {
                anyhow!(
                    "this server looks like it is already running (if it crashed, delete {})",
                    lock.display()
                )
            })?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        *running = Some(Running {
            name: name.to_string(),
            shutdown: shutdown.clone(),
            tx: tx.clone(),
        });
        drop(running);
        *self.0.tunnel.lock().unwrap() = TunnelInfo::default();
        self.emit_status(Some(name));

        let me = self.clone();
        let name = name.to_string();
        thread::spawn(move || {
            let _awake = power::KeepAwake::start();
            me.log(&format!(
                "[blockhost] starting {name} ({} MB, port {})",
                cfg.heap_mb, cfg.port
            ));
            backup::scheduled(
                tx,
                dir.clone(),
                name.clone(),
                Duration::from_secs(6 * 3600),
                10,
                shutdown.clone(),
            );

            let tunnel_thread = use_tunnel.then(|| {
                let (m, sd) = (me.clone(), shutdown.clone());
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
                                        t.address = Some(a);
                                        t.claim = None;
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
                max_uptime: Some(Duration::from_secs(24 * 3600)),
                ..supervisor::Policy::default()
            };
            let res = supervisor::supervise(
                || Ok(server::java_command(&java_bin, &dir, cfg.heap_mb)),
                &policy,
                &shutdown,
                &rx,
                &mut |l| me.log(l),
            );
            match res {
                Ok(o) => me.log(&format!("[blockhost] stopped: {o:?}")),
                Err(e) => me.log(&format!("[blockhost] error: {e:#}")),
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

    pub fn stop(&self) {
        if let Some(r) = self.0.running.lock().unwrap().as_ref() {
            r.shutdown.store(true, Ordering::SeqCst);
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

    pub fn command(&self, cmd: &str) -> Result<()> {
        if cmd.contains('\n') || cmd.contains('\r') {
            bail!("one command at a time");
        }
        match self.0.running.lock().unwrap().as_ref() {
            Some(r) => {
                r.tx.send(cmd.to_string())
                    .map_err(|_| anyhow!("server is not accepting commands"))
            }
            None => bail!("start the server first"),
        }
    }

    pub fn backup_now(&self, name: &str) -> Result<()> {
        if !setup::valid_name(name) {
            bail!("bad server name");
        }
        let dir = paths::server_dir(name);
        let live = self
            .0
            .running
            .lock()
            .unwrap()
            .as_ref()
            .filter(|r| r.name == name)
            .map(|r| r.tx.clone());
        let (me, name) = (self.clone(), name.to_string());
        thread::spawn(move || {
            if let Some(tx) = &live {
                let _ = tx.send("save-off".into());
                let _ = tx.send("save-all flush".into());
                thread::sleep(Duration::from_secs(15));
            }
            match backup::create(&dir, &name, 10) {
                Ok(p) => me.log(&format!("[blockhost] backup saved: {}", p.display())),
                Err(e) => me.log(&format!("[blockhost] backup failed: {e:#}")),
            }
            if let Some(tx) = &live {
                let _ = tx.send("save-on".into());
            }
            me.emit_status(Some(&name));
        });
        Ok(())
    }
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
}
