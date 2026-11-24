//! Redacted diagnostics zip exporter: bundles hardware summary, sanitized config,
//! sanitized server.properties, installed addons list, and recent server logs
//! while ALWAYS stripping secrets (`rcon_password`, `duckdns_token`, `discord_webhook`).

use crate::{config, hardware, plugins};
use anyhow::Result;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

/// Redact sensitive keys from `server.properties` text before exporting.
pub fn redact_properties(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            if line.starts_with("rcon.password=") {
                "rcon.password=[REDACTED]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Export a safe, secret-redacted diagnostics `.zip` for troubleshooting.
pub fn export_diagnostics_zip(server_dir: &Path, dest_zip: &Path, recent_logs: &[String]) -> Result<PathBuf> {
    if let Some(parent) = dest_zip.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest_zip.with_extension("zip.part");
    let mut zip = zip::ZipWriter::new(File::create(&part)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // 1. System & hardware summary
    let hw = hardware::detect();
    let prof = hardware::recommend(&hw);
    let sys_txt = format!(
        "BlockHost v{}\nOS: {} ({})\nRAM: {} MB\nCPU cores: {}\nRecommended tier: {} ({} MB heap)\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        hw.total_ram_mb,
        hw.cpu_cores,
        prof.tier,
        prof.heap_mb
    );
    zip.start_file("system.txt", opts)?;
    zip.write_all(sys_txt.as_bytes())?;

    // 2. Redacted blockhost.json
    if let Ok(mut cfg) = config::ServerConfig::load(server_dir) {
        cfg.rcon_password = "[REDACTED]".into();
        if cfg.duckdns_token.is_some() {
            cfg.duckdns_token = Some("[REDACTED]".into());
        }
        if cfg.discord_webhook.is_some() {
            cfg.discord_webhook = Some("[REDACTED]".into());
        }
        zip.start_file("blockhost.redacted.json", opts)?;
        zip.write_all(&serde_json::to_vec_pretty(&cfg)?)?;

        let addons = plugins::list_addons(server_dir, cfg.software);
        let list = addons
            .iter()
            .map(|a| format!("{} ({} bytes)", a.filename, a.size_bytes))
            .collect::<Vec<_>>()
            .join("\n");
        zip.start_file("addons.txt", opts)?;
        zip.write_all(list.as_bytes())?;
    }

    // 3. Redacted server.properties
    if let Ok(props) = std::fs::read_to_string(server_dir.join("server.properties")) {
        zip.start_file("server.properties.redacted", opts)?;
        zip.write_all(redact_properties(&props).as_bytes())?;
    }

    // 4. Server latest.log (last 500 lines if present)
    if let Ok(log) = std::fs::read_to_string(server_dir.join("logs/latest.log")) {
        let tail: Vec<&str> = log
            .lines()
            .rev()
            .take(500)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        zip.start_file("latest.log", opts)?;
        zip.write_all(tail.join("\n").as_bytes())?;
    }

    // 5. Recent in-memory BlockHost log ring
    if !recent_logs.is_empty() {
        zip.start_file("blockhost-console.log", opts)?;
        zip.write_all(recent_logs.join("\n").as_bytes())?;
    }

    zip.finish()?.flush()?;
    std::fs::rename(&part, dest_zip)?;
    Ok(dest_zip.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn redacts_secrets_in_diagnostics_bundle() {
        let dir = std::env::temp_dir().join(format!("bh-diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut cfg = config::ServerConfig::new("smp".into(), "1.21.4".into(), 10, 21, 2048, 25565, 8);
        cfg.rcon_password = "SUPER_SECRET_RCON".into();
        cfg.duckdns_token = Some("SUPER_SECRET_DUCK".into());
        cfg.discord_webhook = Some("https://discord.com/api/webhooks/123/SUPER_SECRET_HOOK".into());
        cfg.save(&dir).unwrap();
        std::fs::write(
            dir.join("server.properties"),
            "server-port=25565\nrcon.password=SUPER_SECRET_RCON\n",
        )
        .unwrap();

        let out = dir.join("diag.zip");
        export_diagnostics_zip(&dir, &out, &["[blockhost] hello".into()]).unwrap();

        let mut archive = zip::ZipArchive::new(File::open(&out).unwrap()).unwrap();
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).unwrap();
            let mut content = String::new();
            f.read_to_string(&mut content).unwrap();
            assert!(!content.contains("SUPER_SECRET_RCON"));
            assert!(!content.contains("SUPER_SECRET_DUCK"));
            assert!(!content.contains("SUPER_SECRET_HOOK"));
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
