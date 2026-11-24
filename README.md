# BlockHost

A lightweight desktop app that turns a spare PC or old 4 GB laptop into a 24/7 Minecraft server with one click.
download: https://github.com/averywsc/blockhost/actions/runs/45309156596

## Highlights
- **Paper (plugins) & Fabric (mods)** with a full **Minecraft version picker** (`26.x`, `1.21.x`, `1.20.x`) and automatic SHA-256 verified Adoptium Java installation
- **Low-RAM 24/7 architecture**: Rust + Tauri v2 shell that destroys the WebView2 window when closed to the system tray (~15-25 MB idle RAM)
- **Built for low-ping NZ hosting**: Direct UPnP port forwarding + free **DuckDNS** auto-updater, **Tailscale** P2P auto-detection for CGNAT connections, CGNAT diagnostics, Wi-Fi jitter check, plus optional playit.gg tunnel
- **Full admin & world management**: Live colour console, RCON player controls (kick, ban, gamemode, teleport, heal, feed, whitelist/ops), world `.zip` upload & export, Modrinth mod/plugin browser + local `.jar` upload, Aikar JVM flags, automatic live backups, plain-English crash analyzer, Discord webhooks, and redacted diagnostics export
- **Safety by default**: Whitelist ON, online-mode ON, explicit EULA gate, CSPRNG localhost RCON password, SHA-256/SHA-512 verified downloads, typed `YES` confirmation for destructive actions, and worlds are never deleted (always backed up and moved aside)

## Repository Layout
- `crates/core` (`blockhost-core`): Pure Rust server engine (zero UI knowledge)
- `crates/cli` (`blockhost-cli`): Headless CLI and service runner
- `apps/desktop`: Tauri v2 + Svelte 5 + TypeScript desktop app + NSIS installer hooks
- `docs/`: [Real-Windows Test Checklist](docs/WINDOWS_TEST_CHECKLIST.md) and [Releasing & Code Signing Guide](docs/RELEASING.md)
- `panel.ps1`, `Minecraft-Server.bat`, `tests/`: Legacy PowerShell stopgap release & behaviour fixtures

## Development & Testing
```bash
# Rust format, clippy, unit + integration tests
cargo fmt --all -- --check
cargo clippy -p blockhost-core -p blockhost-cli --all-targets -- -D warnings
cargo test -p blockhost-core -p blockhost-cli

# Live upstream API contract checks (Paper Fill v3, Fabric Meta v2, Adoptium, Modrinth)
cargo test -p blockhost-core --test live_api -- --ignored --nocapture

# Desktop frontend check & build
cd apps/desktop
npm ci
npm run check
npm run build
```
