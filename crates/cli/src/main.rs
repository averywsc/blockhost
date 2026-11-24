use anyhow::{bail, Context, Result};
use blockhost_core::{
    backup, config, hardware, java, paths, power, server, service, setup as setup_mod, supervisor, tunnel,
};
use std::{
    io::BufRead,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::channel,
        Arc,
    },
};

const HELP: &str = "\
BlockHost CLI - turn a spare PC into a 24/7 Minecraft server

USAGE:
  blockhost info                         show hardware and the recommended setup
  blockhost setup <name> [options]       download Java + Paper and configure a server
      --version <mc>                     Minecraft version (default: newest stable)
      --port <n>                         server port (default 25565)
      --accept-eula                      you accept https://aka.ms/MinecraftEULA
  blockhost run <name> [options]         run the server, restart on crash (Ctrl-C to stop)
      --tunnel                           public address via playit.gg (no router setup)
      --backup-every <hours>             auto backups (default 6, 0 = off)
      --keep <n>                         backups to keep (default 10)
      --restart-every <hours>            scheduled restart (default 24, 0 = off)
  blockhost backup <name>                back up now
  blockhost backups <name>               list backups
  blockhost restore <name> <file.zip>    restore (server must be stopped)
  blockhost service install <name> [--tunnel]   start at boot
  blockhost service uninstall <name>
  blockhost power                        Windows: stop sleep / lid-close from stopping the server
