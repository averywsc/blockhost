# BlockHost

A lightweight, one-click 24/7 Minecraft (Paper) server host for spare PCs and old laptops.

## Repository Layout

- `crates/core` (`blockhost-core`): Pure Rust server management library (hardware presets, Java/Paper installer, supervisor, backups, tunnel, power management, typed event stream). Zero UI dependencies.
- `crates/cli` (`blockhost-cli`): Headless CLI and service runner built over `blockhost-core`.
- `apps/desktop`: Tauri v2 + Svelte 5 + TypeScript desktop app shell with system tray and ephemeral WebView2 lifecycle (destroys the WebView when closed to tray to stay light on 4 GB laptops).
- `panel.ps1`, `Minecraft-Server.bat`, `tests/`: Tested PowerShell control panel stopgap release and behaviour fixtures (kept green until the desktop app reaches full parity).

## Building & Testing

### Prerequisites
- Rust (stable, with `rustfmt` and `clippy`)
- Node.js 22+ and npm
- On Windows for full Tauri MSVC desktop builds: Visual Studio C++ Build Tools (or let GitHub Actions `windows-latest` build it)

### Core & CLI
```bash
cargo fmt --all -- --check
cargo clippy -p blockhost-core -p blockhost-cli --all-targets -- -D warnings
cargo test -p blockhost-core -p blockhost-cli
```

### Live Upstream API Smoke Check
```bash
cargo test -p blockhost-core --test live_api -- --ignored --nocapture
```

### Desktop Frontend
```bash
cd apps/desktop
npm ci
npm run check
npm run build
```

### PowerShell Stopgap Suite (Linux/macOS with `pwsh` + `python3`)
```bash
PWSH=$(which pwsh) tests/run-tests.sh
```
