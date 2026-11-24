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

/// Build the java command for a Paper or Fabric server living in `dir`.
pub fn java_command(java: &Path, dir: &Path, heap_mb: u64) -> Command {
    java_command_with_flags(java, dir, heap_mb, false)
}

/// Build the java command with optional Aikar GC performance flags.
pub fn java_command_with_flags(java: &Path, dir: &Path, heap_mb: u64, aikar: bool) -> Command {
    let mut c = Command::new(java);
    c.current_dir(dir)
        .arg(format!("-Xms{heap_mb}M"))
        .arg(format!("-Xmx{heap_mb}M"));
    if aikar {
        let large = heap_mb >= 12288;
        c.args([
            "-XX:+UseG1GC",
            "-XX:+ParallelRefProcEnabled",
            "-XX:MaxGCPauseMillis=200",
            "-XX:+UnlockExperimentalVMOptions",
            "-XX:+DisableExplicitGC",
            "-XX:+AlwaysPreTouch",
            if large {
                "-XX:G1NewSizePercent=40"
            } else {
                "-XX:G1NewSizePercent=30"
            },
            if large {
                "-XX:G1MaxNewSizePercent=50"
            } else {
                "-XX:G1MaxNewSizePercent=40"
            },
            if large {
                "-XX:G1HeapRegionSize=16M"
            } else {
                "-XX:G1HeapRegionSize=8M"
            },
            if large {
                "-XX:G1ReservePercent=15"
            } else {
                "-XX:G1ReservePercent=20"
            },
            "-XX:G1HeapWastePercent=5",
            "-XX:G1MixedGCCountTarget=4",
            if large {
                "-XX:InitiatingHeapOccupancyPercent=20"
            } else {
                "-XX:InitiatingHeapOccupancyPercent=15"
            },
            "-XX:G1MixedGCLiveThresholdPercent=90",
            "-XX:G1RSetUpdatingPauseTimePercent=5",
            "-XX:SurvivorRatio=32",
            "-XX:+PerfDisableSharedMem",
            "-XX:MaxTenuringThreshold=1",
        ]);
    } else {
        c.args([
            "-XX:+UseG1GC",
            "-XX:+ParallelRefProcEnabled",
            "-XX:MaxGCPauseMillis=200",
            "-XX:+DisableExplicitGC",
        ]);
    }
    c.args(["-jar", "server.jar", "--nogui"]);
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

    pub fn id(&self) -> u32 {
        self.child.id()
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
