//! Feature-parity integration tests ported from `tests/suite-base.ps1`, `tests/suite-features.ps1`,
//! and `tests/suite-network.ps1`. Uses in-process fake TCP RCON and HTTP servers (Paper, Modrinth, Mojang)
//! so the entire suite runs deterministically on Windows, Linux, and macOS with zero external dependencies.

use blockhost_core::{
    backup, config, config::ServerSoftware, manager, network, paper, players, plugins, rcon, world,
};
use sha2::{Digest, Sha256, Sha512};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------- In-process Fake RCON server (mirrors tests/fakercon.py) ----------

fn start_fake_rcon(password: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            thread::spawn(move || handle_rcon(conn, password));
        }
    });
    port
}

fn rcon_pkt(id: i32, kind: i32, body: &[u8]) -> Vec<u8> {
    let len = (10 + body.len()) as i32;
    let mut out = Vec::with_capacity(4 + len as usize);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(body);
    out.push(0);
    out.push(0);
    out
}

fn handle_rcon(mut c: TcpStream, pw: &str) {
    loop {
        let mut lb = [0u8; 4];
        if c.read_exact(&mut lb).is_err() {
            break;
        }
        let n = i32::from_le_bytes(lb) as usize;
        let mut buf = vec![0u8; n];
        if c.read_exact(&mut buf).is_err() {
            break;
        }
        let id = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let kind = i32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let body = String::from_utf8_lossy(&buf[8..n - 2]).to_string();

        if kind == 3 {
            let reply_id = if body == pw { id } else { -1 };
            let _ = c.write_all(&rcon_pkt(reply_id, 2, b""));
        } else if kind == 2 {
            let reply = if body == "list" {
                "§aOK There are 2 of a max of 8 players online: Alice, Bob".to_string()
            } else {
                format!("§aOK ran:{body}")
            };
            let _ = c.write_all(&rcon_pkt(id, 0, reply.as_bytes()));
            if body == "stop" {
                break;
            }
        }
    }
}

// ---------- Tiny in-process HTTP server for Modrinth / Paper / Mojang fixtures ----------

