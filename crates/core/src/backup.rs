use crate::paths;
use anyhow::{bail, Context, Result};
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn backups_dir(name: &str) -> PathBuf {
    paths::root().join("backups").join(name)
}

/// UTC timestamp like 20231114-221320 (no chrono dependency).
pub fn timestamp(secs: u64) -> String {
    let (y, m, d) = civil((secs / 86400) as i64);
    let r = secs % 86400;
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        r / 3600,
        (r % 3600) / 60,
        r % 60
    )
}

fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m as u32, d as u32)
}

fn now_stamp() -> String {
    timestamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

fn level_name(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("server.properties"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("level-name=").map(|v| v.trim().to_string()))
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "world".into())
}

/// What gets backed up: the world folders (world, world_nether, ...) plus player/settings files.
fn items_to_back_up(dir: &Path) -> Vec<String> {
    let level = level_name(dir);
    let mut items = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if e.path().is_dir() && n.starts_with(&level) {
                items.push(n);
            }
        }
    }
    for f in [
        "whitelist.json",
        "ops.json",
        "banned-players.json",
        "banned-ips.json",
        "server.properties",
        "blockhost.json",
    ] {
        if dir.join(f).exists() {
            items.push(f.to_string());
        }
    }
    items.sort();
    items
}

fn add_path(
    zip: &mut zip::ZipWriter<File>,
    base: &Path,
    rel: &Path,
    opts: zip::write::SimpleFileOptions,
) -> Result<()> {
    let full = base.join(rel);
    let name = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if full.is_dir() {
        for e in std::fs::read_dir(&full)? {
            add_path(zip, base, &rel.join(e?.file_name()), opts)?;
        }
    } else if rel.file_name().is_some_and(|n| n != "session.lock") {
        zip.start_file(name, opts)?;
        std::io::copy(&mut File::open(&full)?, zip)?;
    }
    Ok(())
}

/// Zip the server's worlds and settings into backups/<name>/<timestamp>.zip, then prune old ones.
pub fn create(server_dir: &Path, name: &str, keep: usize) -> Result<PathBuf> {
    let items = items_to_back_up(server_dir);
    if items.is_empty() {
        bail!("nothing to back up yet (world not generated?)");
    }
    let out_dir = backups_dir(name);
    std::fs::create_dir_all(&out_dir)?;
    let dest = out_dir.join(format!("{}.zip", now_stamp()));
    let part = dest.with_extension("zip.part");
    let mut zip = zip::ZipWriter::new(File::create(&part)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    for item in &items {
        add_path(&mut zip, server_dir, Path::new(item), opts)?;
    }
    zip.finish()?.flush()?;
    std::fs::rename(&part, &dest)?;
    prune(name, keep)?;
    Ok(dest)
}

/// Newest first.
pub fn list(name: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(backups_dir(name))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "zip"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

pub fn prune(name: &str, keep: usize) -> Result<()> {
    for old in list(name).into_iter().skip(keep.max(1)) {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

/// Restore a backup. Existing worlds are moved aside (never deleted) into pre-restore-<time>/.
/// The server must not be running.
pub fn restore(server_dir: &Path, zip_path: &Path) -> Result<PathBuf> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path).context("cannot open backup")?)?;
    let mut tops: Vec<String> = archive
        .file_names()
        .filter_map(|n| n.split('/').next().map(String::from))
        .collect();
    tops.sort();
    tops.dedup();

    let aside = server_dir.join(format!("pre-restore-{}", now_stamp()));
    std::fs::create_dir_all(&aside)?;
    for t in &tops {
        let cur = server_dir.join(t);
        if cur.exists() {
            std::fs::rename(&cur, aside.join(t))?;
        }
    }
    archive.extract(server_dir)?;
    Ok(aside)
}

/// Background thread: every `every`, flush the world to disk, zip it, resume saving.
pub fn scheduled(
    commands: Sender<String>,
    dir: PathBuf,
    name: String,
    every: Duration,
    keep: usize,
    shutdown: Arc<AtomicBool>,
) {
    thread::spawn(move || loop {
        let mut waited = Duration::ZERO;
        while waited < every {
            if shutdown.load(Ordering::SeqCst) {
                return;
            }
            thread::sleep(Duration::from_secs(1));
            waited += Duration::from_secs(1);
        }
        let _ = commands.send("save-off".into());
        let _ = commands.send("save-all flush".into());
        thread::sleep(Duration::from_secs(15));
        match create(&dir, &name, keep) {
            Ok(p) => println!("[blockhost] backup saved: {}", p.display()),
            Err(e) => eprintln!("[blockhost] backup failed: {e:#}"),
        }
        let _ = commands.send("save-on".into());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_correct() {
        assert_eq!(timestamp(0), "19700101-000000");
        assert_eq!(timestamp(1_700_000_000), "20231114-221320");
        assert_eq!(timestamp(1_709_164_800), "20240229-000000"); // leap day
    }

    #[test]
    fn backup_and_restore_roundtrip() {
        let home = std::env::temp_dir().join(format!("bh-bk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::env::set_var("BLOCKHOST_HOME", &home);
        let srv = home.join("servers/t");
        std::fs::create_dir_all(srv.join("world/region")).unwrap();
        std::fs::create_dir_all(srv.join("world_nether")).unwrap();
        std::fs::write(srv.join("world/region/r.0.0.mca"), b"original").unwrap();
        std::fs::write(srv.join("world/session.lock"), b"x").unwrap();
        std::fs::write(srv.join("whitelist.json"), b"[]").unwrap();
        std::fs::write(srv.join("server.jar"), b"jar").unwrap();

        let z = create(&srv, "t", 10).unwrap();
        let names: Vec<String> = zip::ZipArchive::new(File::open(&z).unwrap())
            .unwrap()
            .file_names()
            .map(String::from)
            .collect();
        assert!(names.contains(&"world/region/r.0.0.mca".to_string()));
        assert!(!names
            .iter()
            .any(|n| n.contains("session.lock") || n.contains("server.jar")));

        std::fs::write(srv.join("world/region/r.0.0.mca"), b"griefed").unwrap();
        let aside = restore(&srv, &z).unwrap();
        assert_eq!(
            std::fs::read(srv.join("world/region/r.0.0.mca")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(aside.join("world/region/r.0.0.mca")).unwrap(),
            b"griefed"
        );
        std::fs::remove_dir_all(home).unwrap();
    }
}
