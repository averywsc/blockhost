use std::path::PathBuf;

/// Root folder for everything BlockHost stores (Java, servers, backups).
/// Override with the BLOCKHOST_HOME environment variable.
pub fn root() -> PathBuf {
    if let Some(p) = std::env::var_os("BLOCKHOST_HOME") {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join("BlockHost")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        home.join("Library/Application Support/BlockHost")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(x).join("blockhost");
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        home.join(".local/share/blockhost")
    }
}

pub fn java_dir(feature: u32) -> PathBuf {
    root().join("java").join(feature.to_string())
}

pub fn server_dir(name: &str) -> PathBuf {
    root().join("servers").join(name)
}
