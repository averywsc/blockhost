# TASK: Turn this prototype into a proper desktop app - "one-click Minecraft server host" (name TBD)

## 1. Product goal
A real Windows app (Mac/Linux later) with a window, tray icon and installer. A non-technical person installs it on a spare PC or
old laptop, clicks "Create server", and gets a Paper Minecraft server that runs 24/7, restarts itself, backs itself up, and
that friends can join with good ping. The owner is in Auckland, NZ; players are friends on home internet.
Design bar: it should feel like a polished consumer app (think a launcher), not a sysadmin tool. Plain language everywhere.
It must stay light: it will run on old laptops (4 GB RAM), so idle app overhead matters.

## 2. What exists (read all of it)
- Minecraft-Server.bat / panel.ps1  - a working-in-tests PowerShell control panel (single file). This is the FUNCTIONAL SPEC and a
  stopgap release. Every feature in section 5 already exists there with tested behaviour. Port the behaviour, not the code style.
- rust-core/  - an earlier Rust crate "blockhost" (CLI + tiny local web dashboard). Has: hardware profile, Java + Paper install with
  SHA-256, EULA gate, whitelist-on defaults, process supervisor (crash backoff, crash-loop guard, scheduled restart, graceful stop),
  backups + restore, service install (systemd/Task Scheduler/launchd), playit tunnel runner, sleep inhibitor.
  Missing vs the PowerShell version: RCON client, whitelist/ops management, updates, plugins, game rules/new world, info/TPS,
  network tools (ping, UPnP, firewall), Windows power plan, timed backups, desktop UI.
  `cargo test` passes on Linux (Cargo.lock pins crates for an old toolchain; on current Rust delete Cargo.lock and rebuild).
- tests/  - PowerShell test suites + fake RCON server + fake Modrinth server. Use them as behaviour fixtures; port the scenarios to
  Rust integration tests (or keep the Python fakes as shared fixtures). `PWSH=/path/to/pwsh tests/run-tests.sh` must stay green while the
  PowerShell version exists.
Nothing here has ever run on real Windows. Treat every Windows-specific line as unverified (section 7).

## 3. Recommended architecture (confirm with the owner in your first message, then proceed)
- App shell + UI: Tauri v2 (Rust backend, web frontend; small installer, low RAM; WebView2 is built into Win11, bundle the
  bootstrapper for Win10). Frontend: your choice of TypeScript + (React | Svelte | Solid) with a small component set; no heavy UI kits.
  Alternative if you can justify it: WinUI 3 / WPF (.NET) for Windows-only. Avoid Electron (too heavy for old laptops).
- Core: grow rust-core into a library crate (no UI knowledge) exposing async commands + an event stream (logs, status, progress).
  The Tauri layer is thin: commands in, events out. Keep a CLI over the same core for headless/testing.
- Server control: RCON (127.0.0.1, random strong password stored in the app config) for live actions; direct file edits only when stopped.
- 24/7: tray app that supervises the server and starts at login; offer an optional "run as a background service" mode
  (Windows Service via the windows-service crate or a Task Scheduler task) for PCs nobody logs in to.
- Networking: (a) direct join via UPnP port forward (igd-next crate) + Windows Firewall rule, (b) playit.gg tunnel fallback,
  (c) later, an optional own relay. Detect CGNAT and explain it plainly.
- Distribution: signed installer (MSI or NSIS via Tauri bundler), auto-update (tauri-plugin-updater with signature verification),
  clean uninstall that asks whether to keep worlds/backups. Code signing is REQUIRED to avoid SmartScreen / Smart App Control
  blocks (the owner had to disable Smart App Control to run the unsigned script). Investigate Azure Trusted Signing vs an OV/EV cert
  and recommend one with cost/trade-offs before building the release pipeline.
- CI: GitHub Actions with windows-latest + ubuntu + macos. Use the Windows runner to actually execute Windows-only code paths
  (powercfg, firewall cmdlets where permitted, process handling, installer smoke test). Anything CI cannot test (UPnP against a real router,
  lid-close behaviour, sleep) goes on a manual test checklist for the owner.

## 4. UI screens (first version)
1. First-run wizard: accept Minecraft EULA (explicit), server name, your username (becomes admin), RAM slider with a recommended value and
   plain-language warning, "let friends join over the internet" choice, then progress for Java/Paper download.
2. Dashboard: big Start/Stop button, status, players online, TPS/lag indicator, RAM/CPU of the server, uptime, update banner,
   join addresses (this PC / same Wi-Fi / internet) each with a Copy button, tunnel/port-forward status.
3. Console: live log with colours, search, command box, quick actions.
4. Players: allowed list + admins + online now; add/remove/op/deop, kick/ban/pardon; import/export.
5. World and rules: difficulty, game mode, PvP, command blocks, view distance, new world with seed (old world kept), low-lag mode.
6. Backups: list, back up now, schedule, restore (with "old world kept aside"), disk usage.
7. Plugins: search Modrinth, install/remove, warnings about trust and dependencies.
8. Network: connection check (latency/jitter/loss graph, Wi-Fi vs cable warning), ping a friend, one-click port forward,
   tunnel setup, router helper, CGNAT explanation.
9. Settings: RAM, max players, auto-restart, auto-backup, start with Windows, performance mode (power plan), whitelist on/off
   (with scary warning), updates, about/logs/export diagnostics.
Also: tray icon (status, quick stop/start), dark/light theme following the OS, keyboard + screen-reader accessibility, i18n-ready strings.

