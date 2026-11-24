//! Per-user login autostart (no admin rights required).
//! Uses the Windows Startup folder (`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`)
//! with an override env var `BLOCKHOST_STARTUP_DIR` for deterministic tests.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const STARTUP_FILENAME: &str = "BlockHost.cmd";

pub fn startup_dir() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("BLOCKHOST_STARTUP_DIR") {
        return Ok(PathBuf::from(p));
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .context("APPDATA environment variable is not set")?;
        Ok(appdata.join(r"Microsoft\Windows\Start Menu\Programs\Startup"))
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME environment variable is not set")?;
        Ok(home.join(".config/autostart"))
    }
}

pub fn startup_file_in(dir: &Path) -> PathBuf {
    dir.join(STARTUP_FILENAME)
}

pub fn is_enabled_in(dir: &Path) -> bool {
    startup_file_in(dir).exists()
}

pub fn is_enabled() -> bool {
    startup_dir().map(|d| is_enabled_in(&d)).unwrap_or(false)
}

pub fn set_enabled_in(dir: &Path, enabled: bool, target_exe: &Path, args: &str) -> Result<()> {
    let file = startup_file_in(dir);
    if enabled {
        std::fs::create_dir_all(dir)?;
        let content = if args.trim().is_empty() {
            format!("@echo off\r\nstart \"\" \"{}\"\r\n", target_exe.display())
        } else {
            format!(
                "@echo off\r\nstart \"\" \"{}\" {}\r\n",
                target_exe.display(),
                args.trim()
            )
        };
        std::fs::write(&file, content)?;
    } else if file.exists() {
        std::fs::remove_file(&file)?;
    }
    Ok(())
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let dir = startup_dir()?;
    let exe = std::env::current_exe().context("cannot determine current executable path")?;
    set_enabled_in(&dir, enabled, &exe, "--minimized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enables_and_disables_startup_entry() {
        let dir = std::env::temp_dir().join(format!("bh-autostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let exe = Path::new(r"C:\Program Files\BlockHost\BlockHost.exe");

        assert!(!is_enabled_in(&dir));
        set_enabled_in(&dir, true, exe, "--minimized").unwrap();
        assert!(is_enabled_in(&dir));
        let txt = std::fs::read_to_string(startup_file_in(&dir)).unwrap();
        assert!(txt.contains("BlockHost.exe"));
        assert!(txt.contains("--minimized"));

        set_enabled_in(&dir, false, exe, "").unwrap();
        assert!(!is_enabled_in(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
