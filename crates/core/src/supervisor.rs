use crate::server::Server;
use anyhow::Result;
use std::{
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct Policy {
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    /// More than this many crashes inside `crash_window` and we stop trying.
    pub max_crashes: usize,
    pub crash_window: Duration,
    /// A run at least this long resets the backoff.
    pub healthy_after: Duration,
    pub stop_timeout: Duration,
    /// Gracefully restart after this much uptime (keeps long-running servers fresh).
    pub max_uptime: Option<Duration>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(5),
            max_backoff: Duration::from_secs(120),
            max_crashes: 5,
            crash_window: Duration::from_secs(600),
            healthy_after: Duration::from_secs(600),
            stop_timeout: Duration::from_secs(60),
            max_uptime: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// We were asked to shut down and the server stopped cleanly.
    Shutdown,
    /// The server exited with code 0 on its own (e.g. an operator typed `stop`).
    CleanExit,
    /// Crash loop: gave up so a broken server doesn't thrash the disk.
    GaveUp { crashes: usize },
}

/// Keep a server running forever: restart on crash with exponential backoff,
/// stop gracefully on `shutdown`, give up on crash loops. Server console lines
/// go to `on_line`; anything arriving on `commands` is typed into the console.
pub fn supervise(
    mut make_cmd: impl FnMut() -> Result<Command>,
    policy: &Policy,
    shutdown: &AtomicBool,
    commands: &Receiver<String>,
    on_line: &mut dyn FnMut(&str),
) -> Result<Outcome> {
    let mut crashes: Vec<Instant> = Vec::new();
    let mut backoff = policy.initial_backoff;

    loop {
        let mut srv = Server::spawn(make_cmd()?)?;
        let started = Instant::now();

        let mut scheduled = false;
        let status = loop {
            if shutdown.load(Ordering::SeqCst) {
                drain(&srv, on_line);
                srv.stop(policy.stop_timeout)?;
                drain(&srv, on_line);
                return Ok(Outcome::Shutdown);
            }
            if policy.max_uptime.is_some_and(|m| started.elapsed() >= m) {
                on_line("[blockhost] scheduled restart");
                let s = srv.stop(policy.stop_timeout)?;
                drain(&srv, on_line);
                scheduled = true;
                break s;
            }
            while let Ok(c) = commands.try_recv() {
                let _ = srv.send(&c);
            }
            match srv.lines.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => on_line(&line),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => thread::sleep(Duration::from_millis(50)),
            }
            if let Some(s) = srv.try_wait()? {
                // give reader threads a moment to flush the last lines
                thread::sleep(Duration::from_millis(100));
                drain(&srv, on_line);
                break s;
            }
        };

        if scheduled {
            continue;
        }
        if status.success() {
            return Ok(Outcome::CleanExit);
        }

        let now = Instant::now();
        if now.duration_since(started) >= policy.healthy_after {
            backoff = policy.initial_backoff;
            crashes.clear();
        }
        crashes.push(now);
        crashes.retain(|t| now.duration_since(*t) <= policy.crash_window);
        if crashes.len() > policy.max_crashes {
            return Ok(Outcome::GaveUp {
                crashes: crashes.len(),
            });
        }

        on_line(&format!(
            "[blockhost] server crashed ({status}); restarting in {backoff:?}"
        ));
        let wake = Instant::now() + backoff;
        while Instant::now() < wake {
            if shutdown.load(Ordering::SeqCst) {
                return Ok(Outcome::Shutdown);
            }
            thread::sleep(Duration::from_millis(50));
        }
        backoff = (backoff * 2).min(policy.max_backoff);
    }
}

fn drain(srv: &Server, on_line: &mut dyn FnMut(&str)) {
    while let Ok(l) = srv.lines.try_recv() {
        on_line(&l);
    }
}
