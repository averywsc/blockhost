# Building, Signing & Releasing BlockHost

## Cutting a Release Installer
1. Bump the version in `Cargo.toml` (`[workspace.package].version`), `apps/desktop/package.json`, and `apps/desktop/src-tauri/tauri.conf.json`.
2. Push a version tag:
   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```
3. GitHub Actions (`.github/workflows/release.yml`) automatically builds the per-user Windows NSIS installer (`BlockHost_0.2.0_x64-setup.exe`) on `windows-latest` and creates a draft GitHub Release.

## Adding Windows Code Signing (to pass Smart App Control & SmartScreen)
Choose one of the signing options below before public distribution:
- **SignPath Foundation (free for open-source MIT)**: apply at `signpath.org`, then add the SignPath GitHub Action step in `.github/workflows/release.yml` to sign both `blockhost-desktop.exe` and the NSIS installer.
- **Certum Open Source Cloud (~$55/yr for NZ individuals)**: configure `SimplySign` credentials in GitHub Secrets and sign via `signtool` in `.github/workflows/release.yml`.
- **Azure Artifact Signing ($9.99/mo, requires a registered NZ business)**: add the `azure/trusted-signing-action` step in `.github/workflows/release.yml`.

## Enabling Signed In-App Auto-Updates (`tauri-plugin-updater`)
BlockHost currently checks GitHub Releases and opens the download page for new versions. When you are ready for one-click in-app updates:
1. Generate a free Ed25519 keypair: `npx @tauri-apps/cli signer generate -w ~/.tauri/blockhost.key`.
2. Add `tauri-plugin-updater` to `apps/desktop/src-tauri/Cargo.toml` and paste the public key into `tauri.conf.json` (`plugins.updater.pubkey`).
3. Store `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in GitHub Repository Secrets so `.github/workflows/release.yml` publishes `latest.json` alongside every release.
