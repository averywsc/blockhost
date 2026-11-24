use anyhow::{Context, Result};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{channel, Receiver},
    thread,
    time::{Duration, Instant},
};

/// A running server process with its console captured.
pub struct Server {
    child: Child,
    stdin: ChildStdin,
    pub lines: Receiver<String>,
}

/// Build the java command for a Paper server living in `dir`.
pub fn java_command(java: &Path, dir: &Path, heap_mb: u64) -> Command {
    let mut c = Command::new(java);
    c.current_dir(dir)
        .arg(format!("-Xms{heap_mb}M"))
        .arg(format!("-Xmx{heap_mb}M"))
        .args([
            "-XX:+UseG1GC",
            "-XX:+ParallelRefProcEnabled",
            "-XX:MaxGCPauseMillis=200",
            "-XX:+DisableExplicitGC",
        ])
        .args(["-jar", "server.jar", "--nogui"]);
    c
}

impl Server {
    pub fn spawn(mut cmd: Command) -> Result<Self> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash when run in background
        }
        let mut child = cmd.spawn().context("could not start server process")?;
        let stdin = child.stdin.take().context("no stdin")?;
        let (tx, rx) = channel();
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let tx = tx.clone();
            thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Ok(Self {
            child,
            stdin,
            lines: rx,
        })
    }

    pub fn send(&mut self, command: &str) -> Result<()> {
        writeln!(self.stdin, "{command}")?;
        self.stdin.flush()?;
        Ok(())
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        Ok(self.child.try_wait()?)
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Ask the server to save and stop; kill it only if it ignores us.
    pub fn stop(&mut self, timeout: Duration) -> Result<ExitStatus> {
        let _ = self.send("stop");
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(s) = self.try_wait()? {
                return Ok(s);
            }
            thread::sleep(Duration::from_millis(100));
        }
        self.child.kill()?;
        Ok(self.child.wait()?)
    }
}
