//! Live upstream API contract smoke tests (Paper Fill v3, Adoptium JRE metadata, Modrinth v2).
//! Run with: cargo test -p blockhost-core --test live_api -- --ignored

use blockhost_core::{fabric, java, net, paper};

#[test]
#[ignore = "hits live PaperMC Fill v3 API"]
fn live_paper_fill_v3_contract() {
    let resolved = paper::resolve(None).expect("Paper Fill v3 resolve failed");
    assert!(!resolved.mc_version.is_empty());
    assert!(resolved.build > 0);
    assert!(resolved.url.starts_with("https://"));
    assert_eq!(resolved.sha256.len(), 64, "expected 64-char SHA-256 hex");
    assert!(
        resolved.java_feature >= 17,
        "unexpected Java feature: {}",
        resolved.java_feature
    );
}

#[test]
#[ignore = "hits live Adoptium v3 metadata API"]
fn live_adoptium_jre_metadata_contract() {
    let resolved = java::resolve_java(21).expect("Adoptium JRE 21 resolve failed");
    assert!(resolved.url.starts_with("https://"));
    assert_eq!(resolved.sha256.len(), 64, "expected 64-char SHA-256 hex");
    assert!(!resolved.release_name.is_empty());
}

#[test]
#[ignore = "hits live Modrinth v2 search API"]
fn live_modrinth_v2_search_contract() {
    let url =
        "https://api.modrinth.com/v2/search?query=chunky&limit=3&facets=%5B%5B%22categories%3Apaper%22%5D%5D";
    let v: serde_json::Value = net::get_json(url).expect("Modrinth search failed");
    let hits = v
        .get("hits")
        .and_then(|h| h.as_array())
        .expect("expected hits array");
    assert!(
        !hits.is_empty(),
        "expected at least one Paper plugin hit for chunky"
    );
}

#[test]
#[ignore = "hits live Fabric Meta v2 API"]
fn live_fabric_meta_v2_contract() {
    let resolved = fabric::resolve(None).expect("Fabric Meta v2 resolve failed");
    assert!(!resolved.mc_version.is_empty());
    assert!(!resolved.mc_version.contains('-'));
    assert!(!resolved.loader_version.is_empty());
    assert!(!resolved.installer_version.is_empty());
    assert!(resolved
        .url
        .starts_with("https://meta.fabricmc.net/v2/versions/loader/"));
}
