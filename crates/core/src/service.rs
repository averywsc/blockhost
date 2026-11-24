//! Start-at-boot: systemd (Linux), Task Scheduler (Windows), launchd (macOS).
//! The text generators are pure so they can be tested anywhere.

use crate::paths;
use anyhow::{bail, Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub fn run_args(name: &str, tunnel: bool) -> Vec<String> {
    let mut a = vec!["run".to_string(), name.to_string()];
    if tunnel {
        a.push("--tunnel".into());
    }
    a
}

pub fn systemd_unit(exe: &Path, name: &str, home: &Path, tunnel: bool) -> String {
    format!(
        "[Unit]\nDescription=BlockHost Minecraft server ({name})\nAfter=network-online.target\nWants=network-online.target\n\n\
         [Service]\nType=simple\nEnvironment=BLOCKHOST_HOME={home}\nExecStart=\"{exe}\" {args}\n\
         Restart=on-failure\nRestartSec=10\nTimeoutStopSec=120\nKillSignal=SIGINT\n\n\
         [Install]\nWantedBy=default.target\n",
        home = home.display(),
        exe = exe.display(),
        args = run_args(name, tunnel).join(" "),
    )
}

pub fn launchd_plist(exe: &Path, name: &str, home: &Path, tunnel: bool) -> String {
    let args: String = std::iter::once(exe.display().to_string())
        .chain(run_args(name, tunnel))
        .map(|a| format!("    <string>{a}</string>\n"))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>com.blockhost.{name}</string>\n  <key>ProgramArguments</key>\n  <array>\n{args}  </array>\n  <key>EnvironmentVariables</key><dict><key>BLOCKHOST_HOME</key><string>{home}</string></dict>\n  <key>RunAtLoad</key><true/>\n  <key>KeepAlive</key><true/>\n</dict>\n</plist>\n",
        home = home.display()
    )
}

pub fn windows_launcher(exe: &Path, name: &str, home: &Path, tunnel: bool) -> String {
    format!(
        "@echo off\r\nset BLOCKHOST_HOME={}\r\n\"{}\" {}\r\n",
        home.display(),
        exe.display(),
        run_args(name, tunnel).join(" ")
    )
}

fn sh(program: &str, args: &[&str]) -> Result<()> {
    let st = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("could not run {program}"))?;
    if !st.success() {
        bail!("`{program} {}` failed", args.join(" "));
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

pub fn install(name: &str, tunnel: bool) -> Result<String> {
    let exe = std::env::current_exe()?;
    let root = paths::root();
    match std::env::consts::OS {
        "linux" => {
            let dir = home_dir()?.join(".config/systemd/user");
            std::fs::create_dir_all(&dir)?;
            let unit = format!("blockhost-{name}.service");
            std::fs::write(dir.join(&unit), systemd_unit(&exe, name, &root, tunnel))?;
            sh("systemctl", &["--user", "daemon-reload"])?;
            sh("systemctl", &["--user", "enable", "--now", &unit])?;
            let user = std::env::var("USER").unwrap_or_default();
            let linger = sh("loginctl", &["enable-linger", &user]).is_ok();
            Ok(format!(
                "Installed {unit}.{}",
                if linger {
                    ""
                } else {
                    " Run `sudo loginctl enable-linger $USER` so it starts at boot without a login."
                }
            ))
        }
        "macos" => {
            let path = home_dir()?.join(format!("Library/LaunchAgents/com.blockhost.{name}.plist"));
            std::fs::write(&path, launchd_plist(&exe, name, &root, tunnel))?;
            sh("launchctl", &["load", "-w", &path.to_string_lossy()])?;
            Ok(format!("Installed {}", path.display()))
        }
        "windows" => {
            std::fs::create_dir_all(&root)?;
            let cmd = root.join(format!("run-{name}.cmd"));
            std::fs::write(&cmd, windows_launcher(&exe, name, &root, tunnel))?;
            let task = format!("BlockHost-{name}");
            let tr = format!("\"{}\"", cmd.display());
            sh(
                "schtasks",
                &[
                    "/create", "/tn", &task, "/tr", &tr, "/sc", "onstart", "/delay", "0000:30", "/ru",
                    "SYSTEM", "/rl", "HIGHEST", "/f",
                ],
            )
            .context("creating the task needs an Administrator terminal")?;
            Ok(format!("Installed scheduled task {task} (starts at boot)."))
        }
        other => bail!("service install not supported on {other}"),
    }
}

pub fn uninstall(name: &str) -> Result<String> {
    match std::env::consts::OS {
        "linux" => {
            let unit = format!("blockhost-{name}.service");
            let _ = sh("systemctl", &["--user", "disable", "--now", &unit]);
            let _ = std::fs::remove_file(home_dir()?.join(".config/systemd/user").join(&unit));
            let _ = sh("systemctl", &["--user", "daemon-reload"]);
            Ok(format!("Removed {unit}"))
        }
        "macos" => {
            let path = home_dir()?.join(format!("Library/LaunchAgents/com.blockhost.{name}.plist"));
            let _ = sh("launchctl", &["unload", &path.to_string_lossy()]);
            let _ = std::fs::remove_file(path);
            Ok("Removed launch agent".into())
        }
        "windows" => {
            sh(
                "schtasks",
                &["/delete", "/tn", &format!("BlockHost-{name}"), "/f"],
            )?;
            let _ = std::fs::remove_file(paths::root().join(format!("run-{name}.cmd")));
            Ok("Removed scheduled task".into())
        }
        other => bail!("not supported on {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_unit_is_wired_for_graceful_stop() {
        let u = systemd_unit(
            Path::new("/usr/bin/blockhost"),
            "smp",
            Path::new("/data/bh"),
            true,
        );
        assert!(u.contains("ExecStart=\"/usr/bin/blockhost\" run smp --tunnel"));
        assert!(u.contains("KillSignal=SIGINT"));
        assert!(u.contains("Environment=BLOCKHOST_HOME=/data/bh"));
        assert!(u.contains("WantedBy=default.target"));
    }

    #[test]
    fn windows_launcher_sets_home() {
        let s = windows_launcher(
            Path::new("C:\\bh\\blockhost.exe"),
            "smp",
            Path::new("C:\\bh"),
            false,
        );
        assert!(s.contains("set BLOCKHOST_HOME=C:\\bh"));
        assert!(s.contains("run smp\r\n") && !s.contains("--tunnel"));
    }

    #[test]
    fn plist_has_label_and_keepalive() {
        let p = launchd_plist(Path::new("/bin/bh"), "smp", Path::new("/h"), false);
        assert!(p.contains("com.blockhost.smp") && p.contains("<key>KeepAlive</key><true/>"));
    }
}
