<script lang="ts">
  import {
    api,
    formatGb,
    type AppUpdateInfo,
    type ResolvedPaper,
    type StatusSnapshot,
    type UpdateStatus,
  } from "../lib/api";
  import ConfirmModal from "./ConfirmModal.svelte";

  interface Props {
    status: StatusSnapshot;
    updateInfo: UpdateStatus | null;
    onNotify: (msg: string, kind?: "info" | "error") => void;
    onRefreshUpdates: () => Promise<void>;
  }

  let { status, updateInfo, onNotify, onRefreshUpdates }: Props = $props();

  let heapMb = $state(2048);
  let maxPlayers = $state(8);
  let backupHours = $state(6);
  let restartHours = $state(0);
  let tunnel = $state(false);
  let perfMode = $state(true);
  let aikarFlags = $state(false);
  let motd = $state("");
  let whitelistOn = $state(true);
  let busy = $state(false);
  let checkingUpdates = $state(false);

  let confirmWhitelistOff = $state(false);
  let pendingMajorUpdate = $state<ResolvedPaper | null>(null);
  let discordWebhook = $state("");
  let appUpdate = $state<AppUpdateInfo | null>(null);
  let checkingApp = $state(false);

  const serverName = $derived(status.current ?? "");
  const isRunning = $derived(status.running !== null && status.running === serverName);
  const maxHeap = $derived(Math.max(1024, status.hardware.ram_mb - 1024));

  $effect(() => {
    heapMb = status.heap_mb ?? status.hardware.recommended_heap_mb;
    maxPlayers = status.max_players ?? status.hardware.recommended_max_players;
    backupHours = status.backup_hours ?? 6;
    restartHours = status.restart_hours ?? 0;
    tunnel = status.tunnel.enabled;
    perfMode = status.perf_mode ?? true;
    aikarFlags = status.aikar_flags;
    if (serverName) {
      api.getGameRules(serverName).then((r) => {
        motd = r.motd;
        whitelistOn = r.white_list;
      }).catch(() => {});
    }
  });

  async function saveGeneral() {
    if (!serverName) return;
    busy = true;
    try {
      await api.updateSettings({
        server_name: serverName,
        heap_mb: heapMb,
        max_players: maxPlayers,
        motd: motd.trim() || undefined,
        tunnel,
        backup_hours: backupHours,
        restart_hours: restartHours,
        perf_mode: perfMode,
        aikar_flags: aikarFlags,
      });
      onNotify(
        isRunning
          ? "Settings saved. Restart the server to apply memory or player limit changes."
          : "Settings saved."
      );
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function toggleAutostart(checked: boolean) {
    try {
      await api.setAutostart(checked);
      onNotify(checked ? "BlockHost will start when you log in to Windows." : "Start with Windows turned off.");
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  async function toggleWhitelist(next: boolean) {
    if (!serverName) return;
    if (!next) {
      confirmWhitelistOff = true;
      return;
    }
    try {
      await api.setWhitelistEnabled(serverName, true);
      whitelistOn = true;
      onNotify("Whitelist turned back on. Only allowed players can join.");
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  async function doDisableWhitelist() {
    if (!serverName) return;
    confirmWhitelistOff = false;
    try {
      await api.setWhitelistEnabled(serverName, false);
      whitelistOn = false;
      onNotify("Whitelist turned off.");
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  async function checkUpdates() {
    checkingUpdates = true;
    try {
      await onRefreshUpdates();
      onNotify("Checked for updates.");
    } finally {
      checkingUpdates = false;
    }
  }

  async function installPaper(pick: ResolvedPaper) {
    if (!serverName) return;
    pendingMajorUpdate = null;
    busy = true;
    try {
      const msg = await api.installPaperUpdate(serverName, pick);
      onNotify(msg);
      await onRefreshUpdates();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function saveDiscord(sendTest: boolean) {
    if (!serverName) return;
    busy = true;
    try {
      const msg = await api.saveDiscordWebhook(serverName, discordWebhook.trim(), sendTest);
      onNotify(msg);
      if (discordWebhook.trim()) discordWebhook = "";
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function exportDiag() {
    if (!serverName) return;
    busy = true;
    try {
      const msg = await api.exportDiagnostics(serverName);
      if (msg) onNotify(msg);
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function checkBlockHostUpdate() {
    checkingApp = true;
    try {
      appUpdate = await api.checkAppUpdate();
      if (!appUpdate.update_available) {
        onNotify(`BlockHost v${appUpdate.current_version} is up to date.`);
      }
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      checkingApp = false;
    }
  }

  async function configureLidPower() {
    busy = true;
    try {
      const msg = await api.applyLaptopLidPower();
      onNotify(msg);
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }
</script>

<div class="stack">
  <div class="row-between">
    <div class="stack-xs">
      <h1 class="page-title">Settings</h1>
      <p class="muted">Memory, schedules, startup, and server updates.</p>
    </div>
    <button class="btn" onclick={() => api.openFolder(serverName)}>Open server folder</button>
  </div>

  <div class="card stack">
    <h2 class="section-title">Server performance and schedules</h2>

    <div class="subcard stack-sm">
      <div class="row-between">
        <span class="field-label">Memory for Minecraft</span>
        <strong>{formatGb(heapMb)}</strong>
      </div>
      <input type="range" min="1024" max={maxHeap} step="256" bind:value={heapMb} />
      <div class="row-between muted tiny">
        <span>This PC has {formatGb(status.hardware.ram_mb)}</span>
        <span>Recommended {formatGb(status.hardware.recommended_heap_mb)}</span>
      </div>
    </div>

    <div class="two-col">
      <label class="stack-sm">
        <span class="field-label">Server list name (MOTD)</span>
        <input class="input" bind:value={motd} maxlength="40" />
      </label>

      <label class="stack-sm">
        <span class="field-label">Max players</span>
        <input class="input" type="number" min="1" max="100" bind:value={maxPlayers} />
      </label>

      <label class="stack-sm">
        <span class="field-label">Auto-backup every (hours, 0 = off)</span>
        <input class="input" type="number" min="0" max="168" bind:value={backupHours} />
      </label>

      <label class="stack-sm">
        <span class="field-label">Auto-restart every (hours, 0 = off)</span>
        <input class="input" type="number" min="0" max="168" bind:value={restartHours} />
      </label>
    </div>

    <div class="two-col">
      <label class="subcard toggle-row">
        <div class="stack-xs">
          <strong>Playit internet tunnel</strong>
          <span class="muted tiny">Start the free playit tunnel with the server.</span>
        </div>
        <input type="checkbox" bind:checked={tunnel} />
      </label>

      <label class="subcard toggle-row">
        <div class="stack-xs">
          <strong>High performance power plan</strong>
          <span class="muted tiny">Switch Windows power plan while running and restore on stop.</span>
        </div>
        <input type="checkbox" bind:checked={perfMode} />
      </label>

      <label class="subcard toggle-row">
        <div class="stack-xs">
          <strong>Aikar JVM performance flags</strong>
          <span class="muted tiny">Tuned Java garbage collection for smoother ticks and fewer lag spikes.</span>
        </div>
        <input type="checkbox" bind:checked={aikarFlags} />
      </label>
    </div>

    <div class="row-end">
      <button class="btn btn-primary" disabled={busy} onclick={saveGeneral}>Save settings</button>
    </div>
  </div>

  <div class="card stack">
    <h2 class="section-title">Windows startup and safety</h2>

    <label class="subcard toggle-row">
      <div class="stack-xs">
        <strong>Start BlockHost with Windows</strong>
        <span class="muted tiny">Launches quietly in the system tray when you log in.</span>
      </div>
      <input
        type="checkbox"
        checked={status.autostart_enabled}
        onchange={(e) => toggleAutostart((e.currentTarget as HTMLInputElement).checked)}
      />
    </label>

    <div class="subcard toggle-row">
      <div class="stack-xs">
        <strong>Keep laptop awake with lid closed (plugged in)</strong>
        <span class="muted tiny">Sets Windows AC sleep and lid-close action to Do Nothing for 24/7 laptop hosting.</span>
      </div>
      <button class="btn btn-sm" disabled={busy} onclick={configureLidPower}>
        Apply power settings
      </button>
    </div>

    <div class="subcard toggle-row">
      <div class="stack-xs">
        <strong>Only allowed players (Whitelist)</strong>
        <span class="muted tiny">
          {whitelistOn
            ? "Protected. Only usernames on your Players list can join."
            : "Warning: anyone who finds your address can join and grief."}
        </span>
      </div>
      <button
        class={whitelistOn ? "btn btn-sm" : "btn btn-primary btn-sm"}
        onclick={() => toggleWhitelist(!whitelistOn)}
      >
        {whitelistOn ? "Turn off" : "Turn back on"}
      </button>
    </div>
  </div>

  {#if status.software === "paper"}
    <div class="card stack">
      <div class="row-between">
        <div class="stack-xs">
          <h2 class="section-title">Paper server updates</h2>
          <span class="muted tiny">
            Installed: Minecraft {status.mc_version ?? "?"} (Paper build {status.paper_build ?? 0})
          </span>
        </div>
        <button class="btn btn-sm" disabled={checkingUpdates} onclick={checkUpdates}>
          {checkingUpdates ? "Checking" : "Check now"}
        </button>
      </div>

      {#if updateInfo?.same_version}
        <div class="subcard row-between">
          <div class="stack-xs">
            <strong>Paper build {updateInfo.same_version.build} for Minecraft {updateInfo.same_version.mc_version}</strong>
            <span class="muted tiny">Safe bug and security fixes. Friends do not need to change their game.</span>
          </div>
          <button
            class="btn btn-primary btn-sm"
            disabled={busy || isRunning}
            onclick={() => updateInfo?.same_version && installPaper(updateInfo.same_version)}
          >
            Install update
          </button>
        </div>
      {/if}

      {#if updateInfo?.latest_major}
        <div class="subcard row-between">
          <div class="stack-xs">
            <strong>Upgrade to Minecraft {updateInfo.latest_major.mc_version}</strong>
            <span class="muted tiny">Friends must update Minecraft too. A backup is taken automatically first.</span>
          </div>
          <button
            class="btn btn-sm"
            disabled={busy || isRunning}
            onclick={() => (pendingMajorUpdate = updateInfo?.latest_major ?? null)}
          >
            Upgrade
          </button>
        </div>
      {/if}

      {#if updateInfo && !updateInfo.same_version && !updateInfo.latest_major}
        <p class="muted tiny">Your server is up to date.</p>
      {/if}

      {#if isRunning && (updateInfo?.same_version || updateInfo?.latest_major)}
        <p class="muted tiny">Stop the server before installing updates.</p>
      {/if}
    </div>
  {/if}

  <div class="card stack">
    <div class="row-between">
      <div class="stack-xs">
        <div class="row">
          <h2 class="section-title">Discord notifications</h2>
          {#if status.discord_enabled}
            <span class="badge badge-green">Enabled</span>
          {/if}
        </div>
        <span class="muted tiny">
          Optional webhook to post server start, stop, and crash alerts to your friends Discord channel.
        </span>
      </div>
    </div>

    <div class="row">
      <input
        class="input mono grow"
        type="password"
        bind:value={discordWebhook}
        placeholder="https://discord.com/api/webhooks/..."
      />
      <button
        class="btn btn-primary btn-sm"
        disabled={busy || !discordWebhook.trim()}
        onclick={() => saveDiscord(true)}
      >
        Save and test
      </button>
      {#if status.discord_enabled}
        <button
          class="btn btn-sm"
          disabled={busy}
          onclick={() => {
            discordWebhook = "";
            saveDiscord(false);
          }}
        >
          Turn off
        </button>
      {/if}
    </div>
  </div>

  <div class="card stack">
    <div class="row-between">
      <div class="stack-xs">
        <h2 class="section-title">About BlockHost and diagnostics</h2>
        <span class="muted tiny mono">v{status.app_version} ({status.data_dir})</span>
      </div>
      <div class="row">
        <button class="btn btn-sm" disabled={busy} onclick={exportDiag}>
          Export diagnostics (.zip)
        </button>
        <button class="btn btn-sm" disabled={checkingApp} onclick={checkBlockHostUpdate}>
          {checkingApp ? "Checking" : "Check app updates"}
        </button>
      </div>
    </div>

    {#if appUpdate?.update_available}
      <div class="banner banner-info row-between">
        <span>BlockHost v{appUpdate.latest_version} is available!</span>
        <button class="btn btn-primary btn-sm" onclick={() => appUpdate && api.openUrl(appUpdate.release_url)}>
          Download update
        </button>
      </div>
    {/if}
  </div>
</div>

<ConfirmModal
  open={confirmWhitelistOff}
  title="Turn off whitelist"
  message="Turning off the whitelist lets anyone on the internet who finds your address join, build, and grief your world. We strongly recommend keeping it on."
  confirmLabel="Turn off whitelist"
  requireYes={true}
  danger={true}
  onConfirm={doDisableWhitelist}
  onCancel={() => (confirmWhitelistOff = false)}
/>

<ConfirmModal
  open={pendingMajorUpdate !== null}
  title="Upgrade Minecraft version"
  message="Upgrading to Minecraft {pendingMajorUpdate?.mc_version ?? ''} converts your world format so it cannot be downgraded directly. BlockHost will back up your world first."
  confirmLabel="Upgrade now"
  requireYes={true}
  onConfirm={() => pendingMajorUpdate && installPaper(pendingMajorUpdate)}
  onCancel={() => (pendingMajorUpdate = null)}
/>

<style>
  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .field-label {
    font-size: 13px;
    font-weight: 600;
  }

  .tiny {
    font-size: 12px;
  }

  .two-col {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
    gap: 12px;
  }

  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .row-end {
    display: flex;
    justify-content: flex-end;
  }

  .grow {
    flex: 1;
    min-width: 220px;
  }
</style>
