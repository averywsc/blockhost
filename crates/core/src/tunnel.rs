//! Public access without router setup, via the playit.gg agent (free tunnel).
//! Best effort: the agent's output format isn't a stable API, so parsing is tolerant,
//! and the one-time claim step (link a free playit account, create a "Minecraft Java"
//! tunnel pointing at the server port) is done by the owner in their browser.

use crate::{net, paths, server::Server};
use anyhow::{bail, Result};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// Open this once in a browser to link the agent to a (free) playit account.
    ClaimUrl(String),
    /// The address friends type into Minecraft.
    Address(String),
}

pub fn agent_asset() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("playit-linux-amd64"),
        ("linux", "aarch64") => Ok("playit-linux-aarch64"),
        ("windows", "x86_64") => Ok("playit-windows-x86_64-signed.exe"),
        (os, arch) => {
            bail!("no prebuilt playit agent for {os}/{arch}; set up port forwarding instead")
        }
    }
}

pub fn agent_path() -> PathBuf {
    paths::root()
        .join("bin")
        .join(if cfg!(windows) { "playit.exe" } else { "playit" })
}

pub fn ensure_agent(progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<PathBuf> {
    let path = agent_path();
    if path.exists() {
        return Ok(path);
    }
    let url = format!(
        "https://github.com/playit-cloud/playit-agent/releases/latest/download/{}",
        agent_asset()?
    );
    net::download(&url, &path, None, progress)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}

pub fn parse_line(line: &str) -> Option<Event> {
    for raw in line.split_whitespace() {
        let tok = raw.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')' | '<' | '>'));
        if tok.contains("playit.gg/claim/") {
            return Some(Event::ClaimUrl(tok.to_string()));
        }
        let host = tok.split(':').next().unwrap_or(tok);
        let looks_like_tunnel = [".playit.gg", ".joinmc.link", ".ply.gg", ".playit.plus"]
            .iter()
            .any(|s| host.ends_with(s));
        if looks_like_tunnel && !tok.starts_with("http") && host.contains('.') {
            return Some(Event::Address(tok.to_string()));
        }
    }
    None
}

/// Run the agent until `shutdown`, restarting it if it dies.
pub fn run(
    shutdown: &AtomicBool,
    on_event: &mut dyn FnMut(Event),
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    let exe = agent_path();
    while !shutdown.load(Ordering::SeqCst) {
        let mut srv = Server::spawn(Command::new(&exe))?;
        let started = Instant::now();
        loop {
            if shutdown.load(Ordering::SeqCst) {
                srv.kill();
                return Ok(());
            }
            if let Ok(line) = srv.lines.recv_timeout(Duration::from_millis(250)) {
                if let Some(ev) = parse_line(&line) {
                    on_event(ev);
                }
                on_line(&line);
            }
            if srv.try_wait()?.is_some() {
                break;
            }
        }
        let pause = if started.elapsed() < Duration::from_secs(10) {
            30
        } else {
            5
        };
        for _ in 0..pause * 10 {
            if shutdown.load(Ordering::SeqCst) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_claim_url() {
        assert_eq!(
            parse_line("Visit link to setup https://playit.gg/claim/abc123, then click Add"),
            Some(Event::ClaimUrl("https://playit.gg/claim/abc123".into()))
        );
    }

    #[test]
    fn finds_tunnel_address_with_port() {
        assert_eq!(
            parse_line("tunnel ready: market-velocity.gl.joinmc.link:25565"),
            Some(Event::Address("market-velocity.gl.joinmc.link:25565".into()))
        );
    }

    #[test]
    fn ignores_unrelated_lines() {
        assert_eq!(parse_line("agent connected to control server"), None);
        assert_eq!(parse_line("see https://playit.gg/account/tunnels"), None);
    }
}