";

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("info") => info(),
        Some("setup") => setup(&args[1..]),
        Some("run") => run(args.get(1).context("usage: blockhost run <name>")?, &args[2..]),
        Some("backup") => {
            let name = args.get(1).context("usage: blockhost backup <name>")?;
            println!(
                "Saved {}",
                backup::create(&paths::server_dir(name), name, 10)?.display()
            );
            Ok(())
        }
        Some("backups") => {
            let name = args.get(1).context("usage: blockhost backups <name>")?;
            for b in backup::list(name) {
                println!("{}", b.display());
            }
            Ok(())
        }
        Some("restore") => {
            let name = args
                .get(1)
                .context("usage: blockhost restore <name> <file.zip>")?;
            let file = args
                .get(2)
                .context("usage: blockhost restore <name> <file.zip>")?;
            let dir = paths::server_dir(name);
            if dir.join("blockhost.lock").exists() {
                bail!("server looks like it is running (delete blockhost.lock if it crashed)");
            }
            let aside = backup::restore(&dir, std::path::Path::new(file))?;
            println!("Restored. Previous worlds kept in {}", aside.display());
            Ok(())
        }
        Some("service") => {
            let name = args
                .get(2)
                .context("usage: blockhost service install|uninstall <name>")?;
            let msg = match args.get(1).map(String::as_str) {
                Some("install") => service::install(name, args.iter().any(|a| a == "--tunnel"))?,
                Some("uninstall") => service::uninstall(name)?,
                _ => bail!("usage: blockhost service install|uninstall <name>"),
            };
            println!("{msg}");
            Ok(())
        }
        Some("power") => {
            power::apply_windows_power()?;
            println!("Sleep and lid-close (plugged in) will no longer stop the server.");
            Ok(())
        }
        None | Some("help") | Some("--help") | Some("-h") | Some(_) => {
            print!("{HELP}");
            Ok(())
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn progress(label: &'static str) -> impl FnMut(u64, Option<u64>) {
    let mut last = 101u64;
    move |done, total| {
        if let Some(t) = total.filter(|t| *t > 0) {
            let pct = done * 100 / t;
            if pct != last && pct % 10 == 0 {
                eprintln!("  {label}: {pct}%");
            }
            last = pct;
        }
    }
}

fn info() -> Result<()> {
    let hw = hardware::detect();
    let p = hardware::recommend(&hw);
    println!("RAM: {} MB   CPU cores: {}", hw.total_ram_mb, hw.cpu_cores);
    println!(
        "Recommended: {} profile - {} MB heap, ~{} players, view distance {}",
        p.tier, p.heap_mb, p.max_players, p.view_distance
    );
    if let Some(w) = p.warning {
        println!("Note: {w}");
    }
    println!("Data folder: {}", paths::root().display());
    Ok(())
}

fn setup(args: &[String]) -> Result<()> {
    let name = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .context("usage: blockhost setup <name> [options]")?;
    if !args.iter().any(|a| a == "--accept-eula") {
        bail!("please read https://aka.ms/MinecraftEULA and re-run with --accept-eula to agree");
    }
    let hw = hardware::detect();
    if let Some(w) = hardware::recommend(&hw).warning {
        println!("Note: {w}");
    }
    let (mut pj, mut pp) = (progress("java"), progress("paper"));
    setup_mod::create_server(
        &setup_mod::Options {
            name: name.clone(),
            version: flag(args, "--version"),
            port: flag(args, "--port")
                .map(|p| p.parse())
                .transpose()?
                .unwrap_or(25565),
            eula: true,
        },
        &mut |m| println!("{m}"),
        &mut |label, d, t| {
            if label == "java" {
                pj(d, t)
            } else {
                pp(d, t)
            }
        },
    )?;
    println!("Done. Start it with: blockhost run {name}");
    Ok(())
}

fn hours(args: &[String], name: &str, default: u64) -> Result<Option<std::time::Duration>> {
    let h: u64 = flag(args, name)
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(default);
    Ok((h > 0).then(|| std::time::Duration::from_secs(h * 3600)))
}

fn run(name: &str, opts: &[String]) -> Result<()> {
    let dir = paths::server_dir(name);
    let cfg = config::ServerConfig::load(&dir).context("server not found - run `blockhost setup` first")?;
    let java_bin =
        java::find_java(&paths::java_dir(cfg.java_feature)).context("Java missing - re-run setup")?;

    let lock = dir.join("blockhost.lock");
    if std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .and_then(|mut f| {
            use std::io::Write;
            write!(f, "{}", std::process::id())
        })
        .is_err()
    {
        bail!("already running? (if it crashed, delete {})", lock.display());
    }
    let result = run_inner(name, &dir, &cfg, &java_bin, opts);
    let _ = std::fs::remove_file(&lock);
    result
}

fn run_inner(
    name: &str,
    dir: &std::path::Path,
    cfg: &config::ServerConfig,
    java_bin: &std::path::Path,
    opts: &[String],
) -> Result<()> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let s2 = shutdown.clone();
    ctrlc::set_handler(move || s2.store(true, Ordering::SeqCst))?;

    let _awake = power::KeepAwake::start();

    let (tx, rx) = channel();
    let stdin_tx = tx.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if stdin_tx.send(line).is_err() {
                break;
            }
        }
    });

    if let Some(every) = hours(opts, "--backup-every", 6)? {
        let keep = flag(opts, "--keep").map(|k| k.parse()).transpose()?.unwrap_or(10);
        backup::scheduled(
            tx.clone(),
            dir.to_path_buf(),
            name.to_string(),
            every,
            keep,
            shutdown.clone(),
        );
    }

    let tunnel_thread = if opts.iter().any(|a| a == "--tunnel") {
        tunnel::ensure_agent(&mut progress("playit agent"))?;
        let sd = shutdown.clone();
        let note = dir.join("tunnel.txt");
        let port = cfg.port;
        Some(std::thread::spawn(move || {
            let _ = tunnel::run(
                &sd,
                &mut |ev| {
                    let msg = match ev {
                        tunnel::Event::ClaimUrl(u) => format!(
                            "Open this once to link your free playit account, then add a Minecraft Java tunnel to port {port}: {u}"
                        ),
                        tunnel::Event::Address(a) => format!("Friends join at: {a}"),
                    };
                    println!("[blockhost] {msg}");
                    let _ = std::fs::write(&note, msg);
                },
                &mut |l| println!("[playit] {l}"),
            );
        }))
    } else {
        None
    };

    let policy = supervisor::Policy {
        max_uptime: hours(opts, "--restart-every", 24)?,
        ..supervisor::Policy::default()
    };

    println!(
        "Starting {} ({} MB heap, port {}). Ctrl-C to stop safely.",
        cfg.name, cfg.heap_mb, cfg.port
    );
    let outcome = supervisor::supervise(
        || Ok(server::java_command(java_bin, dir, cfg.heap_mb)),
        &policy,
        &shutdown,
        &rx,
        &mut |line| println!("{line}"),
    )?;
    shutdown.store(true, Ordering::SeqCst);
    if let Some(t) = tunnel_thread {
        let _ = t.join();
    }
    println!("[blockhost] stopped: {outcome:?}");
    Ok(())
}
