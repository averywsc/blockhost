<script lang="ts">
  import {
    api,
    formatGb,
    formatSize,
    formatUptime,
    type LiveMetrics,
    type StatusSnapshot,
    type UpdateStatus,
  } from "../lib/api";
  import ConfirmModal from "./ConfirmModal.svelte";

  interface Props {
    status: StatusSnapshot;
    metrics: LiveMetrics | null;
    updateInfo: UpdateStatus | null;
    onNotify: (msg: string, kind?: "info" | "error") => void;
    onNavigate: (tab: string) => void;
  }

  let { status, metrics, updateInfo, onNotify, onNavigate }: Props = $props();

  let busy = $state(false);
  let broadcastText = $state("");
  let confirmForceStop = $state(false);
  let copiedKey = $state<string | null>(null);

  const serverName = $derived(status.current ?? "");
  const isRunning = $derived(status.running !== null && status.running === serverName);
  const portSuffix = $derived(status.port && status.port !== 25565 ? `:${status.port}` : "");
  const bestInternetAddr = $derived(
    status.duckdns_domain
      ? `${status.duckdns_domain}${portSuffix}`
      : status.tailscale_ip
        ? `${status.tailscale_ip}${portSuffix}`
        : status.tunnel.address ?? status.tunnel.saved_address
  );
  const internetLabel = $derived(
    status.duckdns_domain
      ? "Direct (DuckDNS)"
      : status.tailscale_ip
        ? "Direct P2P (Tailscale)"
        : "Over the internet"
  );
  const hasUpdate = $derived(
    Boolean(updateInfo && (updateInfo.same_version || updateInfo.latest_major))
  );

  async function copy(key: string, value: string) {
    try {
      await navigator.clipboard.writeText(value);
      copiedKey = key;
      setTimeout(() => {
        if (copiedKey === key) copiedKey = null;
      }, 1800);
    } catch {
      onNotify(`Address: ${value}`);
    }
  }

  async function start() {
    if (!serverName) return;
    busy = true;
    try {
      await api.startServer(serverName);
      onNotify("Starting server");
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function stop(warnSeconds: number) {
    busy = true;
    try {
      await api.stopServer(warnSeconds);
      onNotify(
        warnSeconds > 0
          ? `Warned players. Stopping in ${warnSeconds} seconds.`
          : "Saving and stopping server"
      );
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function restart() {
    if (!serverName) return;
    busy = true;
    try {
      await api.restartServer(serverName);
      onNotify("Server restarted");
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function forceStop() {
    confirmForceStop = false;
    try {
      await api.forceStopServer();
      onNotify("Server force stopped");
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  async function quickCmd(cmd: string, label: string) {
    try {
      const res = await api.sendCommand(cmd);
      onNotify(res || label);
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  async function sendBroadcast() {
    const msg = broadcastText.trim();
    if (!msg) return;
    broadcastText = "";
    await quickCmd(`say ${msg}`, "Announcement sent");
  }
</script>

<div class="stack">
  <div class="card hero-card">
    <div class="row-between">
      <div class="stack-xs">
        <div class="row">
          <h1 class="page-title">{serverName}</h1>
          {#if isRunning}
            <span class="badge badge-green">
              {status.stopping ? "Stopping" : metrics?.reachable ? "Online" : "Starting"}
            </span>
          {:else}
            <span class="badge">Stopped</span>
          {/if}
          {#if status.software}
            <span class="badge badge-blue">
              {status.software === "fabric" ? "Fabric Mods" : "Paper"} {status.mc_version ?? ""}
            </span>
          {/if}
        </div>
        <p class="muted">
          {#if isRunning}
            Friends need Minecraft {status.mc_version ?? ""} to join.
          {:else}
            Ready to launch with {formatGb(status.heap_mb ?? status.hardware.recommended_heap_mb)} memory.
          {/if}
        </p>
      </div>

      <div class="row">
        {#if !isRunning}
          <button class="btn btn-primary hero-btn" disabled={busy} onclick={start}>
            Start server
          </button>
        {:else}
          <button class="btn btn-danger hero-btn" disabled={busy || status.stopping} onclick={() => stop(0)}>
            Stop server
          </button>
          <button class="btn" disabled={busy || status.stopping} onclick={() => stop(30)}>
            Warn 30s and stop
          </button>
          <button class="btn" disabled={busy || status.stopping} onclick={restart}>
            Restart
          </button>
          <button class="btn btn-danger-soft btn-sm" onclick={() => (confirmForceStop = true)}>
            Force stop
          </button>
        {/if}
      </div>
    </div>
  </div>

  {#if status.last_crash && !isRunning}
    <div class="banner banner-danger stack-xs">
      <strong>{status.last_crash.title}</strong>
      <span>{status.last_crash.explanation} {status.last_crash.suggestion}</span>
    </div>
  {/if}

  {#if hasUpdate}
    <div class="banner banner-info row-between">
      <span>
        {#if updateInfo?.same_version}
          A new Paper build ({updateInfo.same_version.build}) is ready for Minecraft {updateInfo.same_version.mc_version}.
        {:else if updateInfo?.latest_major}
          Minecraft {updateInfo.latest_major.mc_version} is available.
        {/if}
      </span>
      <button class="btn btn-sm" onclick={() => onNavigate("settings")}>View update</button>
    </div>
  {/if}

  {#if status.tunnel.claim_url}
    <div class="banner banner-warn row-between">
      <span>Finish one-time playit tunnel setup in your browser: {status.tunnel.claim_url}</span>
      <button class="btn btn-sm" onclick={() => copy("claim", status.tunnel.claim_url ?? "")}>
        {copiedKey === "claim" ? "Copied" : "Copy link"}
      </button>
    </div>
  {/if}

  <div class="metrics-grid">
    <div class="card stat">
      <span class="muted small">Players online</span>
      <strong class="stat-val">
        {#if isRunning && metrics?.reachable}
          {metrics.online_players.length} / {status.max_players ?? 8}
        {:else}
          0 / {status.max_players ?? 8}
        {/if}
      </strong>
    </div>

    <div class="card stat">
      <span class="muted small">Server speed (TPS)</span>
      <strong class="stat-val">
        {#if isRunning && metrics?.tps_text}
          {metrics.tps_text.split(",")[0]}
        {:else if isRunning}
          Warming up
        {:else}
          Offline
        {/if}
      </strong>
    </div>

    <div class="card stat">
      <span class="muted small">Memory usage</span>
      <strong class="stat-val">
        {#if isRunning && metrics?.process_ram_mb != null}
          {formatGb(metrics.process_ram_mb)} / {formatGb(status.heap_mb ?? 2048)}
        {:else}
          {formatGb(status.heap_mb ?? 2048)} limit
        {/if}
      </strong>
    </div>

    <div class="card stat">
      <span class="muted small">{isRunning ? "Uptime and CPU" : "World size"}</span>
      <strong class="stat-val">
        {#if isRunning}
          {formatUptime(status.uptime_secs)}
          {#if metrics?.process_cpu_pct != null}
            ({Math.round(metrics.process_cpu_pct)}%)
          {/if}
        {:else}
          {formatSize(status.world_size_bytes)}
        {/if}
      </strong>
    </div>
  </div>

  <div class="card stack-sm">
    <div class="row-between">
      <h2 class="section-title">Join addresses</h2>
      <button class="btn btn-sm" onclick={() => onNavigate("network")}>Network setup</button>
    </div>

    <div class="addr-grid">
      <div class="subcard row-between">
        <div class="stack-xs">
          <span class="muted tiny">This PC</span>
          <span class="mono">localhost{portSuffix}</span>
        </div>
        <button class="btn btn-sm" onclick={() => copy("local", `localhost${portSuffix}`)}>
          {copiedKey === "local" ? "Copied" : "Copy"}
        </button>
      </div>

      <div class="subcard row-between">
        <div class="stack-xs">
          <span class="muted tiny">Same Wi-Fi</span>
          <span class="mono">{status.lan_ip ? `${status.lan_ip}${portSuffix}` : "Unavailable"}</span>
        </div>
        {#if status.lan_ip}
          <button class="btn btn-sm" onclick={() => copy("lan", `${status.lan_ip}${portSuffix}`)}>
            {copiedKey === "lan" ? "Copied" : "Copy"}
          </button>
        {/if}
      </div>

      <div class="subcard row-between">
        <div class="stack-xs">
          <span class="muted tiny">{internetLabel}</span>
          <span class="mono">
            {bestInternetAddr ?? (status.tunnel.enabled ? "Waiting for playit address" : "Set up in Network")}
          </span>
        </div>
        {#if bestInternetAddr}
          <button class="btn btn-sm" onclick={() => copy("net", bestInternetAddr)}>
            {copiedKey === "net" ? "Copied" : "Copy"}
          </button>
        {/if}
      </div>
    </div>
  </div>

  {#if isRunning}
    <div class="card stack">
      <div class="row-between">
        <h2 class="section-title">Quick admin controls</h2>
        <button class="btn btn-sm" onclick={() => onNavigate("players")}>Manage players</button>
      </div>

      <form
        class="row"
        onsubmit={(e) => {
          e.preventDefault();
          sendBroadcast();
        }}
      >
        <input
          class="input grow"
          bind:value={broadcastText}
          placeholder="Send a message to everyone playing"
        />
        <button class="btn btn-primary" type="submit" disabled={!broadcastText.trim()}>
          Broadcast
        </button>
      </form>

      <div class="admin-groups">
        <div class="subcard stack-sm">
          <span class="muted tiny">Time</span>
          <div class="row">
            <button class="btn btn-sm" onclick={() => quickCmd("time set day", "Time set to day")}>Day</button>
            <button class="btn btn-sm" onclick={() => quickCmd("time set noon", "Time set to noon")}>Noon</button>
            <button class="btn btn-sm" onclick={() => quickCmd("time set sunset", "Time set to sunset")}>Sunset</button>
            <button class="btn btn-sm" onclick={() => quickCmd("time set night", "Time set to night")}>Night</button>
          </div>
        </div>

        <div class="subcard stack-sm">
          <span class="muted tiny">Weather</span>
          <div class="row">
            <button class="btn btn-sm" onclick={() => quickCmd("weather clear", "Weather cleared")}>Clear</button>
            <button class="btn btn-sm" onclick={() => quickCmd("weather rain", "Weather set to rain")}>Rain</button>
            <button class="btn btn-sm" onclick={() => quickCmd("weather thunder", "Weather set to thunder")}>Thunder</button>
          </div>
        </div>

        <div class="subcard stack-sm">
          <span class="muted tiny">World</span>
          <div class="row">
            <button class="btn btn-sm" onclick={() => quickCmd("save-all", "World saved")}>Save now</button>
            <button class="btn btn-sm" onclick={() => api.backupNow(serverName).then(() => onNotify("Backup started"))}>
              Back up now
            </button>
          </div>
        </div>
      </div>
    </div>
  {/if}
</div>

<ConfirmModal
  open={confirmForceStop}
  title="Force stop server"
  message="Force stopping cuts the server off without saving. You could lose recent building progress. Only use this if the server is frozen."
  confirmLabel="Force stop"
  requireYes={true}
  danger={true}
  onConfirm={forceStop}
  onCancel={() => (confirmForceStop = false)}
/>

<style>
  .hero-card {
    padding: 24px;
  }

  .hero-btn {
    padding: 11px 22px;
    font-size: 15px;
  }

  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .small {
    font-size: 13px;
  }

  .tiny {
    font-size: 12px;
  }

  .metrics-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(175px, 1fr));
    gap: 14px;
  }

  .stat {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 16px 18px;
  }

  .stat-val {
    font-size: 20px;
    letter-spacing: -0.02em;
  }

  .addr-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
    gap: 12px;
  }

  .admin-groups {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
    gap: 12px;
  }

  .grow {
    flex: 1;
    min-width: 220px;
  }
</style>
