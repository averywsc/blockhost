//! Modrinth plugin search, verified installation, and removal.
//! Enforces Section 6 safety rules: Paper + MC version facets, `https://cdn.modrinth.com/`
//! URL allow-list, safe `.jar` filename validation, and SHA-512 verification.

use crate::net;
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

/// Validate a plugin jar filename (`^[A-Za-z0-9_.+\- ]+\.jar$`, no path separators).
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

/// Search Modrinth for Paper plugins compatible with `mc_version`.
pub fn search(base_url: &str, query: &str, mc_version: &str, limit: usize) -> Result<Vec<PluginHit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let facets = format!(r#"[["categories:paper"],["versions:{mc_version}"]]"#);
    let url = format!(
        "{}/search?query={}&limit={}&facets={}",
        base_url.trim_end_matches('/'),
        percent_encode(query),
        limit.clamp(1, 20),
        percent_encode(&facets)
    );
    let resp: SearchResponse = net::get_json(&url).context("Modrinth plugin search failed")?;
    Ok(resp.hits)
}

/// Resolve the latest Paper-compatible download for a Modrinth project.
pub fn resolve_version(base_url: &str, project_id: &str, mc_version: &str) -> Result<ResolvedPluginVersion> {
    if !project_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("invalid Modrinth project id");
    }
    let loaders = percent_encode(r#"["paper"]"#);
    let game_versions = percent_encode(&format!(r#"["{mc_version}"]"#));
    let url = format!(
        "{}/project/{project_id}/version?loaders={loaders}&game_versions={game_versions}",
        base_url.trim_end_matches('/')
    );
    let versions: Vec<ModrinthVersion> =
        net::get_json(&url).context("could not fetch plugin versions from Modrinth")?;
    let v = versions
        .into_iter()
        .next()
        .with_context(|| format!("no Paper download available for Minecraft {mc_version}"))?;
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
            "plugin filename '{}' did not look safe, so it was skipped",
            file.filename
        );
    }

    let url_ok = file.url.starts_with(ALLOWED_CDN_PREFIX)
        || (base_url != DEFAULT_MODRINTH_BASE && file.url.starts_with(base_url.trim_end_matches('/')));
    if !url_ok {
        bail!("plugin download URL is not on the allowed Modrinth CDN, so it was skipped");
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

/// Download and SHA-512 verify a resolved plugin into `<server_dir>/plugins/<filename>`.
pub fn install_plugin(
    server_dir: &Path,
    resolved: &ResolvedPluginVersion,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    if !valid_jar_name(&resolved.filename) {
        bail!("unsafe plugin filename");
    }
    let plugins_dir = server_dir.join("plugins");
    std::fs::create_dir_all(&plugins_dir)?;
    let dest = plugins_dir.join(&resolved.filename);
    net::download_sha512(&resolved.url, &dest, resolved.sha512.as_deref(), progress)?;
    Ok(dest)
}

pub fn list_installed(server_dir: &Path) -> Vec<InstalledPlugin> {
    let plugins_dir = server_dir.join("plugins");
    let mut out: Vec<InstalledPlugin> = std::fs::read_dir(plugins_dir)
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

pub fn remove_plugin(server_dir: &Path, filename: &str) -> Result<()> {
    if !valid_jar_name(filename) {
        bail!("invalid plugin filename");
    }
    let path = server_dir.join("plugins").join(filename);
    if !path.exists() {
        bail!("plugin '{filename}' not found");
    }
    std::fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_jar_filenames_safely() {
        assert!(valid_jar_name("Chunky-1.4.0.jar"));
        assert!(valid_jar_name("WorldEdit 7.3+1.jar"));
        assert!(!valid_jar_name("../evil.jar"));
        assert!(!valid_jar_name("bad/path.jar"));
        assert!(!valid_jar_name("script.exe"));
        assert!(!valid_jar_name(".jar"));
    }
}