## 5. Feature parity checklist (source of truth = panel.ps1 function names)
- Hardware presets ................ Get-Hardware            (heap = clamp(min(RAM-reserve, 60% RAM),1 GB,12 GB); tiers for players/view/sim)
- Settings + migration ............ Load-Settings/Ensure-Defaults (keys: javaVersion, minecraft, paperBuild, tunnel, heapMb, rconPassword,
                                    backupHours=6, restartHours=0, playitAddress, perfMode)
- server.properties edit .......... Get-Prop/Set-Prop       (preserve unknown lines/comments)
- Java install .................... Get-JavaExe              (Adoptium JRE, per-major folder)
- Paper resolve/install/update .... Find-Paper/Install-Paper/Menu-Update/Check-UpdateHint
                                    (Fill v3 API, newest STABLE build, skip pre-release versions, SHA-256, keep server.jar.old, backup first)
- RCON client ..................... Rcon/New-RconPacket/Read-RconPacket
- Run loop ........................ Run-Server              (crash backoff 5 crashes/10 min, scheduled restart w/ warning, timed backups, stop flag)
- Safe stop/restart/force ......... Stop-ServerSafely/Force-Stop
- Players ......................... Add-Player/Remove-Player (RCON when running, UUID-via-Mojang JSON edit when stopped)
- Admin actions ................... Menu-Actions
- Backups ......................... Zip-Folders/New-Backup/Backup-Live/Restore-Backup (save-off, save-all flush, zip, save-on; skip session.lock; keep 10)
- Game rules / new world .......... Menu-Game/New-World     (old world moved to old-worlds, never deleted)
- Plugins ......................... Install-PluginFlow       (Modrinth, facets categories:paper + versions:<mc>, https://cdn.modrinth.com allow-list, SHA-512)
- Info/TPS/log .................... Menu-Info/Show-Log
- Network ......................... Ping-Host/Get-PingStats/Get-PingGrade/Get-LinkInfo/Add-UpnpForward/Add-FirewallRule
- Power ........................... Prevent-Sleep/Enter-PerfMode/Exit-PerfMode (keep awake, lid action, high-performance plan then restore)
- Autostart/shortcut .............. Set-AutoStart/Make-Shortcut
- Tunnel .......................... Start-Tunnel (playit agent); the address is NOT auto-detected yet (see roadmap)

## 6. Non-negotiable safety/UX rules (carry over)
Whitelist ON by default; online-mode true; EULA recorded only after an explicit user action; random strong RCON password; username validation
^[A-Za-z0-9_]{3,16}$; every download hash-verified; plugin URLs allow-listed; destructive actions (force stop, new world, restore, major upgrade,
whitelist off) need a typed/explicit confirmation and take a backup first; never delete worlds, move them aside; RCON never exposed beyond
localhost where avoidable (bind/firewall); no telemetry without opt-in; do not redistribute Mojang/Paper jars (download at runtime);
set a real contact URL in the HTTP User-Agent (PaperMC requires it; the code has a placeholder). Friendly error messages, never raw stack traces.

## 7. Known unverified (Windows) - verify first
real Fill/Adoptium/Modrinth/playit downloads and JSON shapes (Fill java requirement path /version/java/version/minimum assumed);
process spawning and exit codes on Windows; powercfg + SetThreadExecutionState; UPnP; Windows Firewall rule creation (needs admin);
Wi-Fi/gateway detection; autostart; shortcuts; the playit claim flow (agent output format unknown).

## 8. Roadmap
P0  Confirm architecture + name with the owner. Set up the repo, CI (Windows/Linux/macOS), lint/format, and a walking skeleton:
    Tauri window -> core command -> live status event.
P0  Port the core: RCON client, settings with migration, Paper/Java install, supervisor, backups, players, with integration tests against
    fake RCON/Modrinth/Paper servers (port the fixtures in tests/). Run them on the Windows CI runner.
P1  Build the UI screens in section 4 in this order: wizard, dashboard, console, players, backups, settings, network, world/rules, plugins.
P1  Installer + code signing + auto-update + uninstall behaviour. Resolve the Smart App Control/SmartScreen issue for real.
P1  Real-Windows manual test checklist for the owner (old laptop + gaming PC), including lid-close, sleep, reboot-autostart, UPnP, firewall.
P2  Auto-detect the playit address (or integrate the official plugin/API); CGNAT detection; DuckDNS helper; optional own relay for better NZ ping.
P2  Discord webhook notifications; live player stats/playtime; crash analyser ("why did it crash" in plain words); multiple servers/profiles;
    import/export worlds; Aikar-style JVM flags and optional Chunky pre-generation; CPU temperature warning for laptops.
P3  Bedrock cross-play (Geyser + Floodgate) opt-in; resource-pack helper; scheduled announcements; Mac/Linux builds; localisation.
Keep the PowerShell version working as a stopgap until the app reaches feature parity, then retire it (tell the owner first).

## 9. How to work and report
- Start by reading panel.ps1 and rust-core/src, then reply with: your architecture recommendation, risks, a milestone plan, and questions.
- Small, reviewable commits; tests with every change; keep CI green. Document how to build, run, test, and release.
- After each milestone report: what changed, new config keys (with migration), tests added, what is still unverified on real Windows.
- Ask the owner before: changing safety defaults, adding paid services/dependencies, anything needing admin rights, or choosing a signing option.
- Write user-facing release notes in plain language.
