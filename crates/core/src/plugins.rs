//! Modrinth plugin & mod search, verified installation, and removal.
//! Supports both Paper plugins (`plugins/`) and Fabric mods (`mods/`).
//! Enforces Section 6 safety rules: loader + MC version facets, `https://cdn.modrinth.com/`
//! URL allow-list, safe `.jar` filename validation, and SHA-512 verification.

use crate::{config::ServerSoftware, net};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_MODRINTH_BASE: &str = "https://api.modrinth.com/v2";
const ALLOWED_CDN_PREFIX: &str = "https://cdn.modrinth.com/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHit {
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedPluginVersion {
    pub version_number: String,
    pub filename: String,
    pub url: String,
    pub sha512: Option<String>,
    pub required_dependency_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub filename: String,
    pub size_bytes: u64,
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<PluginHit>,
}

#[derive(Deserialize)]
struct ModrinthVersion {
    version_number: String,
    files: Vec<ModrinthFile>,
    #[serde(default)]
    dependencies: Vec<ModrinthDependency>,
}

#[derive(Deserialize)]
struct ModrinthFile {
    #[serde(default)]
    primary: bool,
    filename: String,
    url: String,
    #[serde(default)]
    hashes: ModrinthHashes,
}

#[derive(Deserialize, Default)]
struct ModrinthHashes {
    sha512: Option<String>,
}

