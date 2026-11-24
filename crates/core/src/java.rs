use crate::{net, paths};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

fn adoptium_os_arch() -> Result<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "mac",
        other => bail!("unsupported OS: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        other => bail!("unsupported CPU architecture: {other}"),
    };
    Ok((os, arch))
}

#[derive(Debug, Clone)]
pub struct ResolvedJava {
    pub url: String,
    pub sha256: String,
    pub release_name: String,
}

#[derive(Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
    release_name: String,
}

#[derive(Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumPackage,
}

#[derive(Deserialize)]
struct AdoptiumPackage {
    link: String,
    checksum: String,
}

pub fn metadata_url(feature: u32) -> Result<String> {
    let (os, arch) = adoptium_os_arch()?;
    Ok(format!(
        "https://api.adoptium.net/v3/assets/latest/{feature}/hotspot?architecture={arch}&image_type=jre&os={os}&vendor=eclipse"
    ))
}

/// Resolve the latest Adoptium JRE download URL and SHA-256 checksum.
pub fn resolve_java(feature: u32) -> Result<ResolvedJava> {
    let url = metadata_url(feature)?;
    let assets: Vec<AdoptiumAsset> = net::get_json(&url)?;
    let asset = assets
        .into_iter()
        .next()
        .with_context(|| format!("no Adoptium JRE release found for Java {feature}"))?;
    if asset.binary.package.checksum.len() != 64 {
        bail!("Adoptium returned an invalid SHA-256 checksum");
    }
    Ok(ResolvedJava {
        url: asset.binary.package.link,
        sha256: asset.binary.package.checksum,
        release_name: asset.release_name,
    })
}

/// Find `bin/java` (or `bin/java.exe`) anywhere under `dir`. Handles the macOS
/// `Contents/Home/bin` layout and archives that wrap everything in one folder.
pub fn find_java(dir: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == exe)
                && p.parent().and_then(|b| b.file_name()).is_some_and(|n| n == "bin")
            {
                return Some(p);
            }
        }
    }
    None
}

/// Make sure a private JRE of the given major version exists; download and SHA-256 verify it if not.
/// The user never has to install Java themselves.
pub fn ensure_java(feature: u32, progress: &mut dyn FnMut(u64, Option<u64>)) -> Result<PathBuf> {
    let dir = paths::java_dir(feature);
    if let Some(j) = find_java(&dir) {
        return Ok(j);
    }
    let resolved = resolve_java(feature)?;
    let archive = dir.with_extension(if cfg!(windows) { "zip" } else { "tar.gz" });
    net::download(&resolved.url, &archive, Some(&resolved.sha256), progress)?;

    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    extract(&archive, &dir).context("failed to unpack Java")?;
    let _ = std::fs::remove_file(&archive);
    find_java(&dir).context("Java archive unpacked but no bin/java was found")
}

fn extract(archive: &Path, dest: &Path) -> Result<()> {
    if cfg!(windows) {
        let mut z = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
        z.extract(dest)?;
    } else {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        tar::Archive::new(gz).unpack(dest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_java_in_nested_layout() {
        let tmp = std::env::temp_dir().join(format!("bh-java-test-{}", std::process::id()));
        let bin = tmp.join("jdk-21.0.5+11-jre/Contents/Home/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(if cfg!(windows) { "java.exe" } else { "java" });
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(find_java(&tmp), Some(exe));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn metadata_url_mentions_feature_version() {
        let u = metadata_url(21).unwrap();
        assert!(u.contains("/latest/21/hotspot"));
        assert!(u.contains("image_type=jre"));
    }
}
