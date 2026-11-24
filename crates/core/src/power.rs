//! Stop the machine from sleeping while the server runs. This is what makes an
//! old laptop a real 24/7 host (lid closed, screen off, still serving).

use anyhow::{bail, Result};

/// Holds a "don't sleep" request for as long as it's alive.
pub struct KeepAwake {
    #[cfg(unix)]
    child: Option<std::process::Child>,
}

#[cfg(windows)]
extern "system" {
    fn SetThreadExecutionState(flags: u32) -> u32;
}

impl KeepAwake {
    /// Best effort; never fails the server if the OS tool is missing.
    pub fn start() -> Self {
        #[cfg(windows)]
        {
            const ES_CONTINUOUS: u32 = 0x8000_0000;
            const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
            // Applies to the calling thread, so call this from the thread that stays alive.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            Self {}
        }
        #[cfg(unix)]
        {
            use std::process::{Command, Stdio};
            let spawned = if cfg!(target_os = "macos") {
                Command::new("caffeinate")
                    .args(["-i", "-m", "-s"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            } else {
                Command::new("systemd-inhibit")
                    .args([
                        "--what=sleep:idle:handle-lid-switch",
                        "--who=BlockHost",
                        "--why=Minecraft server is running",
                        "--mode=block",
                        "sleep",
                        "infinity",
                    ])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            };
            Self { child: spawned.ok() }
        }
    }

    pub fn active(&self) -> bool {
        #[cfg(unix)]
        {
            self.child.is_some()
        }
        #[cfg(windows)]
        {
            true
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            SetThreadExecutionState(0x8000_0000);
        }
        #[cfg(unix)]
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// The powercfg calls that make a Windows laptop keep running with the lid closed (plugged in).
pub fn windows_power_commands() -> Vec<Vec<&'static str>> {
    vec![
        vec!["powercfg", "/change", "standby-timeout-ac", "0"],
        vec!["powercfg", "/change", "hibernate-timeout-ac", "0"],
        vec![
            "powercfg",
            "/setacvalueindex",
            "SCHEME_CURRENT",
            "SUB_BUTTONS",
            "LIDACTION",
            "0",
        ],
        vec!["powercfg", "/setactive", "SCHEME_CURRENT"],
    ]
}

/// Persistent Windows power settings (survive reboots). Run once from an admin-or-user shell.
pub fn apply_windows_power() -> Result<()> {
    if !cfg!(windows) {
        bail!(
            "this is only needed on Windows; on Linux/macOS BlockHost holds a sleep inhibitor automatically"
        );
    }
    for cmd in windows_power_commands() {
        let status = std::process::Command::new(cmd[0]).args(&cmd[1..]).status()?;
        if !status.success() {
            bail!("`{}` failed", cmd.join(" "));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_awake_starts_and_drops_cleanly() {
        let k = KeepAwake::start();
        let _ = k.active(); // must not panic on any OS
        drop(k);
    }

    #[test]
    fn windows_commands_disable_lid_sleep() {
        let cmds = windows_power_commands();
        assert!(cmds
            .iter()
            .any(|c| c.contains(&"LIDACTION") && c.last() == Some(&"0")));
        assert!(cmds.last().unwrap().contains(&"/setactive"));
    }
}