#[derive(Deserialize)]
struct ModrinthDependency {
    dependency_type: String,
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Validate a plugin/mod jar filename (`^[A-Za-z0-9_.+\- ]+\.jar$`, no path separators).
pub fn valid_jar_name(name: &str) -> bool {
    if !name.ends_with(".jar") || name.len() <= 4 || name.len() > 160 {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    name.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-' | b' '))
}

pub fn addons_dir(server_dir: &Path, software: ServerSoftware) -> PathBuf {
    server_dir.join(software.addons_folder_name())
}

/// Search Modrinth for plugins (Paper) or mods (Fabric) compatible with `mc_version`.
pub fn search_for_software(
    base_url: &str,
    software: ServerSoftware,
    query: &str,
    mc_version: &str,
    limit: usize,
) -> Result<Vec<PluginHit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let loader = software.modrinth_loader();
    let facets = format!(r#"[["categories:{loader}"],["versions:{mc_version}"]]"#);
    let url = format!(
        "{}/search?query={}&limit={}&facets={}",
        base_url.trim_end_matches('/'),
        percent_encode(query),
        limit.clamp(1, 20),
        percent_encode(&facets)
    );
    let resp: SearchResponse = net::get_json(&url).context("Modrinth search failed")?;
    Ok(resp.hits)
}

/// Convenience wrapper for Paper plugin search.
pub fn search(base_url: &str, query: &str, mc_version: &str, limit: usize) -> Result<Vec<PluginHit>> {
    search_for_software(base_url, ServerSoftware::Paper, query, mc_version, limit)
}

/// Resolve the latest compatible download for a Modrinth project.
pub fn resolve_version_for_software(
    base_url: &str,
    software: ServerSoftware,
    project_id: &str,
    mc_version: &str,
) -> Result<ResolvedPluginVersion> {
    if !project_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("invalid Modrinth project id");
    }
    let loader = software.modrinth_loader();
    let loaders = percent_encode(&format!(r#"["{loader}"]"#));
    let game_versions = percent_encode(&format!(r#"["{mc_version}"]"#));
    let url = format!(
        "{}/project/{project_id}/version?loaders={loaders}&game_versions={game_versions}",
        base_url.trim_end_matches('/')
    );
    let versions: Vec<ModrinthVersion> =
        net::get_json(&url).context("could not fetch versions from Modrinth")?;
    let v = versions
        .into_iter()
        .next()
        .with_context(|| format!("no {loader} download available for Minecraft {mc_version}"))?;
    let file = v
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| v.files.first())
        .context("Modrinth release contained no files")?;

    let safe_name = Path::new(&file.filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if !valid_jar_name(&safe_name) {
        bail!(
            "download filename '{}' did not look safe, so it was skipped",
            file.filename
        );
    }

    let url_ok = file.url.starts_with(ALLOWED_CDN_PREFIX)
        || (base_url != DEFAULT_MODRINTH_BASE && file.url.starts_with(base_url.trim_end_matches('/')));
    if !url_ok {
        bail!("download URL is not on the allowed Modrinth CDN, so it was skipped");
    }

    let required_dependency_count = v
        .dependencies
        .iter()
        .filter(|d| d.dependency_type == "required")
        .count();

    Ok(ResolvedPluginVersion {
        version_number: v.version_number,
        filename: safe_name,
        url: file.url.clone(),
        sha512: file.hashes.sha512.clone(),
        required_dependency_count,
    })
}

pub fn resolve_version(base_url: &str, project_id: &str, mc_version: &str) -> Result<ResolvedPluginVersion> {
    resolve_version_for_software(base_url, ServerSoftware::Paper, project_id, mc_version)
}

/// Download and SHA-512 verify a resolved addon into `plugins/` (Paper) or `mods/` (Fabric).
pub fn install_addon(
    server_dir: &Path,
    software: ServerSoftware,
    resolved: &ResolvedPluginVersion,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    if !valid_jar_name(&resolved.filename) {
        bail!("unsafe filename");
    }
    let dir = addons_dir(server_dir, software);
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(&resolved.filename);
    net::download_sha512(&resolved.url, &dest, resolved.sha512.as_deref(), progress)?;
    Ok(dest)
}

pub fn install_plugin(
    server_dir: &Path,
    resolved: &ResolvedPluginVersion,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    install_addon(server_dir, ServerSoftware::Paper, resolved, progress)
}

pub fn list_addons(server_dir: &Path, software: ServerSoftware) -> Vec<InstalledPlugin> {
    let dir = addons_dir(server_dir, software);
    let mut out: Vec<InstalledPlugin> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jar")) {
                        Some(InstalledPlugin {
                            filename: e.file_name().to_string_lossy().to_string(),
                            size_bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
                        })
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|a| a.filename.to_lowercase());
    out
}

pub fn list_installed(server_dir: &Path) -> Vec<InstalledPlugin> {
    list_addons(server_dir, ServerSoftware::Paper)
}

pub fn remove_addon(server_dir: &Path, software: ServerSoftware, filename: &str) -> Result<()> {
    if !valid_jar_name(filename) {
        bail!("invalid filename");
    }
    let path = addons_dir(server_dir, software).join(filename);
    if !path.exists() {
        bail!("'{filename}' not found");
    }
    std::fs::remove_file(path)?;
    Ok(())
}

pub fn remove_plugin(server_dir: &Path, filename: &str) -> Result<()> {
    remove_addon(server_dir, ServerSoftware::Paper, filename)
}

/// Copy a user-selected local `.jar` mod or plugin into `mods/` or `plugins/` after verifying
/// it has a safe `.jar` filename and opens as a valid ZIP/JAR archive.
pub fn import_local_jar(server_dir: &Path, software: ServerSoftware, source_path: &Path) -> Result<String> {
    let fname = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Could not read filename")?
        .to_string();
    if !valid_jar_name(&fname) {
        bail!("'{fname}' is not a valid .jar mod/plugin filename.");
    }
    let file = std::fs::File::open(source_path).context("Could not open selected .jar file.")?;
    zip::ZipArchive::new(file).context("That file is not a valid Minecraft .jar archive.")?;
    let dir = addons_dir(server_dir, software);
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(source_path, dir.join(&fname))?;
    Ok(fname)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_jar_filenames_safely() {
        assert!(valid_jar_name("Chunky-1.4.0.jar"));
        assert!(valid_jar_name("fabric-api-0.115.0+1.21.4.jar"));
        assert!(!valid_jar_name("../evil.jar"));
        assert!(!valid_jar_name("bad/path.jar"));
        assert!(!valid_jar_name("script.exe"));
        assert!(!valid_jar_name(".jar"));
    }
}
