<script lang="ts">
  import {
    api,
    type ActionResult,
    type NetworkOverview,
    type PingStats,
    type StatusSnapshot,
  } from "../lib/api";

  interface Props {
    status: StatusSnapshot;
    onNotify: (msg: string, kind?: "info" | "error") => void;
  }

  let { status, onNotify }: Props = $props();

  let overview = $state<NetworkOverview | null>(null);
  let loadingOverview = $state(false);
  let checkingPing = $state(false);
  let cfPing = $state<PingStats | null>(null);
  let googlePing = $state<PingStats | null>(null);
  let friendTarget = $state("");
  let friendPing = $state<PingStats | null>(null);
  let pingingFriend = $state(false);
  let forwardResults = $state<{ fw: ActionResult; upnp: ActionResult } | null>(null);
  let forwarding = $state(false);
  let playitInput = $state("");

  const serverName = $derived(status.current ?? "");

  $effect(() => {
    playitInput = status.tunnel.saved_address ?? status.tunnel.address ?? "";
  });

  async function refreshOverview() {
    loadingOverview = true;
    try {
      overview = await api.getNetworkOverview();
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      loadingOverview = false;
    }
  }

  $effect(() => {
    void refreshOverview();
  });

  async function runConnectionCheck() {
    checkingPing = true;
    try {
      cfPing = await api.pingTarget("1.1.1.1", 8);
      googlePing = await api.pingTarget("8.8.8.8", 8);
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      checkingPing = false;
    }
  }

  async function pingFriend() {
    if (!friendTarget.trim()) return;
    pingingFriend = true;
    try {
      friendPing = await api.pingTarget(friendTarget.trim(), 6);
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      pingingFriend = false;
    }
  }

  async function setupForward() {
    if (!serverName) return;
    forwarding = true;
    try {
      const [fw, upnp] = await api.setupPortForward(serverName);
      forwardResults = { fw, upnp };
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      forwarding = false;
    }
  }

  async function removeForward() {
    if (!serverName) return;
    forwarding = true;
    try {
      const res = await api.removePortForward(serverName);
      onNotify(res.message, res.ok ? "info" : "error");
    } catch (e) {
      onNotify(String(e), "error");
    } finally {
      forwarding = false;
    }
  }

  async function savePlayit() {
    if (!serverName) return;
    try {
      await api.updateSettings({
        server_name: serverName,
        playit_address: playitInput.trim(),
      });
      onNotify("Saved internet address.");
    } catch (e) {
      onNotify(String(e), "error");
    }
  }

  function barHeight(ms: number, maxMs: number): number {
    return Math.max(14, Math.min(100, Math.round((ms / Math.max(maxMs, 40)) * 100)));
  }
</script>