fn start_fake_http<F>(handler: F) -> String
where
    F: Fn(&str) -> (u16, &'static str, Vec<u8>) + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(handler);
    thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let h = handler.clone();
            thread::spawn(move || {
                let mut reader = BufReader::new(&conn);
                let mut req_line = String::new();
                if reader.read_line(&mut req_line).is_err() {
                    return;
                }
                let path = req_line.split_whitespace().nth(1).unwrap_or("/");
                // Drain headers
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                        break;
                    }
                }
                let (code, ct, body) = h(path);
                let status = match code {
                    200 => "200 OK",
                    _ => "404 Not Found",
                };
                let mut stream = conn;
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn rcon_list_say_bad_password_and_unreachable() {
    let port = start_fake_rcon("secret123");

    let list = rcon::send_command(port, "secret123", "list").unwrap();
    assert_eq!(
        list.text,
        "OK There are 2 of a max of 8 players online: Alice, Bob"
    );

    let say = rcon::send_command(port, "secret123", "say hello world").unwrap();
    assert_eq!(say.text, "OK ran:say hello world");

    let bad = rcon::send_command(port, "wrong", "list").unwrap_err();
    assert!(bad.to_string().contains("bad RCON password"), "{bad}");

    // Closed port must fail cleanly
    let closed = rcon::send_command(1, "secret123", "list");
    assert!(closed.is_err());
}

#[test]
fn players_offline_and_online_rcon_parity() {
    let tmp = std::env::temp_dir().join(format!("bh-players-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let mojang = start_fake_http(|path| {
        if path == "/users/profiles/minecraft/Steve_1" {
            (
                200,
                "application/json",
                br#"{"id":"11111111222233334444555555555555","name":"Steve_1"}"#.to_vec(),
            )
        } else if path == "/users/profiles/minecraft/Alex" {
            (
                200,
                "application/json",
                br#"{"id":"aaaaaaaa222233334444555555555555","name":"Alex"}"#.to_vec(),
            )
        } else {
            (404, "application/json", b"{}".to_vec())
        }
    });

    // Offline adds + invalid username rejected
    players::add_player(&tmp, "Steve_1", true, None, &mojang).unwrap();
    players::add_player(&tmp, "Alex", false, None, &mojang).unwrap();
    assert!(players::add_player(&tmp, "bad name!", false, None, &mojang).is_err());
    assert_eq!(players::read_whitelist(&tmp).len(), 2);
    assert_eq!(players::read_ops(&tmp).len(), 1);

    // No duplicates
    players::add_player(&tmp, "Steve_1", true, None, &mojang).unwrap();
    assert_eq!(players::read_whitelist(&tmp).len(), 2);

    // Remove & deop
    players::remove_player(&tmp, "Alex", false, None).unwrap();
    assert_eq!(players::read_whitelist(&tmp).len(), 1);
    players::remove_player(&tmp, "Steve_1", true, None).unwrap();
    assert_eq!(players::read_ops(&tmp).len(), 0);
    assert_eq!(players::read_whitelist(&tmp).len(), 1);

    // Online RCON path
    let rcon_port = start_fake_rcon("secret123");
    let msg = players::add_player(&tmp, "Steve_1", true, Some((rcon_port, "secret123")), &mojang).unwrap();
    assert!(msg.contains("whitelist add Steve_1"), "{msg}");
    assert!(msg.contains("op Steve_1"), "{msg}");

    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn modrinth_plugin_search_sha512_install_and_remove() {
    let jar_bytes = b"PK-fake-plugin-bytes".to_vec();
    let sha512 = hex(&Sha512::digest(&jar_bytes));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let base_ref = format!("http://127.0.0.1:{port}");
    // Start actual server on dynamic port and embed its own URL
    let jar_clone = jar_bytes.clone();
    let sha_clone = sha512.clone();
    let _ = base_ref;
    let url_holder = Arc::new(std::sync::Mutex::new(String::new()));
    let url_for_handler = url_holder.clone();

    let base = start_fake_http(move |path| {
        let base_url = url_for_handler.lock().unwrap().clone();
        if path.starts_with("/search?") {
            assert!(path.contains("categories%3Apaper"), "{path}");
            assert!(path.contains("versions%3A1.21.10"), "{path}");
            let body = r#"{"hits":[{"title":"Chunky","project_id":"abc","description":"Pre-generate chunks","downloads":1234,"author":"pop4959"}]}"#;
            (200, "application/json", body.as_bytes().to_vec())
        } else if path.starts_with("/project/abc/version?") {
            let body = format!(
                r#"[{{"version_number":"1.4.0","files":[{{"primary":true,"filename":"Chunky-1.4.0.jar","url":"{base_url}/cdn/Chunky-1.4.0.jar","hashes":{{"sha512":"{sha_clone}"}}}}],"dependencies":[{{"dependency_type":"required","project_id":"x"}}]}}]"#
            );
            (200, "application/json", body.into_bytes())
        } else if path == "/cdn/Chunky-1.4.0.jar" {
            (200, "application/java-archive", jar_clone.clone())
        } else {
            (404, "application/json", b"{}".to_vec())
        }
    });
    *url_holder.lock().unwrap() = base.clone();

    let hits = plugins::search(&base, "chunky", "1.21.10", 6).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Chunky");

    let resolved = plugins::resolve_version(&base, "abc", "1.21.10").unwrap();
    assert_eq!(resolved.filename, "Chunky-1.4.0.jar");
    assert_eq!(resolved.required_dependency_count, 1);

    let srv = std::env::temp_dir().join(format!("bh-plugins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&srv);
    std::fs::create_dir_all(&srv).unwrap();

    let installed = plugins::install_plugin(&srv, &resolved, &mut |_, _| {}).unwrap();
    assert_eq!(std::fs::read(&installed).unwrap(), jar_bytes);
    assert_eq!(plugins::list_installed(&srv).len(), 1);

    plugins::remove_plugin(&srv, "Chunky-1.4.0.jar").unwrap();
    assert!(plugins::list_installed(&srv).is_empty());

    std::fs::remove_dir_all(&srv).unwrap();
}

#[test]
fn fabric_mods_install_into_mods_dir_and_online_players_parse() {
    let jar_bytes = b"PK-fake-fabric-mod".to_vec();
    let sha512 = hex(&Sha512::digest(&jar_bytes));
    let url_holder = Arc::new(std::sync::Mutex::new(String::new()));
    let url_for_handler = url_holder.clone();
    let jar_clone = jar_bytes.clone();
    let sha_clone = sha512.clone();

    let base = start_fake_http(move |path| {
        let base_url = url_for_handler.lock().unwrap().clone();
        if path.starts_with("/search?") {
            assert!(path.contains("categories%3Afabric"), "{path}");
            assert!(path.contains("versions%3A1.21.4"), "{path}");
            let body = r#"{"hits":[{"title":"Lithium","project_id":"lith","description":"Fast server optimization mod","downloads":9999,"author":"jellysquid3"}]}"#;
            (200, "application/json", body.as_bytes().to_vec())
        } else if path.starts_with("/project/lith/version?") {
            assert!(path.contains("fabric"), "{path}");
            let body = format!(
                r#"[{{"version_number":"0.14.0","files":[{{"primary":true,"filename":"lithium-fabric-0.14.0.jar","url":"{base_url}/cdn/lithium-fabric-0.14.0.jar","hashes":{{"sha512":"{sha_clone}"}}}}],"dependencies":[]}}]"#
            );
            (200, "application/json", body.into_bytes())
        } else if path == "/cdn/lithium-fabric-0.14.0.jar" {
            (200, "application/java-archive", jar_clone.clone())
        } else {
            (404, "application/json", b"{}".to_vec())
        }
    });
    *url_holder.lock().unwrap() = base.clone();

    let hits = plugins::search_for_software(&base, ServerSoftware::Fabric, "lithium", "1.21.4", 6).unwrap();
    assert_eq!(hits.len(), 1);
    let ver = plugins::resolve_version_for_software(&base, ServerSoftware::Fabric, "lith", "1.21.4").unwrap();

    let srv = std::env::temp_dir().join(format!("bh-fabric-mods-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&srv);
    std::fs::create_dir_all(&srv).unwrap();

    let dest = plugins::install_addon(&srv, ServerSoftware::Fabric, &ver, &mut |_, _| {}).unwrap();
    assert!(dest.starts_with(srv.join("mods")));
    assert_eq!(plugins::list_addons(&srv, ServerSoftware::Fabric).len(), 1);
    plugins::remove_addon(&srv, ServerSoftware::Fabric, "lithium-fabric-0.14.0.jar").unwrap();
    assert!(plugins::list_addons(&srv, ServerSoftware::Fabric).is_empty());

    assert_eq!(
        manager::parse_online_players("There are 2 of a max of 8 players online: Alice, Bob_1"),
        vec!["Alice", "Bob_1"]
    );
    assert!(manager::parse_online_players("There are 0 of a max of 8 players online:").is_empty());

    std::fs::remove_dir_all(&srv).unwrap();
}

#[test]
fn paper_update_checks_sha256_backup_and_old_jar() {
    let jar_bytes = b"new-paper-jar-bytes".to_vec();
    let sha256 = hex(&Sha256::digest(&jar_bytes));
    let url_holder = Arc::new(std::sync::Mutex::new(String::new()));
    let url_for_handler = url_holder.clone();
    let jar_clone = jar_bytes.clone();
    let sha_clone = sha256.clone();

    let base = start_fake_http(move |path| {
        let base_url = url_for_handler.lock().unwrap().clone();
        match path {
            "/" => (
                200,
                "application/json",
                br#"{"versions":{"1.21":["1.21.10","1.21.9"],"26.1":["26.1","26.1-rc1"]}}"#.to_vec(),
            ),
            "/versions/1.21.10/builds" => {
                let body = format!(
                    r#"[
                        {{"id":100,"channel":"STABLE","downloads":{{"server:default":{{"url":"{base_url}/jar/100","checksums":{{"sha256":"{sha_clone}"}}}}}}}},
                        {{"id":130,"channel":"STABLE","downloads":{{"server:default":{{"url":"{base_url}/jar/130","checksums":{{"sha256":"{sha_clone}"}}}}}}}},
                        {{"id":140,"channel":"ALPHA","downloads":{{"server:default":{{"url":"{base_url}/jar/140","checksums":{{"sha256":"{sha_clone}"}}}}}}}}
                    ]"#
                );
                (200, "application/json", body.into_bytes())
            }
            "/versions/26.1/builds" => {
                let body = format!(
                    r#"[{{"id":7,"channel":"STABLE","downloads":{{"server:default":{{"url":"{base_url}/jar/7","checksums":{{"sha256":"{sha_clone}"}}}}}}}}]"#
                );
                (200, "application/json", body.into_bytes())
            }
            "/versions/1.21.10" => (
                200,
                "application/json",
                br#"{"version":{"java":{"version":{"minimum":21}}}}"#.to_vec(),
            ),
            "/versions/26.1" => (
                200,
                "application/json",
                br#"{"version":{"java":{"version":{"minimum":25}}}}"#.to_vec(),
            ),
            p if p.starts_with("/jar/") => (200, "application/java-archive", jar_clone.clone()),
            _ => (404, "application/json", b"{}".to_vec()),
        }
    });
    *url_holder.lock().unwrap() = base.clone();

    let upd = paper::check_updates_from(&base, "1.21.10", 100).unwrap();
    let same = upd.same_version.expect("expected same-version build 130");
    assert_eq!(same.build, 130);
    assert_eq!(same.java_feature, 21);

    let major = upd.latest_major.expect("expected major 26.1");
    assert_eq!(major.mc_version, "26.1");
    assert_eq!(major.java_feature, 25);

    // Perform same-version update and verify backup + server.jar.old
    let home = std::env::temp_dir().join(format!("bh-upd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::env::set_var("BLOCKHOST_HOME", &home);
    let srv = home.join("servers/smp");
    std::fs::create_dir_all(srv.join("world")).unwrap();
    std::fs::write(srv.join("world/level.dat"), b"worlddata").unwrap();
    std::fs::write(srv.join("server.jar"), b"old-jar").unwrap();

    let mut cfg = config::ServerConfig::new("smp".into(), "1.21.10".into(), 100, 21, 3072, 25565, 8);
    cfg.save(&srv).unwrap();

    let mut ensured_java = 0u32;
    let bk = paper::install_update(
        &srv,
        &mut cfg,
        &same,
        &mut |jv| {
            ensured_java = jv;
            Ok(())
        },
        &mut |_, _| {},
    )
    .unwrap();

    assert!(bk.is_some(), "expected backup before update");
    assert_eq!(ensured_java, 21);
    assert_eq!(cfg.paper_build, 130);
    assert_eq!(std::fs::read(srv.join("server.jar")).unwrap(), jar_bytes);
    assert_eq!(std::fs::read(srv.join("server.jar.old")).unwrap(), b"old-jar");

    // New world moves old world aside into old-worlds (never deletes!)
    let moved = world::new_world(&srv, "smp", "my seed 1", 10).unwrap();
    assert!(!srv.join("world").exists());
    assert!(moved.join("world/level.dat").exists());
    assert_eq!(
        config::get_property(&srv.join("server.properties"), "level-seed").as_deref(),
        Some("my seed 1")
    );
    assert!(!backup::list("smp").is_empty());

    // Loopback ping sanity check on real OS
    let ping = network::ping_host("127.0.0.1", 1).unwrap();
    assert!(ping.sent == 1);

    // Export and re-import a nested singleplayer world zip safely (old world preserved!)
    std::fs::create_dir_all(srv.join("world")).unwrap();
    std::fs::write(srv.join("world/level.dat"), b"current-world").unwrap();
    let exported = home.join("exported.zip");
    world::export_world_zip(&srv, &exported).unwrap();
    assert!(exported.exists());

    // Build a nested singleplayer-style zip (MyCustomMap/level.dat)
    let upload_zip = home.join("custom-map.zip");
    {
        let f = std::fs::File::create(&upload_zip).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("MyCustomMap/level.dat", opts).unwrap();
        z.write_all(b"uploaded-adventure-map").unwrap();
        z.finish().unwrap();
    }
    let aside = world::import_world_zip(&srv, "smp", &upload_zip, 10).unwrap();
    assert_eq!(
        std::fs::read(srv.join("world/level.dat")).unwrap(),
        b"uploaded-adventure-map"
    );
    assert_eq!(
        std::fs::read(aside.join("world/level.dat")).unwrap(),
        b"current-world"
    );

    // DuckDNS normalization
    assert_eq!(
        network::normalize_duckdns_subdomain("Kiwi-SMP.duckdns.org").unwrap(),
        "kiwi-smp"
    );
    assert!(network::normalize_duckdns_subdomain("bad name!").is_err());

    // Local .jar import rejects non-zip, accepts valid jar
    let bad_jar = home.join("fake.jar");
    std::fs::write(&bad_jar, b"not a zip").unwrap();
    assert!(plugins::import_local_jar(&srv, ServerSoftware::Fabric, &bad_jar).is_err());

    let good_jar = home.join("coolmod-1.0.jar");
    {
        let f = std::fs::File::create(&good_jar).unwrap();
        let mut z = zip::ZipWriter::new(f);
        z.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"{}").unwrap();
        z.finish().unwrap();
    }
    let added = plugins::import_local_jar(&srv, ServerSoftware::Fabric, &good_jar).unwrap();
    assert_eq!(added, "coolmod-1.0.jar");
    assert!(srv.join("mods/coolmod-1.0.jar").exists());

    std::fs::remove_dir_all(&home).unwrap();
}
