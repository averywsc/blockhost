//! Stop the machine from sleeping while the server runs, and optionally switch to
//! Windows High Performance power plan (restoring the previous plan on exit).

use anyhow::{bail, Result};

const HIGH_PERFORMANCE_GUID: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

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

/// Extract a power scheme GUID (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`) from `powercfg` output.
pub fn parse_scheme_guid(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.len() < 36 {
        return None;
    }
    for i in 0..=bytes.len() - 36 {
        let cand = &text[i..i + 36];
        let b = cand.as_bytes();
        let dashes_ok = b[8] == b'-' && b[13] == b'-' && b[18] == b'-' && b[23] == b'-';
        if !dashes_ok {
            continue;
        }
        let hex_ok = b
            .iter()
            .enumerate()
            .all(|(idx, &ch)| matches!(idx, 8 | 13 | 18 | 23) || ch.is_ascii_hexdigit());
        if hex_ok {
            return Some(cand.to_string());
        }
    }
    None
}

/// RAII guard that activates Windows High Performance power plan (if available on the machine)
/// and restores the user's previous plan on drop. Gracefully no-ops on Modern Standby laptops
/// where the High Performance GUID is unavailable, or on non-Windows OSes.
pub struct PerfModeGuard {
    previous_guid: Option<String>,
}

impl PerfModeGuard {
    pub fn enter(enabled: bool) -> Self {
        if !enabled {
            return Self { previous_guid: None };
        }
        Self {
            previous_guid: enter_perf_mode(),
        }
    }

    pub fn previous_guid(&self) -> Option<&str> {
        self.previous_guid.as_deref()
    }
}

impl Drop for PerfModeGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.previous_guid.take() {
            exit_perf_mode(&prev);
        }
    }
}

pub fn enter_perf_mode() -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("powercfg")
            .arg("/getactivescheme")
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let prev = parse_scheme_guid(&String::from_utf8_lossy(&out.stdout))?;
        let st = std::process::Command::new("powercfg")
            .args(["/setactive", HIGH_PERFORMANCE_GUID])
            .creation_flags(0x0800_0000)
            .status()
            .ok()?;
        if st.success() {
            Some(prev)
        } else {
            // Modern Standby (S0ix) laptops often omit the High Performance scheme; degrade cleanly.
            None
        }
    }
    #[cfg(not(windows))]
    {
        let _ = HIGH_PERFORMANCE_GUID;
        None
    }
}

pub fn exit_perf_mode(prev_guid: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("powercfg")
            .args(["/setactive", prev_guid])
            .creation_flags(0x0800_0000)
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = prev_guid;
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

/// Persistent Windows power settings (survive reboots).
pub fn apply_windows_power() -> Result<()> {
    if !cfg!(windows) {
        bail!(
            "this is only needed on Windows; on Linux/macOS BlockHost holds a sleep inhibitor automatically"
        );
    }
    for cmd in windows_power_commands() {
        let mut c = std::process::Command::new(cmd[0]);
        c.args(&cmd[1..]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let status = c.status()?;
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
        let _ = k.active();
        drop(k);
    }

    #[test]
    fn parses_scheme_guid() {
        assert_eq!(
            parse_scheme_guid("Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)")
                .as_deref(),
            Some("381b4222-f694-41f0-9685-ff5bb260df2e")
        );
        assert_eq!(parse_scheme_guid("nothing here"), None);
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