<div class="stack">
  <div class="row-between">
    <div class="stack-xs">
      <h1 class="page-title">Network and ping</h1>
      <p class="muted">
        Direct port forwarding gives the lowest NZ ping. Use the playit.gg tunnel if your ISP uses CGNAT.
      </p>
    </div>
    <button class="btn" disabled={loadingOverview} onclick={refreshOverview}>
      {loadingOverview ? "Checking" : "Refresh"}
    </button>
  </div>

  {#if overview}
    {#if overview.link?.wifi}
      <div class="banner banner-warn">
        You are on Wi-Fi ({overview.link.name}). Wi-Fi adds jitter spikes. Plug this PC into your router with an Ethernet cable for the smoothest ping.
      </div>
    {/if}

    <div class="banner {overview.cgnat.detected ? 'banner-warn' : 'banner-info'}">
      {overview.cgnat.summary}
    </div>

    <div class="info-grid">
      <div class="card stat">
        <span class="muted tiny">Home LAN IP</span>
        <strong class="mono">{overview.lan_ip ?? "Unknown"}</strong>
      </div>
      <div class="card stat">
        <span class="muted tiny">Public IP</span>
        <strong class="mono">{overview.public_ip ?? "Unknown"}</strong>
      </div>
      <div class="card stat">
        <span class="muted tiny">Connection</span>
        <strong>
          {overview.link
            ? `${overview.link.wifi ? "Wi-Fi" : "Ethernet"} (${overview.link.speed})`
            : "Detected on Windows"}
        </strong>
      </div>
      {#if overview.link?.gateway}
        <div class="card stat">
          <span class="muted tiny">Router gateway</span>
          <strong class="mono">http://{overview.link.gateway}</strong>
        </div>
      {/if}
    </div>
  {/if}

  <div class="card stack">
    <div class="row-between">
      <div class="stack-xs">
        <h2 class="section-title">One-click port forward (lowest NZ ping)</h2>
        <span class="muted tiny">
          Asks your router via UPnP to forward TCP port {status.port ?? 25565} and adds a Windows Firewall rule.
        </span>
      </div>
      <div class="row">
        <button class="btn btn-primary" disabled={forwarding} onclick={setupForward}>
          {forwarding ? "Working" : "Enable port forward"}
        </button>
        <button class="btn" disabled={forwarding} onclick={removeForward}>Remove</button>
      </div>
    </div>

    {#if forwardResults}
      <div class="stack-sm">
        <div class="banner {forwardResults.fw.ok ? 'banner-green' : 'banner-warn'}">
          Firewall: {forwardResults.fw.message}
        </div>
        <div class="banner {forwardResults.upnp.ok ? 'banner-green' : 'banner-warn'}">
          Router: {forwardResults.upnp.message}
        </div>
      </div>
    {/if}
  </div>

  <div class="card stack">
    <div class="stack-xs">
      <h2 class="section-title">Internet tunnel (playit.gg)</h2>
      <span class="muted tiny">Works behind CGNAT with no router changes. Save your tunnel address so it shows on the Dashboard.</span>
    </div>
    <form
      class="row"
      onsubmit={(e) => {
        e.preventDefault();
        savePlayit();
      }}
    >
      <input
        class="input mono grow"
        bind:value={playitInput}
        placeholder="example.joinmc.link"
      />
      <button class="btn btn-primary" type="submit">Save address</button>
    </form>
  </div>

  <div class="card stack">
    <div class="row-between">
      <div class="stack-xs">
        <h2 class="section-title">Connection check</h2>
        <span class="muted tiny">In NZ fibre under 30 ms average with low jitter is ideal for hosting.</span>
      </div>
      <button class="btn btn-primary" disabled={checkingPing} onclick={runConnectionCheck}>
        {checkingPing ? "Testing" : "Run connection check"}
      </button>
    </div>

    {#if cfPing || googlePing}
      <div class="two-col">
        {#each [{ label: "Cloudflare (1.1.1.1)", st: cfPing }, { label: "Google (8.8.8.8)", st: googlePing }] as item (item.label)}
          {#if item.st}
            {@const maxMs = Math.max(...item.st.samples_ms, 40)}
            <div class="subcard stack-sm">
              <div class="row-between">
                <strong>{item.label}</strong>
                <span class="badge {item.st.loss_pct >= 5 ? 'badge-red' : 'badge-green'}">
                  {item.st.grade}
                </span>
              </div>
              {#if item.st.ok}
                <div class="row muted tiny">
                  <span>Avg {item.st.avg_ms} ms</span>
                  <span>Jitter {item.st.jitter_ms} ms</span>
                  <span>Loss {item.st.loss_pct}%</span>
                </div>
                <div class="bars" aria-label="Latency samples">
                  {#each item.st.samples_ms as ms, i (i)}
                    <div
                      class="bar"
                      style="height: {barHeight(ms, maxMs)}%"
                      title="{ms} ms"
                    ></div>
                  {/each}
                </div>
              {:else}
                <p class="muted tiny">No reply</p>
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    {/if}

    <form
      class="row"
      onsubmit={(e) => {
        e.preventDefault();
        pingFriend();
      }}
    >
      <input
        class="input grow"
        bind:value={friendTarget}
        placeholder="Ping a friend or host address"
      />
      <button class="btn" type="submit" disabled={pingingFriend || !friendTarget.trim()}>
        {pingingFriend ? "Pinging" : "Ping"}
      </button>
    </form>

    {#if friendPing}
      <div class="subcard row-between">
        <strong>{friendTarget}</strong>
        {#if friendPing.ok}
          <span class="mono tiny">
            Avg {friendPing.avg_ms} ms  Jitter {friendPing.jitter_ms} ms  Loss {friendPing.loss_pct}%  ({friendPing.grade})
          </span>
        {:else}
          <span class="muted tiny">No reply (some networks block ping)</span>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .stack-xs {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .tiny {
    font-size: 12px;
  }

  .grow {
    flex: 1;
    min-width: 200px;
  }

  .info-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 12px;
  }

  .two-col {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
    gap: 12px;
  }

  .stat {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .bars {
    height: 48px;
    display: flex;
    align-items: flex-end;
    gap: 5px;
    padding-top: 6px;
  }

  .bar {
    flex: 1;
    border-radius: 4px 4px 2px 2px;
    background: var(--accent);
    opacity: 0.85;
  }
</style>
