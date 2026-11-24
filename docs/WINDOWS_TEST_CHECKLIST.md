# Real-Windows Manual Test Checklist

Run through this checklist on both your **old 4 GB laptop** and your **main gaming PC** before tagging `v1.0.0`.

## 1. Installer & Smart App Control
- [ ] Run the per-user NSIS installer (`BlockHost_*_x64-setup.exe`) as a standard (non-admin) user — installs cleanly with no UAC prompt.
- [ ] Once code signing is configured (see [RELEASING.md](./RELEASING.md)), verify launch on Windows 11 with **Smart App Control** enabled.
- [ ] Uninstall BlockHost and verify the prompt **"Keep your Minecraft worlds, backups, and server settings?"** defaults to **Yes** and preserves `%LOCALAPPDATA%\BlockHost`.

## 2. First-Run Wizard & Low-RAM Idle Footprint
- [ ] Create a **Paper** server with a chosen Minecraft version (e.g. latest stable) and confirm Java + Paper download with SHA-256 verification and EULA gate.
- [ ] Create a second **Fabric** server and confirm Modrinth searches filter to Fabric mods and install into `mods/`.
- [ ] Close the BlockHost window to the system tray while the server is stopped; check Task Manager and verify WebView2 processes close and BlockHost idles under ~30 MB RAM.
- [ ] Click the tray icon and verify the window reopens cleanly with live status preserved.

## 3. Server Lifecycle, Admin Controls & Backups
- [ ] Start the server, join from `localhost`, and verify live **Players online**, **TPS**, **Memory usage**, and **CPU %** appear on Dashboard.
- [ ] Test **Quick admin controls**: Broadcast announcement, Time/Weather presets, Save world, and **Players** Heal / Feed / Teleport / Gamemode / Kick / Ban / Unban.
- [ ] Test **Back up now**, stop the server, **Upload world (.zip)**, and confirm the previous world was preserved in `old-worlds/` (never deleted).
- [ ] Test **Warn 30s and stop** and confirm players see the countdown before the server saves and exits cleanly.

## 4. Networking & NZ Ping (Auckland)
- [ ] Open **Network** and run **Connection check** (Cloudflare + Google) — verify Wi-Fi vs Ethernet warning displays accurately.
- [ ] Verify **CGNAT detection** banner matches your NZ ISP setup.
- [ ] If not behind CGNAT: click **Enable port forward**, configure a free **DuckDNS** hostname, and have an NZ friend join and check ping (~5-15 ms on NZ fibre).
- [ ] If behind CGNAT: test **Tailscale** auto-detection OR **playit.gg** claim link + address auto-capture.
- [ ] Confirm Windows Firewall rule opens **only** the game port (default `25565`) and never opens RCON (`25575`).

## 5. 24/7 Old-Laptop Reliability
- [ ] Enable **Start BlockHost with Windows** in Settings, reboot the laptop, log in, and confirm BlockHost launches into the tray.
- [ ] Click **Apply power settings** (lid-close = Do Nothing on AC), plug in the laptop, close the lid, and verify a friend stays connected for 10+ minutes.
- [ ] Simulate a crash (e.g. incompatible mod or force-kill Java) and verify the **Crash Analyzer** shows a plain-English diagnosis on the Dashboard (and optional Discord webhook alert if configured).
